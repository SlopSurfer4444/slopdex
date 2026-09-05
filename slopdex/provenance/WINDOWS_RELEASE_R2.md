# Windows x64 preview: privacy-remapped rebuild r2

This sanitized receipt describes the downloadable four-EXE generation for
[Slopdex v0.1.0-preview.1](https://github.com/SlopSurfer4444/slopdex/releases/tag/v0.1.0-preview.1).
The original Sol-built installed artifact remains a separate historical
record in [BUILD_RECEIPT.md](BUILD_RECEIPT.md).

## Source and build

- Upstream base: `612e6491d50ffb80ffc4330edc4024b86e51e4bf`.
- Base tree: `568d39f181926f57c73dd34c2fceb419bab1979e`.
- Published product commit: `e497c3d358ae4a3411b0bd16b1ea7e79051e9aa7`.
- Accepted product tree: `819d97ef1dc02bd51aae7d99d88951940584e057`.
- Source manifest: `159abbdba8f1d59d996b1dfa88b4a64eb968ae4683cea58f1965dad7d34bde65`;
  all 57 source hashes matched before and after the build.
- Locked offline MSVC release build, `x86_64-pc-windows-msvc`, Rust/Cargo 1.95.0.
- Build ended 2026-09-05T00:37:10Z; exit 0, elapsed 3678 seconds.
- Rust/native path mappings and `/PDBALTPATH:%_PDB%` were applied at build time.
  Native `/W4` was retained so AWS-LC's compile-only intrinsic probe correctly
  rejects unsupported builtins. No source changes or post-build EXE edits.

## Exact artifacts

| File | Bytes | SHA-256 |
| --- | ---: | --- |
| `codex.exe` | 297780736 | `7727dfc499269496b687a1499ee8b109f65a7da7a90f81106a5276934e4fffed` |
| `codex-code-mode-host.exe` | 73126400 | `707dbbb7ddf3d92ecea941e85a9ee316e77afc9fc5f431a40f5d5f6d864f1850` |
| `codex-command-runner.exe` | 8081408 | `6f31a6d9a4fe7ce1a1fdae3b0180daa9d1b6bf9f5877a69f30fd9f9013451563` |
| `codex-windows-sandbox-setup.exe` | 15337472 | `d9cbfe14c1de951b5aa474aeec2b6fdd3f5471c5876aacbd9bfb57bbfd5d701f` |

The ZIP has a companion SHA-256 asset and internal file checksums. Every
packaged EXE must match this table; no helper from another build is included.

- Archive: `slopdex-v0.1.0-preview.1-windows-x86_64.zip`, 141574641 bytes.
- Archive SHA-256: `c815339a35857a223b77a78e0366eebfd1e84232c4fd1411e413ee64606b3b62`.
- Nine safe file entries beneath one top-level folder; eight internal checksum
  records exclude the checksum file itself. EXEs match the table above;
  LICENSE and NOTICE match their published source copies.

## Checks and limits

- CLI `--version`: exit 0, `codex-cli 0.0.0`.
- Code Mode helper `--help`: exit 0.
- The two internal helpers reached their expected invalid-input protocol
  boundary with exit 1. This is a launch check, not a functional sandbox test.
- ASCII and both UTF-16LE alignments: zero identified private-prefix hits;
  zero unexpected structured drive-path candidates. CodeView PDB references
  contain basenames only. This is not a universal secret-free guarantee.
- Frozen and packaged hashes preserve the verified build bytes.

The accepted source tests/review predate this rebuild. The historical live
nested sample used CLI `fe0662c5...52a175`, not this archive. No new r2 Desktop
installation, live-model dogfood, restart durability or benchmark is claimed.
This is a preview with [known limitations](../KNOWN_LIMITATIONS.md), not an
official OpenAI release.
