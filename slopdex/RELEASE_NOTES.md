# Slopdex release notes

## v0.1.0-preview.1: Windows x64 CLI archive

The [downloadable preview](https://github.com/SlopSurfer4444/slopdex/releases/tag/v0.1.0-preview.1)
contains a later privacy-remapped build of the same accepted 57-path source.
There are no new product patches in this binary upload and it is not yet the
newer-upstream transplant. All four EXEs come from one successful Windows
MSVC build; exact offline launch checks and the documented local-path scan
passed. See [usage](PORTABLE_WINDOWS.md) and [r2 provenance](provenance/WINDOWS_RELEASE_R2.md).

The Sol-led source work and original live artifact remain the historical
checkpoint below. The privacy rebuild was coordinated after the switch to
Astra; its binary hashes are different. Historical live-dogfood evidence is
not relabeled as a live test of the new archive.

## Historical source drop

The original source publication was the full source tree at
[SlopSurfer4444/slopdex](https://github.com/SlopSurfer4444/slopdex), produced
from the pinned upstream base and the accepted 57-path patch. Binary
distribution was deferred at that checkpoint.

## Frozen identity

- Upstream repository: `https://github.com/openai/codex`
- Detached HEAD: `612e6491d50ffb80ffc4330edc4024b86e51e4bf`
- Committed tree: `568d39f181926f57c73dd34c2fceb419bab1979e`
- Canonical manifest SHA-256:
  `159abbdba8f1d59d996b1dfa88b4a64eb968ae4683cea58f1965dad7d34bde65`
- Complete patch target tree: `819d97ef1dc02bd51aae7d99d88951940584e057`
- Held CLI SHA-256:
  `fe0662c5486da33415eb3775630cdc83aa9d5636c05e5c8b14b5c3e86f52a175`
- Private immutable build-receipt SHA-256:
  `12b331bd4bb6dcc103deaefc979c8d6a309a72d6c0dcc7d4b3d9432b204a3b4e`

## Feature and evidence table

| Surface | Evidence | Claim ceiling |
| --- | --- | --- |
| Agent-graph store and ownership data | Frozen source delta and accepted build | No general unattended-graph claim |
| Agent control, admission, spawn, residency, and legacy paths | Frozen source delta and prior accepted tests/review | No scheduler or global reliability claim |
| Session, task, state, and thread-manager boundaries | Frozen source delta and prior accepted tests/review | No crash/restart durability claim |
| Native multi-agent wait/join handlers | One live nested retained-successor sample passed | No global exactly-once or all-orderings claim |
| Capacity and recursion configuration | Threads/V2 concurrency 128; depth default 128, configurable via `agents.max_depth` | Not unlimited; not workload acceptance evidence |
| Windows temporary-directory permission seam | Unresolved known-red seam | No stable Windows sandbox safety claim |

## Campaign attribution

The accepted source, tests, review, and held artifact were produced by a
Sol-led campaign with Terra and Luna before the coordinating parent switched
to Astra. Astra then checked installed-runtime binding and release material.
No product source changes occurred after that switch.

For the recorded live nested sample, requested routes were Astra/high,
Terra/medium, and Luna/low. Effective backend routing telemetry was unavailable.

## Source-drop completion

The complete 57-path source patch and transitive skill references are included.
The patch independently applies to the exact base index and recreates the
accepted raw content hashes when materialized with EOL conversion disabled.
The same-generation CLI and three Windows helpers remain local held artifacts;
they are evidence for the live receipt, not current publication payload.
