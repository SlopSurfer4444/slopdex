# Slopdex for Windows x64

Download the ZIP and its `.sha256` file from
[v0.1.0-preview.1](https://github.com/SlopSurfer4444/slopdex/releases/tag/v0.1.0-preview.1).
This is a portable CLI preview, not the ChatGPT/Codex Desktop application or
an installer. It does not change your existing installation, PATH or launcher.

## Start

1. Compare the archive's SHA-256 with the accompanying checksum file:

   ```powershell
   Get-FileHash .\slopdex-v0.1.0-preview.1-windows-x86_64.zip -Algorithm SHA256
   ```

2. Extract the entire archive into a new folder. Keep all four EXEs together.
3. Open that folder in a terminal:

   ```powershell
   .\codex.exe --version
   .\codex.exe
   ```

The executable retains the upstream name `codex.exe`. It currently reports
`codex-cli 0.0.0`; identify this preview by its release name and checksums,
not that version string alone. Authenticate through the CLI's supported flow
if requested; never copy tokens or session files into bug reports.

The archive does not include account state. Interactive CLI use can read or
write normal Codex configuration and state; extraction itself does neither.
Evaluate the preview in a disposable project or separate test environment.
Do not disable operating-system security protections to run it.

## Included files

- `codex.exe`: the user-facing CLI.
- `codex-code-mode-host.exe`: sibling helper used for Code Mode.
- `codex-command-runner.exe` and `codex-windows-sandbox-setup.exe`: internal
  protocol helpers, not commands to invoke directly.
- English usage/build notes, upstream LICENSE/NOTICE and internal checksums.

The [r2 build receipt](provenance/WINDOWS_RELEASE_R2.md) binds the four hashes,
source identity and exact verification scope. Compiler-local source/cache
paths were remapped during compilation; binaries were not stripped or patched
afterward. This package is not a signed or supported OpenAI release.

## Scope

The source is the published 57-path Slopdex candidate on upstream `612e6491`.
Migration to a newer upstream is separate work, not part of this archive.
This exact rebuild passed bounded offline launch/protocol checks and the
documented local-path scan. It has not been installed into Desktop or used
for a new live recursive-model campaign. The earlier nested dogfood receipt
belongs to a different binary generation of the same source.

See [known limitations](KNOWN_LIMITATIONS.md), including the unresolved Windows
sandbox temporary-directory seam. No restart/crash replay, arbitrary graph
reliability, general sandbox-safety or cross-platform binary claim is made.
