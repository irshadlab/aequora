//! Native, authority-fenced PostgreSQL persistence for durable work.

use aequora_authority::{
    AuthorityCommitContext,
    jobs::{
        AdminAction, ClaimRequest, ClaimedJob, DependencyCondition, EpochRecoveryAction,
        FencedUpdate, FencingToken, JobCheckpoint, JobDependency, JobDescriptor,
        JobExecutionRegionPolicy, JobLease, JobPayloadRef, JobProgress, JobRecord, JobRegistry,
        JobRunOutcome, JobState, JobStatus, JobStore, JobStoreError, NewJob, WaitCondition,
        WorkerId, epoch_recovery_action, validate_fenced_update,
    },
};
use aequora_store::{StoreError, StoreErrorKind, StoreErrorReason};
use aequora_types::{AuthorityEpoch, JobId, TenantId};
use async_trait::async_trait;
use sqlx::{PgPool, Postgres, Row, Transaction, postgres::PgRow};
use std::sync::Arc;

const MAX_RECORD_BYTES: usize = 256 * 1024;
const MAX_CALLBACK_BYTES: usize = 1_024;

pub(crate) const MIGRATION_0005: &str = r"
ALTER TABLE aequora_applied_operations ADD COLUMN publication_xid xid8;
ALTER TABLE aequora_applied_operations ALTER COLUMN publication_xid SET DEFAULT pg_current_xact_id();
CREATE TABLE aequora_runtime_jobs (
    job_id uuid PRIMARY KEY,
    tenant_id uuid NOT NULL,
    kind bigint NOT NULL CHECK (kind > 0),
    payload_version integer NOT NULL CHECK (payload_version BETWEEN 1 AND 65535),
    requires_external_idempotency boolean NOT NULL,
    priority smallint NOT NULL CHECK (priority BETWEEN 0 AND 5),
    region integer,
    capabilities bigint[] NOT NULL,
    state text NOT NULL CHECK (state IN ('pending','ready','running','waiting','retry_scheduled',
        'cancel_requested','completed','failed','canceled','quarantined')),
    due_ms bigint CHECK (due_ms >= 0),
    deadline_ms bigint CHECK (deadline_ms >= 0),
    fence bigint NOT NULL DEFAULT 0 CHECK (fence >= 0),
    worker_id uuid,
    lease_expires_ms bigint,
    wait_key text CHECK (octet_length(wait_key) <= 1024),
    reviewed_epoch bigint CHECK (reviewed_epoch > 0),
    retry_override boolean NOT NULL DEFAULT false,
    record bytea NOT NULL CHECK (octet_length(record) <= 262144),
    result bytea CHECK (octet_length(result) <= 262144),
    UNIQUE (tenant_id, job_id),
    CHECK ((worker_id IS NULL) = (lease_expires_ms IS NULL)),
    CHECK ((worker_id IS NOT NULL) = (state IN ('running','cancel_requested')))
);
CREATE INDEX aequora_runtime_jobs_due_idx
  ON aequora_runtime_jobs (kind, payload_version, priority DESC, due_ms, job_id)
  WHERE state IN ('pending','ready','waiting','retry_scheduled');
CREATE INDEX aequora_runtime_jobs_expired_idx
  ON aequora_runtime_jobs (kind, payload_version, lease_expires_ms) WHERE worker_id IS NOT NULL;
CREATE TABLE aequora_runtime_job_dependencies (
    tenant_id uuid NOT NULL,
    job_id uuid NOT NULL,
    dependency_id uuid NOT NULL,
    condition text NOT NULL CHECK (condition IN ('completed','completed_or_skipped','terminal')),
    PRIMARY KEY (job_id, dependency_id),
    FOREIGN KEY (tenant_id, job_id) REFERENCES aequora_runtime_jobs(tenant_id, job_id),
    FOREIGN KEY (tenant_id, dependency_id) REFERENCES aequora_runtime_jobs(tenant_id, job_id),
    CHECK (job_id <> dependency_id)
);
CREATE TABLE aequora_runtime_job_attempts (
    job_id uuid NOT NULL REFERENCES aequora_runtime_jobs(job_id),
    attempt bigint NOT NULL CHECK (attempt > 0),
    worker_id uuid NOT NULL,
    fence bigint NOT NULL CHECK (fence > 0),
    started_ms bigint NOT NULL,
    finished_ms bigint,
    outcome text,
    error_code bigint,
    PRIMARY KEY (job_id, attempt)
);
CREATE TABLE aequora_runtime_workflows (
    tenant_id uuid NOT NULL,
    workflow_id uuid NOT NULL,
    row_version bigint NOT NULL CHECK (row_version > 0),
    record bytea NOT NULL CHECK (octet_length(record) <= 524288),
    PRIMARY KEY (tenant_id, workflow_id)
);
CREATE TABLE aequora_runtime_workflow_transitions (
    tenant_id uuid NOT NULL,
    workflow_id uuid NOT NULL,
    row_version bigint NOT NULL CHECK (row_version > 1),
    event bytea NOT NULL CHECK (octet_length(event) <= 524288),
    actions bytea NOT NULL CHECK (octet_length(actions) <= 1048576),
    occurred_ms bigint NOT NULL CHECK (occurred_ms >= 0),
    PRIMARY KEY (tenant_id, workflow_id, row_version),
    FOREIGN KEY (tenant_id, workflow_id) REFERENCES aequora_runtime_workflows(tenant_id, workflow_id)
);
CREATE TABLE aequora_runtime_side_effect_intents (
    tenant_id uuid NOT NULL,
    intent_id uuid NOT NULL,
    operation_id uuid NOT NULL,
    key_digest bytea NOT NULL CHECK (octet_length(key_digest)=32),
    intent bytea NOT NULL CHECK (octet_length(intent) <= 131072),
    job_id uuid UNIQUE,
    execution bytea CHECK (octet_length(execution) <= 8192),
    PRIMARY KEY (tenant_id,intent_id),
    UNIQUE (tenant_id,key_digest),
    FOREIGN KEY (tenant_id,operation_id) REFERENCES aequora_applied_operations(tenant_id,operation_id)
        DEFERRABLE INITIALLY DEFERRED,
    FOREIGN KEY (tenant_id,job_id) REFERENCES aequora_runtime_jobs(tenant_id,job_id)
);
CREATE FUNCTION aequora_verify_effect_publication() RETURNS trigger
    LANGUAGE plpgsql SET search_path = pg_catalog, pg_temp AS $body$
