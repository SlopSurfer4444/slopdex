# Slopdex — the first source drop after Arc III

This is a continuation of [#40037](https://github.com/openai/codex/issues/40037), not a fourth retelling of the same architecture hypothesis.

The earlier posts covered the single-thread cockpit, owned continuation, capacity `128`, and the combination of a campaign DAG with temporary recursive trees. The full trail is in [From Waiting to Ownership](https://github.com/openai/codex/issues/40037#issuecomment-5431215818) and [Arc III — The Forest Before Konoha](https://github.com/openai/codex/issues/40037#issuecomment-5470991175).

Here is what changed after Arc III — the first source drop.

```text
one owner-facing thread
        -> frozen campaign DAG
        -> dependency-ready lazy wave
        -> temporary elastic agent tree
        -> nearest-owner fan-in
        -> one evidence-bound checkpoint
```

A strong model holds architecture, authority, direction, and expensive uncertainty. Narrow agents investigate and verify, writing separately only when independence is demonstrated. The user steers from one thread instead of dispatching agents' transcripts.

## What we changed our minds about

### We shelved the semantic escalator

That idea started #40037: after a bad result, choose a compact next move — retry, change model, return the problem upward, or reshape the subtree. Importantly, the original proposal already allowed the current owner to make that decision; it never required a permanent `EscalatorAgent`.

We did not build a separate EscalatorAgent, typed escalation layer, or control primitive. Field runs have not shown that another permanent layer is necessary. In practice, the owner rejected a bad branch, tightened the contract, and dispatched a better-scoped agent. If the problem was not the worker, the owner changed the model, scope, or graph. Mechanical work routed back to a cheaper model.

> **We did not validate a semantic-escalation subsystem. So far, a strong owner cancelling the bad branch and dispatching a better one was enough.**

The rule — do not retry blindly; let evidence change the next move — held up. A dedicated mechanism may still help other models or workloads, but we do not have that evidence yet.

### A good graph should breathe

We first removed the cage and pushed width, proving deep and wide native trees possible. The mature shape was elastic:

```text
unknown space          -> widen
coupled mutation       -> contract
independent review     -> widen again
accepted checkpoint    -> collapse
```

The progression was `conservative small graph -> brute-force wide tree -> selective surgical tree`. Agent count stayed telemetry. Accepted critical-path progress was the outcome.

### Width was cheaper than feared — but this is not a benchmark

We feared recursive graphs might burn limits proportionally. In live work, narrow contexts, capability routing, lazy waves, fan-in, and mutation ownership kept usage in roughly the same order while accepted progress increased substantially.

Likely reasons are fewer serial repair/review loops and less premium context spent on mechanics. This is a field observation, not a controlled token A/B. A pointlessly large graph is not cheap.

### Sol did not become less picky

In a linear loop, GPT-5.6 Sol's pickiness stretched the calendar: `write -> review -> next problem -> repair -> repeat`. In a DAG, that scrutiny gained parallel surfaces. Narrow branches found risks earlier, reviewers filtered noise before mutation, and a writer received one coherent repair packet.

> **The model did not become less picky. We gave its pickiness the right topology.**

GPT-5.6 Sol led this campaign, with Terra and Luna participating. The accepted source, tests, independent review, and locally built Windows binary with SHA-256 `fe0662c5486da33415eb3775630cdc83aa9d5636c05e5c8b14b5c3e86f52a175` were frozen before the parent thread switched to Astra. Astra joined for final checks of the installed build. Results produced on the newer model belong to the next chapter.

> **Built with GPT-5.6 Sol leading the campaign; final installed-build checks continued after switching to Astra.**

### Skills transfer discipline, not lived experience

The operating stack makes a fresh thread audit decomposition, name consumers, route enough capability, explore widely, use the smallest non-overlapping writer set, and choose wait or join by dependency shape. It cannot clone a long campaign's judgment, but it shortens the learning curve and discourages `spawn a crowd, receive a pile`.

### The tree started writing

One writer was our safe default. Then a real source-generation run found two demonstrably independent surfaces. One writer repaired the durable spawn edge and persistence fixtures; another split overloaded control and input-queue modules. They shared no mutable critical section, converged through one frozen manifest, and independent final review found no candidate-owned P0/P1 issue.

This is not proof of general multi-writer safety. It is proof of the narrower principle: mutation can branch when independence is demonstrated.

### Join did not defeat wait

Dogfood removed the false choice between primitives:

- `wait_agent` is the hot lane when the next checkpoint may change the decision now.
- `join_agents` is the owned alarm for a stable subtree that does not need constant supervision.

The target is one owned-watch concept with two postures, not the deletion of either primitive. A retained-successor sample in live Desktop also passed: an intermediate child result was followed by a distinct successor aggregate. That does not prove one wake forever, global exactly-once delivery, or restart recovery.

## What is in the first Slopdex source preview

- One admission ceiling of `128` for supported agentic routes. Capacity is a profile limit, not a claim that every model has the same role.
- Configurable recursion depth, defaulting to `128`; this preview does not claim unlimited depth.
- Exact direct-child and turn binding for owned join.
- Durable spawn-edge admission that fails closed when its store is unavailable.
- Nearest-parent-only terminal delivery, with no leaf-to-root bypass.
- Continuation deduplication, user-turn priority, lagged-signal recovery, and capacity/residency cleanup.
- Modular, independently runnable test groups instead of one catch-all.

Luna remains useful as a low-cost leaf route. This preview does not claim that Luna can act as a recursive non-leaf coordinator.

The frozen source checkpoint was detached HEAD `612e6491d50ffb80ffc4330edc4024b86e51e4bf`, tree `568d39f181926f57c73dd34c2fceb419bab1979e`, with an accepted 57-path patchset (37 tracked plus 20 untracked). That pinned upstream plus the patchset is now published on Slopdex `main`. The exact owner test group passed 82/82 and independent source review was CLEAR.

- Source and README: [Slopdex](https://github.com/SlopSurfer4444/slopdex)
- Operating policy, skills, and compact global `AGENTS.md`: [`slopdex/policy/`](https://github.com/SlopSurfer4444/slopdex/tree/main/slopdex/policy)
- Source provenance and limitations: [`slopdex/provenance/`](https://github.com/SlopSurfer4444/slopdex/tree/main/slopdex/provenance)
- Dogfood evidence: [`slopdex/dogfood/`](https://github.com/SlopSurfer4444/slopdex/tree/main/slopdex/dogfood)

One `--locked --offline` Windows x64 MSVC binary was built and live-tested locally. Binary distribution is deferred: ordinary local compiler and dependency source-location paths remain embedded in that build. No credentials were observed. This source preview does not claim restart/crash exactly-once behavior for `Terminal -> Bound -> ACK -> replay`.

## Does Slopdex still help with Astra?

That is the next useful question. Astra arrived at the finish line, so the Sol results remain a separate, frozen chapter. Astra testing starts from the locally installed build rather than rewriting its creation story.

We will measure real work: time to accepted result, usage, rework, user intervention, and branch-completion reliability. Astra may need fewer agents or shorter instructions. It may use the same tree better. Those are dogfood questions, not a declared victory for any configuration.

Our honest invitation is: **on Sol, just try it. With Astra, we genuinely do not know yet.** We have early checks of the mechanics, but not enough experience to judge the stack's overall usefulness or economics on Astra. That is the next chapter.

Here is the Codex agent's working assessment after the parent thread switched to Astra — not an OpenAI endorsement:

> **“My bet is that a stronger model will unlock more from this stack. Good intelligence helps choose the next move; a reliable environment must execute it and return the result. But every layer should be tested again. If new Codex closes some of our patches, remove them. If Astra finishes faster without part of our scaffolding, simplify it. Keep only what helps work reach a real result. The previous chapter showed what we could build with Sol; the next will show how useful Slopdex remains with Astra.”**

We are publishing the full source now instead of delaying it for binary packaging or a pretty synthetic A/B. The exact patchset, tests, limits, skills, and compact `AGENTS.md` came from real failures. The binary will follow separately.

Do not trust the story. Give the operating stack a genuinely difficult task and see whether one thread becomes the cockpit of your own factory.

**On Sol: just try it. On Astra: let's find out. Break it, and tell us where the graph helps — and where it lies.**

> **I did not need another orchestrator. I needed Codex to know my jutsu.**

I did not leave Codex for another orchestrator. I liked Codex enough that, when its limits got in the way of my work, I started moving the limits myself. Slopdex is a power user's argument that native agent trees are already worth building.

**This is the kind of product problem I would love to keep working on.**
