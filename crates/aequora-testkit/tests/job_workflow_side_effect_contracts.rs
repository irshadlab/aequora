use aequora_authority::jobs::{
    AdminAction, ClaimRequest, ClaimedJob, ConcurrencyClass, DependencyCondition,
    EpochRecoveryAction, ErrorClass, FencedUpdate, FencingToken, JitterPolicy, JobCheckpoint,
    JobDependency, JobDescriptor, JobEpochPolicy, JobError, JobExecutionRegionPolicy,
    JobGovernanceMetadata, JobKind, JobLease, JobPayloadRef, JobPayloadSchemaVersion, JobRecord,
    JobRegistry, JobRunOutcome, JobState, JobStatus, JobStore, JobStoreError, NewJob, OccurrenceId,
    RetryPolicy, ScheduleId, WorkerId, WorkflowId, epoch_recovery_action, validate_fenced_update,
    verify_dependency_dag,
};
use aequora_authority::side_effects::{
    AmbiguousRecoveryPolicy, ExternalIdempotencyKey, ExternalOutcome, ExternalOutcomeKind,
    ProviderCapabilities, ProviderError, ReconciliationResult, RecoveryAction, SideEffectIntent,
    SideEffectIntentId, SideEffectKind, SideEffectPayload, SideEffectProvider, recovery_action,
};
use aequora_authority::workflow::{
    WorkflowAction, WorkflowCheckpoint, WorkflowKind, WorkflowRecord, WorkflowState,
    WorkflowTransition, WorkflowVersion,
};
use aequora_server::jobs::{JobServerError, JobWorkerConfig, JobWorkerPlanner, WorkerShutdown};
use aequora_types::scheduler::WorkClass;
use aequora_types::{AuthorityEpoch, CorrelationId, JobId, OperationId, TenantId};
use async_trait::async_trait;
use proptest::prelude::*;
use std::{collections::BTreeSet, sync::Mutex};

fn kind(value: u32) -> JobKind {
    JobKind::new(value).unwrap_or_else(|error| panic!("invalid test kind: {error}"))
}

fn payload_version() -> JobPayloadSchemaVersion {
    JobPayloadSchemaVersion::new(1)
        .unwrap_or_else(|error| panic!("invalid test payload version: {error}"))
}

fn job(job_id: JobId) -> JobRecord {
    let payload = JobPayloadRef::Inline(vec![1, 2, 3]);
    JobRecord {
        job_id,
        tenant_id: TenantId::new(),
        kind: kind(1),
        state: JobState::Running,
        priority: WorkClass::Normal,
        payload_schema_version: payload_version(),
        payload_digest: payload.digest(),
        payload,
        checkpoint: None,
        workflow_id: None,
        lineage: aequora_types::LineageContext::root(),
        attempt_count: 2,
        next_run_at_unix_ms: None,
        deadline_unix_ms: None,
        created_at_unix_ms: 1,
        updated_at_unix_ms: 10,
        created_under_epoch: Some(AuthorityEpoch::INITIAL),
        epoch_policy: JobEpochPolicy::Reconcile,
        region_policy: JobExecutionRegionPolicy::AuthorityOnly,
        governance: JobGovernanceMetadata {
            retention_class: 1,
            contains_personal_data: true,
            legal_hold_eligible: true,
            subject_references: vec!["subject:42".to_owned()],
            artifact_references: Vec::new(),
        },
    }
}

fn descriptor() -> JobDescriptor {
    JobDescriptor {
        kind: kind(1),
        payload_versions: [payload_version()].into(),
        retry_policy: RetryPolicy {
            max_attempts: Some(3),
            base_delay_ms: 10,
            max_delay_ms: 100,
            jitter: JitterPolicy::Full,
            retryable_errors: [ErrorClass::InfrastructureTransient].into(),
        },
        attempt_timeout_ms: 500,
        concurrency_class: ConcurrencyClass::DatabaseLight,
        required_capabilities: BTreeSet::new(),
        requires_external_idempotency: false,
    }
}

#[test]
fn duplicate_job_registration_preserves_the_original_contract() {
    let original = descriptor();
    let mut registry = JobRegistry::new();
    assert_eq!(registry.register(original.clone()), Ok(()));
    let changed = JobDescriptor {
        requires_external_idempotency: true,
        ..original.clone()
    };
    assert_eq!(registry.register(changed), Err(JobError::DuplicateKind));
    assert_eq!(registry.descriptor(original.kind), Some(&original));
}

