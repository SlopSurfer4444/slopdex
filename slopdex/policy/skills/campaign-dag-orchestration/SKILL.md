---
name: campaign-dag-orchestration
description: Plan a multi-slice implementation, migration, build, or release campaign as one frozen logical DAG executed in lazy dependency-ready waves.
---

# Campaign DAG Orchestration

Use this only when several slices converge on one meaningful later integration,
migration, build, or release checkpoint. It is a planning and ledger layer, not
a second scheduler or a census of live agents. Use
`$aggressive-recursive-orchestration` inside ready nodes and
`$adaptive-orchestration` for live ownership and recovery.

## Compile the horizon

Before mutation, bind one compact campaign record, reusing an existing plan or
ledger, containing:

- objective, exclusions, authority, participating root identities, acceptance
  conditions, and terminal checkpoint;
- logical nodes and dependency edges, each with deliverable, readiness,
  disposition, acceptance gate, and eventual terminal receipt;
- `BLOCKED`, `READY`, `ACTIVE`, or `TERMINAL` readiness and explicit reasons
  for unknown or blocked nodes;
- `REQUIRED`, `OPTIONAL`, or `DROP_IF_NATIVE` disposition.

Identify the dependency-closed lane required for the terminal checkpoint.
Optional hardening may run only when it cannot delay, conflict with, or become
an implicit prerequisite of that lane.

## Execute lazy ready waves

1. Reattest participating roots and shared-state posture.
2. Materialize only dependency-ready nodes that fit real authority and host
   capacity; future nodes remain logical, not dormant agents.
3. Decompose each ready node for independent evidence, then keep mutation
   narrow and owned.
4. Record accepted, refused, obsolete, or dropped outcomes against the exact
   source and evidence identity.
5. Update the ledger, recompute readiness, and admit the next useful wave.

Preserve rejected and superseded receipts. Update the same record with a
replan only when the owner changes the terminal contract or authority, a
frozen baseline or external evidence identity is replaced, or evidence changes
a dependency or architecture premise. Planned candidate progress and in-scope
repair do not require a global replan or a separate document. Do not replan
merely because a turn ends, time passes, or a remote branch moves.

Keep an observational ledger of campaign identity, frozen roots, terminal
checkpoint, node state, dependencies, owners/consumers, candidate generations,
tests, reviews, blockers, receipts, and unknown owned obligations. Native
runtime state remains live authority.

For a campaign that intentionally holds a base stable and reconciles later,
read [frozen-upstream-checkpoint.md](references/frozen-upstream-checkpoint.md).
