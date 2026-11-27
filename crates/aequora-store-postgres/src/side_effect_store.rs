//! Typed side-effect intents tied to Tx B, with durable job projection and fenced outcomes.

use aequora_authority::{
    jobs::{FencedUpdate, JobRecord, JobState, JobStoreError, NewJob},
    side_effects::{
        ExternalOutcomeKind, SideEffectExecution, SideEffectExecutionState, SideEffectIntent,
        SideEffectIntentId, SideEffectIntentWrite, SideEffectStore, SideEffectStoreError,
    },
};
use aequora_store::StoreErrorKind;
use aequora_types::{JobId, LineageRef, TenantId};
use async_trait::async_trait;
use sqlx::{Postgres, Row, Transaction, postgres::PgRow};
use std::sync::Arc;

use crate::job_store::{PostgresJobStore, locked_lease, stored_job};

/// Uses an existing authority transaction to stage typed side-effect intents.
pub struct PostgresSideEffectWriter<'a, 'c> {
    jobs: &'a PostgresJobStore,
    tx: &'a mut Transaction<'c, Postgres>,
}

impl<'a, 'c> PostgresSideEffectWriter<'a, 'c> {
    #[must_use]
    pub const fn new(jobs: &'a PostgresJobStore, tx: &'a mut Transaction<'c, Postgres>) -> Self {
        Self { jobs, tx }
    }
}

#[async_trait]
impl SideEffectIntentWrite for PostgresSideEffectWriter<'_, '_> {
    async fn write_intent(&mut self, intent: SideEffectIntent) -> Result<(), SideEffectStoreError> {
        intent.verify().map_err(SideEffectStoreError::Invalid)?;
        // Transparent IDs can come from deserialization, so do not rely on constructors alone.
        if intent.intent_id.as_uuid().is_nil()
            || intent.kind.get() == 0
            || intent.idempotency_key.as_str().is_empty()
            || intent.idempotency_key.as_str().len()
                > aequora_authority::side_effects::MAX_IDEMPOTENCY_KEY_BYTES
        {
            return Err(SideEffectStoreError::IntentConflict);
        }
        self.jobs.lock_authority(self.tx).await.map_err(job_error)?;
        let encoded = postcard::to_allocvec(&intent).map_err(|error| encoding_error(&error))?;
        let key = blake3::hash(intent.idempotency_key.as_str().as_bytes());
        let affected = sqlx::query(
            "INSERT INTO aequora_runtime_side_effect_intents
             (tenant_id,intent_id,operation_id,key_digest,intent) VALUES($1,$2,$3,$4,$5)
             ON CONFLICT DO NOTHING",
        )
        .bind(intent.tenant_id.as_uuid())
        .bind(intent.intent_id.as_uuid())
        .bind(intent.operation_id.as_uuid())
        .bind(key.as_bytes().as_slice())
        .bind(&encoded)
        .execute(&mut **self.tx)
        .await
        .map_err(db_error)?;
        if affected.rows_affected() == 1 {
            return Ok(());
        }
        let existing: Option<Vec<u8>> = sqlx::query_scalar(
            "SELECT intent FROM aequora_runtime_side_effect_intents WHERE tenant_id=$1 AND intent_id=$2",
        ).bind(intent.tenant_id.as_uuid()).bind(intent.intent_id.as_uuid())
            .fetch_optional(&mut **self.tx).await.map_err(db_error)?;
        if existing.as_ref() != Some(&encoded) {
            return Err(SideEffectStoreError::IntentConflict);
        }
        Ok(())
    }
}

/// PostgreSQL store for typed intents and their projected worker executions.
pub struct PostgresSideEffectStore {
    jobs: Arc<PostgresJobStore>,
    worker: Option<FencedUpdate>,
}

impl PostgresSideEffectStore {
    #[must_use]
    pub const fn new(jobs: Arc<PostgresJobStore>) -> Self {
        Self { jobs, worker: None }
    }