struct ClaimStore {
    limit: Mutex<Option<usize>>,
    claims: Vec<ClaimedJob>,
}

#[async_trait]
impl JobStore for ClaimStore {
    async fn claim_ready(&self, request: ClaimRequest) -> Result<Vec<ClaimedJob>, JobStoreError> {
        *self.limit.lock().map_err(|_| JobStoreError::Unavailable)? = Some(request.limit);
        Ok(self.claims.clone())
    }

    async fn insert(&self, _job: NewJob) -> Result<(), JobStoreError> {
        Err(JobStoreError::Unavailable)
    }

    async fn renew_lease(
        &self,
        _update: FencedUpdate,
        _expires_at: u64,
    ) -> Result<JobLease, JobStoreError> {
        Err(JobStoreError::Unavailable)
    }

    async fn checkpoint(
        &self,
        _update: FencedUpdate,
        _checkpoint: JobCheckpoint,
    ) -> Result<(), JobStoreError> {
        Err(JobStoreError::Unavailable)
    }

    async fn finish(
        &self,
        _update: FencedUpdate,
        _outcome: JobRunOutcome,
    ) -> Result<(), JobStoreError> {
        Err(JobStoreError::Unavailable)
    }

    async fn request_cancel(
        &self,
        _job_id: JobId,
        _tenant_id: TenantId,
    ) -> Result<(), JobStoreError> {
        Err(JobStoreError::Unavailable)
    }

    async fn admin_transition(
        &self,
        _job_id: JobId,
        _tenant_id: TenantId,
        _action: AdminAction,
        _reason: u32,
    ) -> Result<(), JobStoreError> {
        Err(JobStoreError::Unavailable)
    }

    async fn status(
        &self,
        _job_id: JobId,
        _tenant_id: TenantId,
    ) -> Result<JobStatus, JobStoreError> {
        Err(JobStoreError::Unavailable)
    }
}

#[tokio::test]
async fn worker_poll_stops_claiming_and_rejects_overclaim_or_invalid_leases()
-> Result<(), Box<dyn std::error::Error>> {
    let mut registry = JobRegistry::new();
    registry.register(descriptor())?;
    let config = JobWorkerConfig {
        worker_id: WorkerId::new(),
        capabilities: BTreeSet::new(),
        allowed_regions: BTreeSet::new(),
        lease_duration_ms: 90,
        maximum_claim_batch: 4,
        class_concurrency: std::collections::BTreeMap::new(),
        idle_backoff_ms: 10,
        failure_backoff_ms: 20,
    };
    let planner = JobWorkerPlanner::new(&registry, &config)?;
    let job = job(JobId::new());
    let claim = ClaimedJob {
        lease: JobLease {
            job_id: job.job_id,
            worker_id: config.worker_id,
            fencing_token: FencingToken::new(1)?,
            expires_at_unix_ms: 100,
        },
        job,
    };
    let mut store = ClaimStore {
        limit: Mutex::new(None),
        claims: vec![claim.clone(), claim.clone()],
    };
    assert_eq!(
        planner
            .poll(&store, WorkerShutdown::StopClaiming, 10, 1)
            .await?,
        Vec::new(),
    );
    assert_eq!(
        planner.poll(&store, WorkerShutdown::Running, 10, 0).await?,
        Vec::new(),
    );
    assert_eq!(
        *store
            .limit
            .get_mut()
            .map_err(|_| JobStoreError::Unavailable)?,
        None
    );
    assert!(matches!(
        planner.poll(&store, WorkerShutdown::Running, 10, 1).await,
        Err(JobServerError::AdapterExceededClaimLimit)
    ));
    assert_eq!(
        *store
            .limit
            .get_mut()
            .map_err(|_| JobStoreError::Unavailable)?,
        Some(1)
    );
    assert!(matches!(
        planner.poll(&store, WorkerShutdown::Running, 10, 2).await,
        Err(JobServerError::InvalidClaimedJob)
    ));
    store.claims.pop();
    assert!(matches!(
        planner.poll(&store, WorkerShutdown::Running, 100, 1).await,
        Err(JobServerError::InvalidClaimedJob)
    ));
    assert_eq!(
        planner.poll(&store, WorkerShutdown::Running, 10, 1).await?,
        vec![claim]
    );
    Ok(())
}

