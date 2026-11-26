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

## Executable evidence

- `cargo test -p aequora-authority admission` exercises permit release, tenant fairness,
  reconnect saturation and recovery, queue aging, and load shedding, including
  `critical_load_reserves_only_admissible_capacity_in_priority_order`.
- `cargo test -p aequora-axum --lib` and `cargo xtask check axum-server` cover transport
  admission and bounded request handling. The stable invariant registry contains
  `AEQ-INV-LOAD001`–`LOAD009`. Part 39 conformance evidence covers the HTTP integration.

## Remaining / Left & Environment Gates

**Repository.** The fair queue, load tracker, and brownout policy are reusable components.
Automatic feedback from production resource gauges and fair queuing for independently hosted
background, blob, and snapshot workers require host composition. Exchange saturation follows
bounded synchronous rejection.

**Environment.** Validate configured limits and workload fairness with production-shaped mixed
traffic, connection surges, slow consumers, storage/pool saturation, and soak runs.
