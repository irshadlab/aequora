# Part 16 authority failover, epochs, and fork detection

The governing specification is [Part 16](../sys-arch/16-authority-failover-timeline-epochs-fork-detection.md).

## Stable contracts and implemented behavior

- `aequora_authority::AuthorityController` validates writer roles, fence tokens, epoch rollback,
  and recovery mode. `AuthorityPromotionPlan::evaluate` applies promotion policy.
  Lossless promotion preserves the epoch while rotating the fence. Lossy and restored timelines
  require explicit approval and a new epoch. `compare_checkpoints` detects same-epoch divergence.
- `aequora_store_postgres::SqlxPostgresBackend` implements PostgreSQL authority state and
  `record_restore_epoch_transition`. The PostgreSQL write transaction validates authority
  context before publishing. Client exchange checks the returned cursor timeline before
  reconciliation. Stoolap records the highest trusted epoch across scopes. SQLite checks the
  installed cursor for the target scope.
- `aequora_store::validate_snapshot_cursor` checks the installed boundary inside both local
  adapter transactions. Resume requires the same snapshot ID, offset, and cursor. On activation
  the adapter discards applied markers above the installed boundary so the first events in a
  newer epoch can reuse sequence numbers. The cursor and replica state commit together.
- The CLI evaluates `authority readiness`, `authority promote`, `authority verify`, and
  `authority fork-check` using supplied evidence. Promotion planning and infrastructure cutover
  have separate owners.
- `AuthorityController::with_epoch_registry` connects a host-supplied `ExternalEpochRegistry`
  to startup, exchange, bootstrap, write admission, pre-commit checks, and recovery completion.
  A configured witness must corroborate the authority's exact epoch. A missing or expired witness
  observation blocks serving. Before an epoch-changing promotion, the host records the approved
  epoch with the witness. Promotion still requires the existing fence and approval evidence.

## Executable evidence

- `aequora-testkit --test authority_contracts` covers promotion classes, old-primary rejection,
  rollback, unresolved side effects, recovery gating, cursor binding, and same-epoch divergence.
- `aequora_testkit::model` checks lossless continuity and ambiguous operations across lossy
  promotion. `sync_invariants` covers stale-timeline writes before mutation. Stoolap's
  `durable_authority_trust_rejects_epoch_rollback_and_identity_change` covers durable rollback.
  `snapshot_epoch_switch_releases_obsolete_applied_sequences` (Stoolap) and
  `snapshot_activation_preserves_the_trusted_cursor_and_staged_boundary` (SQLite) cover
  snapshot activation and sequence reuse without losing pending intent.
- `AEQ-INV-AUTH001`–`AUTH009` and `AEQ-INV-010` own the timeline obligations.
  Storage conformance test 5, `AuthorityFencing`, binds `AEQ-INV-AUTH001`.
- `external_witness_fences_stale_instances_and_requires_recorded_promotion` covers missing
  witness data, an unrecorded epoch, stale writer context, and verified recovery on the new epoch.
- `server_rejects_exchange_and_bootstrap_from_a_rolled_back_authority` checks both server
  entry points and verifies that the rejected exchange produces no authoritative effect.

## Remaining / Left & Environment Gates

**Repository and host integration.** A deployment still supplies independent durable witness
storage, bounded freshness and availability policy, physical fencing, and orchestration of
operator approval and the PostgreSQL restore transition. SQLite store-wide epoch trust is
on hold pending explicit authorization.

**Environment.** Run the old-writer fence, restore/PITR, rollback-witness, ambiguous-ACK recovery,
and fork-quarantine drills against the intended infrastructure. The [Part 17 report](multi-region-read-single-writer-global-completion.md)
covers routing after the writer assignment changes.
