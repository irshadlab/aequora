use std::{collections::BTreeMap, fmt, sync::atomic::AtomicU64, time::Duration};

use aequora_observability::{
    AttributeKey, AttributeValue, Attributes, INITIAL_METRICS, MetricEvent, MetricId, MetricKind,
    MetricObservation, MetricsSink, ObservabilityError, Observer, OutcomeKind, SeriesBudget,
};
use prometheus_client::{
    encoding::text::encode,
    metrics::{counter::Counter, family::Family, gauge::Gauge, histogram::Histogram},
    registry::Registry,
};

type Labels = Vec<(&'static str, String)>;

enum MetricFamily {
    Counter(Family<Labels, Counter<f64, AtomicU64>>),
    Gauge(Family<Labels, Gauge<f64, AtomicU64>>),
    Histogram(Family<Labels, Histogram>),
}

/// A per-server recorder with a hard limit on label combinations and fixed-size latency buckets.
///
/// Pass it to the server or Axum observer hook, or record catalog observations directly through
/// [`MetricsSink`]. The host application owns the scrape endpoint and its access policy.
pub struct PrometheusMetrics {
    registry: Registry,
    metrics: BTreeMap<MetricId, MetricFamily>,
    series: SeriesBudget,
}

impl PrometheusMetrics {
    /// Media type returned by [`Self::render`].
    pub const CONTENT_TYPE: &'static str =
        "application/openmetrics-text; version=1.0.0; charset=utf-8";

    /// Creates a recorder whose series limit applies across the entire Aequora catalog.
    ///
    /// # Errors
    /// Returns an error for an invalid series budget.
    pub fn new(maximum_series: usize) -> Result<Self, ObservabilityError> {
        let series = SeriesBudget::new(maximum_series)?;
        let mut registry = <Registry>::default();
        let metrics = INITIAL_METRICS
            .into_iter()
            .map(|id| {
                let name = id.name();
                let family = match id.kind() {
                    MetricKind::Counter => {
                        let family = Family::default();
                        registry.register(name.trim_end_matches("_total"), name, family.clone());
                        MetricFamily::Counter(family)
                    }
                    MetricKind::Gauge => {
                        let family = Family::default();
                        registry.register(name, name, family.clone());
                        MetricFamily::Gauge(family)
                    }
                    MetricKind::Histogram => {
                        let family = Family::new_with_constructor(latency_histogram as fn() -> _);
                        registry.register(name, name, family.clone());
                        MetricFamily::Histogram(family)
                    }
                };
                (id, family)
            })
            .collect();
        Ok(Self {
            registry,
            metrics,
            series,
        })
    }

    /// Encodes the current `OpenMetrics` scrape.
    ///
    /// # Errors
    /// Returns a formatting error if the registry cannot be encoded.
    pub fn render(&self) -> Result<String, fmt::Error> {
        let mut output = String::new();
        encode(&mut output, &self.registry)?;
        Ok(output)
    }
}

impl MetricsSink for PrometheusMetrics {
    fn record(&self, observation: MetricObservation) {
        let MetricObservation {
            id,
            value,
            attributes,
        } = observation;
        if !value.is_finite() || (value < 0.0 && id.kind() != MetricKind::Gauge) {
            return;
        }
        // Deserialized attributes also go through validation before export.
        if attributes.values().len() > 8
            || attributes
                .values()
                .values()
                .any(|value| AttributeValue::new(value.as_str()).is_err())
            || self.series.observe(id, &attributes).is_err()
        {
            return;
        }
        let labels = attributes
            .values()
            .iter()
            .map(|(key, value)| (label_name(*key), value.as_str().to_owned()))
            .collect();
        match &self.metrics[&id] {
            MetricFamily::Counter(family) => {
                family.get_or_create(&labels).inc_by(value);
            }
            MetricFamily::Gauge(family) => {
                family.get_or_create(&labels).set(value);
            }
            MetricFamily::Histogram(family) => {
                family.get_or_create(&labels).observe(value);
            }
        }
    }
}

impl Observer for PrometheusMetrics {
    fn record(&self, event: MetricEvent) {
        let (count, duration, micros, outcome) = match event {
            MetricEvent::ServerExchange {
                duration_micros,
                outcome,
                ..
            } => (
                MetricId::SyncExchanges,
                MetricId::SyncExchangeDuration,
                duration_micros,
                outcome,
            ),
            MetricEvent::BootstrapPage {
                duration_micros,
                outcome,
                ..
            } => (
                MetricId::BootstrapOutcomes,
                MetricId::BootstrapDuration,
                duration_micros,
                outcome,
            ),
            _ => return,
        };
        let outcome = match outcome {
            OutcomeKind::Success => "success",
            OutcomeKind::TransientFailure => "transient_failure",
            OutcomeKind::PermanentFailure => "permanent_failure",
        };
        let Ok(attributes) = AttributeValue::new(outcome)
            .and_then(|value| Attributes::new([(AttributeKey::ResultClass, value)]))
        else {
            return;
        };
        MetricsSink::record(
            self,
            MetricObservation {
                id: count,
                value: 1.0,
                attributes: attributes.clone(),
            },
        );
        MetricsSink::record(
            self,
            MetricObservation {
                id: duration,
                value: Duration::from_micros(micros).as_secs_f64(),
                attributes,
            },
        );
    }
}

fn latency_histogram() -> Histogram {
    Histogram::new([
        0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 15.0, 30.0,
    ])
}

const fn label_name(key: AttributeKey) -> &'static str {
    match key {
        AttributeKey::Route => "route",
        AttributeKey::Method => "method",
        AttributeKey::StatusClass => "status_class",
        AttributeKey::ResultClass => "result_class",
        AttributeKey::OperationKind => "operation_kind",
        AttributeKey::EntityType => "entity_type",
        AttributeKey::Adapter => "adapter",
        AttributeKey::Environment => "environment",
        AttributeKey::Region => "region",
        AttributeKey::Release => "release",
        AttributeKey::Platform => "platform",
        AttributeKey::Component => "component",
        AttributeKey::JobKind => "job_kind",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scrape_uses_stable_names_and_rejects_series_beyond_the_budget() {
        let metrics = PrometheusMetrics::new(2).unwrap_or_else(|error| panic!("{error}"));
        for outcome in [
            OutcomeKind::Success,
            OutcomeKind::TransientFailure,
            OutcomeKind::Success,
        ] {
            Observer::record(
                &metrics,
                MetricEvent::BootstrapPage {
                    duration_micros: 13_000,
                    entities: 0,
                    outcome,
                },
            );
        }
        let output = metrics.render().unwrap_or_else(|error| panic!("{error}"));
        assert!(output.contains("aequora_bootstrap_outcomes_total{result_class=\"success\"} 2"));
        assert!(
            output.contains("aequora_bootstrap_duration_seconds_count{result_class=\"success\"} 2")
        );
        assert!(!output.contains("transient_failure"));
    }
}
