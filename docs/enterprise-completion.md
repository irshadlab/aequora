# `enterprise.md` implementation completion

Governing specification is [enterprise.md](../enterprise.md).

This matrix reconciles the 120 architecture sections in `enterprise.md` with the implemented
Aequora workspace. It distinguishes reusable framework behavior from application integration and
real-infrastructure acceptance. The [implementation plan](../plan.md) tracks remaining work.
The [v1 product scope](v1-productization-ga-completion.md) selects PostgreSQL, SQLite, and Axum
with HTTPS from the database-neutral SDK composition options.

## Section map

| Sections | Enterprise area | Current implementation or resolved ownership | Status |
|---|---|---|---|
| 1–4 | objectives, principles, deployment profiles, topology | Database-neutral composition supports development, small-production, enterprise, private-network, and intermittent-network shapes without changing the kernel; named profiles are typed values in `aequora-config` | Implemented |
| 5–8 | runtime bootstrap, layered configuration, secrets | Host applications own process bootstrap and secret providers; strict RON parsing rejects unknown/unsafe values, profiles supply safe defaults, and `Secret<T>` redacts diagnostics and deliberately has no serialization surface | Implemented at the framework boundary |
| 9–12 | binaries, containers, systemd, Kubernetes | Aequora publishes composable Rust crates and does not impose a server binary or orchestrator; application packaging, ingress, process supervision, and pod policy are deployment artifacts | Host/deployment-owned by design |
| 13–18 | health, graceful shutdown, rollout, migrations | Axum exposes separate live/ready probes, bounded readiness, irreversible draining, in-flight accounting, and deadline outcomes; Stoolap and PostgreSQL use checksummed migration ledgers and PostgreSQL uses transaction-scoped advisory locking | Implemented; rollout execution is deployment acceptance |
| 19–27 | protocol upgrades, HA, pools, replicas, optional infrastructure, external effects | Independent protocol/schema compatibility, typed upgrade responses, stateless exchange nodes, bounded PostgreSQL pools, database-neutral adapters, and atomic authoritative transactions are implemented; replica routing and application notification outboxes remain host policy | Implemented at the library boundary |
| 28–34 | SLOs, health indicators, telemetry, alerts | Payload-free client/server/transport/transaction metrics, phase timings, durable queue/conflict health, request correlation, structured tracing hooks, overload/readiness/drain metrics, and bounded-cardinality guidance are present | Implemented; SLO values and alert delivery are operator-owned |
| 35–45 | security, identity boundary, tenants, rate/resource limits, bounded compute | Authentication is injected before body processing; identities and scopes are revalidated; global/per-tenant admission and rate buckets are bounded; wire/decompression/field limits and a dedicated Rayon pool prevent untrusted allocation and CPU starvation | Implemented |
| 46–60 | client/server ergonomics, bootstrap, resync, pending work, conflict UX, mobile, hints, snapshots | Builders and typed configs compose the client/server; atomic local mutation/outbox and reconciliation, durable retry deadlines, conflict inbox, automatic snapshot bootstrap, staged hash-checked install, adaptive batching, and optional push hints are implemented | Implemented |
| 61–67 | blobs, backup/PITR, disaster and client recovery | A separate resumable hash-verified blob contract avoids database coupling; scope identity and typed resync protect restored timelines; durable outbox/snapshot primitives support client recovery | Framework implemented; backup/PITR and restore drills are deployment acceptance |
| 68–71 | admin plane, CLI, maintenance, operational errors | Store/schema/lifecycle/metrics APIs provide read-only diagnostic inputs; `MaintenanceService` enforces `Normal`, `ReadOnly`, and `SyncPaused` before execution; stable payload-free `AEQ-*` codes cross HTTP and QUIC | Implemented at the framework boundary; authenticated UI/CLI is host-owned |
| 72–81 | scale, journal retention, devices, modular monolith, workers and jobs | Stateless nodes, scoped journal paging/retention floors, typed inactive-device resync, adapter-neutral transactions, and idempotent operation IDs support modular deployment and application-owned workers | Implemented at the reusable boundary |
| 82–88 | supply chain, release artifacts, staging, canary, blue/green, rollback, CI gates | Locked dependencies, formatting, strict Clippy, all-target/all-feature checks, tests, rustdoc, fuzz/benchmark builds, packaging, MSRV, Guppy, database-neutrality, PostgreSQL, and composed Stoolap-to-PostgreSQL CI gates exist | Repository gates implemented; signing, SBOM, scans, promotion and rollout evidence are distribution-owned |
| 89–96 | runbooks, outage/conflict investigation, chaos, soak, load, herd control | Typed errors, stable IDs, fault injection, deterministic simulation, model/property tests, bounded backoff with jitter, admission/load shedding, and benchmark harnesses supply the executable foundation | Framework implemented; environment runbooks, soak and capacity results are deployment acceptance |
| 97–107 | promotion, reproducibility, licensing, retention, offboarding, capacity, DB security/timeouts/namespaces | Cargo.lock and release profiles support repeatable builds; application data ownership keeps retention/offboarding outside sync metadata; bounded deadlines/pools and prefixed Aequora metadata isolate operational state | Implemented where framework-owned; governance and infrastructure policy remain host-owned |
| 108–110 | supportability, support bundle, documentation | Stable request/operation/session/device/tenant/cursor types, operational codes, schema status, lifecycle state, and payload-free metrics form a sanitized support-data API; architecture, ACID, protocol, tutorial, adapter, and completion docs cover the library | Implemented at the framework boundary; bundle packaging is host-owned |
| 111–120 | production/release checklists, golden paths, architecture and guarantees | CI and reusable contracts prove repository invariants; the golden client/server composition is runnable in examples and live environment-gated suites; production-only checks remain explicitly unclaimed | Repository scope complete; production acceptance remains mandatory |

