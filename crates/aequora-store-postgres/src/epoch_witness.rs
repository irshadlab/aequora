//! Epoch witness storage for a PostgreSQL control plane outside the authority restore boundary.

use crate::{postgres_error, to_i64};
use aequora_authority::{AuthorityError, ExternalEpochRegistry};
use aequora_store::StoreError;
use aequora_types::{AuthorityEpoch, AuthorityId};
use sqlx::PgPool;
use std::{
    sync::{Arc, RwLock},
    time::{Duration, Instant},
};

/// Creates the witness schema using the control-plane administrator's pool.
///
/// # Errors
/// Returns a storage error if schema installation cannot commit.
pub async fn install(pool: &PgPool) -> Result<(), StoreError> {
    let mut transaction = pool.begin().await.map_err(postgres_error)?;
    sqlx::query(
        "SELECT pg_advisory_xact_lock(hashtextextended('aequora-epoch-witness-schema-v1', 0))",
    )
    .execute(&mut *transaction)
    .await
    .map_err(postgres_error)?;
    sqlx::query("CREATE SCHEMA IF NOT EXISTS aequora_epoch_witness")
        .execute(&mut *transaction)
        .await
        .map_err(postgres_error)?;
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS aequora_epoch_witness.authority_epochs (
            authority_id uuid PRIMARY KEY CHECK (authority_id <> '00000000-0000-0000-0000-000000000000'),
            highest_epoch bigint NOT NULL CHECK (highest_epoch > 0)
        )",
    )
    .execute(&mut *transaction)
    .await
    .map_err(postgres_error)?;
    transaction.commit().await.map_err(postgres_error)
}

/// Records an approved epoch using a control-plane writer's pool with synchronous commit.
///
/// # Errors
/// Rejects unbound identities, invalid epochs, epoch regressions, and storage failures.
pub async fn record_epoch(
    pool: &PgPool,
    authority_id: AuthorityId,
    epoch: AuthorityEpoch,
) -> Result<(), StoreError> {
    validate_authority_id(authority_id)?;
    if epoch.get() == 0 {
        return Err(StoreError::permanent("invalid witness epoch"));
    }
    let epoch_value = to_i64(epoch.get(), "witness epoch")?;
    let mut transaction = pool.begin().await.map_err(postgres_error)?;
    sqlx::query("SET LOCAL synchronous_commit = 'on'")
        .execute(&mut *transaction)
        .await
        .map_err(postgres_error)?;
    let stored: i64 = sqlx::query_scalar(
        "INSERT INTO aequora_epoch_witness.authority_epochs AS witness (authority_id, highest_epoch)
         VALUES ($1, $2)
         ON CONFLICT (authority_id) DO UPDATE SET
             highest_epoch = GREATEST(witness.highest_epoch, EXCLUDED.highest_epoch)
         RETURNING highest_epoch",
    )
    .bind(authority_id.as_uuid())
    .bind(epoch_value)
    .fetch_one(&mut *transaction)
    .await
    .map_err(postgres_error)?;
    if stored != epoch_value {
        return Err(rollback(epoch, decode_epoch(stored)?));
    }
    transaction.commit().await.map_err(postgres_error)
}

/// Refreshes a local, expiring view from a dedicated witness reader pool.
/// The host owns the refresh schedule and database request deadline.
pub struct PostgresEpochWitness {
    pool: PgPool,
    view: Arc<EpochView>,
}

impl PostgresEpochWitness {
    /// Binds a read-only runtime view to one authority and a bounded observation age.
    ///
    /// # Errors
    /// Rejects an unbound authority ID or a zero observation lifetime.
    pub fn new(
        pool: PgPool,
        authority_id: AuthorityId,
        maximum_age: Duration,
    ) -> Result<Self, StoreError> {
        validate_authority_id(authority_id)?;
        if maximum_age.is_zero() {
            return Err(StoreError::permanent(
                "witness observation lifetime is zero",
            ));
        }
        Ok(Self {
            pool,
            view: Arc::new(EpochView {
                authority_id,
                maximum_age,
                observation: RwLock::new(Observation::default()),
            }),
        })
    }

    /// Returns the view used by the authority controller. Lookups never query PostgreSQL.
    #[must_use]
    pub fn registry(&self) -> Arc<dyn ExternalEpochRegistry> {
        self.view.clone()
    }

