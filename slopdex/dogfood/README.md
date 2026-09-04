# Exact live feature receipt

Status: **LIVE_NESTED_RETAINED_SUCCESSOR_SAMPLE_PASS**.

## Binding

- Installed Desktop CLI SHA-256:
  `fe0662c5486da33415eb3775630cdc83aa9d5636c05e5c8b14b5c3e86f52a175`.
- Sanitized topology: root -> Astra verifier -> Terra verifier -> Luna checksum.
- Requested routes: Astra/high, Terra/medium, Luna/low. Backend effective-route
  telemetry was unavailable, so no observed-routing claim is made.

## Observed sequence

The Luna leaf returned checksum evidence. Terra resumed automatically and
aggregated it. Astra received both a distinct intermediate Terra `PENDING`
delivery and the later `TERRA_LIVE_RELEASE_VERIFIER` successor result without
registering a second join.

An initial duplicate-delivery diagnosis was retracted after read-only
frozen-source adjudication. The retained-successor contract intentionally
permits one intermediate delivery followed by a distinct child-successor
result. The source guard in
`codex-rs/core/src/agent/control/join_observer.rs`, delivery ownership transfer,
and the existing recursive-delivery test support that interpretation. No same
exact terminal result was observed twice.

No retry, polling, replacement, new test, rebuild, runtime switch, or product
source edit occurred during the sample or adjudication.

## Claim ceiling

This receipt accepts only the one observed nested retained-successor sample.
It does not establish global exactly-once behavior, every intermediate/successor
ordering, crash or restart recovery, arbitrary depth/width reliability,
backend routing identity, performance, or production readiness.

The earlier checksum receipt covered a historical 17-file package only. Its
sum-file hash is retained as historical evidence, not as current package
closure. The published source and documentation have a separate inventory in
[`SOURCE_SHA256SUMS.txt`](../../SOURCE_SHA256SUMS.txt).
