# Aequora implementation plan

## Document ownership

The `sys-arch/` documents are the authoritative system-architecture and design specifications.
They contain the detailed protocol, data, security, performance, deployment, and operational
design. This file intentionally does not copy that design.

`plan.md` is limited to implementation sequencing, status, acceptance evidence, and repository
workflow. When architecture changes, update the applicable `sys-arch/*.md` document first; add
only a short status or link here.

Completion evidence lives in the relevant `docs/*-completion.md` reports. Those reports record
what is executable in this repository and which application or deployment responsibilities remain
outside the reusable core.

## Architecture authority index

| Part | Authoritative specification |
|---:|---|
| 01 | [Formal correctness](sys-arch/01-formal-correctness.md) |
| 02 | [Causality, provenance, and lineage](sys-arch/02-causality-provenance-lineage.md) |
| 03 | [Anti-entropy and self-repair](sys-arch/03-anti-entropy-self-repair.md) |
| 04 | [Offline compaction and rebase](sys-arch/04-offline-compaction-rebase.md) |
| 05 | [Local multiprocess coordination](sys-arch/05-local-multiprocess-coordination.md) |
| 06 | [Adaptive sync scheduler and QoS](sys-arch/06-adaptive-sync-scheduler-qos.md) |
| 07 | [Subscription scope and dynamic dataset](sys-arch/07-subscription-scope-dynamic-dataset.md) |
| 08 | [Live sync, push, and presence](sys-arch/08-live-sync-push-presence.md) |
| 09 | [Bulk import, export, seed, and migration](sys-arch/09-bulk-import-export-seed-migration.md) |
| 10 | [Large snapshot streaming and bootstrap](sys-arch/10-large-snapshot-streaming-bootstrap.md) |
| 11 | [Operation semantics and consistency profiles](sys-arch/11-operation-semantics-consistency-profiles.md) |
| 12 | [Deterministic execution and replay](sys-arch/12-deterministic-execution-replay.md) |
| 13 | [Data provenance, auditability, and explainability](sys-arch/13-data-provenance-auditability-explainability.md) |
| 14 | [Data governance, retention, and erasure](sys-arch/14-data-governance-retention-erasure.md) |
| 15 | [Cryptographic integrity, key management, and E2E](sys-arch/15-cryptographic-integrity-key-management-e2e.md) |
| 16 | [Authority failover, epochs, and fork detection](sys-arch/16-authority-failover-timeline-epochs-fork-detection.md) |
| 17 | [Multi-region reads and single-writer global architecture](sys-arch/17-multi-region-read-single-writer-global.md) |
| 18 | [Backpressure, admission, fairness, and overload](sys-arch/18-backpressure-admission-fairness-overload.md) |
| 19 | [Performance engineering and memory architecture](sys-arch/19-performance-engineering-memory-architecture.md) |
| 20 | [Resource-constrained client architecture](sys-arch/20-resource-constrained-client-architecture.md) |
| 21 | [Protocol negotiation and compatibility governance](sys-arch/21-protocol-negotiation-compatibility-governance.md) |
| 22 | [Sync metadata schema and internal persistence](sys-arch/22-sync-metadata-schema-internal-persistence.md) |
| 23 | [Background jobs, durable workflows, and side effects](sys-arch/23-background-jobs-durable-workflows-side-effects.md) |
| 24 | [Operational control plane and admin API](sys-arch/24-operational-control-plane-admin-api.md) |
| 25 | [Diagnostics, forensics, and incident bundles](sys-arch/25-diagnostics-forensics-reproducible-incident-bundles.md) |
| 26 | [Legacy application compatibility](sys-arch/26-legacy-application-compatibility-incremental-adoption.md) |
| 27 | [Security threat model and abuse resistance](sys-arch/27-security-threat-model-abuse-resistance.md) |
| 28 | [Multi-consumer change feed](sys-arch/28-multi-consumer-change-feed-architecture.md) |
| 29 | [Schema, operation registry, and developer governance](sys-arch/29-schema-operation-registry-developer-governance.md) |
| 30 | [Certification and conformance ecosystem](sys-arch/30-certification-conformance-ecosystem-architecture.md) |
| 31 | [Android and iOS mobile runtime platform architecture](sys-arch/31-android-ios-mobile-runtime-platform-architecture.md) |
| 32 | [Linux, Windows, and macOS desktop runtime architecture](sys-arch/32-linux-windows-macos-desktop-runtime-architecture.md) |
| 33 | [Cross-platform local storage architecture](sys-arch/33-cross-platform-local-storage-mobile-desktop-architecture.md) |
| 34 | [Reference implementation and workspace crate boundaries](sys-arch/34-reference-implementation-workspace-crate-boundary-architecture.md) |
| 35 | [Public Rust API and SDK stability](sys-arch/35-public-rust-api-sdk-stability-architecture.md) |
| 36 | [Storage adapter SDK and official adapter architecture](sys-arch/36-storage-adapter-sdk-official-adapter-architecture.md) |
| 37 | [PostgreSQL/Neon authoritative adapter](sys-arch/37-postgresql-neon-authoritative-adapter-detailed-architecture.md) |
| 38 | [Stoolap embedded local persistence](sys-arch/38-stoolap-embedded-local-replica-client-persistence-architecture.md) |
| 39 | [Axum server integration and middleware](sys-arch/39-axum-server-integration-middleware-architecture.md) |
| 40 | Removed from the active architecture. [Archive record](docs/dioxus-client-integration-completion.md). |
| 41 | [CLI and developer toolchain](sys-arch/41-cli-developer-toolchain-architecture.md) |
| 42 | [SQLite embedded local adapter](sys-arch/42-sqlite-embedded-local-adapter-architecture.md) |
| 43 | [Configuration, secrets, runtime policy, and feature flags](sys-arch/43-configuration-secrets-environment-profiles-runtime-policy-feature-flags-architecture.md) |
| 44 | [Packaging, distribution, and release engineering](sys-arch/44-packaging-distribution-release-engineering-artifact-signing-update-channels-cross-platform-delivery-architecture.md) |
| 45 | [Deployment topologies and operational environments](sys-arch/45-deployment-topologies-single-node-ha-multi-region-edge-enterprise-air-gapped-operational-environment-architecture.md) |
| 46 | [Observability, metrics, tracing, logging, SLOs, and alerts](sys-arch/46-observability-metrics-tracing-logging-slos-alerting-production-telemetry-architecture.md) |
| 47 | [Benchmarking, regression, capacity planning, and sizing](sys-arch/47-benchmarking-performance-regression-capacity-planning-workload-modeling-scalability-testing-production-sizing-architecture.md) |
| 48 | [Testkit, verification, fault injection, and release quality gates](sys-arch/48-testkit-verification-fault-injection-property-model-integration-e2e-release-quality-gates-architecture.md) |
| 49 | [Licensing, dependency policy, and supply-chain security](sys-arch/49-licensing-dependency-policy-supply-chain-security-crate-governance-sbom-reproducible-builds-third-party-risk-architecture.md) |
| 50 | [v1 scope, productization milestones, readiness, RC, and GA](sys-arch/50-aequora-v1-scope-productization-milestones-production-readiness-release-candidate-ga-long-term-evolution-architecture.md) |

