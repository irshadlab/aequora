#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

required=(
    crates/aequora-config/src/part43.rs
    crates/aequora-security/Cargo.toml
    crates/aequora-security/src/secrets/mod.rs
    crates/aequora-security/Cargo.toml
    crates/aequora-security/src/policy/mod.rs
    crates/aequora-security/Cargo.toml
    crates/aequora-security/src/feature_flags/mod.rs
    crates/aequora-testkit/tests/configuration_policy_contracts.rs
    config/production.ron
    docs/configuration-secrets-runtime-policy-feature-flags-completion.md
)
for path in "${required[@]}"; do
    test -f "$path" || { echo "missing Part 43 artifact: $path" >&2; exit 1; }
done

for suffix in 001 002 003 004 005 006 007 008 009 010; do
    rg -q "AEQ-INV-CONFIG${suffix}" crates/aequora-types/src/invariants/mod.rs
    rg -q "AEQ-INV-CONFIG${suffix}" crates/aequora-adapter-sdk/src/certification/mod.rs
    rg -q "AEQ-INV-CONFIG${suffix}" registry/core/certification.ron
    rg -q "AEQ-INV-CONFIG${suffix}" \
        sys-arch/43-configuration-secrets-environment-profiles-runtime-policy-feature-flags-architecture.md
done

for symbol in \
    RawDeploymentConfig ValidatedDeploymentConfig EffectiveConfig ConfigLoader ConfigSchemaVersion \
    Environment ConfigSource AdapterCapabilities CONFIGURATION_REFERENCE; do
    rg -q "\b${symbol}\b" crates/aequora-config/src/part43.rs
done
for symbol in \
    SecretRef SecretBytes SecretString SecretProvider EnvironmentSecretProvider SecretResolver \
    RotationClass; do
    rg -q "\b${symbol}\b" crates/aequora-security/src/secrets/mod.rs
done
for symbol in \
    RuntimePolicy ConfigGeneration ConfigDigest PolicySnapshot RuntimeConfigStore \
    AtomicRuntimeConfigStore MutabilityClass ChangeClass merge_policy; do
    rg -q "\b${symbol}\b" crates/aequora-security/src/policy/mod.rs
done
for symbol in \
    FeatureSafety RolloutStage FeatureEvaluator FeaturePolicy FeatureCache EntitlementId; do
    rg -q "\b${symbol}\b" crates/aequora-security/src/feature_flags/mod.rs
done

if rg -n 'sqlx|stoolap|rusqlite|tokio|axum|reqwest|quinn|dioxus' \
    crates/aequora-security/Cargo.toml; then
    echo "runtime, transport, UI, or physical database dependency leaked into neutral Part 43 contracts" >&2
    exit 1
fi

if rg -n 'disable_auth|ignore_cursor|skip_ledger|disable_fencing|accept_invalid_schema' \
    crates/aequora-config/src crates/aequora-security/src/policy crates/aequora-security/src/feature_flags; then
    echo "correctness-disabling switch leaked into configuration contracts" >&2
    exit 1
fi

rg -q 'Environment::Production' crates/aequora-config/src/part43.rs
rg -q 'Environment::Staging' crates/aequora-config/src/part43.rs
rg -q 'SecurityPolicyIsNotAFlag' crates/aequora-security/src/feature_flags/mod.rs
rg -q 'AtomicRuntimeConfigStore' crates/aequora-security/src/policy/mod.rs
rg -q 'config_command' crates/aequora-cli/src/lib.rs

CARGO_BUILD_JOBS=1 cargo test -q \
    -p aequora-security -p aequora-config \
    --locked --offline
CARGO_BUILD_JOBS=1 cargo test -q -p aequora-testkit \
    --test configuration_policy_contracts --locked --offline
CARGO_BUILD_JOBS=1 cargo test -q -p aequora-types -p aequora-adapter-sdk \
    --locked --offline
CARGO_BUILD_JOBS=1 cargo run -q -p aequora-cli --offline --bin aequora-registry \
    -- verify .

effective="$(CARGO_BUILD_JOBS=1 cargo run -q -p aequora-cli --locked --offline -- \
    config effective config/production.ron)"
rg -q '<redacted>' <<<"$effective"
if rg -q 'AEQUORA_DATABASE_URL|AEQUORA_TLS_PRIVATE_KEY' <<<"$effective"; then
    echo "effective configuration exposed a secret reference" >&2
    exit 1
fi

echo "configuration, secrets, runtime policy, and feature-flag architecture checks passed"
