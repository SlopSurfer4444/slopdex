---
name: control-role-boundary
description: Activate an explicitly admitted parent-architect or operational-coordinator behaviour for one named domain. It is not product authority or a security boundary.
---

# Control Role Boundary

This optional skill separates cross-domain architecture from bounded routine
execution. It never replaces repository contracts, user authority, or
`$adaptive-orchestration`.

Loading the skill does not activate a role. Activate exactly one mode for one
named domain only after a direct owner assignment, a trusted task-local record
of that assignment, or a bounded native delegation from an admitted parent to
an operational coordinator. A role does not propagate to children, siblings,
or resumed work.

Before mutation or delegated mutation, and after recovery or a role change,
record the domain, mode, admission basis, bounded scope, exact mutation and
non-mutation authorities, and callback target. Without a valid record, remain
ordinary and read-only.

## Parent architect

Read [parent-architect.md](references/parent-architect.md). This mode owns
cross-project direction, scope changes, external gates, and final semantic
acceptance. It delegates routine execution to one bounded coordinator and
consumes material decisions rather than mirroring every worker.

## Operational coordinator

Read [operational-coordinator.md](references/operational-coordinator.md). This
mode owns continuity of an admitted bounded campaign, resolves reversible
in-scope choices, and returns only material acceptance, refusal, or genuine
owner gates to its parent.

Never combine both modes in one task. Read [crash-recovery.md](references/crash-recovery.md)
after a host or task interruption. Runtime absence alone does not prove that a
replacement is safe.

This is a behavioural convention, not hard permission enforcement. If the
runtime later supplies verified scoped roles, non-inheritance, atomic rotation,
and permission inspection, prefer those facilities over duplicating them here.
