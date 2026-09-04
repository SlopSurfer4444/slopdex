# Slopdex findings: 57-path source reconciliation

This index reconciles the earlier 38-path working ledger with the accepted
57-path source generation. IDs are stable: F-01 through F-12 retain their old
meaning; F-08 remains intentionally unused; F-13 and F-14 name newly accepted
source seams. Historical observations are not claims about current upstream.

## Evidence boundary

The published source commit is `e497c3d358ae4a3411b0bd16b1ea7e79051e9aa7`,
applied over upstream `612e6491d50ffb80ffc4330edc4024b86e51e4bf`.
The canonical 57-path manifest is bound by SHA-256
`159abbdba8f1d59d996b1dfa88b4a64eb968ae4683cea58f1965dad7d34bde65`.
See the [patch receipt](../source/PATCH_RECEIPT.md),
[provenance](../provenance/), and [dogfood receipt](../dogfood/README.md).

Recorded accepted test groups were owner 82/82, stores 3/3, spawn 9/9, state
5/5, and join/lag/wait 27/27. These groups overlap and must not be summed into
a new total. They are source/unit evidence, not proof of every runtime path.
Only F-02/F-03 have additional support from the one live nested sample, and
only within that receipt's exact claim ceiling.

## Reconciled findings

| ID | Current status and evidence class | Observed, claimed, and bounded reproduction |
| --- | --- | --- |
| **F-01** | **Reported RED; still open.** Connected local Windows observation plus source-confirmed sandbox-planning seam; no Slopdex repair. | Ambient `TEMP`/`TMP` could cause broad ACL planning and a bounded timeout before the command body ran. Reproduce only in a disposable Windows workspace: run the owning sandbox metadata selector once with inherited temporary roots and once with both rebound to a private bounded root, under an outer timeout and stage markers. A scrubbed selector receipt is still missing. Do not claim the exact Win32 cause, exploitability, or a current-upstream regression. |
| **F-02** | **Source-patched + unit evidence; one live sample is supportive, not exhaustive.** | Exact direct-child join can start a fresh immediate-parent continuation after parent finalization. Selector: `multi_agent_v2_join_completion_defers_to_active_user_turn_then_wakes_once` in [`subagent_notifications.rs`](../../codex-rs/core/tests/suite/subagent_notifications.rs). The [live sample](../dogfood/README.md) observed an automatic nearest-owner continuation, but does not prove every ordering, global exactly-once, or restart behavior. The pre-patch RED was historical; freshness against current upstream is untested. |
| **F-03** | **Source-patched + unit evidence; one bounded nested live sample.** | Nearest-owner recursive continuation is covered by `multi_agent_v2_recursive_owned_join_wakes_root_after_child_continuation` in [`subagent_notifications.rs`](../../codex-rs/core/tests/suite/subagent_notifications.rs). Live dogfood observed `root -> Astra -> Terra -> Luna` with a distinct intermediate delivery followed by a distinct successor result. It was not a six-level run and proves neither arbitrary depth nor every completion order. |
| **F-04** | **Source-patched + unit evidence; no live reproducer.** The old `PendingInit` entry is no longer an open static finding against this source. | The accepted path clears a taskless reserved idle turn on rejected preparation/commit paths in [`turn_input.rs`](../../codex-rs/core/src/session/turn_input.rs). Hostile arbitration/cancellation coverage includes `b5_cancelled_startup_commit_preserves_join_for_later_winner`, `b5_replaced_startup_turn_state_rejects_loser_installation`, `user_start_preempts_uncommitted_pending_wake_reservation`, and `cancelled_committed_pending_wake_releases_claim_for_user_start`. No live fault-injection receipt or fresh-upstream reproduction exists, so claim the guarded source behavior, not a confirmed upstream bug or universal absence of `PendingInit` stalls. |
| **F-05** | **Native behavior reused.** Exact base/current source classification. | Spawn, direct-parent lineage, same-turn `wait_agent`, mailbox delivery, and pending-work scheduling pre-existed in the pinned upstream. Slopdex integrates retained exact-target join/continuation behavior with them; it did not invent basic child transport or same-turn wait. Reproduce by comparing the base and source commit around the cited seams. |
| **F-06** | **Deferred product backlog; explicit missing capability.** | Restart-safe join-obligation persistence, consumer ACK, replay, and crash-point dedupe are not implemented or tested. A valid future reproducer must interrupt at defined points in `Terminal -> Bound -> ACK -> replay`, restart, and observe the exact generation. No such bounded receipt exists; durable spawn edges do not satisfy this contract. |
| **F-07** | **Non-product build-provenance concern; a bounded build receipt is recorded.** | The earlier risk was reuse of an unowned Cargo target. The held local build has an explicit target/source/toolchain/hash [receipt](../provenance/BUILD_RECEIPT.md). This is not proof of arbitrary cache isolation or bit-for-bit reproducibility; the binaries are not part of the source drop. |
| **F-09** | **Source-patched + unit evidence; no saturation proof.** | Defaults are 128 for V1 threads, V2 concurrency including the root, and configurable recursion depth. Selector: `multi_agent_default_caps_are_uniform_128_and_v2_counts_root` in [`config_tests.rs`](../../codex-rs/core/src/config/config_tests.rs). This is configuration evidence only: not an unlimited-depth, 128-agent safety, backend-acceptance, performance, or cost claim. |
| **F-10** | **Deferred/non-product policy boundary.** | The source contains no mutable-scope lease, path lock, or conflict-free multi-writer broker. Two writers were admitted operationally only after disjointness was established. No product reproducer applies; overlapping mutation remains future hardening, not a runtime guarantee. |
| **F-11** | **Deferred capability probe.** | Luna was used as a leaf in the live sample. Recursive V2 coordination and full reserved-tool schema acceptance were not probed. A later bounded non-spawning backend schema probe is required before changing that claim. |
| **F-12** | **Native upstream backlog; static source observation, unconfirmed at runtime.** | With V2 capacity set to one, the active task may hold the sole guard while compaction requests replacement capacity before abort/rebind, yielding `AgentLimitReached`. Base/current comparison found the ordering upstream, not in the Slopdex delta. The missing reproducer is a disposable cap=1 active-turn compaction oracle; until it exists, this is neither a Slopdex regression nor a confirmed current-upstream report. |
| **F-13** | **Source-patched + unit evidence.** | Non-ephemeral child spawn now fails closed before session creation when the durable graph store is unavailable. Selector: `non_ephemeral_thread_spawn_without_graph_store_fails_before_session_spawn` in [`durable_spawn_edge_tests.rs`](../../codex-rs/core/src/thread_manager/durable_spawn_edge_tests.rs). This proves the tested admission boundary, not general store availability or restart-safe completion delivery. |
| **F-14** | **Source-patched + unit evidence.** | An exact join observer can recover a target's authoritative terminal state after forced broadcast lag. Selector: `exact_join_observer_recovers_target_terminal_after_broadcast_lag` in [`join_observer_lag_tests.rs`](../../codex-rs/core/src/agent/control/join_observer_lag_tests.rs). This is deterministic lag recovery, not packet-loss, crash/restart, arbitrary-ordering, or global exactly-once evidence. |

## Historical leads: not confirmed current-upstream bugs

| Lead | What was reported | What is still missing |
| --- | --- | --- |
| Hook command/environment, zero logs | The source-acceptance report retained two hook/environment selectors as pre-existing zero-log failures rather than candidate-owned regressions. This is not counted as a Slopdex fix or proof that structured errors are missing. | The exact failing invocation and scrubbed outcome are not packaged here. Reconcile the selector, source identity and environment before filing a product issue. |
| Accepted spawn, temporarily absent graph listing | A separate field campaign reported accepted child paths initially absent from listing; the children later returned, and duplicate recovery was avoided. | An isolated, source-bound reproducer and a documented listing-consistency contract. Do not infer result loss or a current Slopdex defect. |
| Authentication errors, blank or silent UI | Historical user-facing symptoms were reported. | A bound request, exact component/source identity and connected failure evidence. No root cause or Slopdex attribution is established here. |

These leads are retained so they are not lost, not counted as verified bugs.
For a new report, bind the exact source identity, smallest owning selector,
abstract topology, expected/actual result, outer timeout, and a sanitized
receipt. Do not include credentials, machine paths, private logs, or local
execution identifiers.
