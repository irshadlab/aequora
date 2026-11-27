use aequora_authority::{
    AuthorityCommitContext, AuthorityController, AuthorityPromotionPolicy, AuthorityRole,
    AuthorityRuntimeMode, AuthorityState,
    jobs::{
        ClaimRequest, ClaimedJob, ConcurrencyClass, DependencyCondition, ErrorClass, FencedUpdate,
        JitterPolicy, JobCheckpoint, JobDependency, JobDescriptor, JobEpochPolicy,
        JobExecutionRegionPolicy, JobGovernanceMetadata, JobKind, JobPayloadRef,
        JobPayloadSchemaVersion, JobRecord, JobRegistry, JobRunOutcome, JobState, JobStore,
        JobStoreError, NewJob, RetryPolicy, WorkerId, WorkflowId,
    },
    side_effects::{
        AmbiguousRecoveryPolicy, ExternalIdempotencyKey, ExternalOutcome, ExternalOutcomeKind,
        SideEffectExecution, SideEffectExecutionState, SideEffectIntent, SideEffectIntentId,
        SideEffectIntentWrite, SideEffectKind, SideEffectPayload, SideEffectStore,
        SideEffectStoreError,
    },
    workflow::{
        WorkflowAction, WorkflowCheckpoint, WorkflowEvent, WorkflowKind, WorkflowRecord,
        WorkflowState, WorkflowStore, WorkflowStoreError, WorkflowTransition, WorkflowVersion,
    },
};
use aequora_protocol::ChangeKind;
use aequora_store::CommitOperation;
use aequora_store_postgres::{
    SqlxPostgresBackend,
    job_store::PostgresJobStore,
    side_effect_store::{PostgresSideEffectStore, PostgresSideEffectWriter},
    workflow_store::PostgresWorkflowStore,
};
use aequora_types::{
    ActorId, AuthorityEpoch, AuthorityId, AuthorityInstanceId, CorrelationId, DeviceId, EntityId,
    EntityRef, EntityType, EntityVersion, EventId, HybridTimestamp, JobId, LineageContext,
    LineageRef, NodeId, OperationId, SyncScopeId, TenantId, scheduler::WorkClass,
};
use std::{collections::BTreeSet, sync::Arc};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

#[tokio::test]
async fn native_job_store_preserves_atomicity_dependencies_and_leases() -> TestResult {
    let Ok(url) = std::env::var("AEQUORA_TEST_POSTGRES_RUNTIME_URL") else {
        eprintln!("skipped: AEQUORA_TEST_POSTGRES_RUNTIME_URL is unset");
        return Ok(());
    };
    let backend = SqlxPostgresBackend::connect(&url, 5).await?;
    let state = if let Some(state) = backend.load_authority_state().await? {
        state
    } else {
        let mut state = AuthorityState::new(
            AuthorityId::new(),
            AuthorityInstanceId::new(),
            AuthorityRole::Primary,
            1,
        );
        state.runtime_mode = AuthorityRuntimeMode::Serving;
        backend.initialize_authority_state(state).await?
    };
    let context =
        AuthorityController::new(state, AuthorityPromotionPolicy::default()).authorize_write()?;
    let registry = job_registry(1)?;
    let store = Arc::new(PostgresJobStore::new(
        backend.pool().clone(),
        context,
        registry,
    ));
    verify_transactions_and_dependencies(&backend, &store, context.epoch).await?;
    verify_claim_and_reclaim(&backend, &store, context.epoch).await?;
    verify_mixed_version_claims(&backend, &store, context).await?;
    verify_atomic_workflow(&store, context.epoch).await?;
    verify_side_effect_intent(&backend, &store, context).await?;
    Ok(())
}