## Implemented enterprise controls

The framework supplies the following controls.

- `DeploymentProfile` supplies validated `Development`, `SmallProduction`, `Enterprise`, and
  `HighLatencyNetwork` defaults without selecting a database or transport;
- `Secret<T>` makes redaction the default and cannot be accidentally serialized into RON;
- `MaintenanceController` and `MaintenanceService` enforce `Normal`, `ReadOnly`, and `SyncPaused`
  before domain execution, so rejected writes stay durably pending with their existing operation IDs;
- `OperationalErrorCode` supplies stable `AEQ-*` categories, emitted as
  `x-aequora-error-code` over HTTP and inside typed QUIC transport errors while retaining transient
  versus permanent retry classification.

## Remaining / Left & Environment Gates

### Repository and host composition

The deploying application composes the operation registry, a concrete database adapter,
`aequora_config::AequoraConfig`, secret and identity providers, the Axum listener, and telemetry.
`aequora_server::admin` and `aequora_authority::admin` provide private control-plane contracts.
Hosts supply authentication/authorization, durable action audit and subsystem executors.
PostgreSQL provides durable job, workflow, and typed side-effect stores. Host worker execution,
provider delivery and reconciliation, and transactional change-feed consumers still require
integration. See [Part 23](background-jobs-durable-workflows-side-effects-completion.md),
[Part 24](operational-control-plane-admin-api-completion.md), and [Part 28](multi-consumer-change-feed-completion.md).

### Production evidence

Repository tests cannot prove external TLS termination, a cloud secret reference, Neon capacity,
backup retention, a restore time objective, a telemetry exporter, or a safe traffic shift. Those
are mandatory per-deployment release gates. The absence of credentials or infrastructure means
"not exercised", never "passed".

## Verification contract

The focused gates are `cargo xtask check axum-server`, `cargo xtask check configuration`,
`cargo xtask check deployment`, and `cargo xtask check observability`.
`AEQ-INV-LOAD001`–`LOAD009` cover admission, with Axum conformance IDs 79–88 and
`AEQ-INV-AXUM001`–`AXUM010` covering integration behavior.

The repository verification baseline is sequential and resource-bounded:

```bash
cargo fmt --all -- --check
CARGO_BUILD_JOBS=1 cargo check --workspace --all-targets --all-features --locked
CARGO_BUILD_JOBS=1 cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
CARGO_BUILD_JOBS=1 cargo test --workspace --all-features --locked
RUSTDOCFLAGS=-Dwarnings CARGO_BUILD_JOBS=1 cargo doc --workspace --all-features --no-deps --locked
cargo run -q -p aequora-dev -- check
cargo xtask check database-neutrality
```

Live PostgreSQL/Neon, backup/restore, TLS, capacity, staging, canary, rolling-upgrade, rollback,
chaos, and soak gates are additional and require the actual target environment.
