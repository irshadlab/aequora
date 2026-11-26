# Part 23 completion: background jobs, durable workflows, and side effects

The governing specification is
[`sys-arch/23-background-jobs-durable-workflows-side-effects.md`](../sys-arch/23-background-jobs-durable-workflows-side-effects.md).
The implementation is split into database-neutral `aequora_authority::jobs`, `aequora_authority::workflow`, and
`aequora_authority::side_effects` modules, with bounded server orchestration in `aequora_server::jobs`.

## Executable scope

| Requirement | Evidence |
|---|---|
| Durable jobs | Typed job identity/kind/state/payload records, bounded inline payloads and checkpoints, `JobStore`, status/admin contracts, and notification-independent claim requests |
| Claims and fencing | Monotonic `FencingToken`, worker-owned expiring `JobLease`, compare-and-swap `FencedUpdate`, and executable stale-worker validation |
| Retry and bulkheads | Typed error taxonomy, bounded exponential backoff with mandatory jitter, provider retry-after, concurrency classes, worker capabilities, bounded claim batches, and server-side permit-aware planning |
| Dependencies and timers | Bounded acyclic dependency validation, durable `next_run_at`, recurring schedules, missed-occurrence policy, and deterministic unique occurrence identity |
| Durable workflows | Versioned workflow records, optimistic row versions, deterministic event transitions, bounded checkpoints/fan-out, explicit compensation actions, and atomic workflow-plus-child-job transaction contracts |
| Side-effect outbox | Immutable typed intent with operation/tenant/correlation lineage, payload digest, stable external idempotency key, and an authoritative-transaction write trait |
| Provider ambiguity | Explicit external outcomes, provider capability certification, reconcile/same-key/manual/suppress policies, and provider reconciliation contracts |
| Authoritative result path | `AuthoritativeResultOperation` and `AuthoritativeOperationSink` require provider results that affect business state to re-enter the authoritative handler pipeline |
| Epoch and region recovery | Authority-epoch recovery actions and authority-only/allowed/specific-region placement policies |
| Governance | Retention class, PII/legal-hold metadata, subject/artifact references, and side-effect retention metadata cover pending jobs and external intents |
| Operations | Tenant-authorized status/admin service, supported-kind/capability-aware claiming, bounded polling inputs, graceful stop-claiming state, and stable diagnostic invariant entries |
| Correctness registry | `AEQ-INV-JOB001` through `AEQ-INV-JOB009` in the workspace invariant registry |

`aequora_server::jobs::JobWorkerPlanner::poll` performs one bounded claim against a supplied
`JobStore`. It skips polling during shutdown or when the caller has reserved zero permits.
Returned claims must match the actual batch limit and pass lease, payload version, capability,
worker, and region checks. Failed validation leaves recovery to the durable store's lease policy.
`JobRegistry::register` preserves the existing descriptor on duplicate registration.

## Remaining / Left & Environment Gates

**Repository and adapter integration.** PostgreSQL persists hook-produced intents into
`aequora_side_effect_jobs` in Tx B. The live adapter test proves one pending intent after a
successful commit and verifies transactional rollback on failure. A production claimant and
concrete implementations of the full `JobStore`, `WorkflowStore`, and `SideEffectStore`
contracts remain open. Physical adapters must implement the store traits with atomic insert,
claim, lease, compare-and-swap, uniqueness, indexes, and transaction participation appropriate to
their database. Provider integrations live outside these crates and must honor declared timeout,
idempotency, lookup, egress, secret-reference, and reconciliation policy.

Application handlers own job kinds, payload schemas, business cancellation semantics, workflow
versions, authoritative result operations, retention policy values, provider allowlists, and admin
authorization. The reusable safety contracts and failure models run in the focused suite.

**Environment.** Qualify provider idempotency and ambiguous delivery, and measure worker
recovery and capacity against the deployed stores and providers.

## Verification

```text
CARGO_BUILD_JOBS=1 cargo test -p aequora-authority
CARGO_BUILD_JOBS=1 cargo test -p aequora-testkit --test job_workflow_side_effect_contracts
CARGO_BUILD_JOBS=1 cargo clippy -p aequora-authority -p aequora-server --all-targets -- -D warnings
cargo xtask check jobs
cargo run -q -p aequora-dev -- check
cargo xtask check database-neutrality
```

The focused suite covers lease expiry/reclaim fencing (including generated stale tokens), cyclic
dependencies, provider timeout ambiguity, stable-key idempotent retry, retry backoff, workflow
compensation, duplicate recurring schedulers, authority-epoch recovery, and governance coverage.
It also runs `worker_poll_stops_claiming_and_rejects_overclaim_or_invalid_leases` and
`duplicate_job_registration_preserves_the_original_contract`.
