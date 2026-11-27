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
| PostgreSQL job store | `aequora_store_postgres::job_store::PostgresJobStore` implements `JobStore`. Its transaction-scoped insert validates tenant, epoch, and dependency lineage. Claims use a bounded indexed poll with row locking, append attempt records, and fence updates with database time. Expired work with ambiguous external delivery is quarantined. |
| PostgreSQL workflow store | `aequora_store_postgres::workflow_store::PostgresWorkflowStore::apply_with_jobs` commits the workflow version, event, checkpoint, and action record with all child jobs. Invalid child lineage or any failed child insert rolls the batch back. |
| PostgreSQL typed side-effect store | `aequora_store_postgres::side_effect_store::PostgresSideEffectWriter` binds immutable intents to the operation ledger in Tx B. `PostgresSideEffectStore::materialize_job` records a single durable job projection. Provider execution records use a live job lease and expected-state compare-and-swap. |

`aequora_server::jobs::JobWorkerPlanner::poll` performs one bounded claim against a supplied
`JobStore`. It skips polling during shutdown or when the caller has reserved zero permits.
Returned claims must match the actual batch limit and pass lease, payload version, capability,
worker, and region checks. Failed validation leaves recovery to the durable store's lease policy.
`JobRegistry::register` preserves the existing descriptor on duplicate registration.

## Remaining / Left & Environment Gates

**Repository and host integration.** The host still needs to drive worker execution, workflow
timers and events, recurring occurrence persistence, typed outbox projection and reconciliation,
provider dispatch, and authoritative result publication through the normal operation pipeline.
The workflow store rejects transition actions that would emit an authoritative operation
until that pipeline is wired into the transition. Existing hook-produced jobs continue to be
recorded in `aequora_side_effect_jobs` inside Tx B.

Application handlers own job kinds, payload schemas, business cancellation semantics, workflow
versions, authoritative result operations, retention policy values, provider allowlists, and admin
authorization. The reusable safety contracts and failure models run in the focused suite.

**Environment.** Qualify provider idempotency and ambiguous delivery, and measure worker
recovery and capacity against the deployed stores and providers.

## Verification

```text
CARGO_BUILD_JOBS=1 cargo test -p aequora-authority
CARGO_BUILD_JOBS=1 cargo test -p aequora-testkit --test job_workflow_side_effect_contracts
AEQUORA_TEST_POSTGRES_RUNTIME_URL=<disposable-postgresql-url> cargo test -p aequora-store-postgres --test postgres_jobs
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

The CI PostgreSQL 18 job runs `native_job_store_preserves_atomicity_dependencies_and_leases`
against a dedicated database. It covers insertion rollback, dependency and tenant checks,
concurrent claims, mixed payload versions, stale-worker rejection, workflow and child rollback, Tx B intent publication,
single job projection, and provider outcome fencing. The same case passed locally on PostgreSQL 18.