async fn verify_transactions_and_dependencies(
    backend: &SqlxPostgresBackend,
    store: &PostgresJobStore,
    epoch: AuthorityEpoch,
) -> TestResult {
    let tenant = TenantId::new();
    let rolled_back = new_job(tenant, epoch)?;
    let mut tx = backend.pool().begin().await?;
    store
        .insert_in_transaction(&mut tx, rolled_back.clone())
        .await?;
    tx.rollback().await?;
    assert_eq!(
        store.status(rolled_back.job.job_id, tenant).await,
        Err(JobStoreError::NotFound)
    );

    let parent = new_job(tenant, epoch)?;
    store.insert(parent.clone()).await?;
    let mut invalid = new_job(TenantId::new(), epoch)?;
    invalid.dependencies.push(JobDependency {
        job_id: invalid.job.job_id,
        depends_on_job_id: parent.job.job_id,
        condition: DependencyCondition::Completed,
    });
    let mut tx = backend.pool().begin().await?;
    assert!(
        store
            .insert_in_transaction(&mut tx, invalid.clone())
            .await
            .is_err()
    );
    tx.commit().await?;
    assert_eq!(
        store
            .status(invalid.job.job_id, invalid.job.tenant_id)
            .await,
        Err(JobStoreError::NotFound)
    );

    let mut child = new_job(tenant, epoch)?;
    child.dependencies.push(JobDependency {
        job_id: child.job.job_id,
        depends_on_job_id: parent.job.job_id,
        condition: DependencyCondition::Completed,
    });
    store.insert(child.clone()).await?;
    let worker = WorkerId::new();
    let first = store.claim_ready(claim_request(worker, 2)?).await?;
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].job.job_id, parent.job.job_id);
    store
        .finish(update(&first[0]), JobRunOutcome::Completed { result: None })
        .await?;
    let second = store.claim_ready(claim_request(worker, 2)?).await?;
    assert_eq!(second.len(), 1);
    assert_eq!(second[0].job.job_id, child.job.job_id);
    store
        .finish(
            update(&second[0]),
            JobRunOutcome::Completed { result: None },
        )
        .await?;
    Ok(())
}

async fn verify_claim_and_reclaim(
    backend: &SqlxPostgresBackend,
    store: &PostgresJobStore,
    epoch: AuthorityEpoch,
) -> TestResult {
    let tenant = TenantId::new();
    let job = new_job(tenant, epoch)?;
    store.insert(job.clone()).await?;
    assert_eq!(
        store.insert(job.clone()).await,
        Err(JobStoreError::Duplicate)
    );
    assert_eq!(
        store.status(job.job.job_id, TenantId::new()).await,
        Err(JobStoreError::NotFound)
    );

    let (left, right) = tokio::join!(
        store.claim_ready(claim_request(WorkerId::new(), 1)?),
        store.claim_ready(claim_request(WorkerId::new(), 1)?),
    );
    let mut claims = left?;
    claims.extend(right?);
    assert_eq!(claims.len(), 1);
    let first = &claims[0];
    assert_eq!(first.job.job_id, job.job.job_id);
    let checkpoint = JobCheckpoint {
        bytes: vec![1, 2],
        completed_units: 1,
        total_units: Some(2),
    };
    store.checkpoint(update(first), checkpoint.clone()).await?;
    sqlx::query("UPDATE aequora_runtime_jobs SET lease_expires_ms=0 WHERE job_id=$1")
        .bind(job.job.job_id.as_uuid())
        .execute(backend.pool())
        .await?;
    assert_eq!(
        store.checkpoint(update(first), checkpoint.clone()).await,
        Err(JobStoreError::LeaseLost)
    );

    let second = store
        .claim_ready(claim_request(first.lease.worker_id, 1)?)
        .await?;
    assert_eq!(second.len(), 1);
    assert!(second[0].lease.fencing_token > first.lease.fencing_token);
    assert_eq!(second[0].job.checkpoint, Some(checkpoint));
    assert_eq!(
        store
            .finish(update(first), JobRunOutcome::Completed { result: None })
            .await,
        Err(JobStoreError::StaleFence),
    );
    let result = JobPayloadRef::Inline(vec![4]);
    store
        .finish(
            update(&second[0]),
            JobRunOutcome::Completed {
                result: Some(result.clone()),
            },
        )
        .await?;
    let status = store.status(job.job.job_id, tenant).await?;
    assert_eq!(status.attempt_count, 2);
    assert_eq!(status.result, Some(result));
    Ok(())
}

async fn verify_mixed_version_claims(
    backend: &SqlxPostgresBackend,
    old_store: &PostgresJobStore,
    context: AuthorityCommitContext,
) -> TestResult {
    let new_store = PostgresJobStore::new(backend.pool().clone(), context, job_registry(2)?);
    let tenant = TenantId::new();
    let mut newer = new_job(tenant, context.epoch)?;
    newer.job.payload_schema_version = JobPayloadSchemaVersion::new(2)?;
    new_store.insert(newer.clone()).await?;
    let older = new_job(tenant, context.epoch)?;
    old_store.insert(older.clone()).await?;

    for (store, expected) in [
        (old_store, older.job.job_id),
        (&new_store, newer.job.job_id),
    ] {
        let claims = store
            .claim_ready(claim_request(WorkerId::new(), 2)?)
            .await?;
        assert_eq!(claims.len(), 1);
        assert_eq!(claims[0].job.job_id, expected);
        store
            .finish(
                update(&claims[0]),
                JobRunOutcome::Completed { result: None },
            )
            .await?;
    }
    Ok(())
}

