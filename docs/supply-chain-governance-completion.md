# Part 49 supply-chain governance completion

Governing specification is [Part 49](../sys-arch/49-licensing-dependency-policy-supply-chain-security-crate-governance-sbom-reproducible-builds-third-party-risk-architecture.md).

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
- cargo xtask supply-chain uses `spdx-rs` and `spdx-expression` to emit an SPDX 2.3 document, a
  duplicate/native/build-script/proc-macro/license/source dependency report, and release-specific
  third-party license/notice inventories from locked Cargo metadata. The SPDX document binds the
  artifact, source commit, and lockfile digest. Package references are independent of the absolute
  checkout path. Cargo manifest licenses are declared licenses and supplier and concluded-license
  fields use SPDX's `NOASSERTION` when those facts are unavailable.
- The CLI exposes dependency verification/explanation plus supply-chain summary, SBOM validation,
  and release-evidence verification without accepting secret material.
- Workspace publication is deny-by-default through release/publish.ron; the publisher
  ignores crates not explicitly classified as public and rejects stale unknown allowlist entries.

## Dependency adoption review

The owners, approved registry, licenses, capability boundaries, and replacement points are recorded
in [the dependency policy](../supply-chain/policy.ron). The root lockfile records the resolved
transitive versions.

| Dependency and boundary | Transitive and build impact | Required semantic verification |
|---|---|---|
| Clap, `clap_complete`, and `clap_mangen` in `aequora-cli`. [Upstream](https://github.com/clap-rs/clap) | Clap and its derive macro were already resolved. The generators add ROFF output support with no native compiler or build script. | Check existing command spelling, argument forwarding, help and machine error envelopes, registry verification, and generated shell and manual output. |
| Governor in `aequora-axum`. [Upstream](https://github.com/boinkor-net/governor) | Uses the in-memory limiter with `std` and excludes the default DashMap, jitter, and Quanta features. The new transitive timer, nonzero and atomic support are Rust code. `portable-atomic` has an architecture-selection build script. | Check burst, refill, saturation and idle/oldest eviction with resident active tenants. Preserve the zero burst and zero refill cases and the existing HTTP error contract. |
| `prometheus-client` behind `aequora-server/prometheus`. [Upstream](https://github.com/prometheus/client_rust) | Adds formatting and label-encoding support, including a Rust derive macro. The crate build script is a no-op with protobuf disabled. The server's direct dependency budget includes this optional recorder. | Check stable metric names, finite values, bounded series and latency storage, and payload-free observer output. |
| Secrecy in `aequora-config`. [Upstream](https://github.com/iqlusioninc/crates/tree/main/secrecy) | Reuses Zeroize. Serde integration stays disabled and there is no build script or native dependency. | Check redacted debug output, explicit value access, zeroizing ownership, and the existing configuration precedence and validation. |
| `spdx-rs` and `spdx-expression` in `xtask`. [Upstream SPDX model](https://github.com/doubleopen-project/spdx-rs), [upstream license expression parser](https://github.com/doubleopen-project/spdx-expression) | Adds Rust parser and derive dependencies. Cargo metadata remains the graph source. CycloneDX's `time` dependency requires Rust 1.88 for the patched versions, so the existing SPDX 2.3 format uses a library compatible with the workspace baseline. [RustSec advisory](https://rustsec.org/advisories/RUSTSEC-2026-0009.html) | Round-trip the SPDX document and verify package relationships, format version, artifact checksum, lockfile binding, and release notes generation. |

## Registered evidence

AEQ-INV-SUPPLY001 through AEQ-INV-SUPPLY010 are registered in aequora_types::invariants and the durable
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