DECLARE
    published boolean;
BEGIN
    EXECUTE format('SELECT EXISTS (SELECT 1 FROM %I.aequora_applied_operations
        WHERE tenant_id=$1 AND operation_id=$2 AND publication_xid=pg_current_xact_id())',
        TG_TABLE_SCHEMA) INTO published USING NEW.tenant_id, NEW.operation_id;
    IF NOT published THEN
        RAISE check_violation USING MESSAGE='side-effect intent must share its authority publication transaction';
    END IF;
    RETURN NEW;
END;
$body$;
CREATE CONSTRAINT TRIGGER aequora_effect_publication_tx
    AFTER INSERT ON aequora_runtime_side_effect_intents
    DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION aequora_verify_effect_publication();
CREATE TABLE aequora_runtime_job_admin_events (
    id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    job_id uuid NOT NULL REFERENCES aequora_runtime_jobs(job_id),
    action text NOT NULL,
    reason_code bigint NOT NULL,
    occurred_ms bigint NOT NULL
);
";

/// PostgreSQL job adapter bound to a serving authority and the host's job registry.
pub struct PostgresJobStore {
    pool: PgPool,
    authority: AuthorityCommitContext,
    registry: Arc<JobRegistry>,
}

impl PostgresJobStore {
    /// Composes the migrated authority pool with registered durable job contracts.
    #[must_use]
    pub const fn new(
        pool: PgPool,
        authority: AuthorityCommitContext,
        registry: Arc<JobRegistry>,
    ) -> Self {
        Self {
            pool,
            authority,
            registry,
        }
    }

    /// Inserts a job into a caller-owned authority transaction.
    ///
    /// # Errors
    /// Rejects invalid jobs, unknown descriptors, stale authority contexts, and storage failures.
    pub async fn insert_in_transaction(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        new: NewJob,
    ) -> Result<(), JobStoreError> {
        // Validate before writing. A savepoint keeps a failed dependency insertion from leaving
        // an incomplete job if a caller handles the error and commits its outer transaction.
        new.verify().map_err(JobStoreError::Invalid)?;
        let descriptor = self.descriptor(&new.job)?;
        if new.job.attempt_count != 0
            || new.job.checkpoint.is_some()
            || new.job.created_under_epoch != Some(self.authority.epoch)
        {
            return Err(JobStoreError::StateConflict);
        }
        crate::lock_authority_for_write(tx, self.authority)
            .await
            .map_err(store_error)?;
        let mut savepoint = sqlx::Acquire::begin(&mut *tx).await.map_err(db_error)?;
        insert_row(&mut savepoint, &new, descriptor).await?;
        for edge in &new.dependencies {
            insert_dependency(&mut savepoint, new.job.tenant_id, *edge).await?;
        }
        savepoint.commit().await.map_err(db_error)
    }

