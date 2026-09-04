# Slopdex Windows x64 release-build receipt

Status: **SANITIZED_PUBLIC_RECEIPT**. This is a sanitized derivative of the
immutable private build receipt; it is not byte-identical to that receipt. The
private receipt remains authority for the complete local build record.

## Provenance

- Source root: `<SOURCE_CHECKOUT_ROOT>`
- Detached HEAD: `612e6491d50ffb80ffc4330edc4024b86e51e4bf`
- Committed tree / clean base index tree:
  `568d39f181926f57c73dd34c2fceb419bab1979e`
- Canonical content manifest:
  `159abbdba8f1d59d996b1dfa88b4a64eb968ae4683cea58f1965dad7d34bde65`
- Canonical set: 37 tracked diff paths + 20 new files; sorted UTF-8
  LF-terminated `T<TAB>path<TAB>lowercase_sha256` /
  `U<TAB>path<TAB>lowercase_sha256` records.
- Unrelated dirty tail: 187 tracked paths preserved and excluded.

## Build

- Command class: MSVC x64 wrapper invoking
  `cargo build --locked --offline --release` for the CLI and three Windows
  helper binaries.
- Working directory: `<SOURCE_CHECKOUT_ROOT>/codex-rs`
- Target directory: `<CARGO_TARGET_DIR>`
- Cargo home: `<CARGO_HOME>`
- V8 inputs: `<RUSTY_V8_ARCHIVE>`, `<RUSTY_V8_SRC_BINDING_PATH>`
- Parallelism: `CARGO_BUILD_JOBS=12`; `RUST_MIN_STACK=16777216`
- Outcome: exit code 0; release profile finished in 43m 05s.
- Build window: 2026-09-04T20:16Z to 2026-09-04T20:59:05Z.

## Immutable artifact facts

| File | Bytes | SHA-256 |
| --- | ---: | --- |
| `slopdex-windows-x64.exe` | 298399744 | `fe0662c5486da33415eb3775630cdc83aa9d5636c05e5c8b14b5c3e86f52a175` |
| `codex-code-mode-host.exe` | 73581568 | `8c1e06c7c164d09c21a7b988908c1fe268ce4840eb32732943d2d062446116b6` |
| `codex-command-runner.exe` | 8219648 | `9e7a53cc5657ed0b915c6fbdf4bc74759a902f8e9396ccee990ee2348ceeae95` |
| `codex-windows-sandbox-setup.exe` | 15521792 | `510ee34ad331f89505253275645523d68dab5460c746af02a6bf4244efcad09d` |

The immutable private build-receipt SHA-256 is
`12b331bd4bb6dcc103deaefc979c8d6a309a72d6c0dcc7d4b3d9432b204a3b4e`.

## Verification

- Source manifest re-read after build: exact match.
- Cargo/rustc processes after build: none observed.
- CLI bounded non-network smoke: `--version` -> `codex-cli 0.0.0`, exit 0.
- Code Mode helper bounded `--help`: exit 0.
- All four public copies re-hashed and matched the immutable artifact
  generation byte-for-byte.
- Installed Desktop binding to the exact CLI hash was independently verified.
- The complete patch proof is recorded in `../source/PATCH_RECEIPT.md`.
- No install, runtime switch, commit, push, credential access, or source edit
  was performed during package completion.