    /// Binds outcome writes to the exact worker lease and state used for execution.
    #[must_use]
    pub const fn with_worker_lease(mut self, worker: FencedUpdate) -> Self {
        self.worker = Some(worker);
        self
    }

    /// Loads the persisted provider state for a tenant's intent.
    ///
    /// # Errors
    /// Rejects missing intents, invalid stored state, and storage failures.
    pub async fn execution(
        &self,
        tenant: TenantId,
        intent: SideEffectIntentId,
    ) -> Result<Option<SideEffectExecution>, SideEffectStoreError> {
        let row = sqlx::query(
            "SELECT job_id,execution FROM aequora_runtime_side_effect_intents
             WHERE tenant_id=$1 AND intent_id=$2",
        )
        .bind(tenant.as_uuid())
        .bind(intent.as_uuid())
        .fetch_optional(self.jobs.pool())
        .await
        .map_err(db_error)?
        .ok_or(SideEffectStoreError::NotFound)?;
        decode_execution(&row, intent)
    }

    /// Creates the durable job and projects it once in the same transaction.
    ///
    /// # Errors
    /// Rejects a mismatched intent, conflicting projection, or failed fenced job insert.
    pub async fn materialize_job(
        &self,
        intent_id: SideEffectIntentId,
        tenant_id: TenantId,
        job: NewJob,
    ) -> Result<bool, SideEffectStoreError> {
        let mut tx = self.jobs.transaction().await.map_err(job_error)?;
        let row = locked_intent(&mut tx, intent_id, tenant_id).await?;
        let intent = decode_intent(&row, intent_id, tenant_id)?;
        self.verify_job(&intent, &job.job)?;
        if let Some(existing) = projected_job(&row)? {
            tx.commit().await.map_err(db_error)?;
            return same_projection(existing, job.job.job_id);
        }
        let job_id = job.job.job_id;
        self.jobs
            .insert_in_transaction(&mut tx, job)
            .await
            .map_err(job_error)?;
        bind_job(&mut tx, intent_id, tenant_id, job_id).await?;
        tx.commit().await.map_err(db_error)?;
        Ok(true)
    }

    fn verify_job(
        &self,
        intent: &SideEffectIntent,
        job: &JobRecord,
    ) -> Result<(), SideEffectStoreError> {
        if intent.tenant_id != job.tenant_id
            || intent.correlation_id != job.lineage.correlation_id
            || job.lineage.caused_by != Some(LineageRef::Operation(intent.operation_id))
            || !self
                .jobs
                .descriptor(job)
                .map_err(job_error)?
                .requires_external_idempotency
        {
            return Err(SideEffectStoreError::IntentConflict);
        }
        Ok(())
    }
}

#[async_trait]
impl SideEffectStore for PostgresSideEffectStore {
    async fn load_intent(
        &self,
        intent_id: SideEffectIntentId,
        tenant_id: TenantId,
    ) -> Result<SideEffectIntent, SideEffectStoreError> {
        let row = sqlx::query(
            "SELECT intent FROM aequora_runtime_side_effect_intents WHERE tenant_id=$1 AND intent_id=$2",
        ).bind(tenant_id.as_uuid()).bind(intent_id.as_uuid()).fetch_optional(self.jobs.pool())
            .await.map_err(db_error)?.ok_or(SideEffectStoreError::NotFound)?;
        decode_intent(&row, intent_id, tenant_id)
    }

