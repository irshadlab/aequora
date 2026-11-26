use aequora_authority::{
    AuthorityCommitContext, AuthorityController, AuthorityError, AuthorityPromotionPolicy,
    AuthorityRole, AuthorityRuntimeMode, AuthorityState, PromotionClass, PromotionEvidence,
    PromotionRequest, RecoveryVerification, TransitionReason,
};
use aequora_protocol::ChangeKind;
use aequora_store::{CommitOperation, CommitOutcome};
use aequora_store_postgres::epoch_witness::{PostgresEpochWitness, install, record_epoch};
use aequora_store_postgres::{PostgresBackend, SqlxPostgresBackend};
use aequora_types::{
    ActorId, AuthorityEpoch, AuthorityId, AuthorityInstanceId, DeviceId, EntityId, EntityRef,
    EntityType, EntityVersion, EventId, HybridTimestamp, LineageContext, NodeId, OperationId,
    Sequence, SyncScopeId, TenantId,
};
use sqlx::postgres::PgPoolOptions;
use std::time::Duration;

#[tokio::test]
async fn dedicated_postgres_witness_is_monotonic_and_fences_live_controllers()
-> Result<(), Box<dyn std::error::Error>> {
    let Ok(url) = std::env::var("AEQUORA_TEST_POSTGRES_WITNESS_URL") else {
        eprintln!("skipped: AEQUORA_TEST_POSTGRES_WITNESS_URL is unset");
        return Ok(());
    };
    let pool = PgPoolOptions::new()
        .max_connections(3)
        .connect(&url)
        .await?;
    install(&pool).await?;
    install(&pool).await?;
    let id = AuthorityId::new();
    let age = Duration::from_secs(60);
    let mut witness = PostgresEpochWitness::new(pool.clone(), id, age)?;
    let registry = witness.registry();
    let mut state = AuthorityState::new(id, AuthorityInstanceId::new(), AuthorityRole::Primary, 1);
    state.runtime_mode = AuthorityRuntimeMode::Serving;
    let controller = AuthorityController::new(state, AuthorityPromotionPolicy::default())
        .with_epoch_registry(registry.clone());
    assert_eq!(
        controller.authorize_write(),
        Err(AuthorityError::ExternalEpochUnavailable)
    );
    assert!(witness.refresh().await.is_err());

    record_epoch(&pool, id, AuthorityEpoch::INITIAL).await?;
    assert_eq!(witness.refresh().await?, AuthorityEpoch::INITIAL);
    assert_eq!(registry.highest_epoch(AuthorityId::new()), None);
    assert_eq!(
        registry.record_epoch(id, AuthorityEpoch::new(2)?),
        Err(AuthorityError::ExternalEpochUnavailable)
    );
    let token = controller.authorize_write()?;

    let two = AuthorityEpoch::new(2)?;
    let three = AuthorityEpoch::new(3)?;
    let (_, highest) = tokio::join!(record_epoch(&pool, id, two), record_epoch(&pool, id, three));
    highest?;
    assert!(record_epoch(&pool, id, two).await.is_err());
    record_epoch(&pool, id, three).await?;
    assert_eq!(witness.refresh().await?, three);
    assert!(matches!(controller.verify_commit_context(token),
        Err(AuthorityError::AuthorityRollbackDetected { highest, .. }) if highest == three));

    let mut restarted = PostgresEpochWitness::new(pool.clone(), id, age)?;
    assert_eq!(restarted.refresh().await?, three);
    pool.close().await;
    assert!(restarted.refresh().await.is_err());
    assert_eq!(restarted.registry().highest_epoch(id), None);
    Ok(())
}