    pub(crate) async fn transaction(&self) -> Result<Transaction<'_, Postgres>, JobStoreError> {
        let mut tx = self.pool.begin().await.map_err(db_error)?;
        crate::lock_authority_for_write(&mut tx, self.authority)
            .await
            .map_err(store_error)?;
        Ok(tx)
    }

    pub(crate) async fn lock_authority(
        &self,
        tx: &mut Transaction<'_, Postgres>,
    ) -> Result<(), JobStoreError> {
        crate::lock_authority_for_write(tx, self.authority)
            .await
            .map_err(store_error)
    }

    pub(crate) const fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// Resumes a callback-waiting job after the host authenticates the callback.
    ///
    /// # Errors
    /// Rejects an unknown job, mismatched callback, or stale authority.
    pub async fn resume_callback(
        &self,
        tenant: TenantId,
        job: JobId,
        key: &str,
    ) -> Result<(), JobStoreError> {
        let mut tx = self.transaction().await?;
        let mut stored = load_job(&mut tx, job, Some(tenant)).await?;
        if stored.job.state != JobState::Waiting || stored.wait_key.as_deref() != Some(key) {
            return Err(JobStoreError::StateConflict);
        }
        let now = database_now(&mut tx).await?;
        stored.wait_key = None;
        stored.job.state = JobState::Ready;
        stored.job.next_run_at_unix_ms = Some(now);
        stored.job.updated_at_unix_ms = now.max(stored.job.updated_at_unix_ms);
        save(&mut tx, &stored).await?;
        tx.commit().await.map_err(db_error)
    }

    pub(crate) fn descriptor(&self, job: &JobRecord) -> Result<&JobDescriptor, JobStoreError> {
        self.registry
            .descriptor(job.kind)
            .filter(|descriptor| {
                descriptor
                    .payload_versions
                    .contains(&job.payload_schema_version)
            })
            .ok_or(JobStoreError::StateConflict)
    }

    async fn claim(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        mut stored: StoredJob,
        request: &ClaimRequest,
    ) -> Result<Option<ClaimedJob>, JobStoreError> {
        let now = database_now(tx).await?;
        let job = &mut stored.job;
        let descriptor = self.descriptor(job)?;
        let recovery = epoch_recovery_action(
            stored.reviewed_epoch.or(job.created_under_epoch),
            self.authority.epoch,
            job.epoch_policy,
        );
        // Neither a newer epoch nor a missing provider response proves an external effect failed.
        // Keep ambiguous work quarantined pending reviewed reconciliation.
        let had_lease = stored.lease.is_some();
        if had_lease {
            finish_attempt(tx, job, now, "lease_lost", None).await?;
        }
        let blocked_state = match recovery {
            EpochRecoveryAction::CancelDerived if !had_lease => Some(JobState::Canceled),
            EpochRecoveryAction::Continue => {
                if job.state == JobState::CancelRequested
                    || (had_lease
                        && (stored.requires_external_idempotency
                            || descriptor.requires_external_idempotency))
                {
                    Some(JobState::Quarantined)
                } else if job.deadline_unix_ms.is_some_and(|deadline| deadline <= now)
                    || (!stored.retry_override
                        && descriptor
                            .retry_policy
                            .max_attempts
                            .is_some_and(|max| job.attempt_count >= max))
                {
                    Some(JobState::Failed)
                } else {
                    None
                }
            }
            _ => Some(JobState::Quarantined),
        };
        if let Some(blocked_state) = blocked_state {
            job.state = blocked_state;
            job.updated_at_unix_ms = now.max(job.updated_at_unix_ms);
            job.next_run_at_unix_ms = None;
            stored.lease = None;
            save(tx, &stored).await?;
            return Ok(None);
        }

        let fence = stored
            .fence
            .checked_add(1)
            .ok_or(JobStoreError::StateConflict)?;
        let lease = JobLease {
            job_id: job.job_id,
            worker_id: request.worker_id,
            fencing_token: FencingToken::new(fence).map_err(JobStoreError::Invalid)?,
            expires_at_unix_ms: now
                .checked_add(request.lease_duration_ms)
                .ok_or(JobStoreError::StateConflict)?,
        };
        job.state = JobState::Running;
        job.attempt_count = job
            .attempt_count
            .checked_add(1)
            .ok_or(JobStoreError::StateConflict)?;
        job.next_run_at_unix_ms = None;
        job.updated_at_unix_ms = now.max(job.updated_at_unix_ms);
        sqlx::query("INSERT INTO aequora_runtime_job_attempts (job_id,attempt,worker_id,fence,started_ms) VALUES ($1,$2,$3,$4,$5)")
            .bind(job.job_id.as_uuid()).bind(i64::from(job.attempt_count))
            .bind(lease.worker_id.as_uuid()).bind(number(fence)?).bind(number(now)?)
            .execute(&mut **tx).await.map_err(db_error)?;
        stored.fence = fence;
        stored.retry_override = false;
        stored.lease = Some(lease);
        stored.wait_key = None;
        save(tx, &stored).await?;
        Ok(Some(ClaimedJob {
            job: stored.job,
            lease,
        }))
    }
}

#[async_trait]
impl JobStore for PostgresJobStore {
    async fn insert(&self, job: NewJob) -> Result<(), JobStoreError> {
        let mut tx = self.pool.begin().await.map_err(db_error)?;
        self.insert_in_transaction(&mut tx, job).await?;
        tx.commit().await.map_err(db_error)
    }