fn job_registry(version: u16) -> TestResult<Arc<JobRegistry>> {
    let mut registry = JobRegistry::new();
    let descriptor = JobDescriptor {
        kind: JobKind::new(1)?,
        payload_versions: [JobPayloadSchemaVersion::new(version)?].into(),
        retry_policy: RetryPolicy {
            max_attempts: Some(3),
            base_delay_ms: 5,
            max_delay_ms: 1_000,
            jitter: JitterPolicy::Full,
            retryable_errors: [ErrorClass::InfrastructureTransient].into(),
        },
        attempt_timeout_ms: 30_000,
        concurrency_class: ConcurrencyClass::DatabaseLight,
        required_capabilities: BTreeSet::new(),
        requires_external_idempotency: false,
    };
    registry.register(descriptor.clone())?;
    registry.register(JobDescriptor {
        kind: JobKind::new(2)?,
        requires_external_idempotency: true,
        ..descriptor
    })?;
    Ok(Arc::new(registry))
}

fn new_job(tenant: TenantId, epoch: AuthorityEpoch) -> TestResult<NewJob> {
    let payload = JobPayloadRef::Inline(vec![1]);
    Ok(NewJob {
        job: JobRecord {
            job_id: JobId::new(),
            tenant_id: tenant,
            kind: JobKind::new(1)?,
            state: JobState::Ready,
            priority: WorkClass::Normal,
            payload_schema_version: JobPayloadSchemaVersion::new(1)?,
            payload_digest: payload.digest(),
            payload,
            checkpoint: None,
            workflow_id: None,
            lineage: LineageContext::root(),
            attempt_count: 0,
            next_run_at_unix_ms: None,
            deadline_unix_ms: None,
            created_at_unix_ms: 1,
            updated_at_unix_ms: 1,
            created_under_epoch: Some(epoch),
            epoch_policy: JobEpochPolicy::Reconcile,
            region_policy: JobExecutionRegionPolicy::AuthorityOnly,
            governance: JobGovernanceMetadata {
                retention_class: 1,
                contains_personal_data: false,
                legal_hold_eligible: true,
                subject_references: Vec::new(),
                artifact_references: Vec::new(),
            },
        },
        dependencies: Vec::new(),
        required_capabilities: BTreeSet::new(),
    })
}

fn claim_request(worker_id: WorkerId, limit: usize) -> TestResult<ClaimRequest> {
    Ok(ClaimRequest {
        worker_id,
        supported_kinds: [JobKind::new(1)?].into(),
        capabilities: BTreeSet::new(),
        allowed_regions: BTreeSet::new(),
        now_unix_ms: 0,
        lease_duration_ms: 60_000,
        limit,
    })
}

fn update(claim: &ClaimedJob) -> FencedUpdate {
    FencedUpdate {
        job_id: claim.job.job_id,
        worker_id: claim.lease.worker_id,
        fencing_token: claim.lease.fencing_token,
        expected_state: JobState::Running,
        now_unix_ms: 0,
    }
}

