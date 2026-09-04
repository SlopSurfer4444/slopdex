# Review the Slopdex changes

Start with the code, not the field narrative:

- [Exact product diff: pinned upstream to Slopdex source](https://github.com/SlopSurfer4444/slopdex/compare/612e6491d50ffb80ffc4330edc4024b86e51e4bf...e497c3d358ae4a3411b0bd16b1ea7e79051e9aa7).
- [Source-only commit](https://github.com/SlopSurfer4444/slopdex/commit/e497c3d358ae4a3411b0bd16b1ea7e79051e9aa7): 57 changed/added paths, including tests. Later documentation commits are not part of this product diff.
- [Complete portable patch](source/slopdex-612e6491d50f.patch) and its [application/byte receipt](source/PATCH_RECEIPT.md).
- [Findings index](findings/README.md): source-patched behavior, open reported REDs, unconfirmed leads, native reuse and deferred work are separated.

## Remove display noise, preserve the canonical patch

Eighteen accepted files contain mixed CRLF/LF line endings. The exact patch
preserves those bytes, so the default GitHub diff can look much larger than the
semantic change and may collapse large files. For a local inspection view:

```sh
git diff --ignore-cr-at-eol 612e6491d50ffb80ffc4330edc4024b86e51e4bf e497c3d358ae4a3411b0bd16b1ea7e79051e9aa7 -- codex-rs
```

This is a display aid, not a replacement patch or a new accepted generation.
Use the complete patch and canonical manifest when checking exact bytes. Line
counts, test counts and module extractions are not counts of bugs fixed.

## Suggested review order

These are reading units, not independently cherry-pickable patches. Ownership,
turn admission and delivery are coupled; a smaller upstream submission needs
its own dependency mapping and tests.

| Contract | Implementation entry points | Regression evidence to read alongside |
| --- | --- | --- |
| Capacity configuration | [config/mod.rs](../codex-rs/core/src/config/mod.rs) | [config tests](../codex-rs/core/src/config/config_tests.rs); F-09 in the findings index |
| Public join/wait contract and exact targets | [join handler](../codex-rs/core/src/tools/handlers/multi_agents_v2/join.rs), [wait handler](../codex-rs/core/src/tools/handlers/multi_agents_v2/wait.rs), [tool specification](../codex-rs/core/src/tools/handlers/multi_agents_spec.rs) | [wait/join regressions](../codex-rs/core/src/tools/handlers/multi_agents/wait_join_regression_tests.rs) |
| Retained ownership, successor delivery and signal lag | [owned join state](../codex-rs/core/src/agent/control/owned_join.rs), [observer](../codex-rs/core/src/agent/control/join_observer.rs), [delivery](../codex-rs/core/src/session/input_queue/owned_join/delivery.rs) | [recursive delivery](../codex-rs/core/src/agent/control/owned_join_tests/recursive_delivery_tests.rs), [forced lag](../codex-rs/core/src/agent/control/join_observer_lag_tests.rs), [integration selectors](../codex-rs/core/tests/suite/subagent_notifications.rs) |
| User priority, taskless start and capacity cleanup | [turn input](../codex-rs/core/src/session/turn_input.rs), [tasks](../codex-rs/core/src/tasks/mod.rs), [execution](../codex-rs/core/src/agent/control/execution.rs), [residency](../codex-rs/core/src/agent/control/residency.rs) | [turn-input tests](../codex-rs/core/src/session/turn_input_tests.rs), [capacity tests](../codex-rs/core/src/agent/control/owned_join_tests/capacity_tests.rs), [residency tests](../codex-rs/core/src/agent/control/residency_tests.rs) |
| Persisted spawn-edge admission and exact close | [spawn-edge lifecycle](../codex-rs/core/src/thread_manager/spawn_edge_lifecycle.rs), [graph-store contract](../codex-rs/agent-graph-store/src/store.rs), [state adapter](../codex-rs/state/src/runtime/threads.rs) | [missing-store admission](../codex-rs/core/src/thread_manager/durable_spawn_edge_tests.rs), [lifecycle tests](../codex-rs/core/src/thread_manager/spawn_edge_lifecycle_tests.rs), [state tests](../codex-rs/state/src/runtime/thread_spawn_edge_tests.rs) |

The published tests can be inspected and run under the repository's owning
test workflow; this documentation update did not rerun them. Recorded test
groups and the much narrower live sample have different claim ceilings, listed
in the [findings](findings/README.md) and [live receipt](dogfood/README.md).

## Useful maintainer feedback

An exact counterexample, a smaller equivalent native contract, or an upstream
commit that covers a row is more useful than agreeing with the narrative.
Please distinguish a source defect, a host/test-environment failure, a policy
problem and an unimplemented capability. The index explicitly marks where a
public reproducer is still missing; it is not a claim that all historical
reports remain bugs in current upstream.
