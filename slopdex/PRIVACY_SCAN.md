# Privacy and closure scan

Source/text result: **CLEAR**. Current Windows r2 result:
**DOCUMENTED LOCAL-PATH SCAN PASS**.

## Source and text payload

The scan covered the complete source patch, canonical manifest, public build
and patch receipts, policy sample, copied skills and references, attribution,
limitations, release notes, and sanitized live receipt.

- No actual local user-profile root, build-cache root, workspace root, private
  evidence path, PID, thread ID, session identifier, email address, credential,
  private key, bearer token, API key, or account identifier was found.
- Placeholder paths in the sanitized build receipt are angle-bracket labels,
  not machine values.
- Public URLs are limited to the upstream `openai/codex` repository and the
  owner-selected `SlopSurfer4444/slopdex` repository.
- The current public narrative is the English shortgrid. Owner-review drafts
  are not part of the current published file set; earlier commits remain in
  Git history.
- Relative Markdown links in the authored Slopdex guides and current policy
  resolve inside the repository. The preserved `UPSTREAM_README.md` excerpt
  is not a relocated installation guide; its original source-relative links
  are outside this closure check.
- Every `references/...` link in the three current skills resolves. The
  campaign skill includes its frozen-upstream reference. The retired role
  skill and its three references are no longer in the installable bundle;
  earlier versions remain in Git history.

## Exact source closure

- Canonical manifest SHA-256:
  `159abbdba8f1d59d996b1dfa88b4a64eb968ae4683cea58f1965dad7d34bde65`.
- The manifest contains exactly 57 records: 37 tracked modifications and 20
  new files. All 57 source bytes matched their recorded SHA-256 values.
- Patch SHA-256:
  `b4cd2ce24d68c323a8d6f1406a3421248ea76781955f2296e55258e30e687b98`.
- Independent exact-base cached apply produced target tree
  `819d97ef1dc02bd51aae7d99d88951940584e057`; all 57 target object IDs and
  exact-byte materialized hashes matched.

## Downloadable Windows r2 binaries

The [r2 receipt](provenance/WINDOWS_RELEASE_R2.md) records a distinct, successful
rebuild using compile-time Rust/native path remapping and basename-only PDB
references. All four EXEs were scanned as raw ASCII and both UTF-16LE byte
alignments for the identified private prefixes, structured drive paths and
CodeView/PDB paths. No targeted private prefix or unexpected drive-path
candidate was found. Generic Windows runtime paths and a source-code example
are not build-machine identifiers.

The exact four rebuilt files, not the original held files, are packaged for
download. The scan proves its stated rules, not the absence of every possible
secret or identifying string. Raw build logs and local receipts are not assets.

## Historical local held binaries

All four accepted executables contain compiler source-location strings with a
local user-profile Rust toolchain prefix and a local build-cache Cargo registry
prefix. The matches are compiler/library/dependency source paths, not embedded
credentials, account IDs, runtime configuration, user data, or session data.

The owner deferred publication of that generation, so those files remain local provenance
artifacts and are outside the current source drop. They were not modified,
stripped, rebuilt, uploaded, or published.

The private preparation directory had a separate checksum inventory including
the held binaries. The public repository instead includes
[`SOURCE_SHA256SUMS.txt`](../SOURCE_SHA256SUMS.txt) for its final source and
documentation, excluding that checksum file itself. No executable is added by
this Slopdex source publication.
