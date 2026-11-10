# frpc-tray

[![CI](https://github.com/luckzylp/frpc-tray/actions/workflows/ci.yml/badge.svg)](https://github.com/luckzylp/frpc-tray/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

A lightweight Windows system-tray manager for [frpc](https://github.com/fatedier/frp). It keeps
the `frpc` client running in the background, lets you start/stop it and switch between multiple
configuration files from the tray menu, and downloads or updates the `frpc.exe` binary for you.

## Features

- **Tray status icon** — different icons for *running*, *idle* (binary ready, stopped) and
  *no binary*.
- **Start / stop frpc** from the tray menu; `frpc` runs without a console window, with its
  working directory set to the config folder.
- **Crash visibility** — if `frpc` exits on its own the icon returns to *idle* and the exit
  code is logged.
- **Multiple configurations** — every `*.toml` / `*.ini` file in `frp/config` shows up in the
  `配置` submenu. Switching while running restarts `frpc` with the new file.
- **Automatic frpc provisioning** — on first launch the matching `frpc.exe` is downloaded from
  the official frp GitHub release and extracted next to the app. Downloads try
  `reqwest` first, then `curl`, then PowerShell as fallbacks.
- **Update check** — compares the installed frp version with the latest GitHub release at
  startup (silently) and on demand from the menu, then replaces `frpc.exe` atomically with a
  `.bak` rollback.
- **Start with Windows** — per-user autostart via `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`.
- **Single instance** — launching a second copy simply exits.
- **Logging** — app events are appended to `frp/logs/frpc-tray.log`; `frpc`'s own output goes
  to `frp/logs/frpc.log`, which is truncated once it exceeds 2 MB.

> The tray menu is currently in Chinese only.

## Requirements

- Windows 10/11 (x64)
- To build: Rust 1.85 or newer (the crate uses the 2024 edition) with the MSVC toolchain
- Internet access on first run, so `frpc.exe` can be downloaded

## Build

```powershell
git clone https://github.com/luckzylp/frpc-tray.git
cd frpc-tray
cargo build --release
```

The binary is written to `target/release/frpc-tray.exe`. The build script embeds the icon and
the application manifest (`app.rc`, `app.manifest`) via `embed-resource`, so no extra tooling
is needed on a Windows machine with the MSVC toolchain installed.

## Usage

1. Put `frpc-tray.exe` in a folder where it has write permission (for example
   `%LOCALAPPDATA%\frpc-tray`). The app creates its runtime files next to the executable.
2. Add one or more frp client configuration files to `frp/config` (for example `frpc.toml`).
   `frpc.toml` is used by default when present; otherwise the first file alphabetically.
3. Run `frpc-tray.exe` and use the tray icon:
   - `启动 frpc` / `停止 frpc` — start or stop the client
   - `配置` — pick the active configuration file
   - `开机自启` — toggle start with Windows
   - `检查更新` — check for and install a newer frp release
   - `重新下载 frpc` — delete and re-fetch `frpc.exe`
   - `退出` — stop `frpc` and exit

### Runtime file layout

```text
frpc-tray.exe
frp/
├── frpc.exe              # downloaded automatically
├── settings.json         # persisted state (frp version, last config)
├── debug.log             # download/update diagnostics
├── config/               # your frpc configuration files (*.toml, *.ini)
│   └── frpc.toml
└── logs/
    ├── frpc-tray.log     # application events
    └── frpc.log          # frpc output
```

`settings.json` holds:

```json
{
  "frpc_version": "0.71.0",
  "last_config": "frpc.toml"
}
```

## CI/CD

Two workflows run on `windows-latest`:

- **CI** (`.github/workflows/ci.yml`) — on pushes to `main` / `master`, pull requests and manual
  runs: builds with `cargo build --release --locked`, runs `cargo test --locked`, and runs
  `cargo clippy` as an advisory (non-blocking) check.
- **Release** (`.github/workflows/release.yml`) — triggered by pushing a `v*` tag (for example
  `git tag v0.2.0 && git push origin v0.2.0`) or manually via *Run workflow* with an existing
  tag. It verifies the tag matches the `version` in `Cargo.toml`, builds the binary, packs
  `frpc-tray.exe` with `README.md` and `LICENSE` into `frpc-tray-windows-x64.zip`, writes
  `SHA256SUMS.txt`, and publishes a GitHub Release with generated release notes. Tags containing
  a hyphen (`v0.3.0-beta.1`) are published as pre-releases.

### Publishing a release

The release workflow can only check out a ref that **already exists on GitHub**. A tag created
locally but never pushed — or a typo in the *Run workflow* input — makes `actions/checkout@v4`
fail with `A branch or tag with the name 'vX.Y.Z' could not be found`. Make sure the tag matches
the `version` in `Cargo.toml`, then create and push it:

```powershell
git tag -a v0.2.0 -m "Release v0.2.0"
git push origin main
git push origin v0.2.0
```

Pushing the tag triggers the release automatically. Alternatively, use *Run workflow* in the
Actions tab with a tag that is already on the remote; the workflow verifies that the tag exists
before checkout and fails with an actionable hint if it does not.

## License

[MIT](LICENSE)
