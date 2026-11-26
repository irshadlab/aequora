#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

required=(
    crates/aequora-store/src/storage_core.rs
    crates/aequora-store/src/storage_profile.rs
    crates/aequora-store/src/storage_maintenance.rs
    crates/aequora-store/src/storage_backup.rs
    crates/aequora-store/src/storage_encryption.rs
    crates/aequora-store/src/storage_conformance.rs
    crates/aequora-store/src/blob_store.rs
    crates/aequora-testkit/tests/local_storage_contracts.rs
    docs/cross-platform-local-storage-completion.md
)
for path in "${required[@]}"; do
    test -f "$path" || { echo "missing local storage artifact: $path" >&2; exit 1; }
done

for suffix in 001 002 003 004 005 006 007 008 009 010; do
    rg -q "AEQ-INV-STORAGE${suffix}" crates/aequora-types/src/invariants/mod.rs
    rg -q "AEQ-INV-STORAGE${suffix}" crates/aequora-store/src/storage_core.rs
    rg -q "AEQ-INV-STORAGE${suffix}" crates/aequora-adapter-sdk/src/certification/mod.rs
done

if rg -n 'tokio|async-std|sqlx|stoolap|rusqlite|reqwest|windows-sys|objc|appkit|gtk' \
    crates/aequora-store/Cargo.toml; then
    echo "the neutral storage crate gained a runtime, transport, database, or OS-framework dependency" >&2
    exit 1
fi

rg -q 'MobileLocalStoreFull' crates/aequora-store/src/storage_conformance.rs
rg -q 'DesktopLocalStoreFull' crates/aequora-store/src/storage_conformance.rs
rg -q 'RebindAndRebase' crates/aequora-store/src/storage_backup.rs
rg -q 'ReadMostly' crates/aequora-store/src/storage_core.rs
rg -q 'SecureProvider' crates/aequora-store/src/storage_encryption.rs

CARGO_BUILD_JOBS=1 cargo test -q -p aequora-store --locked --offline
CARGO_BUILD_JOBS=1 cargo test -q -p aequora-testkit --test local_storage_contracts --locked --offline

echo "local storage architecture checks passed"