async fn verify_atomic_workflow(jobs: &Arc<PostgresJobStore>, epoch: AuthorityEpoch) -> TestResult {
    let store = PostgresWorkflowStore::new(jobs.clone());
    let tenant = TenantId::new();
    let workflow = WorkflowRecord {
        workflow_id: WorkflowId::new(),
        tenant_id: tenant,
        kind: WorkflowKind::new(1)?,
        version: WorkflowVersion::new(1)?,
        state: WorkflowState::Running,
        checkpoint: WorkflowCheckpoint {
            phase: 0,
            state_bytes: Vec::new(),
            completed_steps: BTreeSet::new(),
            scheduled_compensations: BTreeSet::new(),
        },
        correlation_id: CorrelationId::new(),
        row_version: 1,
        child_count: 0,
        created_at_unix_ms: 1,
        updated_at_unix_ms: 1,
    };
    store.create(workflow.clone()).await?;
    let mut child = new_job(tenant, epoch)?;
    child.job.workflow_id = Some(workflow.workflow_id);
    child.job.lineage.correlation_id = workflow.correlation_id;
    let transition = WorkflowTransition {
        expected_row_version: 1,
        next_state: WorkflowState::Waiting,
        checkpoint: workflow.checkpoint.clone(),
        actions: [1, 2]
            .into_iter()
            .map(|step| WorkflowAction::ScheduleJob {
                step,
                kind: child.job.kind,
                payload: vec![1],
            })
            .collect(),
    };
    assert!(
        store
            .apply_with_jobs(
                workflow.workflow_id,
                tenant,
                WorkflowEvent::Started,
                transition.clone(),
                vec![child.clone(), child.clone()]
            )
            .await
            .is_err()
    );
    assert_eq!(store.load(workflow.workflow_id, tenant).await?, workflow);
    assert_eq!(
        jobs.status(child.job.job_id, tenant).await,
        Err(JobStoreError::NotFound)
    );
    let mut second = child.clone();
    second.job.job_id = JobId::new();
    let updated = store
        .apply_with_jobs(
            workflow.workflow_id,
            tenant,
            WorkflowEvent::Started,
            transition.clone(),
            vec![child.clone(), second.clone()],
        )
        .await?;
    assert_eq!(updated.row_version, 2);
    assert_eq!(updated.child_count, 2);
    assert_eq!(
        jobs.status(child.job.job_id, tenant).await?.state,
        JobState::Ready
    );
    assert_eq!(
        jobs.status(second.job.job_id, tenant).await?.state,
        JobState::Ready
    );
    assert_eq!(
        store
            .apply_with_jobs(
                workflow.workflow_id,
                tenant,
                WorkflowEvent::Started,
                transition,
                vec![child.clone(), second.clone()]
            )
            .await,
        Err(WorkflowStoreError::StaleVersion)
    );
    jobs.request_cancel(child.job.job_id, tenant).await?;
    jobs.request_cancel(second.job.job_id, tenant).await?;
    Ok(())
}

async fn verify_side_effect_intent(
    backend: &SqlxPostgresBackend,
    jobs: &Arc<PostgresJobStore>,
    authority: AuthorityCommitContext,
) -> TestResult {
    let store = PostgresSideEffectStore::new(jobs.clone());
    let tenant = TenantId::new();
    let op = OperationId::new();
    let payload = SideEffectPayload {
        schema_version: 1,
        bytes: b"notice".to_vec(),
        secret_references: Vec::new(),
    };
    let id = SideEffectIntentId::derive(op, b"notice");
    let intent = SideEffectIntent {
        intent_id: id,
        operation_id: op,
        tenant_id: tenant,
        kind: SideEffectKind::new(1)?,
        idempotency_key: ExternalIdempotencyKey::from_intent(id),
        payload_digest: payload.digest(),
        payload,
        ambiguity_policy: AmbiguousRecoveryPolicy::ManualReview,
        correlation_id: CorrelationId::new(),
        workflow_id: None,
        created_at_unix_ms: 1,
        retention_class: 1,
        contains_personal_data: false,
    };
    let mut tx = backend.pool().begin().await?;
    PostgresSideEffectWriter::new(jobs, &mut tx)
        .write_intent(intent.clone())
        .await?;
    assert!(tx.commit().await.is_err());
    assert_eq!(
        store.load_intent(id, tenant).await,
        Err(SideEffectStoreError::NotFound)
    );

    let mut tx = backend.pool().begin().await?;
    let mut writer = PostgresSideEffectWriter::new(jobs, &mut tx);
    writer.write_intent(intent.clone()).await?;
    writer.write_intent(intent.clone()).await?;
    let commit = effect_commit(authority, &intent)?;
    SqlxPostgresBackend::publish_in_transaction(&mut tx, &commit, &commit.payload).await?;
    tx.commit().await?;
    assert_eq!(store.load_intent(id, tenant).await?, intent);
    assert_eq!(
        store.load_intent(id, TenantId::new()).await,
        Err(SideEffectStoreError::NotFound)
    );
    let late_id = SideEffectIntentId::derive(op, b"late");
    let late = SideEffectIntent {
        intent_id: late_id,
        idempotency_key: ExternalIdempotencyKey::from_intent(late_id),
        ..intent.clone()
    };
    let mut later = backend.pool().begin().await?;
    PostgresSideEffectWriter::new(jobs, &mut later)
        .write_intent(late)
        .await?;
    assert!(later.commit().await.is_err());
    assert_eq!(
        store.load_intent(late_id, tenant).await,
        Err(SideEffectStoreError::NotFound)
    );

    let mut job = new_job(tenant, authority.epoch)?;
    job.job.kind = JobKind::new(2)?;
    job.job.lineage = LineageContext {
        correlation_id: intent.correlation_id,
        caused_by: Some(LineageRef::Operation(op)),
    };
    assert!(store.materialize_job(id, tenant, job.clone()).await?);
    assert!(!store.materialize_job(id, tenant, job.clone()).await?);
    verify_side_effect_execution(backend, jobs, &intent, job).await
}