## Implementation status

The index covers 49 active numbered specifications plus the three root specifications.
Part 40 remains reserved for the archived integration. The evidence column identifies local
contracts. Test names refer to `aequora-testkit` unless a package is named. Gate names refer
to `cargo xtask check <name>`. The linked reports describe target acceptance and record the
difference between framework behavior and completed integrations.

| Tier | Part | Report | Focused evidence | Status and remaining work |
|---:|---:|---|---|---|
| 1 | 01 | [Part 01](docs/formal-correctness-completion.md) | `model_based, property_invariants` | Bounded model coverage. Live adapter and extended fault campaigns remain. |
| 1 | 02 | [Part 02](docs/causality-provenance-completion.md) | `audit_contracts, sync_invariants` | Application lineage declarations and live authority persistence evidence. |
| 1 | 03 | [Part 03](docs/anti-entropy-self-repair-completion.md) | `integrity_contracts` | Host repair authorization and live differential capture. |
| 1 | 04 | [Part 04](docs/offline-compaction-rebase-completion.md) | `property_invariants, sync_invariants` | Domain-specific merge equivalence and crash recovery on shipped stores. |
| 2 | 05 | [Part 05](docs/local-multiprocess-coordination-completion.md) | `coordination_contracts` | Process takeover on supported filesystems and OS targets. |
| 2 | 06 | [Part 06](docs/adaptive-sync-scheduler-qos-completion.md) | `scheduler_contracts` | Platform signal and wake-up integration. Device power and carrier tests. |
| 2 | 07 | [Part 07](docs/subscription-scope-dynamic-dataset-completion.md) | `scope_contracts` | Application scope rules and revocation cleanup. Test on real datasets. |
| 2 | 08 | [Part 08](docs/live-sync-push-presence-completion.md) | `live_contracts` | Host WebSocket, SSE and push bindings. Live broker and reconnect acceptance. |
| 3 | 09 | [Part 09](docs/bulk-import-export-seed-migration-completion.md) | `migration_contracts` | Concrete source/sink mappings and CDC catch-up. Cutover rehearsal and throughput. |
| 3 | 10 | [Part 10](docs/large-snapshot-streaming-bootstrap-completion.md) | `large_bootstrap_contracts` | Host object storage and scoped read views. Large live bootstrap and resumability. |
| 3 | 11 | [Part 11](docs/operation-semantics-consistency-profiles-completion.md) | `profile_contracts` | Application profile registration and transaction certification by adapter. |
| 3 | 12 | [Part 12](docs/deterministic-execution-replay-completion.md) | `replay_contracts` | Host retained state, handlers and committer. Native restart and incident evidence. |
| 4 | 13 | [Part 13](docs/data-provenance-auditability-explainability-completion.md) | `audit_contracts` | Deployment-specific indexes, archives, anchors and key rotation. Verify restore chains. |
| 4 | 14 | [Part 14](docs/data-governance-retention-erasure-completion.md) | `governance_contracts` | Host retention, erasure, archive and key-destruction integrations. Verify registered surfaces. |
| 4 | 15 | [Part 15](docs/cryptographic-integrity-key-management-e2e-completion.md) | `crypto_contracts` | KMS, HSM and OS keystore adapters and deployment custody, rotation and revocation evidence. |
| 4 | 16 | [Part 16](docs/authority-failover-timeline-epochs-fork-detection-completion.md) | `authority_contracts`, local adapter suites | Host witness is enforced when configured. Durable witness and promotion orchestration, failover and PITR remain. SQLite is on hold. |
| 5 | 17 | [Part 17](docs/multi-region-read-single-writer-global-completion.md) | `regional_contracts` | Split sync pull is deferred. Regional topology and lag/partition acceptance remain. |
| 5 | 18 | [Part 18](docs/backpressure-admission-fairness-overload-completion.md) | `aequora-authority admission`, `configuration`, `axum-server` | Shared load feedback implemented. Host pressure sampling and worker queue/brownout composition remain. |
| 5 | 19 | [Part 19](docs/performance-engineering-memory-architecture-completion.md) | `performance` | Target-specific memory, query plans and throughput baselines. |
| 5 | 20 | [Part 20](docs/resource-constrained-client-architecture-completion.md) | `client-resource` | Native service bridges. Device background, power and storage pressure evidence. |
| 6 | 21 | [Part 21](docs/protocol-negotiation-compatibility-governance-completion.md) | `compatibility` | Release-policy signing and mixed-version fleet rollout. |
| 6 | 22 | [Part 22](docs/sync-metadata-schema-internal-persistence-completion.md) | `metadata` | Physical mappings and conformance for each declared adapter profile. |
| 6 | 23 | [Part 23](docs/background-jobs-durable-workflows-side-effects-completion.md) | `jobs` | Bounded worker polling implemented. Durable production stores, worker execution and provider adapters remain. |
| 6 | 24 | [Part 24](docs/operational-control-plane-admin-api-completion.md) | `admin` | Durable admin store/executors, private listener and authorization integration. Verify against real identity policy. |
| 7 | 25 | [Part 25](docs/diagnostics-forensics-reproducible-incident-bundles-completion.md) | `diagnostics` | Host durable registry, protected archive and audit integration. Custody/retention exercises. |
| 7 | 26 | [Part 26](docs/legacy-application-compatibility-incremental-adoption-completion.md) | `legacy` | Host CDC, route translation and write-ownership wiring. Shadow and cutover runs. |
| 7 | 27 | [Part 27](docs/security-threat-model-abuse-resistance-completion.md) | `security` | Host identity, edge policy, security review and deployment acceptance. |
| 7 | 28 | [Part 28](docs/multi-consumer-change-feed-completion.md) | `change-feed` | Physical consumer offset/effect transactions, broker and archive adapters. Live consumer load evidence. |
| 7 | 29 | [Part 29](docs/schema-operation-registry-developer-governance-completion.md) | `registry` | Signed registry packages, hosted namespace allocation, macros and schema UI are deferred. |
| 7 | 30 | [Part 30](docs/certification-conformance-ecosystem-completion.md) | `certification` | Target-bound certificates, reviewer keys and publishing/revocation custody. |
| 8 | 31 | [Part 31](docs/android-ios-mobile-runtime-platform-completion.md) | `mobile` | Application OS bindings and physical Android/iOS target campaigns. |
| 8 | 32 | [Part 32](docs/linux-windows-macos-desktop-runtime-completion.md) | `desktop` | Application agent, IPC/OS adapters and signed install/update on supported targets. |
| 8 | 33 | [Part 33](docs/cross-platform-local-storage-completion.md) | `local-storage` | Target filesystem, process-kill, backup and keystore certification. |
| 8 | 34 | [Part 34](docs/reference-implementation-workspace-boundaries-completion.md) | `reference-implementation` | Current 35-crate graph governs local boundaries. Target acceptance is tracked by integration. |
| 8 | 35 | [Part 35](docs/public-rust-sdk-completion.md) | `public-sdk` | Released consumers and supported upgrade windows require artifact-specific evidence. |
| 8 | 36 | [Part 36](docs/storage-adapter-sdk-completion.md) | `storage-adapter-sdk` | Independent object store and other adapter integrations. Certify the real engine and capability profile. |
| 8 | 37 | [Part 37](docs/postgresql-neon-authority-completion.md) | `postgres-authority, postgres_live` | Isolated PostgreSQL 18.7 passed. Hosted Neon, PITR and topology-specific connection behavior remain. |
| 8 | 38 | [Part 38](docs/stoolap-local-persistence-completion.md) | `stoolap-local-persistence` | Device/filesystem fault and production suitability evidence. v1 support is conditional. |
| 8 | 39 | [Part 39](docs/axum-server-integration-completion.md) | `axum-server` | Host live routes, proxy/TLS and CORS/CSRF policy. Mixed-version and slow-client qualification. |
| 9 | 41 | [Part 41](docs/cli-developer-toolchain-completion.md) | `cli-toolchain, aequora-cli lib` | Shell completions, man pages and optional advanced tooling are deferred. Package and credential integration remain. |
| 9 | 42 | [Part 42](docs/sqlite-local-adapter-completion.md) | `sqlite-local-adapter, aequora-store-sqlite lib` | Windows, macOS, Android and iOS full-profile filesystem/device evidence. |
| 9 | 43 | [Part 43](docs/configuration-secrets-runtime-policy-feature-flags-completion.md) | `configuration` | Host secret providers and signed remote policy delivery. |
| 10 | 44 | [Part 44](docs/packaging-distribution-release-engineering-completion.md) | `release` | Release signing custody, protected promotion, platform notarization and installation campaigns. |
| 10 | 45 | [Part 45](docs/deployment-topologies-completion.md) | `deployment` | Host provisioning and topology acceptance, including HA, edge and air-gapped hardware. |
| 10 | 46 | [Part 46](docs/observability-production-telemetry-completion.md) | `observability` | Telemetry exporters, dashboards and real alert routing/SLO validation. |
| 10 | 47 | [Part 47](docs/benchmarking-capacity-planning-completion.md) | `benchmarking` | Representative workloads, platform baselines, production capacity and soak. |
| 11 | 48 | [Part 48](docs/testkit-verification-quality-gates-completion.md) | `verification` | Long fuzz/fault campaigns, target runners and candidate artifact evidence. |
| 11 | 49 | [Part 49](docs/supply-chain-governance-completion.md) | `supply-chain` | Protected signing/provenance and independent reproducibility evidence. |
| 11 | 50 | [Part 50](docs/v1-productization-ga-completion.md) | `productization` | RC milestones, upgrade and subsystem evidence, pilots, release risks and digest-bound GA decision remain open. |

