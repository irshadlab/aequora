# Part 01 formal correctness and deterministic verification

The governing specification is [Part 01](../sys-arch/01-formal-correctness.md).

## Stable contracts and implemented behavior

- `aequora_types::invariants::InvariantId` owns `AEQ-INV-001` through `AEQ-INV-010`.
  The IDs cover authoritative idempotency, Tx A intent, Tx B publication, Tx C cursor safety,
  monotonic versions, authorization, retry preservation, reconciliation, tombstones, and timelines.
- `aequora_testkit::model::Model` models separate client, authority, and network state.
  `explore` checks bounded interleavings, including duplicate delivery and lost responses.
  `FailureTrace::replay` checks the version and initial state digest before replaying the
  trace. The CLI exposes `verify model` and `verify trace`.
- `aequora-client` uses a single-flight reservation checked with Loom. The shared
  `aequora_testkit::contracts` adapter suites check atomic storage behavior outside the model.

## Executable evidence

- `aequora-testkit`: `model_based`, `property_invariants`, `sync_invariants`, and model unit tests.
- `aequora-client`: `loom_single_flight_reservation_never_has_two_owners`.
- Storage conformance IDs 1–4 cover local intent atomicity, authoritative publication atomicity,
  authoritative idempotency, and cursor safety. The common contract is exercised by the
  in-memory reference and official adapters. See the [ACID report](acid-compliance.md) for
  concrete adapter commands and [Part 48](testkit-verification-quality-gates-completion.md)
  for the verification profiles and fault schedules.

## Remaining / Left & Environment Gates

**Repository.** The checked-in explorer and property generators establish bounded evidence.
Additional interleavings and larger campaigns extend coverage under Part 48.

**Environment.** Live PostgreSQL/Neon, real process kills on supported filesystems, and extended
fault campaigns require their target infrastructure and separate execution records.