    async fn project_job_once(
        &self,
        intent_id: SideEffectIntentId,
        tenant_id: TenantId,
        job_id: JobId,
    ) -> Result<bool, SideEffectStoreError> {
        let mut tx = self.jobs.transaction().await.map_err(job_error)?;
        let row = locked_intent(&mut tx, intent_id, tenant_id).await?;
        if let Some(existing) = projected_job(&row)? {
            tx.commit().await.map_err(db_error)?;
            return same_projection(existing, job_id);
        }
        let job = sqlx::query(
            "SELECT * FROM aequora_runtime_jobs WHERE tenant_id=$1 AND job_id=$2 FOR NO KEY UPDATE",
        )
        .bind(tenant_id.as_uuid())
        .bind(job_id.as_uuid())
        .fetch_optional(&mut *tx)
        .await
        .map_err(db_error)?
        .ok_or(SideEffectStoreError::NotFound)?;
        let job = stored_job(&job).map_err(job_error)?.job;
        if job.attempt_count != 0 || !matches!(job.state, JobState::Pending | JobState::Ready) {
            return Err(SideEffectStoreError::StateConflict);
        }
        let intent = decode_intent(&row, intent_id, tenant_id)?;
        self.verify_job(&intent, &job)?;
        bind_job(&mut tx, intent_id, tenant_id, job_id).await?;
        tx.commit().await.map_err(db_error)?;
        Ok(true)
    }

    async fn compare_and_set_execution(
        &self,
        tenant_id: TenantId,
        expected: SideEffectExecutionState,
        mut execution: SideEffectExecution,
    ) -> Result<(), SideEffectStoreError> {
        execution.verify().map_err(SideEffectStoreError::Invalid)?;
        verify_outcome(expected, &execution)?;
        let worker = self
            .worker
            .filter(|worker| {
                worker.job_id == execution.job_id && worker.expected_state == JobState::Running
            })
            .ok_or(SideEffectStoreError::StateConflict)?;
        let mut tx = self.jobs.transaction().await.map_err(job_error)?;
        let row = locked_intent(&mut tx, execution.intent_id, tenant_id).await?;
        if projected_job(&row)? != Some(execution.job_id) {
            return Err(SideEffectStoreError::StateConflict);
        }
        let previous = decode_execution(&row, execution.intent_id)?;
        if previous
            .as_ref()
            .map_or(SideEffectExecutionState::Pending, |value| value.state)
            != expected
            || previous
                .as_ref()
                .is_some_and(|value| value.provider != execution.provider)
        {
            return Err(SideEffectStoreError::StateConflict);
        }
        // Acquire locks in the same order as projection and check time after both lock waits.
        let (job, _, now) = locked_lease(&mut tx, worker).await.map_err(job_error)?;
        if job.job.tenant_id != tenant_id {
            return Err(SideEffectStoreError::NotFound);
        }
        execution.updated_at_unix_ms = now;
        sqlx::query(
            "UPDATE aequora_runtime_side_effect_intents SET execution=$3 WHERE tenant_id=$1 AND intent_id=$2",
        ).bind(tenant_id.as_uuid()).bind(execution.intent_id.as_uuid())
            .bind(postcard::to_allocvec(&execution).map_err(|error| encoding_error(&error))?)
            .execute(&mut *tx).await.map_err(db_error)?;
        tx.commit().await.map_err(db_error)
    }
}

fn verify_outcome(
    expected: SideEffectExecutionState,
    execution: &SideEffectExecution,
) -> Result<(), SideEffectStoreError> {
    use SideEffectExecutionState::{
        Ambiguous, Confirmed, Executing, Failed, ManualReview, Pending, Retryable,
    };
    if expected == execution.state || !expected.can_transition_to(execution.state) {
        return Err(SideEffectStoreError::StateConflict);
    }
    let evidence = match (expected, execution.state) {
        (Pending | Retryable, Executing) => None,
        (_, Confirmed) => Some(ExternalOutcomeKind::ConfirmedSuccess),
        (_, Failed) => Some(ExternalOutcomeKind::ConfirmedFailure),
        (_, Retryable) => Some(ExternalOutcomeKind::RetryableFailure),
        (_, Ambiguous | ManualReview) => Some(ExternalOutcomeKind::Ambiguous),
        // An uncertain provider response requires reconciliation, not another execution claim.
        _ => return Err(SideEffectStoreError::StateConflict),
    };
    if execution.last_outcome.as_ref().map(|outcome| outcome.kind) != evidence {
        return Err(SideEffectStoreError::StateConflict);
    }
    Ok(())
}

