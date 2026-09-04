# Slopdex

**One thread. A whole workshop behind it.**

Slopdex is a power-user Codex source preview for recursive agent work: keep the
architecture in one conversation, split useful independent work into temporary
trees, and bring results back to the agents responsible for them.

**This is the first source drop.** The modified Rust source and tests are in
this repository, alongside the portable patch, verification notes and optional
orchestration skills. A Windows build has been tested locally; downloadable
binaries will be published separately. Nothing here replaces your installed
Codex automatically.

This is an independent derivative of [OpenAI Codex](https://github.com/openai/codex),
not an official OpenAI release or an OpenAI-endorsed project.

## Start here

- [Review the exact code diff and regression tests](slopdex/REVIEW_GUIDE.md)
- [Findings: patched, open RED, unconfirmed and deferred](slopdex/findings/README.md)
- [What we learned: from Sol to the Astra chapter](slopdex/SHORTGRID.en.md)
- [What changed](slopdex/RELEASE_NOTES.md)
- [Known limitations and findings not claimed as fixed](slopdex/KNOWN_LIMITATIONS.md)
- [Source and build provenance](slopdex/provenance/)
- [Observed live nested completion](slopdex/dogfood/)
- [Optional skills and global instruction sample](slopdex/policy/)
- [Portable patch against the pinned upstream](slopdex/source/)
- [The original public discussion, #40037](https://github.com/openai/codex/issues/40037)

## What is in this checkpoint?

| Area | Included behavior |
| --- | --- |
| Capacity | Defaults raised to 128 in the supported agent configuration; capacity is permission to use a graph, not a target agent count. |
| Depth | Configurable recursive depth, default 128. This does not promise infinite depth or identical orchestration capabilities for every model. |
| Owned joins | Direct-child and turn binding, with retained ownership across the nested successor continuation covered by the implementation. |
| Delivery | Nearest-parent aggregation, duplicate suppression for the relevant delivery identity, user-turn priority and recovery from a lagged terminal signal. |
| Spawn lifecycle | Required spawn-edge persistence and fail-closed admission when the required store is unavailable. |
| Resource lifecycle | Capacity and residency handling so terminal work and retained obligations are treated appropriately. |
| Verification | Focused, independently runnable source tests around these contracts. |

The source candidate contains 57 changed or added paths. Its recorded owner
test group passed 82/82, and independent source review accepted that candidate.
Those are scoped results, not a claim that every upstream suite or every
possible runtime scenario passed.

The local Windows dogfood observed a nested parent-to-coordinator-to-leaf
chain deliver an intermediate result and then the distinct successor's
aggregate. That sequence matches retained ownership; two different results
are not automatically a duplicate delivery. See the receipt for the exact
claim and exclusions. Restart/crash exactly-once replay is not claimed.

## Build the source

The baseline is upstream commit
[`612e6491d50ffb80ffc4330edc4024b86e51e4bf`](https://github.com/openai/codex/commit/612e6491d50ffb80ffc4330edc4024b86e51e4bf).
This checkout already contains the Slopdex changes: do **not** apply the
portable patch to it again.

Use the repository's Rust toolchain and platform build prerequisites. A
normal CLI build starts with:

```sh
git clone https://github.com/SlopSurfer4444/slopdex.git
cd slopdex/codex-rs
cargo build --locked --release -p codex-cli
```

Run `target/release/codex` (`target/release/codex.exe` on Windows). Keep your
normal Codex installation and state separate when evaluating the preview.
The upstream source, dependency locks and build documentation are retained.
The Windows build receipt records the additional helper binaries used for
our local Desktop integration; a portable Desktop installer is not included.

The patch in `slopdex/source/` is for applying this candidate to a clean
checkout of the pinned upstream instead. Its receipt documents line endings,
the complete path set and the application check. Bit-for-bit reproducible
executables across machines are not claimed.

## The optional operating stack

The runtime and the instructions solve different parts of the problem. The
skills teach the working pattern: maintain a campaign dependency graph,
launch ready independent branches, choose models with enough capability,
and aggregate through the nearest responsible agent. They do not require a
fixed number of agents or a permanent coordinator for every task.

The [policy bundle](slopdex/policy/) preserves the historical Sol configuration.
You can use the code without it; review the files before adopting them.

**Post-drop decision, 2026-09-05:** in the first Astra field campaigns we chose
to retire the separate role-initialization layer from the active recipe. Give
the project an ordinary task; a separate coordinator is useful when the work
justifies one, not because the user remembered a title or skill name. Scope,
ownership, verification and real approval boundaries still apply. See
[the policy-evolution note](slopdex/policy/EVOLUTION.md); the original Sol
snapshot and narrative are retained, and global rollout is a separate action.

Independent writers are useful only when their write surfaces and dependencies
really are separate.

## Built with Sol. Next: Astra.

GPT-5.6 Sol led this campaign, with Terra and Luna contributing. The accepted
source, tests, source review and locally built artifact were frozen before
the parent conversation switched to Astra. Astra participated in final
installed-runtime checks and publication preparation.

Now we will test how much this approach helps with Astra: completion time,
cost, repeated work and the amount of owner intervention. We may simplify
the skills or retire patches as upstream covers their behavior. The current
observations are field evidence, not a controlled token-economics benchmark.

**On Sol: just try it. On Astra: let's find out.** Early mechanics checks do
not yet establish the stack's overall usefulness or economics on Astra.

Give it a real task. Tell us where the graph helps, where it wastes effort,
and where it fails. A reproducible counterexample is welcome.

> I did not leave Codex for another orchestrator. I liked Codex enough that,
> when its limits got in the way of my work, I started moving the limits myself.

## Attribution and license

Codex is developed by OpenAI and its contributors. The upstream [LICENSE](LICENSE)
and [NOTICE](NOTICE) are preserved; see [Slopdex attribution](slopdex/ATTRIBUTION.md)
for the derivative source and instruction bundle.