fn effect_commit(
    authority: AuthorityCommitContext,
    intent: &SideEffectIntent,
) -> TestResult<CommitOperation> {
    let payload = b"domain state".to_vec();
    Ok(CommitOperation {
        authority: Some(authority),
        operation_id: intent.operation_id,
        event_id: EventId::new(),
        operation_lineage: LineageContext {
            correlation_id: intent.correlation_id,
            caused_by: None,
        },
        actor_id: ActorId::new(),
        device_id: DeviceId::new(),
        operation_kind: 1,
        tenant_id: intent.tenant_id,
        scope_id: SyncScopeId::new(),
        entity: EntityRef {
            entity_type: EntityType::new(1)?,
            entity_id: EntityId::new(),
        },
        expected_version: None,
        next_version: EntityVersion::INITIAL,
        change_kind: ChangeKind::Upsert,
        command_digest: *blake3::hash(&payload).as_bytes(),
        payload,
        timestamp: HybridTimestamp {
            physical_ms: 1,
            logical: 0,
            node: NodeId::new(),
        },
    })
}

async fn verify_side_effect_execution(
    backend: &SqlxPostgresBackend,
    jobs: &Arc<PostgresJobStore>,
    intent: &SideEffectIntent,
    job: NewJob,
) -> TestResult {
    let mut request = claim_request(WorkerId::new(), 1)?;
    request.supported_kinds = [job.job.kind].into();
    let claims = jobs.claim_ready(request.clone()).await?;
    assert_eq!(claims.len(), 1);
    assert_eq!(claims[0].job.job_id, job.job.job_id);
    let store = PostgresSideEffectStore::new(jobs.clone());
    let tenant = intent.tenant_id;
    let id = intent.intent_id;
    let executing = SideEffectExecution {
        intent_id: id,
        job_id: job.job.job_id,
        provider: "test-provider".to_owned(),
        state: SideEffectExecutionState::Executing,
        provider_reference: None,
        last_outcome: None,
        updated_at_unix_ms: 0,
    };
    assert_eq!(
        store
            .compare_and_set_execution(tenant, SideEffectExecutionState::Pending, executing.clone())
            .await,
        Err(SideEffectStoreError::StateConflict)
    );
    let store = store.with_worker_lease(update(&claims[0]));
    store
        .compare_and_set_execution(tenant, SideEffectExecutionState::Pending, executing.clone())
        .await?;
    sqlx::query("UPDATE aequora_runtime_jobs SET lease_expires_ms=0 WHERE job_id=$1")
        .bind(job.job.job_id.as_uuid())
        .execute(backend.pool())
        .await?;
    let finished = SideEffectExecution {
        state: SideEffectExecutionState::Confirmed,
        last_outcome: Some(ExternalOutcome {
            kind: ExternalOutcomeKind::ConfirmedSuccess,
            provider_reference: None,
            retry_after_ms: None,
            result_code: 1,
        }),
        ..executing
    };
    assert_eq!(
        store
            .compare_and_set_execution(tenant, SideEffectExecutionState::Executing, finished)
            .await,
        Err(SideEffectStoreError::StateConflict)
    );
    assert_eq!(jobs.claim_ready(request).await?, Vec::new());
    assert_eq!(
        jobs.status(job.job.job_id, tenant).await?.state,
        JobState::Quarantined
    );
    assert_eq!(
        store
            .execution(tenant, id)
            .await?
            .ok_or("execution missing")?
            .state,
        SideEffectExecutionState::Executing
    );
    Ok(())
}