async fn locked_intent(
    tx: &mut Transaction<'_, Postgres>,
    intent: SideEffectIntentId,
    tenant: TenantId,
) -> Result<PgRow, SideEffectStoreError> {
    sqlx::query(
        "SELECT intent,job_id,execution FROM aequora_runtime_side_effect_intents
         WHERE tenant_id=$1 AND intent_id=$2 FOR NO KEY UPDATE",
    )
    .bind(tenant.as_uuid())
    .bind(intent.as_uuid())
    .fetch_optional(&mut **tx)
    .await
    .map_err(db_error)?
    .ok_or(SideEffectStoreError::NotFound)
}

fn decode_intent(
    row: &PgRow,
    id: SideEffectIntentId,
    tenant: TenantId,
) -> Result<SideEffectIntent, SideEffectStoreError> {
    let intent: SideEffectIntent = postcard::from_bytes(row.try_get("intent").map_err(db_error)?)
        .map_err(|error| encoding_error(&error))?;
    intent.verify().map_err(SideEffectStoreError::Invalid)?;
    if intent.intent_id != id || intent.tenant_id != tenant {
        return Err(SideEffectStoreError::IntentConflict);
    }
    Ok(intent)
}

fn projected_job(row: &PgRow) -> Result<Option<JobId>, SideEffectStoreError> {
    let id: Option<uuid::Uuid> = row.try_get("job_id").map_err(db_error)?;
    Ok(id.map(JobId::from_uuid))
}

fn decode_execution(
    row: &PgRow,
    intent: SideEffectIntentId,
) -> Result<Option<SideEffectExecution>, SideEffectStoreError> {
    let bytes: Option<&[u8]> = row.try_get("execution").map_err(db_error)?;
    let Some(bytes) = bytes else {
        return Ok(None);
    };
    let execution: SideEffectExecution =
        postcard::from_bytes(bytes).map_err(|error| encoding_error(&error))?;
    execution.verify().map_err(SideEffectStoreError::Invalid)?;
    if execution.intent_id != intent || projected_job(row)? != Some(execution.job_id) {
        return Err(SideEffectStoreError::StateConflict);
    }
    Ok(Some(execution))
}

fn same_projection(existing: JobId, requested: JobId) -> Result<bool, SideEffectStoreError> {
    if existing == requested {
        Ok(false)
    } else {
        Err(SideEffectStoreError::IntentConflict)
    }
}

async fn bind_job(
    tx: &mut Transaction<'_, Postgres>,
    intent: SideEffectIntentId,
    tenant: TenantId,
    job: JobId,
) -> Result<(), SideEffectStoreError> {
    sqlx::query(
        "UPDATE aequora_runtime_side_effect_intents SET job_id=$3 WHERE tenant_id=$1 AND intent_id=$2",
    ).bind(tenant.as_uuid()).bind(intent.as_uuid()).bind(job.as_uuid())
        .execute(&mut **tx).await.map_err(db_error)?;
    Ok(())
}

fn job_error(error: JobStoreError) -> SideEffectStoreError {
    match error {
        JobStoreError::Unavailable => SideEffectStoreError::Unavailable,
        JobStoreError::LeaseLost | JobStoreError::StaleFence | JobStoreError::StateConflict => {
            SideEffectStoreError::StateConflict
        }
        other => SideEffectStoreError::Adapter(other.to_string()),
    }
}

fn db_error(error: sqlx::Error) -> SideEffectStoreError {
    let error = crate::postgres_error(error);
    match error.kind {
        StoreErrorKind::Transient => SideEffectStoreError::Unavailable,
        StoreErrorKind::Permanent => SideEffectStoreError::Adapter(error.message),
    }
}

fn encoding_error(error: &postcard::Error) -> SideEffectStoreError {
    SideEffectStoreError::Adapter(format!("invalid durable side-effect record: {error}"))
}