    async fn claim_ready(&self, request: ClaimRequest) -> Result<Vec<ClaimedJob>, JobStoreError> {
        request.verify().map_err(JobStoreError::Invalid)?;
        if request.worker_id.as_uuid().is_nil() {
            return Err(JobStoreError::StateConflict);
        }
        // Bound both allocated result bytes and the number of locked rows per poll.
        let limit = request.limit.min(64);
        let descriptors = request
            .supported_kinds
            .iter()
            .filter_map(|kind| {
                self.registry.descriptor(*kind).filter(|descriptor| {
                    descriptor
                        .required_capabilities
                        .is_subset(&request.capabilities)
                })
            })
            .flat_map(|descriptor| {
                descriptor
                    .payload_versions
                    .iter()
                    .map(|version| (i64::from(descriptor.kind.get()), i32::from(version.get())))
            });
        let (kinds, versions): (Vec<_>, Vec<_>) = descriptors.unzip();
        let capabilities = request
            .capabilities
            .iter()
            .map(|capability| i64::from(capability.get()))
            .collect::<Vec<_>>();
        let regions = request
            .allowed_regions
            .iter()
            .map(|region| i32::from(region.get()))
            .collect::<Vec<_>>();
        let mut tx = self.transaction().await?;
        let now = number(database_now(&mut tx).await?)?;
        let rows = sqlx::query(
            "SELECT job.*
             FROM aequora_runtime_jobs AS job
             WHERE (job.kind, job.payload_version) IN (SELECT * FROM unnest($1::bigint[], $6::integer[]))
               AND capabilities <@ $2::bigint[]
               AND (region IS NULL OR region = ANY($3))
               AND ((job.state IN ('running','cancel_requested') AND job.lease_expires_ms <= $4)
                 OR (job.state IN ('pending','ready','waiting','retry_scheduled')
                   AND job.due_ms <= $4 AND NOT EXISTS (
                     SELECT 1 FROM aequora_runtime_job_dependencies dep
                     JOIN aequora_runtime_jobs parent ON parent.job_id = dep.dependency_id
                     WHERE dep.job_id = job.job_id AND NOT (
                         parent.state = 'completed'
                         OR (dep.condition = 'terminal' AND parent.state IN ('failed','canceled','quarantined'))
                     )
                 )))
             ORDER BY job.priority DESC, COALESCE(job.due_ms,job.lease_expires_ms), job.job_id
             LIMIT $5 FOR NO KEY UPDATE OF job SKIP LOCKED",
        )
        .bind(&kinds).bind(&capabilities).bind(&regions).bind(now)
        .bind(i64::try_from(limit).map_err(|_| JobStoreError::StateConflict)?)
        .bind(&versions)
        .fetch_all(&mut *tx).await.map_err(db_error)?;
        let mut claims = Vec::with_capacity(rows.len());
        for row in &rows {
            if let Some(claim) = self.claim(&mut tx, stored_job(row)?, &request).await? {
                claims.push(claim);
            }
        }
        tx.commit().await.map_err(db_error)?;
        Ok(claims)
    }

    async fn renew_lease(
        &self,
        update: FencedUpdate,
        expires_at_unix_ms: u64,
    ) -> Result<JobLease, JobStoreError> {
        let mut tx = self.transaction().await?;
        let (mut stored, mut lease, now) = locked_lease(&mut tx, update).await?;
        if stored.job.state != JobState::Running || expires_at_unix_ms <= lease.expires_at_unix_ms {
            return Err(JobStoreError::StateConflict);
        }
        lease.expires_at_unix_ms = expires_at_unix_ms;
        stored.lease = Some(lease);
        stored.job.updated_at_unix_ms = now.max(stored.job.updated_at_unix_ms);
        save(&mut tx, &stored).await?;
        tx.commit().await.map_err(db_error)?;
        Ok(lease)
    }

    async fn checkpoint(
        &self,
        update: FencedUpdate,
        checkpoint: JobCheckpoint,
    ) -> Result<(), JobStoreError> {
        checkpoint.verify().map_err(JobStoreError::Invalid)?;
        let mut tx = self.transaction().await?;
        let (mut stored, _, now) = locked_lease(&mut tx, update).await?;
        if stored
            .job
            .checkpoint
            .as_ref()
            .is_some_and(|last| checkpoint.completed_units < last.completed_units)
        {
            return Err(JobStoreError::StateConflict);
        }
        stored.job.checkpoint = Some(checkpoint);
        stored.job.updated_at_unix_ms = now.max(stored.job.updated_at_unix_ms);
        save(&mut tx, &stored).await?;
        tx.commit().await.map_err(db_error)
    }

    async fn finish(
        &self,
        update: FencedUpdate,
        outcome: JobRunOutcome,
    ) -> Result<(), JobStoreError> {
        let mut tx = self.transaction().await?;
        let (mut stored, lease, now) = locked_lease(&mut tx, update).await?;
        let descriptor = self.descriptor(&stored.job)?;
        let (state, error_code) =
            apply_outcome(&mut tx, &mut stored, descriptor, outcome, now).await?;
        if !stored.job.state.can_transition_to(state) {
            return Err(JobStoreError::StateConflict);
        }
        let finished_at = database_now(&mut tx).await?;
        if !lease.is_current_at(finished_at) {
            return Err(JobStoreError::LeaseLost);
        }
        finish_attempt(
            &mut tx,
            &stored.job,
            finished_at,
            state_name(state),
            error_code,
        )
        .await?;
        stored.job.state = state;
        stored.job.updated_at_unix_ms = now.max(stored.job.updated_at_unix_ms);
        stored.lease = None;
        save(&mut tx, &stored).await?;
        tx.commit().await.map_err(db_error)
    }