#[tokio::test]
async fn witnessed_restore_keeps_the_database_fence_through_tx_b()
-> Result<(), Box<dyn std::error::Error>> {
    let (Ok(witness_url), Ok(authority_url)) = (
        std::env::var("AEQUORA_TEST_POSTGRES_WITNESS_URL"),
        std::env::var("AEQUORA_TEST_POSTGRES_RECOVERY_URL"),
    ) else {
        eprintln!("skipped: dedicated witness and recovery URLs are required");
        return Ok(());
    };
    let pool = PgPoolOptions::new().connect(&witness_url).await?;
    install(&pool).await?;
    let backend = SqlxPostgresBackend::connect(&authority_url, 4).await?;
    let id = AuthorityId::new();
    let mut old = AuthorityState::new(id, AuthorityInstanceId::new(), AuthorityRole::Primary, 1);
    old.runtime_mode = AuthorityRuntimeMode::Serving;
    backend.initialize_authority_state(old).await?;
    record_epoch(&pool, id, old.epoch).await?;
    let mut witness = PostgresEpochWitness::new(pool.clone(), id, Duration::from_secs(60))?;
    witness.refresh().await?;
    let controller = AuthorityController::new(old, AuthorityPromotionPolicy::default())
        .with_epoch_registry(witness.registry());
    let old_token = controller.authorize_write()?;
    verify_tx_b_holds_the_authority_lock(&backend, old_token).await?;
    assert!(backend.commit_operation(commit(None)?).await.is_err());

    let epoch = AuthorityEpoch::new(4)?;
    record_epoch(&pool, id, epoch).await?;
    witness.refresh().await?;
    let outcome = controller.promote_to_recorded_epoch(
        restore_request(),
        Some(Sequence(1)),
        Sequence(1),
        epoch,
    )?;
    let mut wrong_origin = outcome.transition;
    wrong_origin.old_epoch = AuthorityEpoch::new(2)?;
    assert!(
        backend
            .record_restore_epoch_transition(old, outcome.state, wrong_origin)
            .await
            .is_err()
    );
    backend
        .record_restore_epoch_transition(old, outcome.state, outcome.transition)
        .await?;
    assert_eq!(backend.load_authority_state().await?, Some(outcome.state));
    assert!(
        backend
            .commit_operation(commit(Some(old_token))?)
            .await
            .is_err()
    );
    assert!(controller.authorize_write().is_err());

    let verified = recovery_verification();
    let incomplete = RecoveryVerification {
        journal_consistent: false,
        ..verified
    };
    assert!(
        backend
            .complete_restore_recovery(outcome.state, incomplete, witness.registry(), 3)
            .await
            .is_err()
    );
    assert_eq!(backend.load_authority_state().await?, Some(outcome.state));
    let serving = backend
        .complete_restore_recovery(outcome.state, verified, witness.registry(), 3)
        .await?;
    assert_eq!(
        backend
            .complete_restore_recovery(outcome.state, verified, witness.registry(), 3)
            .await?,
        serving
    );
    assert_eq!(backend.load_authority_state().await?, Some(serving));
    let new_controller = AuthorityController::new(serving, AuthorityPromotionPolicy::default())
        .with_epoch_registry(witness.registry());
    let result = backend
        .commit_operation(commit(Some(new_controller.authorize_write()?))?)
        .await?;
    assert!(matches!(result, CommitOutcome::Applied(_)));
    Ok(())
}

async fn verify_tx_b_holds_the_authority_lock(
    backend: &SqlxPostgresBackend,
    context: AuthorityCommitContext,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut writer = backend.pool().begin().await?;
    let change = commit(Some(context))?;
    let outcome =
        SqlxPostgresBackend::publish_in_transaction(&mut writer, &change, &change.payload).await?;
    assert!(matches!(outcome, CommitOutcome::Applied(_)));
    let mut transition = backend.pool().begin().await?;
    sqlx::query("SET LOCAL lock_timeout = '100ms'")
        .execute(&mut *transition)
        .await?;
    let result = sqlx::query(
        "UPDATE aequora_authority_state SET updated_at_unix_ms=updated_at_unix_ms+1 WHERE singleton=TRUE"
    ).execute(&mut *transition).await;
    assert!(
        matches!(result, Err(sqlx::Error::Database(error)) if error.code().as_deref() == Some("55P03"))
    );
    transition.rollback().await?;
    writer.commit().await?;
    Ok(())
}

fn commit(
    authority: Option<AuthorityCommitContext>,
) -> Result<CommitOperation, aequora_types::ValueError> {
    let payload = b"witnessed transition".to_vec();
    Ok(CommitOperation {
        authority,
        operation_id: OperationId::new(),
        event_id: EventId::new(),
        operation_lineage: LineageContext::root(),
        actor_id: ActorId::new(),
        device_id: DeviceId::new(),
        operation_kind: 1,
        tenant_id: TenantId::new(),
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

fn restore_request() -> PromotionRequest {
    PromotionRequest {
        class: PromotionClass::RestoredTimeline,
        reason: TransitionReason::PointInTimeRestore,
        evidence: PromotionEvidence {
            old_primary_externally_fenced: true,
            replication_lag: None,
            journal_continuity: false,
            operation_ledger_continuity: false,
            audit_continuity: false,
            authority_metadata_continuity: false,
            scope_metadata_continuity: false,
            snapshot_catalog_consistent: false,
            governance_reconciled: true,
            side_effect_status_known: true,
        },
        allow_data_loss: true,
        manually_approved: true,
        new_instance_id: AuthorityInstanceId::new(),
        created_at_unix_ms: 2,
    }
}

fn recovery_verification() -> RecoveryVerification {
    RecoveryVerification {
        authority_metadata_valid: true,
        journal_consistent: true,
        operation_ledger_consistent: true,
        audit_chain_valid: true,
        governance_reconciled: true,
        scope_metadata_valid: true,
        snapshot_strategy_ready: true,
        side_effect_status_known: true,
        external_epoch_valid: true,
    }
}
