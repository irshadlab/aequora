# Conformance invariant coverage matrix

The stable invariant registry in `aequora_types::invariants` is normative. This matrix identifies the
primary executable evidence family for every architecture part; certification artifacts retain
the exact invariant IDs and test IDs they exercised. Manual and deployment evidence cannot replace
required automated correctness tests.

| Part | Primary verification | Executable evidence |
|---:|---|---|
| 01 | model + automated | `aequora_testkit::model`, `verify model` |
| 02 | automated | lineage and audit contract tests |
| 03 | property + automated | integrity and repair contracts |
| 04 | property + automated | queue compaction/rebase contracts |
| 05 | concurrency + automated | coordination and fencing contracts |
| 06 | property + automated | scheduler QoS contracts |
| 07 | automated | scope transition contracts |
| 08 | automated | live hint loss/backpressure contracts |
| 09 | differential + automated | migration/export contracts |
| 10 | failure injection + automated | bootstrap streaming contracts |
| 11 | property + automated | consistency profile contracts |
| 12 | replay + model | deterministic replay contracts |
| 13 | tamper + automated | audit provenance contracts |
| 14 | failure injection + automated | governance/erasure contracts |
| 15 | known-answer + automated | crypto contracts |
| 16 | model + failure injection | authority failover contracts |
| 17 | model + automated | regional routing contracts |
| 18 | unit + deterministic load | admission/fairness contracts |
| 19 | benchmark + correctness | bounded performance architecture gate |
| 20 | property + resource | constrained-client contracts |
| 21 | differential + automated | compatibility negotiation contracts |
| 22 | adapter differential | metadata architecture and adapter contracts |
| 23 | failure injection + automated | jobs/workflow/side-effect contracts |
| 24 | authorization + automated | admin control-plane contracts |
| 25 | tamper + replay | diagnostic incident contracts |
| 26 | shadow + differential | legacy adoption contracts |
| 27 | abuse matrix + automated | security abuse contracts |
| 28 | failure injection + automated | change-feed contracts |
| 29 | generation + compatibility | registry lock/codegen contracts |
| 30 | harness + artifact verification | `aequora_adapter_sdk::certification` and certification contracts |
| 31 | runtime + device | `mobile_runtime_contracts`, profile 60 |
| 32 | runtime + platform | `desktop_runtime_contracts`, profiles 61–62 |
| 33 | storage contracts + platform | `local_storage_contracts`, profiles 63–64 |
| 34 | workspace dependency policy | `cargo xtask check reference-implementation` |
| 35 | SDK contract | `cargo xtask check public-sdk` |
| 36 | adapter contract | `storage_adapter_sdk_contracts`, profiles 65–68 |
| 37 | authoritative adapter | `postgres_live`, profiles 69–70 |
| 38 | persistent local adapter | `cargo xtask check stoolap-local-persistence`, profiles 71–73 |
| 39 | transport integration | `cargo xtask check axum-server`, profile 74 |
| 40 | archived | [Historical profile 75 and tests 89–98](dioxus-client-integration-completion.md) |
| 41 | toolchain contract | `cargo xtask check cli-toolchain`, profile 76 |
| 42 | persistent local adapter | `cargo xtask check sqlite-local-adapter`, profiles 77–79 |
| 43 | configuration and policy | `configuration_policy_contracts`, profile 80 |
| 44 | release engineering | `release_engineering_contracts`, profile 81 |
| 45 | topology contracts | `deployment_topology_contracts`, profile 82 |
| 46 | telemetry contracts | `observability_production_contracts`, profile 83 |
| 47 | workload and capacity evidence | `benchmarking_capacity_contracts`, profile 84 |
| 48 | deterministic release verification | `verification_quality_gates`, profile 85 |
| 49 | supply-chain policy and evidence | `supply_chain_contracts`, profile 86 |
| 50 | milestone and release eligibility | `productization_ga_contracts`, profile 87 |

Deployment-specific rows such as production restore, failover, external provider credentials,
database version ranges, and signed release provenance are recorded in the certification artifact
as `DeploymentValidation` or `ManualReview` evidence with limitations. They are never silently
marked automated.