    async fn request_cancel(
        &self,
        job_id: JobId,
        tenant_id: TenantId,
    ) -> Result<(), JobStoreError> {
        let mut tx = self.transaction().await?;
        let mut stored = load_job(&mut tx, job_id, Some(tenant_id)).await?;
        cancel(&mut stored)?;
        stored.job.updated_at_unix_ms = database_now(&mut tx)
            .await?
            .max(stored.job.updated_at_unix_ms);
        save(&mut tx, &stored).await?;
        tx.commit().await.map_err(db_error)
    }

    async fn admin_transition(
        &self,
        job_id: JobId,
        tenant_id: TenantId,
        action: AdminAction,
        reason_code: u32,
    ) -> Result<(), JobStoreError> {
        if reason_code == 0 {
            return Err(JobStoreError::StateConflict);
        }
        let mut tx = self.transaction().await?;
        let mut stored = load_job(&mut tx, job_id, Some(tenant_id)).await?;
        let now = database_now(&mut tx).await?;
        admin_transition(
            &mut tx,
            &mut stored,
            action,
            reason_code,
            now,
            self.authority.epoch,
        )
        .await?;
        tx.commit().await.map_err(db_error)
    }

    async fn status(&self, job_id: JobId, tenant_id: TenantId) -> Result<JobStatus, JobStoreError> {
        let row =
            sqlx::query("SELECT * FROM aequora_runtime_jobs WHERE job_id=$1 AND tenant_id=$2")
                .bind(job_id.as_uuid())
                .bind(tenant_id.as_uuid())
                .fetch_optional(&self.pool)
                .await
                .map_err(db_error)?
                .ok_or(JobStoreError::NotFound)?;
        let stored = stored_job(&row)?;
        let job = stored.job;
        Ok(JobStatus {
            job_id,
            tenant_id,
            kind: job.kind,
            state: job.state,
            attempt_count: job.attempt_count,
            progress: job.checkpoint.map(|checkpoint| JobProgress {
                completed_units: checkpoint.completed_units,
                total_units: checkpoint.total_units,
            }),
            next_run_at_unix_ms: job.next_run_at_unix_ms,
            result: stored.result,
        })
    }
}

pub(crate) struct StoredJob {
    pub(crate) job: JobRecord,
    fence: u64,
    lease: Option<JobLease>,
    requires_external_idempotency: bool,
    wait_key: Option<String>,
    reviewed_epoch: Option<AuthorityEpoch>,
    retry_override: bool,
    result: Option<JobPayloadRef>,
}

async fn insert_row(
    tx: &mut Transaction<'_, Postgres>,
    new: &NewJob,
    descriptor: &JobDescriptor,
) -> Result<(), JobStoreError> {
    let job = &new.job;
    let region = match job.region_policy {
        JobExecutionRegionPolicy::SpecificRegion(region) => Some(i32::from(region.get())),
        JobExecutionRegionPolicy::AuthorityOnly | JobExecutionRegionPolicy::AnyAllowedRegion => {
            None
        }
    };
    let capabilities = new
        .required_capabilities
        .union(&descriptor.required_capabilities)
        .map(|capability| i64::from(capability.get()))
        .collect::<Vec<_>>();
    let changed = sqlx::query(
        "INSERT INTO aequora_runtime_jobs
           (job_id,tenant_id,kind,priority,region,capabilities,state,due_ms,deadline_ms,record,
            payload_version,requires_external_idempotency)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12) ON CONFLICT (job_id) DO NOTHING",
    )
    .bind(job.job_id.as_uuid())
    .bind(job.tenant_id.as_uuid())
    .bind(i64::from(job.kind.get()))
    .bind(i16::from(job.priority.rank()))
    .bind(region)
    .bind(capabilities)
    .bind(state_name(job.state))
    .bind(due(job)?)
    .bind(job.deadline_unix_ms.map(number).transpose()?)
    .bind(encode_job(job)?)
    .bind(i32::from(job.payload_schema_version.get()))
    .bind(descriptor.requires_external_idempotency)
    .execute(&mut **tx)
    .await
    .map_err(db_error)?;
    if changed.rows_affected() == 0 {
        return Err(JobStoreError::Duplicate);
    }
    Ok(())
}

async fn insert_dependency(
    tx: &mut Transaction<'_, Postgres>,
    tenant: TenantId,
    edge: JobDependency,
) -> Result<(), JobStoreError> {
    let condition = match edge.condition {
        DependencyCondition::Completed => "completed",
        // The canonical job state enum has no skipped state.
        DependencyCondition::CompletedOrSkipped => "completed_or_skipped",
        DependencyCondition::Terminal => "terminal",
    };
    sqlx::query(
        "INSERT INTO aequora_runtime_job_dependencies (tenant_id,job_id,dependency_id,condition)
         VALUES ($1,$2,$3,$4)
         ON CONFLICT (job_id,dependency_id) DO UPDATE SET condition=EXCLUDED.condition",
    )
    .bind(tenant.as_uuid())
    .bind(edge.job_id.as_uuid())
    .bind(edge.depends_on_job_id.as_uuid())
    .bind(condition)
    .execute(&mut **tx)
    .await
    .map_err(db_error)?;
    Ok(())
}

