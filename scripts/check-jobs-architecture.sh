#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

for suffix in 001 002 003 004 005 006 007 008 009; do
    rg -q "AEQ-INV-JOB${suffix}" crates/aequora-types/src/invariants/mod.rs
done

for module in jobs workflow side_effects; do
    test -f "crates/aequora-authority/src/${module}/mod.rs"
done
if rg -q '(^|[^[:alnum:]_-])(sqlx|stoolap|tokio|axum|reqwest|rayon)([^[:alnum:]_-]|$)' \
    crates/aequora-authority/Cargo.toml; then
    echo "jobs architecture: runtime, database, transport, or provider dependency in aequora-authority" >&2
    exit 1
fi

for symbol in JobStore JobLease FencingToken RetryPolicy RecurringSchedule \
    JobEpochPolicy JobGovernanceMetadata JobAccessPolicy JobAdminService; do
    rg -q "${symbol}" crates/aequora-authority/src/jobs crates/aequora-server/src/jobs.rs
done

for symbol in WorkflowDefinition WorkflowTransition ScheduleCompensation WorkflowStore; do
    rg -q "${symbol}" crates/aequora-authority/src/workflow
done

for symbol in SideEffectIntent SideEffectIntentWrite ExternalIdempotencyKey \
    AmbiguousRecoveryPolicy AuthoritativeOperationSink SideEffectProvider; do
    rg -q "${symbol}" crates/aequora-authority/src/side_effects
done

test -f crates/aequora-testkit/tests/job_workflow_side_effect_contracts.rs

echo "jobs architecture: durable jobs, fenced workflows, typed side effects, nine invariants, and neutral dependencies verified"
