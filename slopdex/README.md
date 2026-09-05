# Slopdex source-drop materials

Slopdex is an experimental derivative of `openai/codex`. It is not an
official OpenAI release and makes no general production-readiness claim.

- Repository: [SlopSurfer4444/slopdex](https://github.com/SlopSurfer4444/slopdex)
- Distribution: **SOURCE + WINDOWS X64 CLI PREVIEW**

The current owner-authorized publication is a full source checkout pinned to
the recorded upstream base plus the accepted 57-path content. A separate
[Windows release archive](https://github.com/SlopSurfer4444/slopdex/releases/tag/v0.1.0-preview.1)
contains a privacy-remapped rebuild of the same source.

This directory documents the published source generation, complete patch,
optional operating-policy references and a narrow live feature receipt.
Executables are release assets, not Git source files. No installer is included.

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
| `provenance/WINDOWS_RELEASE_R2.md` | Downloadable Windows rebuild, exact hashes and verification scope |
| `provenance/manifest.current.json` | Machine-readable source, artifact, patch, and claim closure |
| `dogfood/README.md` | Sanitized narrow live retained-successor receipt |
| `KNOWN_LIMITATIONS.md` | Genuine unresolved limits and non-claims |
| `LICENSE`, `NOTICE`, `ATTRIBUTION.md` | Upstream and derivative attribution |
| `policy/` | Optional policy profile, skills, and their required references |
| `PRIVACY_SCAN.md` | Source/text privacy and reference-closure scan |

## Current Windows archive and historical artifacts

Use [PORTABLE_WINDOWS.md](PORTABLE_WINDOWS.md) for the downloadable CLI package.
Keep all four executables together; no Desktop application or launcher is
included. Current artifact hashes and bounded checks are in the
[r2 receipt](provenance/WINDOWS_RELEASE_R2.md).

The original four executable bytes used for the installed-Desktop sample
remain privately held and are not the downloadable rebuild. Their historical
record remains in `provenance/BUILD_RECEIPT.md`.

[`SOURCE_SHA256SUMS.txt`](../SOURCE_SHA256SUMS.txt) inventories the published
source and documentation, excluding the checksum file itself. Historical
checksums of the private preparation package are not inventories of this repo.

The installed Desktop binding to the historical held CLI hash was independently verified.
The live sample proves one exact nested retained-successor flow only; it does
not prove global exactly-once behavior, crash recovery, arbitrary graph
reliability, or production readiness.