async fn wait_for_job(
    tx: &mut Transaction<'_, Postgres>,
    job: &JobRecord,
    dependency: JobId,
) -> Result<(), JobStoreError> {
    // Competing edges in one tenant cannot both pass the cycle check on stale graph snapshots.
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind(format!("aequora-job-wait:{}", job.tenant_id))
        .execute(&mut **tx)
        .await
        .map_err(db_error)?;
    let cycle: bool = sqlx::query_scalar(
        "WITH RECURSIVE reachable(job_id) AS (
            SELECT $1::uuid
            UNION
            SELECT edge.dependency_id FROM aequora_runtime_job_dependencies edge
            JOIN reachable ON reachable.job_id=edge.job_id WHERE edge.tenant_id=$2
         )
         SELECT EXISTS(SELECT 1 FROM reachable WHERE job_id=$3)",
    )
    .bind(dependency.as_uuid())
    .bind(job.tenant_id.as_uuid())
    .bind(job.job_id.as_uuid())
    .fetch_one(&mut **tx)
    .await
    .map_err(db_error)?;
    if cycle {
        return Err(JobStoreError::Invalid(
            aequora_authority::jobs::JobError::DependencyCycle,
        ));
    }
    let limit_exceeded: bool = sqlx::query_scalar(
        "SELECT COUNT(*) >= $3 AND NOT COALESCE(bool_or(dependency_id=$2),false)
         FROM aequora_runtime_job_dependencies WHERE job_id=$1",
    )
    .bind(job.job_id.as_uuid())
    .bind(dependency.as_uuid())
    .bind(i64::try_from(aequora_authority::jobs::MAX_DEPENDENCIES).unwrap_or(i64::MAX))
    .fetch_one(&mut **tx)
    .await
    .map_err(db_error)?;
    if limit_exceeded {
        return Err(JobStoreError::Invalid(
            aequora_authority::jobs::JobError::DependencyLimit,
        ));
    }
    insert_dependency(
        tx,
        job.tenant_id,
        JobDependency {
            job_id: job.job_id,
            depends_on_job_id: dependency,
            condition: DependencyCondition::Completed,
        },
    )
    .await
}

async fn apply_outcome(
    tx: &mut Transaction<'_, Postgres>,
    stored: &mut StoredJob,
    descriptor: &JobDescriptor,
    outcome: JobRunOutcome,
    now: u64,
) -> Result<(JobState, Option<u32>), JobStoreError> {
    let job = &mut stored.job;
    job.next_run_at_unix_ms = None;
    stored.wait_key = None;
    match outcome {
        JobRunOutcome::Completed { result } => {
            if let Some(result) = &result {
                result.verify().map_err(JobStoreError::Invalid)?;
            }
            stored.result = result;
            Ok((JobState::Completed, None))
        }
        JobRunOutcome::Retryable {
            error,
            retry_after_ms,
        } => {
            if !descriptor.retry_policy.permits(job.attempt_count, error) {
                return Ok((JobState::Failed, None));
            }
            let digest = blake3::hash(job.job_id.as_uuid().as_bytes());
            let mut entropy = [0; 8];
            entropy.copy_from_slice(&digest.as_bytes()[..8]);
            let delay = descriptor
                .retry_policy
                .delay_ms(job.attempt_count, u64::from_le_bytes(entropy))
                .map_err(JobStoreError::Invalid)?;
            let due = now
                .checked_add(delay.max(retry_after_ms.unwrap_or_default()))
                .ok_or(JobStoreError::StateConflict)?;
            if job.deadline_unix_ms.is_some_and(|deadline| due >= deadline) {
                return Ok((JobState::Failed, None));
            }
            job.next_run_at_unix_ms = Some(due);
            Ok((JobState::RetryScheduled, None))
        }
        JobRunOutcome::TerminalFailure { error_code } => Ok((JobState::Failed, Some(error_code))),
        JobRunOutcome::Quarantined { reason_code } => {
            Ok((JobState::Quarantined, Some(reason_code)))
        }
        JobRunOutcome::Waiting(condition) => {
            match condition {
                WaitCondition::Until(timestamp) => job.next_run_at_unix_ms = Some(timestamp),
                WaitCondition::Job(dependency) => {
                    wait_for_job(tx, job, dependency).await?;
                    job.next_run_at_unix_ms = Some(now);
                }
                WaitCondition::ExternalCallback(key) => {
                    if key.is_empty() || key.len() > MAX_CALLBACK_BYTES || key.contains('\0') {
                        return Err(JobStoreError::StateConflict);
                    }
                    stored.wait_key = Some(key);
                }
                WaitCondition::ManualApproval => {}
            }
            Ok((JobState::Waiting, None))
        }
    }
}

fn cancel(stored: &mut StoredJob) -> Result<(), JobStoreError> {
    let job = &mut stored.job;
    match job.state {
        JobState::Running => job.state = JobState::CancelRequested,
        JobState::CancelRequested | JobState::Canceled => {}
        state if !state.is_terminal() => {
            job.state = JobState::Canceled;
            job.next_run_at_unix_ms = None;
            stored.wait_key = None;
        }
        _ => return Err(JobStoreError::StateConflict),
    }
    Ok(())
}

