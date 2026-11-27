//! PostgreSQL workflow state, transition history, and atomic child job scheduling.

use aequora_authority::{
    jobs::{JobPayloadRef, JobStoreError, NewJob, WorkflowId},
    workflow::{
        WorkflowAction, WorkflowError, WorkflowEvent, WorkflowRecord, WorkflowStore,
        WorkflowStoreError, WorkflowTransition,
    },
};
use aequora_store::StoreErrorKind;
use aequora_types::TenantId;
use async_trait::async_trait;
use sqlx::{Postgres, Row, Transaction, postgres::PgRow};
use std::sync::Arc;

use crate::job_store::{PostgresJobStore, database_now};

/// Stores workflow transitions alongside their durable child jobs on the fenced authority.
pub struct PostgresWorkflowStore {
    jobs: Arc<PostgresJobStore>,
}

impl PostgresWorkflowStore {
    #[must_use]
    pub const fn new(jobs: Arc<PostgresJobStore>) -> Self {
        Self { jobs }
    }

    /// Commits the workflow transition, event, and exact ordered child jobs in one transaction.
    ///
    /// # Errors
    /// Rejects stale versions, unsupported actions, mismatching children, and storage failures.
    pub async fn apply_with_jobs(
        &self,
        id: WorkflowId,
        tenant: TenantId,
        event: WorkflowEvent,
        transition: WorkflowTransition,
        children: Vec<NewJob>,
    ) -> Result<WorkflowRecord, WorkflowStoreError> {
        event.verify().map_err(WorkflowStoreError::Invalid)?;
        let mut tx = self.jobs.transaction().await.map_err(job_error)?;
        let current = locked_workflow(&mut tx, id, tenant).await?;
        transition.verify(&current).map_err(|error| {
            if error == WorkflowError::StaleVersion {
                WorkflowStoreError::StaleVersion
            } else {
                WorkflowStoreError::Invalid(error)
            }
        })?;
        verify_children(&current, &transition.actions, &children)?;
        let mut action_buffer = vec![0; 1024 * 1024];
        let actions: &[u8] =
            postcard::to_slice(&transition.actions, &mut action_buffer).map_err(|error| {
                if matches!(error, postcard::Error::SerializeBufferFull) {
                    WorkflowStoreError::Invalid(WorkflowError::ActionPayloadTooLarge)
                } else {
                    encoding_error(&error)
                }
            })?;
        let now = database_now(&mut tx).await.map_err(job_error)?;
        let new_version = current
            .row_version
            .checked_add(1)
            .ok_or(WorkflowStoreError::StaleVersion)?;
        let child_count = current
            .child_count
            .checked_add(
                u32::try_from(children.len())
                    .map_err(|_| WorkflowStoreError::Invalid(WorkflowError::ChildLimit))?,
            )
            .ok_or(WorkflowStoreError::Invalid(WorkflowError::ChildLimit))?;
        let updated = WorkflowRecord {
            state: transition.next_state,
            checkpoint: transition.checkpoint,
            row_version: new_version,
            child_count,
            updated_at_unix_ms: now.max(current.updated_at_unix_ms),
            ..current
        };
        updated.verify().map_err(WorkflowStoreError::Invalid)?;
        for child in children {
            self.jobs
                .insert_in_transaction(&mut tx, child)
                .await
                .map_err(job_error)?;
        }
        let affected = sqlx::query(
            "UPDATE aequora_runtime_workflows SET row_version=$3,record=$4
             WHERE tenant_id=$1 AND workflow_id=$2 AND row_version=$5",
        )
        .bind(tenant.as_uuid())
        .bind(id.as_uuid())
        .bind(number(new_version)?)
        .bind(postcard::to_allocvec(&updated).map_err(|error| encoding_error(&error))?)
        .bind(number(transition.expected_row_version)?)
        .execute(&mut *tx)
        .await
        .map_err(db_error)?;
        if affected.rows_affected() != 1 {
            return Err(WorkflowStoreError::StaleVersion);
        }
        sqlx::query(
            "INSERT INTO aequora_runtime_workflow_transitions
             (tenant_id,workflow_id,row_version,event,actions,occurred_ms) VALUES ($1,$2,$3,$4,$5,$6)",
        ).bind(tenant.as_uuid()).bind(id.as_uuid()).bind(number(new_version)?)
            .bind(postcard::to_allocvec(&event).map_err(|error| encoding_error(&error))?)
            .bind(actions).bind(number(now)?).execute(&mut *tx).await.map_err(db_error)?;
        tx.commit().await.map_err(db_error)?;
        Ok(updated)
    }
}