#[test]
fn newer_claim_fences_expired_worker() {
    let job_id = JobId::new();
    let worker_a = WorkerId::new();
    let worker_b = WorkerId::new();
    let record = job(job_id);
    let current_lease = JobLease {
        job_id,
        worker_id: worker_b,
        fencing_token: FencingToken::new(2)
            .unwrap_or_else(|error| panic!("invalid fence: {error}")),
        expires_at_unix_ms: 100,
    };
    let stale_update = FencedUpdate {
        job_id,
        worker_id: worker_a,
        fencing_token: FencingToken::INITIAL,
        expected_state: JobState::Running,
        now_unix_ms: 20,
    };
    assert_eq!(
        validate_fenced_update(&record, current_lease, stale_update),
        Err(aequora_authority::jobs::JobStoreError::LeaseLost)
    );
}

proptest! {
    #[test]
    fn every_older_fencing_token_is_rejected(current in 2_u64..u64::MAX, stale in 1_u64..1_000) {
        prop_assume!(stale < current);
        let job_id = JobId::new();
        let worker = WorkerId::new();
        let lease = JobLease {
            job_id,
            worker_id: worker,
            fencing_token: FencingToken::new(current)
                .unwrap_or_else(|error| panic!("invalid fence: {error}")),
            expires_at_unix_ms: 100,
        };
        let update = FencedUpdate {
            job_id,
            worker_id: worker,
            fencing_token: FencingToken::new(stale)
                .unwrap_or_else(|error| panic!("invalid fence: {error}")),
            expected_state: JobState::Running,
            now_unix_ms: 10,
        };
        prop_assert_eq!(
            validate_fenced_update(&job(job_id), lease, update),
            Err(aequora_authority::jobs::JobStoreError::StaleFence)
        );
    }
}

#[test]
fn dependency_dag_rejects_cycles() {
    let first = JobId::new();
    let second = JobId::new();
    assert_eq!(
        verify_dependency_dag(&[
            JobDependency {
                job_id: first,
                depends_on_job_id: second,
                condition: DependencyCondition::Completed,
            },
            JobDependency {
                job_id: second,
                depends_on_job_id: first,
                condition: DependencyCondition::Completed,
            },
        ]),
        Err(aequora_authority::jobs::JobError::DependencyCycle)
    );
}

struct IdempotentProvider {
    effects: Mutex<BTreeSet<String>>,
}

#[async_trait]
impl SideEffectProvider for IdempotentProvider {
    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            supports_idempotency: true,
            supports_lookup: true,
            supports_cancel: false,
        }
    }

    async fn execute(
        &self,
        intent: &SideEffectIntent,
        _timeout_ms: u64,
    ) -> Result<ExternalOutcome, ProviderError> {
        let mut effects = self
            .effects
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        effects.insert(intent.idempotency_key.as_str().to_owned());
        Ok(ExternalOutcome {
            kind: ExternalOutcomeKind::ConfirmedSuccess,
            provider_reference: Some("provider-result".to_owned()),
            retry_after_ms: None,
            result_code: 200,
        })
    }

    async fn reconcile(
        &self,
        key: &ExternalIdempotencyKey,
        _provider_reference: Option<&str>,
        _timeout_ms: u64,
    ) -> Result<ReconciliationResult, ProviderError> {
        let effects = self
            .effects
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if effects.contains(key.as_str()) {
            Ok(ReconciliationResult::Confirmed(ExternalOutcome {
                kind: ExternalOutcomeKind::ConfirmedSuccess,
                provider_reference: Some("provider-result".to_owned()),
                retry_after_ms: None,
                result_code: 200,
            }))
        } else {
            Ok(ReconciliationResult::NotFound)
        }
    }
}

fn side_effect_intent() -> SideEffectIntent {
    let operation_id = OperationId::new();
    let intent_id = SideEffectIntentId::derive(operation_id, b"capture-payment");
    let payload = SideEffectPayload {
        schema_version: 1,
        bytes: vec![9],
        secret_references: vec!["kms://payment/current".to_owned()],
    };
    SideEffectIntent {
        intent_id,
        operation_id,
        tenant_id: TenantId::new(),
        kind: SideEffectKind::new(1)
            .unwrap_or_else(|error| panic!("invalid side-effect kind: {error}")),
        idempotency_key: ExternalIdempotencyKey::from_intent(intent_id),
        payload_digest: payload.digest(),
        payload,
        ambiguity_policy: AmbiguousRecoveryPolicy::QueryProvider,
        correlation_id: CorrelationId::new(),
        workflow_id: None,
        created_at_unix_ms: 1,
        retention_class: 1,
        contains_personal_data: true,
    }
}

