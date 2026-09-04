# Deferred Windows binary notes

Binary distribution is deferred by the owner. The current public action is a
source-only drop; there is no runnable download or installer. These notes
describe the local held generation only.

If a later separately authorized binary release reuses this generation, all
four files must remain together and the three `codex-*` helpers must not be
renamed.

The local non-network verification invocation was:

```powershell
.\artifact\slopdex-windows-x64.exe --version
```

It returned `codex-cli 0.0.0` with exit code 0. No interactive CLI use was
included in package preparation.

## Helper compatibility

- All four executables are Windows x64 MSVC release outputs from the same
  accepted build generation.
- `codex-code-mode-host.exe` is discovered next to the CLI when Code Mode is
  enabled. Its bounded `--help` invocation exited 0 during package preparation.
- `codex-command-runner.exe` and `codex-windows-sandbox-setup.exe` are internal
  protocol helpers discovered next to the CLI. They are not user-facing
  commands and should not be invoked directly.
- The CLI filename may remain `slopdex-windows-x64.exe`; helper discovery is
  based on the executable directory and the exact helper filenames.
- No cross-platform, installer, PATH registration, file association,
  auto-update, or broad Windows sandbox guarantee is claimed.

The accepted binaries embed compiler source-location paths containing local
build-root prefixes. They contain no observed credential or runtime config,
but remain held locally under the current owner decision. A future binary
generation needs its own authority and privacy acceptance.
