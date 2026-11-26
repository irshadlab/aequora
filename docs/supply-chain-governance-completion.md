# Part 49 supply-chain governance completion

Part 49 is implemented at the reusable repository boundary. The implementation keeps dependency
and release policy independent of Cargo scanners, advisory services, CI vendors, signing systems,
SBOM encoders, storage engines, transports, and operational providers.

## Executable contracts

- aequora-supply-chain owns versioned dependency classifications and risk tiers, license/source
  admission, owned expiring exceptions and risk acceptance, per-artifact SBOM binding, semantic
  dependency-update evidence, offline controlled-build evidence, reproducibility levels, and
  third-party service failure boundaries.
- The checked-in supply-chain RON files record reviewed Tier 2/3 dependencies, approved sources,
  current exceptions, critical native/security risks, replacement plans, and optional provider
  behavior. Unknown license/source data fails closed.
- deny.toml governs the complete transitive graph; Cargo.lock is the release resolution authority.
  CI retains RustSec advisory scanning and adds pinned cargo-deny license, ban, and source checks.
- cargo xtask supply-chain deterministically emits an SPDX 2.3 document, a
  duplicate/native/build-script/proc-macro/license/source dependency report, and release-specific
  third-party license/notice inventories from locked Cargo metadata. The SPDX document binds the
  artifact, source commit, and lockfile digest.
- The CLI exposes dependency verification/explanation plus supply-chain summary, SBOM validation,
  and release-evidence verification without accepting secret material.
- Workspace publication is deny-by-default through release/publish.ron; the publisher
  ignores crates not explicitly classified as public and rejects stale unknown allowlist entries.

## Registered evidence

AEQ-INV-SUPPLY001 through AEQ-INV-SUPPLY010 are registered in aequora-invariants and the durable
certification registry. SupplyChainFull maps stable conformance test IDs 174 through 183 to those
invariants. The focused testkit suite covers fail-closed policy, exact SBOM/build identity,
authority-impact update verification, honest reproducibility claims, and non-authoritative
provider behavior.

The structural gate cargo xtask check supply-chain checks required artifacts, invariant
registration, neutral crate dependencies, resolved license/source metadata, immutable Git-source
policy, CLI behavior, generated SPDX/notices, and focused Rust contracts under locked offline
resolution.

## Release and host responsibilities

The repository can prove data-model validation, deterministic generation, dependency boundaries,
and offline locked builds. A distributor still owns legal review, private vulnerability intake,
protected or hardware-backed signing keys, independent rebuild infrastructure, upstream license
file packaging, release approval, artifact repository controls, continuous advisory response, and
profile-specific platform/container/mobile SBOM acceptance. No Level 3 byte-reproducibility or
universal legal-compliance claim is made without that external evidence.

## Verification

The completion gate is:

    cargo xtask check supply-chain

Repository-wide acceptance additionally uses formatting, strict Clippy, Rust 1.87 MSRV, warning-
denied Rustdoc, workspace tests, Guppy workspace policy, database-neutrality profiles, registry
verification, and git diff checks.

Validated on 2026-09-08: the focused Part 49 gate, cargo-deny license/ban/source checks, registry
generation and verification, all workspace tests and doctests, strict Clippy, Rust 1.87 MSRV,
warning-denied Rustdoc, Guppy workspace policy, database-neutrality profiles, formatting, and diff
checks passed. The full workspace test run required permission for its loopback HTTP acceptance
test; the permitted run passed. Legal review, signing infrastructure, and independent rebuild
attestation remain distributor-owned evidence and were not claimed.

## Crate consolidation

The root and archived client lockfiles retain the same third-party package versions,
crates.io sources, and checksums after the CLI, platform, and core moves. The existing
BLAKE3 native build and its versioned-digest replacement plan follow the certification,
registry, and authority modules. Serde and Postcard continue to encode the same
typed contracts under their existing compatibility policy. The software crypto
providers moved into `aequora_security::crypto`. The `crypto` feature activates the
provider dependencies for consumers that previously imported the crypto package.
Security retains ownership and the `CryptoProvider` replacement boundary in the
[supply-chain policy](../supply-chain/policy.ron). Registry lock verification,
crypto and protocol tests, and database-neutrality checks cover the affected boundaries.


The protocol consolidation carries ten previously approved production dependencies
into `aequora-protocol`. BLAKE3 retains its reviewed crates.io source, CC0 or Apache
license terms, optional native acceleration, and versioned digest replacement boundary.
Serde and Postcard retain the existing wire-encoding policies. `cargo xtask check
protocol` verifies the resolved production graph and rejects database, runtime, and
transport-framework packages. The protocol unit and metadata integration tests verify
the consolidated wire contracts.

The xtask uses the locked crates.io versions of SHA-2 for artifact digests and
getrandom for the chaos harness's ephemeral authentication key. Both declare MIT
or Apache-2.0. SHA-2 has no build script. Getrandom retains its platform entropy
backends and build script. These dependencies are confined to developer tooling
and keep the release digest provider and platform entropy provider boundaries
recorded in the supply-chain policy. The local supply-chain gate verifies the
generated artifact evidence, and the chaos configuration test verifies the
loopback endpoint and scenario definitions.