async fn admin_transition(
    tx: &mut Transaction<'_, Postgres>,
    stored: &mut StoredJob,
    action: AdminAction,
    reason_code: u32,
    now: u64,
    authority_epoch: AuthorityEpoch,
) -> Result<(), JobStoreError> {
    let action_name = match action {
        AdminAction::Cancel => {
            cancel(stored)?;
            "cancel"
        }
        AdminAction::Retry if stored.job.state == JobState::Failed => {
            stored.job.state = JobState::Ready;
            stored.job.next_run_at_unix_ms = Some(now);
            stored.retry_override = true;
            "retry"
        }
        AdminAction::Quarantine if !stored.job.state.is_terminal() => {
            if stored.lease.is_some() {
                finish_attempt(tx, &stored.job, now, "quarantined", Some(reason_code)).await?;
            }
            stored.lease = None;
            stored.wait_key = None;
            stored.job.state = JobState::Quarantined;
            stored.job.next_run_at_unix_ms = None;
            "quarantine"
        }
        AdminAction::Release
            if stored.job.state == JobState::Quarantined
                || (stored.job.state == JobState::Waiting
                    && stored.wait_key.is_none()
                    && stored.job.next_run_at_unix_ms.is_none()) =>
        {
            if stored.job.state == JobState::Quarantined {
                stored.reviewed_epoch = Some(authority_epoch);
                stored.retry_override = true;
            }
            stored.job.state = JobState::Ready;
            stored.job.next_run_at_unix_ms = Some(now);
            "release"
        }
        _ => return Err(JobStoreError::StateConflict),
    };
    stored.job.updated_at_unix_ms = now.max(stored.job.updated_at_unix_ms);
    save(tx, stored).await?;
    sqlx::query(
        "INSERT INTO aequora_runtime_job_admin_events(job_id,action,reason_code,occurred_ms)
         VALUES ($1,$2,$3,$4)",
    )
    .bind(stored.job.job_id.as_uuid())
    .bind(action_name)
    .bind(i64::from(reason_code))
    .bind(number(now)?)
    .execute(&mut **tx)
    .await
    .map_err(db_error)?;
    Ok(())
}

pub(crate) async fn locked_lease(
    tx: &mut Transaction<'_, Postgres>,
    mut update: FencedUpdate,
) -> Result<(StoredJob, JobLease, u64), JobStoreError> {
    let stored = load_job(tx, update.job_id, None).await?;
    let lease = stored.lease.ok_or(JobStoreError::LeaseLost)?;
    // The worker's request timestamp can precede a row-lock wait or come from a skewed host.
    let now = database_now(tx).await?;
    update.now_unix_ms = now;
    validate_fenced_update(&stored.job, lease, update)?;
    Ok((stored, lease, now))
}

async fn load_job(
    tx: &mut Transaction<'_, Postgres>,
    job: JobId,
    tenant: Option<TenantId>,
) -> Result<StoredJob, JobStoreError> {
    let row = sqlx::query(
        "SELECT * FROM aequora_runtime_jobs
         WHERE job_id=$1 AND ($2::uuid IS NULL OR tenant_id=$2) FOR NO KEY UPDATE",
    )
    .bind(job.as_uuid())
    .bind(tenant.map(TenantId::as_uuid))
    .fetch_optional(&mut **tx)
    .await
    .map_err(db_error)?
    .ok_or(JobStoreError::NotFound)?;
    stored_job(&row)
}

pub(crate) fn stored_job(row: &PgRow) -> Result<StoredJob, JobStoreError> {
    let bytes: Vec<u8> = row.try_get("record").map_err(db_error)?;
    let job: JobRecord = postcard::from_bytes(&bytes).map_err(|error| encoding_error(&error))?;
    job.verify().map_err(JobStoreError::Invalid)?;
    if row.try_get::<uuid::Uuid, _>("job_id").map_err(db_error)? != job.job_id.as_uuid()
        || row
            .try_get::<uuid::Uuid, _>("tenant_id")
            .map_err(db_error)?
            != job.tenant_id.as_uuid()
        || row.try_get::<&str, _>("state").map_err(db_error)? != state_name(job.state)
        || row.try_get::<i64, _>("kind").map_err(db_error)? != i64::from(job.kind.get())
        || row.try_get::<i32, _>("payload_version").map_err(db_error)?
            != i32::from(job.payload_schema_version.get())
    {
        return Err(JobStoreError::StateConflict);
    }
    let fence = unsigned(row.try_get("fence").map_err(db_error)?)?;
    let worker_id: Option<uuid::Uuid> = row.try_get("worker_id").map_err(db_error)?;
    let expiry: Option<i64> = row.try_get("lease_expires_ms").map_err(db_error)?;
    let lease = match (worker_id, expiry) {
        (Some(worker), Some(expiry)) => Some(JobLease {
            job_id: job.job_id,
            worker_id: WorkerId::from_uuid(worker),
            fencing_token: FencingToken::new(fence).map_err(JobStoreError::Invalid)?,
            expires_at_unix_ms: unsigned(expiry)?,
        }),
        (None, None) => None,
        _ => return Err(JobStoreError::StateConflict),
    };
    let result: Option<Vec<u8>> = row.try_get("result").map_err(db_error)?;
    Ok(StoredJob {
        job,
        fence,
        lease,
        requires_external_idempotency: row
            .try_get("requires_external_idempotency")
            .map_err(db_error)?,
        wait_key: row.try_get("wait_key").map_err(db_error)?,
        reviewed_epoch: row
            .try_get::<Option<i64>, _>("reviewed_epoch")
            .map_err(db_error)?
            .map(|value| {
                AuthorityEpoch::new(unsigned(value)?).map_err(|_| JobStoreError::StateConflict)
            })
            .transpose()?,
        retry_override: row.try_get("retry_override").map_err(db_error)?,
        result: result
            .as_deref()
            .map(postcard::from_bytes)
            .transpose()
            .map_err(|error| encoding_error(&error))?,
    })
}

