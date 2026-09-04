# Known limitations

This is an experimental preview, not an official, stable, upstream-accepted,
or generally production-ready release.

See the [reconciled findings index](findings/README.md) for exact status and
evidence boundaries. It separates source-patched behavior from still-open
reported REDs, unconfirmed historical leads and unimplemented capabilities;
not every entry has a publishable reproducer yet.

- One installed-Desktop nested retained-successor sample passed. It does not
  establish global exactly-once delivery, all successor sequences,
  restart/crash durability, durable acknowledgement/replay, exactly-once model
  execution, unattended graph reliability, or a general scheduler/inbox.
- The thread-capacity default and V2 maximum concurrency are 128, including
  the root for the V2 envelope. The default agent depth is also 128 and is
  configurable through `agents.max_depth`; depth is not unlimited. These
  settings are capacity limits, not proof that a 128-wide or 128-deep workload
  is safe, useful, economical, or accepted by every backend.
- Luna was exercised only as the leaf of the recorded nested sample. This does
  not establish Luna as a recursive V2 coordinator or prove backend schema
  acceptance for other graph shapes.
- An unresolved Windows sandbox temporary-directory permission-propagation
  seam remains known-red. No general Windows sandbox safety claim is made.
- The patch contains only the canonical 37 modified and 20 new files; no
  unrelated workspace changes are included.
- Eighteen accepted source files contain mixed CRLF/LF bytes. The patch
  preserves those exact bytes in Git blobs, but a checkout with automatic EOL
  conversion may materialize different working-tree byte hashes. See
  `source/PATCH_RECEIPT.md`.
- No benchmarked speedup, cost saving, broad compatibility, resource claim,
  installer behavior, or binary distribution is claimed by this source drop.

Run only in a disposable isolated environment. Do not copy authentication,
session, configuration, or machine-identifying data into reports or assets.
