#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

required=(
    crates/aequora-adapter-sdk/src/certification/verification.rs
    crates/aequora-testkit/src/verification.rs
    crates/aequora-testkit/tests/verification_quality_gates.rs
    tests/fixtures/part48/core-library-pass.ron
    docs/testkit-verification-quality-gates-completion.md
)
for path in "${required[@]}"; do
    test -f "$path" || { echo "missing Part 48 artifact: $path" >&2; exit 1; }
done

for suffix in 001 002 003 004 005 006 007 008 009 010; do
    rg -q "AEQ-INV-VERIFY${suffix}" crates/aequora-types/src/invariants/mod.rs
    rg -q "AEQ-INV-VERIFY${suffix}" crates/aequora-adapter-sdk/src/certification/mod.rs
    rg -q "AEQ-INV-VERIFY${suffix}" registry/core/certification.ron
    rg -q "AEQ-INV-VERIFY${suffix}" sys-arch/48-*.md
done

for symbol in QualityGateManifest GateWaiver VerificationReport InvariantCoverageMap \
    ReleaseProfile Qualification SuiteId EvidenceRef; do
    rg -q "\b${symbol}\b" crates/aequora-adapter-sdk/src/certification/verification.rs
done
for symbol in TestContext TestSeed DeterministicClock DeterministicIdSource FaultController \
    FaultAction GoldenFixture TestConfiguration; do
    rg -q "\b${symbol}\b" crates/aequora-testkit/src/verification.rs
done
for symbol in FailureTrace SearchBounds explore; do
    rg -q "\b${symbol}\b" crates/aequora-testkit/src/model.rs
done

for scenario in retrying_an_operation_returns_the_ledger_result_without_a_second_effect \
    every_authoritative_transaction_phase_is_retry_safe \
    lost_response_after_commit_retries_to_the_same_logical_effect \
    snapshot_bootstrap_resumes_from_durable_staging_progress \
    tombstones_remain_in_authoritative_state_and_journal \
    spoofed_operation_tenant_is_rejected_without_an_effect; do
    rg -q "${scenario}" crates/aequora-testkit/tests/sync_invariants.rs
done

test "$(find config/test -maxdepth 1 -type f -name '*.ron' | wc -l)" -eq 4

if rg -n '(sqlx|stoolap|rusqlite|tokio|axum|reqwest|quinn|dioxus|opentelemetry|prometheus)::' crates/aequora-testkit/src/model.rs; then
    echo "runtime, transport, or database API leaked into neutral verification code" >&2
    exit 1
fi

for manifest in crates/aequora-adapter-sdk/Cargo.toml; do
    if rg -n 'sqlx|stoolap|rusqlite|tokio|axum|reqwest|quinn|dioxus|opentelemetry|prometheus' "$manifest"; then
        echo "runtime, transport, database, UI, or telemetry vendor leaked into neutral verification contracts" >&2
        exit 1
    fi
done

normal_dependents="$(CARGO_BUILD_JOBS=1 cargo tree -e normal -i aequora-testkit \
    --prefix none --locked --offline | cut -d' ' -f1 | LC_ALL=C sort -u)"
test "$normal_dependents" = "$(printf '%s\n' aequora-cli aequora-testkit)" || {
    echo "unexpected normal dependents of aequora-testkit:" >&2
    echo "$normal_dependents" >&2
    exit 1
}

rg -q 'aequora verify release' crates/aequora-cli/src/lib.rs
rg -q 'aequora verify report' crates/aequora-cli/src/lib.rs
rg -q 'aequora verify replay' crates/aequora-cli/src/lib.rs
rg -q 'VerificationFull' crates/aequora-adapter-sdk/src/certification/mod.rs
rg -q 'VerificationFull' registry/core/certification.ron

CARGO_BUILD_JOBS=1 cargo test -q -p aequora-types -p aequora-adapter-sdk \
    -p aequora-testkit --lib --locked --offline
CARGO_BUILD_JOBS=1 cargo test -q -p aequora-testkit \
    --test verification_quality_gates --test property_invariants --test model_based \
    --test sync_invariants --locked --offline
CARGO_BUILD_JOBS=1 cargo test -q -p aequora-cli --lib --locked --offline
CARGO_BUILD_JOBS=1 cargo run -q -p aequora-cli --locked --offline -- verify model \
    | rg -q 'model: ok version='
CARGO_BUILD_JOBS=1 cargo run -q -p aequora-cli --locked --offline -- verify release \
    tests/fixtures/part48/core-library-pass.ron | rg -q 'release: qualified'

echo "verification architecture: ok (4 profiles, 10 invariants, 10 conformance tests)"