async fn save(tx: &mut Transaction<'_, Postgres>, stored: &StoredJob) -> Result<(), JobStoreError> {
    sqlx::query(
        "UPDATE aequora_runtime_jobs SET state=$2,due_ms=$3,fence=$4,worker_id=$5,
                lease_expires_ms=$6,wait_key=$7,record=$8,result=$9,reviewed_epoch=$10,retry_override=$11 WHERE job_id=$1",
    )
    .bind(stored.job.job_id.as_uuid())
    .bind(state_name(stored.job.state))
    .bind(due(&stored.job)?)
    .bind(number(stored.fence)?)
    .bind(stored.lease.map(|lease| lease.worker_id.as_uuid()))
    .bind(
        stored
            .lease
            .map(|lease| number(lease.expires_at_unix_ms))
            .transpose()?,
    )
    .bind(&stored.wait_key)
    .bind(encode_job(&stored.job)?)
    .bind(
        stored
            .result
            .as_ref()
            .map(postcard::to_allocvec)
            .transpose()
            .map_err(|error| encoding_error(&error))?,
    )
    .bind(stored.reviewed_epoch.map(|epoch| number(epoch.get())).transpose()?)
    .bind(stored.retry_override)
    .execute(&mut **tx)
    .await
    .map_err(db_error)?;
    Ok(())
}

async fn finish_attempt(
    tx: &mut Transaction<'_, Postgres>,
    job: &JobRecord,
    now: u64,
    outcome: &str,
    error_code: Option<u32>,
) -> Result<(), JobStoreError> {
    let changed = sqlx::query(
        "UPDATE aequora_runtime_job_attempts SET finished_ms=$3,outcome=$4,error_code=$5
         WHERE job_id=$1 AND attempt=$2 AND finished_ms IS NULL",
    )
    .bind(job.job_id.as_uuid())
    .bind(i64::from(job.attempt_count))
    .bind(number(now)?)
    .bind(outcome)
    .bind(error_code.map(i64::from))
    .execute(&mut **tx)
    .await
    .map_err(db_error)?;
    if changed.rows_affected() != 1 {
        return Err(JobStoreError::StateConflict);
    }
    Ok(())
}

fn encode_job(job: &JobRecord) -> Result<Vec<u8>, JobStoreError> {
    let bytes = postcard::to_allocvec(job).map_err(|error| encoding_error(&error))?;
    if bytes.len() > MAX_RECORD_BYTES {
        return Err(JobStoreError::StateConflict);
    }
    Ok(bytes)
}

fn due(job: &JobRecord) -> Result<Option<i64>, JobStoreError> {
    let timestamp = if matches!(job.state, JobState::Pending | JobState::Ready) {
        Some(job.next_run_at_unix_ms.unwrap_or_default())
    } else {
        job.next_run_at_unix_ms
    };
    timestamp.map(number).transpose()
}

pub(crate) async fn database_now(tx: &mut Transaction<'_, Postgres>) -> Result<u64, JobStoreError> {
    let value =
        sqlx::query_scalar("SELECT floor(EXTRACT(EPOCH FROM clock_timestamp()) * 1000)::bigint")
            .fetch_one(&mut **tx)
            .await
            .map_err(db_error)?;
    unsigned(value)
}

fn state_name(state: JobState) -> &'static str {
    match state {
        JobState::Pending => "pending",
        JobState::Ready => "ready",
        JobState::Running => "running",
        JobState::Waiting => "waiting",
        JobState::RetryScheduled => "retry_scheduled",
        JobState::CancelRequested => "cancel_requested",
        JobState::Completed => "completed",
        JobState::Failed => "failed",
        JobState::Canceled => "canceled",
        JobState::Quarantined => "quarantined",
    }
}

fn number(value: u64) -> Result<i64, JobStoreError> {
    i64::try_from(value).map_err(|_| JobStoreError::StateConflict)
}

fn unsigned(value: i64) -> Result<u64, JobStoreError> {
    u64::try_from(value).map_err(|_| JobStoreError::StateConflict)
}

fn encoding_error(error: &postcard::Error) -> JobStoreError {
    JobStoreError::Adapter(format!("invalid durable job record: {error}"))
}

fn db_error(error: sqlx::Error) -> JobStoreError {
    store_error(crate::postgres_error(error))
}

fn store_error(error: StoreError) -> JobStoreError {
    match (error.kind, error.reason) {
        (_, StoreErrorReason::LeadershipLost) => JobStoreError::StaleFence,
        (StoreErrorKind::Transient, _) => JobStoreError::Unavailable,
        _ => JobStoreError::Adapter(error.message),
    }
}