| Root specification | Report | Repository boundary and remaining work |
|---|---|---|
| [ACID](ACID.md) | [Compliance](docs/acid-compliance.md) | Tx A/B/C and adapter atomicity are implemented. Distributed group atomicity is outside scope. Live certification and workload-specific recovery remain. |
| [Database interoperability](database-interoperability.md) | [Completion](docs/database-interoperability-completion.md) | Native domain sync and bounded record conversion are implemented. Generic CDC, cross-engine local-store cutover and a runtime multi-store registry are deferred. |
| [Enterprise](enterprise.md) | [Completion](docs/enterprise-completion.md) | Reusable enterprise controls are implemented. Concrete jobs, admin, feed and provider integrations depend on the host. Production approval requires target evidence. |

### Open repository and host integration

- SQLite changes are on hold pending explicit authorization.
- Part 16 still needs durable witness storage and external promotion orchestration. Part 18
  still needs host pressure sampling and worker queue/brownout composition.
- Parts 23, 24, and 28 still need concrete durable job/workflow/effect, admin, and feed adapters.
  Their transaction participation, fencing, and recovery need real-adapter tests.
- Generic CDC adapters, cross-engine local-store cutover, and the runtime multi-store registry
  remain deferred under the database interoperability spec. The per-part reports give the
  remaining host and optional feature boundaries.

