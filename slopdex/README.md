# Slopdex source-drop materials

Slopdex is an experimental derivative of `openai/codex`. It is not an
official OpenAI release and makes no general production-readiness claim.

- Repository: [SlopSurfer4444/slopdex](https://github.com/SlopSurfer4444/slopdex)
- Distribution: **SOURCE DROP ONLY**

The current owner-authorized publication is a full source checkout pinned to
the recorded upstream base plus the accepted 57-path content. Windows binary
distribution is explicitly deferred.

This directory documents the published source generation, complete patch,
optional operating-policy references and a narrow live feature receipt.
Executables and an installer are not included.

## Reproduce the source drop

In a fresh disposable checkout of `openai/codex`:

```powershell
git checkout --detach 612e6491d50ffb80ffc4330edc4024b86e51e4bf
git -c core.autocrlf=false apply --check --cached --whitespace=nowarn <PATCH>
git -c core.autocrlf=false apply --cached --whitespace=nowarn <PATCH>
git -c core.autocrlf=false checkout-index --all --force
```

Replace `<PATCH>` with the path to `source/slopdex-612e6491d50f.patch`. This
exact-byte workflow and its destructive-to-worktree final step are documented
in `source/PATCH_RECEIPT.md`; never run it over unrelated work.

The repository root already contains that patched full source tree. Do not
apply the patch a second time there. The source-only commit is
[`e497c3d`](https://github.com/SlopSurfer4444/slopdex/commit/e497c3d358ae4a3411b0bd16b1ea7e79051e9aa7).
Its product tree and all 57 content hashes match the accepted candidate.

## Source-drop materials

| Path | Purpose |
| --- | --- |
| `source/slopdex-612e6491d50f.patch` | Complete 37-tracked + 20-new-file source patch |
| `source/PATCH_RECEIPT.md` | Exact-base apply, target-tree, EOL, and isolation proof |
| `provenance/canonical-content-manifest.txt` | Exact accepted path/content hashes |
| `provenance/BUILD_RECEIPT.md` | Sanitized build and artifact provenance |
| `provenance/manifest.current.json` | Machine-readable source, artifact, patch, and claim closure |
| `dogfood/README.md` | Sanitized narrow live retained-successor receipt |
| `KNOWN_LIMITATIONS.md` | Genuine unresolved limits and non-claims |
| `LICENSE`, `NOTICE`, `ATTRIBUTION.md` | Upstream and derivative attribution |
| `policy/` | Optional policy profile, skills, and their required references |
| `PRIVACY_SCAN.md` | Source/text privacy and reference-closure scan |

## Local held artifacts

The four executables are retained privately for provenance only and
are not part of this repository. Their same-generation layout
and compatibility are recorded in `PORTABLE_WINDOWS.md`. The owner deferred
binary publication; no runnable download is offered by this source drop.

[`SOURCE_SHA256SUMS.txt`](../SOURCE_SHA256SUMS.txt) inventories the published
source and documentation, excluding the checksum file itself. Historical
checksums of the private preparation package are not inventories of this repo.

The installed Desktop binding to the held CLI hash was independently verified.
The live sample proves one exact nested retained-successor flow only; it does
not prove global exactly-once behavior, crash recovery, arbitrary graph
reliability, or production readiness.
