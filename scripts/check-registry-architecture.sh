#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

for path in \
    crates/aequora-registry/Cargo.toml \
    crates/aequora-registry/src/registry.rs \
    crates/aequora-cli/src/bin/aequora-registry.rs \
    registry/manifest.ron registry/registry.lock \
    docs/registry/generated-registry.md; do
    test -f "$path"
done

for directory in registry/core registry/app registry/extensions; do
    test -d "$directory"
done

for suffix in 001 002 003 004 005 006 007 008 009; do
    rg -q "AEQ-INV-REGISTRY${suffix}" crates/aequora-types/src/invariants/mod.rs
done

if rg -q '(^|[^[:alnum:]_-])(sqlx|stoolap|tokio|axum|quinn|reqwest)([^[:alnum:]_-]|$)' \
    crates/aequora-registry/Cargo.toml; then
    echo "registry architecture: runtime, transport, or database dependency in registry runtime layers" >&2
    exit 1
fi

for domain in Entity Operation Event Field Capability ConsistencyProfile Error Job Consumer \
    AuditAction Migration Protocol Message Permission AdminAction Reason DecisionRule ArtifactFormat \
    ConformanceProfile ConformanceTest CertificationTier; do
    rg -q "${domain}" crates/aequora-registry/src/types.rs
done

rg -q 'pub use aequora_registry as registry' crates/aequora/src/lib.rs
rg -q 'durable_registry_provenance' crates/aequora-diagnostics/src/lib.rs
rg -q 'cargo .*aequora-cli.*verify' .github/workflows/ci.yml

CARGO_BUILD_JOBS=1 cargo run -q -p aequora-cli --locked --bin aequora-registry -- verify . >/dev/null
CARGO_BUILD_JOBS=1 cargo run -q -p aequora-cli --locked --bin aequora-registry -- rust . --check >/dev/null

echo "registry architecture: canonical RON, immutable lock, code generation, namespace/range checks, compatibility tooling, historical resolution, diagnostics provenance, and nine invariants verified"
