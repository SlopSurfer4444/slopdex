---
name: adaptive-orchestration
description: Organize non-trivial Codex work when delegation, verification, long-running execution, or changing uncertainty can materially improve the result.
---

# Adaptive Orchestration

Build the execution shape from current evidence, authority, and shared state;
revise or collapse it when the next best move changes. Work directly when
coordination costs more than it returns.

## Ownership and dispatch

- Keep objective, authority boundary, acceptance criteria, current evidence,
  and state owner explicit.
- Delegate only for independent work, context isolation, specialized
  capability, independent judgment, or cheaper correct execution. Give each
  child bounded scope, required sources, constraints, acceptance evidence, and
  one expected handoff.
- Bind exactly one consumer before dispatch. For a result that affects the
  parent's next decision, keep it owned; detached work must still name a
  non-blocking consumer.
- One shared mutable surface has one owner. Parallel writers require proven
  disjoint roots and an explicit fan-in owner; otherwise keep alternatives
  read-only until one writer is selected.
- Add a coordinator only for useful bounded work, never as a mandatory tier
  or a manually admitted title.

## Wait, join, and delivery

Use native primitives only when the runtime provides them.

- If the parent keeps its current turn open, consume an owned child with native
  wait. A wait protects that active turn only.
- If the parent may end before owned direct-child results arrive, arm one exact
  native join for the settled direct-child turn set before ending. The join may
  schedule a fresh parent continuation when those exact turns are terminal; a
  queued continuation never displaces an active user turn.
- Nearest parents consume their own children; do not join leaves through an
  intermediate owner. A join is exact-turn scoped. Do not infer coverage of a
  later child turn, restart, replay, or external side effect.
- On steering, preserve rather than consume the owned obligation unless the
  user explicitly cancels, pauses, or reassigns it. Never replace possibly live
  work merely because its result is not yet visible.

## Admit evidence, not narration

Before a child result may accept, block, freeze, or redirect shared work, its
consumer verifies the execution root, source/candidate identity, assigned
scope, freshness, evidence class, and the actually exercised observation or
capability seam. Ambiguous or disconnected evidence is inadmissible; a credible
security, authority, data-loss, or shared-state risk pauses only the affected
surface.

For consequential operations, bind an attempt identity and record the outcome
before follow-up changes the evidence surface. Missing or interrupted outcomes
remain unknown until reconciled; do not blind-retry.

Already-authorized offline implementation, tests and in-scope repairs iterate
normally without renewed approval or a new attempt ceremony. A material
`BLOCK`/`STOP` prevents acceptance and consequential use of the affected
candidate; it does not cancel authorized repair. Explicit user pauses, scope
limits and genuine safety stops still apply.

Bind acceptance to an exact path set and one integration owner. Stop its
writers before final hashes and create the receipt last. Repairs create a new
generation: preserve historical receipts, reuse only evidence whose source,
dependencies and claims remain valid, and refresh affected proofs and final
bindings. Never mix generations or patch only the receipt.

## Verification and hygiene

Verify the narrow owning seam first, then widen according to coupling and
risk. A focused pass never replaces a bypassed aggregate. Negative tests count
only when their hostile input changes and the observation seam is connected.

Every temporary or rebuildable artifact needs an exact path, one owner, a last
consumer, and a retention decision. Do not delete active, shared,
unknown-provenance, source, evidence, or user-owned state.