    /// Refreshes the current epoch and invalidates the view on a missing or failed read.
    ///
    /// # Errors
    /// Returns a storage error, or rejects an expired or regressing observation.
    pub async fn refresh(&mut self) -> Result<AuthorityEpoch, StoreError> {
        let started = Instant::now();
        let result = sqlx::query_scalar::<_, i64>(
            "SELECT highest_epoch FROM aequora_epoch_witness.authority_epochs
             WHERE authority_id = $1",
        )
        .bind(self.view.authority_id.as_uuid())
        .fetch_optional(&self.pool)
        .await
        .map_err(postgres_error)
        .and_then(|value| {
            value.map_or_else(
                || {
                    Err(StoreError::transient(
                        "authority has no recorded witness epoch",
                    ))
                },
                decode_epoch,
            )
        });
        let mut observation = self
            .view
            .observation
            .write()
            .map_err(|_| StoreError::permanent("witness observation state is unavailable"))?;
        observation.record(result, started, self.view.maximum_age)
    }
}

fn validate_authority_id(authority_id: AuthorityId) -> Result<(), StoreError> {
    if authority_id == AuthorityId::LEGACY_UNBOUND {
        return Err(StoreError::permanent(
            "witness requires a bound authority ID",
        ));
    }
    Ok(())
}

fn decode_epoch(value: i64) -> Result<AuthorityEpoch, StoreError> {
    u64::try_from(value)
        .ok()
        .and_then(|epoch| AuthorityEpoch::new(epoch).ok())
        .ok_or_else(|| StoreError::permanent("invalid stored witness epoch"))
}

fn rollback(presented: AuthorityEpoch, highest: AuthorityEpoch) -> StoreError {
    StoreError::permanent(
        AuthorityError::AuthorityRollbackDetected { presented, highest }.to_string(),
    )
}

#[derive(Default)]
struct Observation {
    highest: Option<AuthorityEpoch>,
    observed_at: Option<Instant>,
}

impl Observation {
    fn record(
        &mut self,
        result: Result<AuthorityEpoch, StoreError>,
        started: Instant,
        maximum_age: Duration,
    ) -> Result<AuthorityEpoch, StoreError> {
        self.observed_at = None;
        let epoch = result?;
        if let Some(highest) = self.highest {
            if epoch < highest {
                return Err(rollback(epoch, highest));
            }
        }
        self.highest = Some(epoch);
        if started.elapsed() >= maximum_age {
            return Err(StoreError::transient(
                "witness observation expired during refresh",
            ));
        }
        self.observed_at = Some(started);
        Ok(epoch)
    }

    fn current(&self, now: Instant, maximum_age: Duration) -> Option<AuthorityEpoch> {
        self.observed_at
            .filter(|observed| now.saturating_duration_since(*observed) < maximum_age)
            .and(self.highest)
    }
}

struct EpochView {
    authority_id: AuthorityId,
    maximum_age: Duration,
    observation: RwLock<Observation>,
}

impl ExternalEpochRegistry for EpochView {
    fn highest_epoch(&self, authority_id: AuthorityId) -> Option<AuthorityEpoch> {
        if authority_id != self.authority_id {
            return None;
        }
        self.observation
            .read()
            .ok()?
            .current(Instant::now(), self.maximum_age)
    }

    fn record_epoch(
        &self,
        _authority_id: AuthorityId,
        _epoch: AuthorityEpoch,
    ) -> Result<(), AuthorityError> {
        Err(AuthorityError::ExternalEpochUnavailable)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expired_or_failed_observations_cannot_reset_the_high_watermark() -> Result<(), StoreError> {
        let mut state = Observation::default();
        let next = AuthorityEpoch::new(2).unwrap_or_else(|error| panic!("{error}"));
        let start = Instant::now();
        let age = Duration::from_secs(60);
        state.record(Ok(next), start, age)?;
        assert_eq!(state.current(start, age), Some(next));
        assert_eq!(state.current(start + age, age), None);
        assert!(
            state
                .record(Err(StoreError::transient("offline")), start, age)
                .is_err()
        );
        assert!(
            state
                .record(Ok(AuthorityEpoch::INITIAL), start, age)
                .is_err()
        );
        assert_eq!(state.current(start, age), None);
        state.record(Ok(next), start, age)?;
        assert_eq!(state.current(start, age), Some(next));
        Ok(())
    }
}
