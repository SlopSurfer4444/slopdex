# Slopdex

**One thread. A whole workshop behind it.**

**Status:** Development is paused at the published preview. Further time and AI-agent budget are directed toward applied projects. Source, test records and the experimental Windows CLI remain available; see [known limitations](slopdex/KNOWN_LIMITATIONS.md) before evaluating the preview.

Slopdex is a power-user Codex source preview for recursive agent work: keep the
architecture in one conversation, split useful independent work into temporary
trees, and bring results back to the agents responsible for them.

**Source and a Windows x64 CLI preview are available.** The modified Rust
source and tests are in this repository, alongside the portable patch,
verification notes. Nothing here replaces
your installed Codex automatically.

[Download Slopdex v0.1.0-preview.1 for Windows x64](https://github.com/SlopSurfer4444/slopdex/releases/tag/v0.1.0-preview.1)
and follow the [portable CLI instructions](slopdex/PORTABLE_WINDOWS.md).

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

Prefer the ready-made Windows archive? See the download above. Building from
source remains available for reviewing and modifying the patch.

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

## Native waiting beyond the current turn

The parent can register an owned join for a specific child turn and finish its current turn while the child continues working. Completion is delivered through the native session queue, allowing the parent to resume and aggregate the result without a polling loop.

The patch preserves the relevant parent-child binding across the covered successor continuation. It also handles delayed completion signals, suppresses duplicate delivery for the same result identity, and gives user input priority. These lifecycle changes are separate from increasing agent capacity or recursion depth.

In the recorded Windows sample, a leaf completed, its coordinator resumed automatically, and the parent received a distinct intermediate result followed by the coordinator's later result without registering a second join. See the [live sample](slopdex/dogfood/README.md) and [implementation and regression map](slopdex/REVIEW_GUIDE.md). The sample is scoped evidence, not a general crash/restart delivery guarantee.

## Development history

GPT-5.6 Sol led the original campaign, with Terra and Luna contributing. The source, tests and review checkpoint were frozen before the coordinating conversation switched to Astra. Astra participated in installed-runtime checks and publication preparation.

The downloadable Windows archive is a later privacy-remapped rebuild of the same source, coordinated on Astra. Its checks cover build, bounded offline launch and local-path scanning; the earlier live nested sample belongs to the original artifact. The published observations are field evidence, not a controlled token-economics benchmark.

The experiment is retained as a source and CLI preview. Further development is paused so that time and AI-agent budget can go to applied projects. Earlier plans and campaign details remain in the project notes and Git history.

## Attribution and license

Codex is developed by OpenAI and its contributors. The upstream [LICENSE](LICENSE)
and [NOTICE](NOTICE) are preserved; see [Slopdex attribution](slopdex/ATTRIBUTION.md)
for the derivative source and instruction bundle.
