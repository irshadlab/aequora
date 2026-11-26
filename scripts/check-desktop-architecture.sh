#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

required=(
    crates/aequora-desktop/Cargo.toml
    crates/aequora-desktop/src/runtime/mod.rs
    crates/aequora-desktop/src/ipc/mod.rs
    crates/aequora-desktop/src/agent/mod.rs
    crates/aequora-desktop/src/linux/mod.rs
    crates/aequora-desktop/src/windows/mod.rs
    crates/aequora-desktop/src/macos/mod.rs
    crates/aequora-desktop/src/update/mod.rs
    crates/aequora-testkit/tests/desktop_runtime_contracts.rs
    docs/linux-windows-macos-desktop-runtime-completion.md
)

for path in "${required[@]}"; do
    test -f "$path" || { echo "missing desktop architecture artifact: $path" >&2; exit 1; }
done

for suffix in 001 002 003 004 005 006 007 008 009; do
    rg -q "AEQ-INV-DESKTOP${suffix}" crates/aequora-types/src/invariants/mod.rs
    rg -q "AEQ-INV-DESKTOP${suffix}" crates/aequora-desktop/src/runtime/mod.rs
    rg -q "AEQ-INV-DESKTOP${suffix}" crates/aequora-adapter-sdk/src/certification/mod.rs
done

if rg -n 'tokio|async-std|sqlx|stoolap|rusqlite|reqwest|windows-sys|objc|appkit|gtk' \
    crates/aequora-desktop/Cargo.toml; then
    echo "neutral desktop crates gained a runtime, transport, database, or OS-framework dependency" >&2
    exit 1
fi

rg -q 'DesktopClientFull' crates/aequora-adapter-sdk/src/certification/mod.rs
rg -q 'DesktopAgentFull' crates/aequora-adapter-sdk/src/certification/mod.rs
rg -q 'CoordinationSnapshot' crates/aequora-desktop/src/runtime/mod.rs
rg -q 'SessionToken' crates/aequora-desktop/src/ipc/mod.rs
rg -q 'peer_is_current_user' crates/aequora-desktop/src/agent/mod.rs
rg -q 'SecretService' crates/aequora-desktop/src/linux/mod.rs
rg -q 'CredentialManager' crates/aequora-desktop/src/windows/mod.rs
rg -q 'Keychain' crates/aequora-desktop/src/macos/mod.rs

CARGO_BUILD_JOBS=1 cargo test -q \
    -p aequora-desktop --features platform-test-support \
    --locked --offline
CARGO_BUILD_JOBS=1 cargo test -q -p aequora-testkit --test desktop_runtime_contracts --locked --offline

echo "desktop architecture checks passed"