#[async_trait]
impl WorkflowStore for PostgresWorkflowStore {
    async fn create(&self, workflow: WorkflowRecord) -> Result<(), WorkflowStoreError> {
        workflow.verify().map_err(WorkflowStoreError::Invalid)?;
        if workflow.row_version != 1 || workflow.child_count != 0 {
            return Err(WorkflowStoreError::Invalid(WorkflowError::InvalidRecord));
        }
        let mut tx = self.jobs.transaction().await.map_err(job_error)?;
        let affected = sqlx::query(
            "INSERT INTO aequora_runtime_workflows (tenant_id,workflow_id,row_version,record)
             VALUES ($1,$2,1,$3) ON CONFLICT (tenant_id,workflow_id) DO NOTHING",
        )
        .bind(workflow.tenant_id.as_uuid())
        .bind(workflow.workflow_id.as_uuid())
        .bind(postcard::to_allocvec(&workflow).map_err(|error| encoding_error(&error))?)
        .execute(&mut *tx)
        .await
        .map_err(db_error)?;
        if affected.rows_affected() != 1 {
            return Err(WorkflowStoreError::Duplicate);
        }
        tx.commit().await.map_err(db_error)
    }

    async fn load(
        &self,
        id: WorkflowId,
        tenant: TenantId,
    ) -> Result<WorkflowRecord, WorkflowStoreError> {
        let row = sqlx::query(
            "SELECT row_version,record FROM aequora_runtime_workflows WHERE tenant_id=$1 AND workflow_id=$2",
        ).bind(tenant.as_uuid()).bind(id.as_uuid()).fetch_optional(self.jobs.pool()).await
            .map_err(db_error)?.ok_or(WorkflowStoreError::NotFound)?;
        decode(&row, id, tenant)
    }

    async fn apply(
        &self,
        id: WorkflowId,
        tenant: TenantId,
        event: WorkflowEvent,
        transition: WorkflowTransition,
    ) -> Result<WorkflowRecord, WorkflowStoreError> {
        self.apply_with_jobs(id, tenant, event, transition, Vec::new())
            .await
    }
}

fn verify_children(
    workflow: &WorkflowRecord,
    actions: &[WorkflowAction],
    children: &[NewJob],
) -> Result<(), WorkflowStoreError> {
    let mut jobs = children.iter();
    for action in actions {
        match action {
            WorkflowAction::ScheduleJob { kind, payload, .. }
            | WorkflowAction::ScheduleCompensation { kind, payload, .. } => {
                let child = jobs
                    .next()
                    .ok_or(WorkflowStoreError::Invalid(WorkflowError::InvalidRecord))?;
                if child.job.tenant_id != workflow.tenant_id
                    || child.job.workflow_id != Some(workflow.workflow_id)
                    || child.job.lineage.correlation_id != workflow.correlation_id
                    || child.job.kind != *kind
                    || !matches!(&child.job.payload, JobPayloadRef::Inline(bytes) if bytes == payload)
                {
                    return Err(WorkflowStoreError::Invalid(WorkflowError::InvalidRecord));
                }
            }
            WorkflowAction::EmitAuthoritativeOperation { .. } => {
                return Err(WorkflowStoreError::Adapter(
                    "workflow requires host-owned authoritative operation publication".to_owned(),
                ));
            }
            _ => {}
        }
    }
    if jobs.next().is_some() {
        return Err(WorkflowStoreError::Invalid(WorkflowError::InvalidRecord));
    }
    Ok(())
}

async fn locked_workflow(
    tx: &mut Transaction<'_, Postgres>,
    id: WorkflowId,
    tenant: TenantId,
) -> Result<WorkflowRecord, WorkflowStoreError> {
    let row = sqlx::query(
        "SELECT row_version,record FROM aequora_runtime_workflows WHERE tenant_id=$1 AND workflow_id=$2 FOR NO KEY UPDATE",
    ).bind(tenant.as_uuid()).bind(id.as_uuid()).fetch_optional(&mut **tx).await
        .map_err(db_error)?.ok_or(WorkflowStoreError::NotFound)?;
    decode(&row, id, tenant)
}

fn decode(
    row: &PgRow,
    id: WorkflowId,
    tenant: TenantId,
) -> Result<WorkflowRecord, WorkflowStoreError> {
    let bytes: &[u8] = row.try_get("record").map_err(db_error)?;
    let record: WorkflowRecord =
        postcard::from_bytes(bytes).map_err(|error| encoding_error(&error))?;
    record.verify().map_err(WorkflowStoreError::Invalid)?;
    if record.workflow_id != id
        || record.tenant_id != tenant
        || row.try_get::<i64, _>("row_version").map_err(db_error)? != number(record.row_version)?
    {
        return Err(WorkflowStoreError::Invalid(WorkflowError::InvalidRecord));
    }
    Ok(record)
}

fn number(value: u64) -> Result<i64, WorkflowStoreError> {
    i64::try_from(value).map_err(|_| WorkflowStoreError::Invalid(WorkflowError::InvalidRecord))
}

fn job_error(error: JobStoreError) -> WorkflowStoreError {
    match error {
        JobStoreError::Unavailable | JobStoreError::StaleFence => WorkflowStoreError::Unavailable,
        other => WorkflowStoreError::Adapter(other.to_string()),
    }
}

fn db_error(error: sqlx::Error) -> WorkflowStoreError {
    let error = crate::postgres_error(error);
    match error.kind {
        StoreErrorKind::Transient => WorkflowStoreError::Unavailable,
        StoreErrorKind::Permanent => WorkflowStoreError::Adapter(error.message),
    }
}

fn encoding_error(error: &postcard::Error) -> WorkflowStoreError {
    WorkflowStoreError::Adapter(format!("invalid durable workflow record: {error}"))
}