### Open environment and release gates

- Qualify hosted PostgreSQL/Neon and deployment-specific HA, failover, PITR, upgrades, and restore.
- Exercise physical desktop/mobile devices and filesystems, air-gapped deployments, mixed load,
  capacity and soak on supported configurations.
- Supply independently verifiable signing, promotion, pilot and supported-upgrade evidence for the
  specific candidate. The [GA report](docs/v1-productization-ga-completion.md) tracks eligibility.

### November 27, 2026 reconciliation

- Added dedicated reports for Parts 01, 16, and 18 and repaired stale specification links and
  package references. All 52 active specification/report pairs resolve.
- Part 18 load shedding now allocates reservations only to eligible classes, within the reduced
  in-flight ceiling. Part 42's SQLite manifest now participates in CLI adapter discovery and pairing.
- SQLite and Stoolap now reject regressing snapshot cursors and mismatched resume boundaries.
  Snapshot activation clears stale applied markers above the installed cursor within Tx C.
- PostgreSQL 18.7 passed the live authority contract on an isolated cluster after the TestKit
  fixture assigned distinct event IDs to distinct operations and isolated the race journal from
  the single-commit checks. See the [ACID compliance report](docs/acid-compliance.md).
- Part 40 remains archived. Its verification gate reads the historical design and archive record.
  The v1 readiness snapshot still records an ineligible release candidate.

