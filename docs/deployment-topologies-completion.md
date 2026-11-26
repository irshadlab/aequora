# Part 45 deployment topology completion

Governing specification is [Part 45](../sys-arch/45-deployment-topologies-single-node-ha-multi-region-edge-enterprise-air-gapped-operational-environment-architecture.md).

Part 45 is implemented by `aequora-deployment`, a secret-free descriptor and validation crate that
keeps deployment choices separate from synchronization semantics. It provides topology profiles,
single-writer scope validation, production and air-gap readiness, connection and timeout budgets,
regional consistency routing, failover epoch decisions, and deterministic fleet-drift diagnostics.

`aequora doctor deployment <descriptor.ron>` validates a bounded descriptor without connecting to
infrastructure or printing secrets. Reference assets cover local, OCI, systemd, Kubernetes, and
air-gapped operation. The restore/promotion and rolling-upgrade runbooks make epoch continuity,
fencing, immutable promotion, and migration ownership explicit.

The implementation is intentionally free of SQL drivers, cloud SDKs, container/Kubernetes clients,
Redis, Kafka, NATS, service-mesh APIs, and runtime transports. PostgreSQL remains the authoritative
writer; SQLite and Stoolap remain client-local adapters.


## Executable evidence

`aequora_deployment::DeploymentDescriptor::validate` checks configuration and topology budgets.
`cargo xtask check deployment` runs the `deployment_topology_contracts` suite and validates
reference assets. `DeploymentTopologyFull` (82) binds tests 134–143 to
`AEQ-INV-DEPLOY001`–`DEPLOY010`.

## Remaining / Left & Environment Gates

**Repository and host integration.** The host owns topology provisioning, database and ingress
credential binding, health propagation, and the cutover executor called by the restore runbook.

**Environment.** Qualify HA, regional lag and failover, edge interruption, DNS/TLS and
upgrade/rollback on the intended topology. Air-gapped acceptance requires isolated target
hardware, an offline artifact set and an actual restore drill.
