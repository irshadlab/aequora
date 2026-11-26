#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

required=(
    archive/aequora-dioxus/Cargo.toml
    archive/aequora-dioxus/src/lib.rs
    archive/aequora-dioxus/src/client_handle.rs
    archive/aequora-dioxus/src/provider.rs
    archive/aequora-dioxus/src/hooks/query.rs
    archive/aequora-dioxus/src/hooks/mutation.rs
    archive/aequora-dioxus/src/hooks/conflicts.rs
    archive/aequora-dioxus/src/hooks/scope.rs
    archive/aequora-dioxus/src/hooks/bootstrap.rs
    archive/aequora-dioxus/src/hooks/lifecycle.rs
    docs/dioxus-client-integration-completion.md
)
for path in "${required[@]}"; do
    test -f "$path" || { echo "missing Part 40 artifact: $path" >&2; exit 1; }
done

for suffix in 001 002 003 004 005 006 007 008 009 010; do
    rg -q "AEQ-INV-DIOXUS${suffix}" crates/aequora-types/src/invariants/mod.rs
    rg -q "AEQ-INV-DIOXUS${suffix}" crates/aequora-adapter-sdk/src/certification/mod.rs
    rg -q "AEQ-INV-DIOXUS${suffix}" registry/core/certification.ron
    rg -q "AEQ-INV-DIOXUS${suffix}" \
        sys-arch/40-dioxus-client-integration-reactive-state-architecture.md
done

for symbol in \
    AequoraHandle QueryNamespace QueryKey QueryState MutationState MutationHandle \
    SyncStatus BootstrapState ScopeState Invalidation UiError QueryError \
    provide_aequora provide_aequora_with_namespace use_aequora_query \
    use_aequora_mutation use_sync_status use_conflicts use_operation use_aequora_scope; do
    rg -q "\b${symbol}\b" archive/aequora-dioxus/src
done

rg -Fq 'dioxus = { version = "=0.7.10", default-features = false, features = ["hooks", "signals"] }' archive/aequora-dioxus/Cargo.toml
rg -Fq 'watch::channel(initial_notice)' archive/aequora-dioxus/src/client_handle.rs
rg -Fq 'while receiver.changed().await.is_ok()' archive/aequora-dioxus/src/hooks/events.rs
rg -Fq 'MutationState::SavedLocally(receipt)' archive/aequora-dioxus/src/hooks/mutation.rs
rg -Fq 'handle.invalidate(Invalidation::Key(key))' archive/aequora-dioxus/src/hooks/mutation.rs
rg -q 'MAX_TARGETED_INVALIDATIONS: usize = 64' archive/aequora-dioxus/src/hooks/mutation.rs
rg -Fq 'QueryNamespace' archive/aequora-dioxus/src/provider.rs

if rg -n 'dioxus' crates/aequora-client/Cargo.toml crates/aequora-store/Cargo.toml \
    crates/aequora-operation/Cargo.toml crates/aequora-adapter-sdk/Cargo.toml; then
    echo "Dioxus dependency leaked below the UI integration boundary" >&2
    exit 1
fi

if rg -n 'sqlx|stoolap|axum|reqwest|quinn' archive/aequora-dioxus/Cargo.toml \
    archive/aequora-dioxus/src; then
    echo "physical storage or transport framework leaked into the Dioxus boundary" >&2
    exit 1
fi

rg -q '"rust-analyzer.checkOnSave": false' .vscode/settings.json
rg -q '"rust-analyzer.numThreads": 1' .vscode/settings.json
rg -q '"rust-analyzer.cachePriming.enable": false' .vscode/settings.json
rg -q '"rust-analyzer.procMacro.enable": false' .vscode/settings.json

CARGO_BUILD_JOBS=1 cargo test -q --manifest-path archive/aequora-dioxus/Cargo.toml \
    --target-dir target --locked --offline
CARGO_BUILD_JOBS=1 cargo test -q -p aequora-types -p aequora-adapter-sdk \
    --locked --offline
CARGO_BUILD_JOBS=1 cargo run -q -p aequora-cli --offline --bin aequora-registry \
    -- verify .

echo "Dioxus client integration architecture checks passed"