## Implementation workflow

For each future part:

1. Retrieve the relevant specification sections and verify the corresponding workspace contracts.
2. Add or update the implementation and its focused tests.
3. Add a concise evidence row or completion report; do not copy the specification into this plan.
4. Run the repository architecture, dependency-direction, database-neutrality, formatting, and
   relevant workspace gates.
5. Refresh the retrieval index after each audit batch and before claiming repository-wide coverage.

## Required repository gates

Run the bounded, offline-friendly checks with one Cargo build job when memory is constrained:

```text
cargo fmt --all -- --check
cargo run -q -p aequora-dev -- check
cargo xtask check database-neutrality
cargo xtask check performance
cargo xtask check client-resource
cargo xtask check protocol
cargo xtask check compatibility
cargo xtask check metadata
cargo xtask check jobs
cargo xtask check admin
cargo xtask check diagnostics
cargo xtask check legacy
cargo xtask check security
cargo xtask check change-feed
cargo xtask check registry
cargo xtask check certification
cargo xtask check mobile
cargo xtask check desktop
cargo xtask check local-storage
cargo xtask check reference-implementation
cargo xtask check public-sdk
cargo xtask check storage-adapter-sdk
cargo xtask check postgres-authority
cargo xtask check stoolap-local-persistence
cargo xtask check axum-server
cargo xtask check cli-toolchain
cargo xtask check sqlite-local-adapter
cargo xtask check configuration
cargo xtask check release
cargo xtask check deployment
cargo xtask check observability
cargo xtask check benchmarking
cargo xtask check verification
cargo xtask check supply-chain
cargo xtask check productization

cargo check --workspace --all-targets --all-features --locked --offline
cargo clippy --workspace --all-targets --all-features --locked --offline -- -D warnings
cargo test --workspace --all-features --locked --offline
RUSTDOCFLAGS=-Dwarnings cargo doc --workspace --all-features --no-deps --locked --offline
```

Deployment-specific PostgreSQL, Neon, mobile, soak, capacity, and production benchmark gates are
reported as environment-dependent evidence rather than represented as universal architecture
claims in this plan.

## Change policy

Architecture prose belongs in exactly one numbered `sys-arch` specification. This plan may link to
that prose, summarize implementation status, or record a decision needed to schedule work, but it
must not reproduce the design. Completion reports may cite both the specification and executable
evidence without becoming a second design source.
