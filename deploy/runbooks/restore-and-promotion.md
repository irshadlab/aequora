# Restore and authority promotion

1. Stop routing writes and fence the old database writer at the infrastructure and Aequora layers.
2. Restore or promote the candidate in isolation. Verify ledger, journal, timeline head, snapshots,
   migrations, registry, and business/audit state.
3. Prove whether every acknowledged commit exists on the candidate. If this cannot be proved,
   establish a new `AuthorityEpoch` before any synchronization resumes.
4. Publish the authority endpoint only after readiness passes. DNS or traffic-manager changes alone
   never grant authority.
5. Reconnect clients using their original `OperationId`s, monitor epoch mismatches and duplicate
   outcomes, and retain all promotion evidence in durable audit.

## PostgreSQL witness and restored authority

1. Provision the witness on a control-plane database outside the authority's backup and restore
   boundary. Use `epoch_witness::install` with its administrator pool. Scope the control-plane
   writer to the witness table and use a separate read-only witness pool for runtime refreshes.
2. Serialize approval for the logical authority. After fencing the old writer, choose an unused
   epoch above both restored metadata and the latest witness epoch. Commit it with
   `epoch_witness::record_epoch`. Retain the approved epoch and promotion evidence for retries.
3. Refresh `PostgresEpochWitness` from the witness primary and bind its registry to the authority
   controller. Use an observation lifetime and refresh deadline appropriate to the deployment.
   Refresh failure or expiry blocks witness-backed serving. The cached observation may lag a
   newer witness record until refresh or expiry, so keep physical fencing in force.
4. Use `promote_to_recorded_epoch` with the approved epoch, then persist the resulting state and
   manifest with `record_restore_epoch_transition`. The database remains in read-only verification
   while the candidate is checked.
5. Supply the finished verification to `complete_restore_recovery`. Load the resulting committed
   serving state and bind the witness to the server controller before opening traffic. An ambiguous
   response can be retried with the same expected transition and completion timestamp.

