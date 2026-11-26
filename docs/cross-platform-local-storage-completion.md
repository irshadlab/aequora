# Part 33 cross-platform local storage completion evidence

The authoritative design is [Part 33](../sys-arch/33-cross-platform-local-storage-mobile-desktop-architecture.md). This report records executable repository evidence and does not redefine that architecture.

## Implemented boundary

- `aequora_store::storage_core` defines the five storage classes, durability/admission/pressure states, canonical errors, safe platform path policy, format downgrade checks, secure binding classification, mandatory adapter capabilities, and verify-before-publish state transitions.
- `aequora_store::storage_profile` supplies mobile and desktop presets, conservative free-space preflight, read-mostly pressure transitions, and intent-safe eviction decisions.
- `aequora_store::storage_maintenance`, `aequora_store::storage_backup`, and `aequora_store::storage_encryption` define bounded checkpointed migration/GC, transactionally consistent manifests and restore classification, resumable key rotation, and legal-hold-aware cryptographic erase.
- `aequora_store::blob_store` supplies content-addressed BLAKE3 verification, pin/reconstructability classification, and manifest-last publication.
- `aequora_store::storage_conformance` binds full mobile/desktop certification to an adapter version, exact target, mandatory capabilities, shared tests, and platform-specific tests; missing or unsupported observations fail the claim.
- The facade, testkit, invariant registry, durable certification registry, CI, and focused architecture gate include the Part 33 surface.

The `blake3` dependency moved with `blob_store` to keep content-addressed verification at
the storage-contract boundary. Cargo still resolves the same crates.io package and checksum,
with no added transitive packages or build scripts. The package metadata declares
`CC0-1.0 OR Apache-2.0 OR Apache-2.0 WITH LLVM-exception` and uses the existing native code
and build script. Security retains ownership and the versioned digest provider replacement
plan recorded in [the supply-chain policy](../supply-chain/policy.ron). The store unit
tests and testkit storage contracts exercise digest verification and publication ordering.

## Deployment ownership

Applications select and configure an embedded engine, materialize the documented private/cache/diagnostic paths through native platform APIs, connect Android Keystore, iOS/macOS Keychain, Windows DPAPI/Credential Manager, Linux Secret Service or an explicit fallback, and establish backup inclusion/exclusion rules. Adapter maintainers run destructive crash, process-kill, low-disk, clone, locking, antivirus, sleep/resume, and key-loss tests on each supported target and publish the resulting certification record. The reusable core does not claim those deployment observations on behalf of an adapter.

SQLite and Stoolap remain adapters rather than architectural dependencies. Part 33 supplies the target-neutral contract and target-bound certification model; official adapter SDK and engine implementation work remains governed by the later adapter specification.

## Verification

The focused gate is `bash scripts/check-local-storage-architecture.sh`. It checks required artifacts, all ten invariant/profile integrations, neutral dependency manifests, focused crate tests, and testkit integration contracts. Repository-wide formatting, Clippy, tests, Rustdoc, Guppy direction, database neutrality, registry generation, and diff checks are recorded in the final task handoff after execution.
