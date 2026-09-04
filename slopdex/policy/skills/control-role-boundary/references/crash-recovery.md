# Crash Recovery

Use this procedure only to reconstitute an already admitted control role after
a host, app, or task interruption. It grants no role or mutation authority and
does not make the role durable by itself.

## Establish the recovery boundary

1. Pause new mutation and replacement dispatch for the affected scope.
2. Reattest the current owner instruction or still-valid campaign lease, domain,
   exact scope, repository or execution root, source or candidate generation,
   and authorities and non-authorities.
3. Inspect all available independent liveness surfaces: the native agent tree
   or task state, relevant processes and locks, repository or shared-state
   posture, and the observational campaign or evidence ledger. The ledger
   preserves intent and generations; it is not a scheduler and cannot prove
   liveness alone.
4. Classify the incumbent as:
   - `LIVE`: keep it; do not dispatch a replacement.
   - `TERMINAL`: consume its exact result once or close its obligation.
   - `NO_OVERLAP_PROVEN`: the available surfaces agree that no holder or writer
     remains for the exact scope, so one replacement may be admitted.
   - `UNKNOWN`: remain read-only and return the smallest liveness or authority
     gate; absence from one surface is insufficient.

## Reconstitute exactly one holder

Only after `NO_OVERLAP_PROVEN`, activate at most one replacement through an
otherwise valid admission route. Bind the same domain and bounded scope, the
freshly reattested source or candidate generation, exact authorities and
non-authorities, callback, evidence locations, and terminal condition. Record
the recovery and no-overlap basis in the task-local authority record before
mutation or delegated mutation.

Preserve partial logs and immutable prior receipts for interrupted operations;
use Adaptive's consequential-attempt rules for outcome classification and
evidence-backed retries. Never overwrite the old receipt or accept a mixed
source generation.

The parent-architect may perform this bounded admission and consume the next
material checkpoint, but it does not become the recovered coordinator or a
second execution owner. A stale task label, heartbeat, cross-thread message,
or ledger entry cannot revive an expired role or widen its lease.
