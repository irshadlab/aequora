# Part 46 observability completion

Governing specification is [Part 46](../sys-arch/46-observability-metrics-tracing-logging-slos-alerting-production-telemetry-architecture.md).

Part 46 extends `aequora-observability` with a vendor-neutral production contract rather than
binding core synchronization to a collector or monitoring product. Stable metric identities and
closed attribute keys prevent unbounded labels. Structured fields are classified and sanitized
before retention. Request, trace, correlation, and operation identities remain separate.

The bounded queue uses non-blocking admission, priority-aware eviction, finite batches, and
self-metrics. Export failure drops telemetry only; it cannot roll back Tx A, Tx B, Tx C, or hold an
authority transaction. `NoopMetrics` and disabled/local-only export modes preserve zero-telemetry
operation.

Sampling policy always retains critical security hints, can retain errors/slow traces, and rejects
sensitive remote traces. Monotonic timers measure latency. SLO eligibility distinguishes accepted
and infrastructure outcomes from validation, authorization, conflicts, and business decisions.
Stable alert definitions require owner, actionable condition, bounded grouping, and runbook.
`AequoraConfig` carries and validates `ObservabilityConfig`, so the same bounded production defaults
participate in strict RON parsing and every named deployment profile.

Reference assets under `deploy/observability` provide validated SLO/alert catalogs, five initial
dashboards, Prometheus-compatible rules, collector guidance, and runbooks. The operational CLI
provides bounded summary/metric-catalog output and explicitly falls back from expired traces to the
durable ledger, journal, audit, jobs, and authority metadata.

## Executable evidence

`aequora_observability::BoundedTelemetryQueue` and `SloCatalog` own the bounded queue and SLO
definitions. `observability_production_contracts` and `cargo xtask check observability` exercise
the production telemetry contracts. `ObservabilityFull` (83) binds tests 144–153 to
`AEQ-INV-OBS001`–`OBS010`.

## Remaining / Left & Environment Gates

**Repository and host integration.** The host selects and configures its collector and exporter,
metric retention, SLO values, and alert delivery. Checked-in catalog and dashboard assets provide
the integration contract.

**Environment.** Validate live telemetry delivery and redaction, capacity and failure isolation
under the destination's actual resource limits, plus alert routing and operator response.