#[tokio::test]
async fn provider_retry_with_same_key_has_one_logical_effect() {
    let provider = IdempotentProvider {
        effects: Mutex::new(BTreeSet::new()),
    };
    let intent = side_effect_intent();
    assert!(provider.execute(&intent, 1_000).await.is_ok());
    assert!(provider.execute(&intent, 1_000).await.is_ok());
    let effects = provider
        .effects
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    assert_eq!(effects.len(), 1);
}

#[test]
fn provider_timeout_uses_reconciliation_policy() {
    let capabilities = ProviderCapabilities {
        supports_idempotency: true,
        supports_lookup: true,
        supports_cancel: false,
    };
    assert_eq!(
        recovery_action(AmbiguousRecoveryPolicy::QueryProvider, capabilities),
        RecoveryAction::Reconcile
    );
}

#[test]
fn retry_policy_backs_off_without_hot_loop() {
    let policy = RetryPolicy {
        max_attempts: None,
        base_delay_ms: 100,
        max_delay_ms: 60_000,
        jitter: JitterPolicy::Equal,
        retryable_errors: [ErrorClass::ProviderTransient].into_iter().collect(),
    };
    for attempt in 1..100 {
        assert!(
            policy
                .delay_ms(attempt, 42)
                .is_ok_and(|delay| delay > 0 && delay <= 60_000)
        );
    }
}

#[test]
fn workflow_failure_schedules_compensation_not_rollback() {
    let checkpoint = WorkflowCheckpoint {
        phase: 2,
        state_bytes: Vec::new(),
        completed_steps: [1].into_iter().collect(),
        scheduled_compensations: [1].into_iter().collect(),
    };
    let workflow = WorkflowRecord {
        workflow_id: WorkflowId::new(),
        tenant_id: TenantId::new(),
        kind: WorkflowKind::new(1).unwrap_or_else(|error| panic!("invalid workflow kind: {error}")),
        version: WorkflowVersion::new(1)
            .unwrap_or_else(|error| panic!("invalid workflow version: {error}")),
        state: WorkflowState::Running,
        checkpoint: checkpoint.clone(),
        correlation_id: CorrelationId::new(),
        row_version: 2,
        child_count: 1,
        created_at_unix_ms: 1,
        updated_at_unix_ms: 2,
    };
    let transition = WorkflowTransition {
        expected_row_version: 2,
        next_state: WorkflowState::Compensating,
        checkpoint,
        actions: vec![WorkflowAction::ScheduleCompensation {
            for_step: 1,
            kind: kind(9),
            payload: vec![1],
        }],
    };
    assert!(transition.verify(&workflow).is_ok());
}

#[test]
fn recurring_schedulers_share_one_occurrence_identity() {
    let schedule = ScheduleId::new();
    let first_scheduler = OccurrenceId::derive(schedule, 1_000);
    let second_scheduler = OccurrenceId::derive(schedule, 1_000);
    assert_eq!(first_scheduler, second_scheduler);
}

#[test]
fn epoch_change_applies_external_reconciliation_policy() {
    let next_epoch =
        AuthorityEpoch::new(2).unwrap_or_else(|error| panic!("invalid authority epoch: {error}"));
    assert_eq!(
        epoch_recovery_action(
            Some(AuthorityEpoch::INITIAL),
            next_epoch,
            JobEpochPolicy::Reconcile
        ),
        EpochRecoveryAction::ReconcileExternal
    );
}

#[test]
fn governance_metadata_covers_pending_payload_and_artifacts() {
    let mut record = job(JobId::new());
    record.checkpoint = Some(JobCheckpoint {
        bytes: vec![1],
        completed_units: 1,
        total_units: Some(2),
    });
    record
        .governance
        .artifact_references
        .push("export:42".to_owned());
    assert!(record.verify().is_ok());
    assert!(record.governance.contains_personal_data);
    assert!(record.governance.legal_hold_eligible);
}
