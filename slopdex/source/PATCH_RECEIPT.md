# Complete source patch receipt

Status: **EXACT_BASE_APPLY_AND_CONTENT_CLOSURE_PASS**.

## Patch identity

- File: `slopdex-612e6491d50f.patch`
- Base commit: `612e6491d50ffb80ffc4330edc4024b86e51e4bf`
- Base tree: `568d39f181926f57c73dd34c2fceb419bab1979e`
- Target tree: `819d97ef1dc02bd51aae7d99d88951940584e057`
- Patch bytes: 2876642
- Patch SHA-256:
  `b4cd2ce24d68c323a8d6f1406a3421248ea76781955f2296e55258e30e687b98`
- Coverage: 57 paths, comprising 37 tracked modifications and all 20 new
  files from `../provenance/canonical-content-manifest.txt`.

The patch was generated with full 40-hex object headers, `--binary`, and no
rename detection. It is binary-capable; this generation contains text source
files, so no `GIT binary patch` payload was needed.

## Isolation and apply proof

Generation used a task-private index and task-private object directory with the
source repository object store mounted read-only as an alternate. Exact source
bytes were inserted with `git hash-object --no-filters`; the real source index
SHA-256 was identical before and after generation and verification.

A second independent temporary index/object directory was seeded from the
exact base and accepted the patch with:

```text
git apply --cached --whitespace=nowarn slopdex-612e6491d50f.patch
```

It produced the same target tree as generation, all 57 object IDs matched,
the path set exactly matched the canonical manifest, and all 20 new files were
present.

## Line endings and exact materialization

Thirty-nine accepted files are LF-only. Eighteen accepted files contain mixed
CRLF/LF bytes. A normal `git add` under `core.autocrlf=true` would normalize
those 18 files, so that candidate was rejected. The final patch preserves the
accepted raw bytes in its target blobs.

In a fresh disposable checkout at the exact base, the verified exact-byte
workflow is:

```text
git -c core.autocrlf=false apply --check --cached --whitespace=nowarn <PATCH>
git -c core.autocrlf=false apply --cached --whitespace=nowarn <PATCH>
git -c core.autocrlf=false checkout-index --all --force
```

The final materialization test produced all 57 files with zero SHA-256
mismatches against the canonical content manifest. Do not use
`--whitespace=fix`; it would change accepted bytes. A checkout that enables EOL
conversion may have different working-tree byte hashes even though the index
target tree remains exact.

The publication consumer independently applied this patch to a clean clone and
confirmed the same target tree and all 57 paths. The resulting source commit is
`e497c3d358ae4a3411b0bd16b1ea7e79051e9aa7`; publication remains the consumer's
separate action.

The workflow intentionally uses a clean disposable checkout because the final
materialization command overwrites working-tree files from the temporary
index. It must not be run over unrelated user work.
