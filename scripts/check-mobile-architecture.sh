#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

required=(
    crates/aequora-mobile/Cargo.toml
    crates/aequora-mobile/src/runtime/mod.rs
    crates/aequora-mobile/src/android/mod.rs
    crates/aequora-mobile/src/ios/mod.rs
    crates/aequora-mobile/src/bindings/mod.rs
    crates/aequora-testkit/tests/mobile_runtime_contracts.rs
    docs/android-ios-mobile-runtime-platform-completion.md
)

for path in "${required[@]}"; do
    test -f "$path" || { echo "missing mobile architecture artifact: $path" >&2; exit 1; }
done

for suffix in 001 002 003 004 005 006 007 008 009; do
    rg -q "AEQ-INV-MOBILE${suffix}" crates/aequora-types/src/invariants/mod.rs
    rg -q "AEQ-INV-MOBILE${suffix}" crates/aequora-mobile/src/runtime/mod.rs
    rg -q "AEQ-INV-MOBILE${suffix}" crates/aequora-adapter-sdk/src/certification/mod.rs
done

if rg -n 'tokio|jni|ndk|swift|objc|uikit|android[_-]context|sqlx|stoolap|rusqlite|reqwest' \
    crates/aequora-mobile/Cargo.toml; then
    echo "platform-neutral mobile runtime gained a platform, runtime, transport, or database dependency" >&2
    exit 1
fi

rg -q 'MobileClientFull' crates/aequora-adapter-sdk/src/certification/mod.rs
rg -q 'AEQUORA_MOBILE_ABI_VERSION' crates/aequora-mobile/src/runtime/mod.rs
rg -q 'catch_unwind' crates/aequora-mobile/src/bindings/mod.rs
rg -q 'WorkManagerGrant' crates/aequora-mobile/src/android/mod.rs
rg -q 'BgTaskGrant' crates/aequora-mobile/src/ios/mod.rs

CARGO_BUILD_JOBS=1 cargo test -q \
    -p aequora-mobile --features platform-test-support \
    --locked --offline
CARGO_BUILD_JOBS=1 cargo test -q -p aequora-testkit --test mobile_runtime_contracts --locked --offline

echo "mobile architecture checks passed"
