# Part 18 backpressure, admission, fairness, and overload

The governing specification is [Part 18](../sys-arch/18-backpressure-admission-fairness-overload.md).

## Stable contracts and implemented behavior

- `aequora_authority::admission::HierarchicalAdmission::try_admit` checks request shape, load
  state, global/tenant/class ceilings, and resource budgets. `AdmissionPermit` releases the
  counters on drop. Reserved slots fit the effective global ceiling in class priority order.
  Classes shed by the current load state have no reservation for new work. Work already admitted
  continues to count against the effective ceiling until its permit drops.
- `FairQueue` bounds total, class, and tenant occupancy and combines weighted class rotation
  with aging. `LoadStateTracker` implements hysteresis, and `BrownoutPolicy` preserves required
  security checks. Axum limits pre-body admission, bytes, and request duration. The server checks
  admitted work before entering the authoritative transaction.
- `AequoraConfig::admission_controller` builds the shared controller from strict RON settings.
  Hosts pass the same controller to `AdmittedExchangeService` and feed normalized observations
  to `HierarchicalAdmission::observe_load`. Hysteresis and admission use one shared state.
- `SqlxPostgresBackend::pool_pressure_permille` samples busy slots against configured pool
  capacity without querying the database. Hosts can feed the sample into `LoadSignals::database`.
- `FairQueue::dispatch` reserves capacity before removing work. Failed reservations preserve
  order and accounting, and another tenant or class can still dispatch. Hosts use
  `HierarchicalAdmission::try_admit_optional` for explicitly optional work and hold the
  returned permit through execution. Durable discovery and intent stay in the owning store.
- Queue and permit byte accounting reject overflows even at the largest configured budget.

## Executable evidence

- `cargo test -p aequora-authority admission` exercises permit release, tenant fairness,
  reconnect saturation and recovery, queue aging, and load shedding, including
  `critical_load_reserves_only_admissible_capacity_in_priority_order`.
- `pressure_samples_shed_and_restore_work_on_the_shared_controller` verifies dwell time,
  shedding, and recovery. `admission_controller_uses_configured_pressure_thresholds` verifies
  that configured thresholds reach the controller.
- Admission tests cover same-class tenant saturation, brownout deferral and recovery, and
  queue byte overflow. `postgres_live::authoritative_transaction_snapshot_and_compaction_are_real`
  samples a real half-full and saturated pool before the transaction contract.
- `cargo test -p aequora-axum --lib` and `cargo xtask check axum-server` cover transport
  admission and bounded request handling. The stable invariant registry contains
  `AEQ-INV-LOAD001`–`LOAD009`. Part 39 conformance evidence covers the HTTP integration.

## Remaining / Left & Environment Gates

**Repository and host integration.** PostgreSQL pool sampling and permit-backed worker queue
dispatch are available. Hosts still own the sampling cadence, system pressure sources beyond
the pool, durable discovery, and execution for their background, blob, and snapshot workers.

**Environment.** Validate configured limits and workload fairness with production-shaped mixed
traffic, connection surges, slow consumers, storage/pool saturation, and soak runs.
