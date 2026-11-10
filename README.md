# frpc-tray

[![CI](https://github.com/luckzylp/frpc-tray/actions/workflows/ci.yml/badge.svg)](https://github.com/luckzylp/frpc-tray/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

A lightweight, cross-platform system-tray manager for [frpc](https://github.com/fatedier/frp).
It keeps the `frpc` client running in the background, lets you start/stop it and switch between
multiple configuration files from the tray menu, and downloads or updates the matching `frpc`
binary for your OS and CPU.

**Supported platforms:** Windows, Linux, macOS (x64 and ARM64 each).

## Features

- **Tray status icon** — different icons for *running*, *idle* (binary ready, stopped) and
  *no binary*. On macOS the app runs as an accessory (`LSUIElement`), so it does not occupy
  the Dock.
- **Start / stop frpc** from the tray menu; the process runs detached with its working
  directory set to the config folder.
- **Crash visibility** — if `frpc` exits on its own the icon returns to *idle* and the exit
  code is logged.
- **Multiple configurations** — every `*.toml` / `*.ini` file in `frp/config` shows up in the
  `配置` submenu. Switching while running restarts `frpc` with the new file.
- **Automatic frpc provisioning** — on first launch the correct official frp release asset is
  downloaded and extracted for you:

  | OS | Arch | frp release asset | Binary |
  | --- | --- | --- | --- |
  | Windows | x64 / arm64 | `frp_<ver>_windows_amd64.zip` / `_windows_arm64.zip` | `frpc.exe` |
  | Linux | x64 / arm64 / armv7 / loong64 / riscv64 | `frp_<ver>_linux_<arch>.tar.gz` | `frpc` |
  | macOS | x64 (Intel) / arm64 (Apple Silicon) | `frp_<ver>_darwin_<arch>.tar.gz` | `frpc` |

  Downloads try `reqwest` first, then `curl`, then PowerShell on Windows. Extracted Unix
  binaries are given executable permissions automatically.
- **Update check** — compares the installed frp version with the latest GitHub release at
  startup (silently) and on demand from the menu, then replaces the binary atomically with a
  `.bak` rollback.
- **Start on login** — per-user autostart, implemented natively per platform (see below).
- **Single instance** — launching a second copy simply exits.
- **Logging** — app events are appended to `frp/logs/frpc-tray.log`; `frpc`'s own output goes
  to `frp/logs/frpc.log`, which is truncated once it exceeds 2 MB.

> The tray menu is currently in Chinese only.

### Autostart per platform

| OS | Mechanism | Location |
| --- | --- | --- |
| Windows | `HKCU\...\CurrentVersion\Run` | registry value `frpc-tray` |
| Linux | XDG desktop entry | `~/.config/autostart/frpc-tray.desktop` (or `$XDG_CONFIG_HOME`) |
| macOS | launchd LaunchAgent | `~/Library/LaunchAgents/com.frpc-tray.plist` |

Autostart changes take effect at the next login; the current session is not relaunched.

## Requirements

- **Windows** 10/11, x64 (ARM64 also supported by the asset mapping)
- **Linux** x64/arm64 with a desktop session that provides a system tray. The default
  `tray-icon` backend needs GTK 3 and AppIndicator; on GNOME install the
  *AppIndicator and KStatusNotifierItem Support* extension.
- **macOS** 10.15+, Intel or Apple Silicon
- Rust 1.85 or newer (the crate uses the 2024 edition)
- Internet access on first run, so the matching `frpc` binary can be downloaded

## Build

Install platform build dependencies first:

```sh
# Debian / Ubuntu (Linux only)
sudo apt install -y pkg-config libgtk-3-dev libayatana-appindicator3-dev libxdo-dev
# Arch / Manjaro: sudo pacman -S gtk3 libappindicator-gtk3
```

Windows additionally needs the MSVC toolchain; `build.rs` embeds the icon and application
manifest (`app.rc`, `app.manifest`) via `embed-resource`. macOS needs Xcode command line tools.
No extra tooling is required on Windows or macOS.

```sh
git clone https://github.com/luckzylp/frpc-tray.git
cd frpc-tray
cargo build --release
```

Output: `target/release/frpc-tray.exe` (Windows) or `target/release/frpc-tray`
(Linux/macOS).

## Usage

1. Put the binary in a folder where it has write permission — or let the app fall back to the
   per-user data directory (see below).
2. Add one or more frp client configuration files to `frp/config` (for example `frpc.toml`).
   `frpc.toml` is used by default when present; otherwise the first file alphabetically.
3. Run the app and use the tray icon:
   - `启动 frpc` / `停止 frpc` — start or stop the client
   - `配置` — pick the active configuration file
   - `自动启动` — toggle start-on-login
   - `检查更新` — check for and install a newer frp release
   - `重新下载 frpc` — delete and re-fetch the binary
   - `退出` — stop `frpc` and exit

On Linux the tray icon appears in the notification area / panel; on macOS it appears in the
menu bar. macOS builds downloaded from GitHub are unsigned: if Gatekeeper blocks the `.app`,
right-click it and choose **Open**, or run
`xattr -d com.apple.quarantine frpc-tray.app`.

### Runtime file layout

Runtime files live in a **portable** location next to the executable whenever that directory
is writable (the original Windows behaviour). If the binary is installed into a read-only
location such as `/usr/bin`, `/Applications` or `C:\Program Files`, the app falls back to the
per-user data directory:

| OS | Fallback directory |
| --- | --- |
| Windows | `%LOCALAPPDATA%\frpc-tray\frp\` |
| Linux | `$XDG_DATA_HOME/frpc-tray/frp/` (default `~/.local/share/frpc-tray/frp/`) |
| macOS | `~/Library/Application Support/frpc-tray/frp/` |

macOS `.app` bundles always use Application Support, even when the bundle directory is
writable, so an app upgrade never discards your configs and the bundle stays intact.
Set `FRPC_TRAY_HOME` to an absolute path to override both rules (the `frp/` tree is created
inside it).

```text
<base>/
├── frpc-tray[.exe]
└── frp/
    ├── frpc[.exe]            # downloaded automatically
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

- **CI** (`.github/workflows/ci.yml`) — builds and tests on `windows-latest`,
  `ubuntu-latest` and `macos-latest` for pushes to `main` / `master`, pull requests and
  manual runs. Linux jobs install the GTK/AppIndicator development packages. `cargo clippy`
  runs as an advisory (non-blocking) check.
- **Release** (`.github/workflows/release.yml`) — triggered by pushing a `v*` tag (for example
  `git tag v0.2.0 && git push origin v0.2.0`) or manually via *Run workflow* with an existing
  tag. It verifies the tag matches the `version` in `Cargo.toml`, then builds a matrix of
  five artifacts and publishes one GitHub Release containing all of them plus a combined
  `SHA256SUMS.txt`:

  | Artifact | Runner |
  | --- | --- |
  | `frpc-tray-windows-x64.zip` | `windows-latest` |
  | `frpc-tray-linux-x64.tar.gz` | `ubuntu-latest` |
  | `frpc-tray-linux-arm64.tar.gz` | `ubuntu-24.04-arm` |
  | `frpc-tray-macos-x64.tar.gz` | `macos-15-intel` |
  | `frpc-tray-macos-arm64.tar.gz` | `macos-latest` |

  macOS archives contain `frpc-tray.app`, `README.md` and `LICENSE`. Tags containing a hyphen
  (`v0.3.0-beta.1`) are published as pre-releases. ARM runners are free for public
  repositories; for private repositories you may want to trim the matrix to save minutes.

### Publishing a release

The release workflow can only check out a ref that **already exists on GitHub**. A tag created
locally but never pushed — or a typo in the *Run workflow* input — makes `actions/checkout@v4`
fail with `A branch or tag with the name 'vX.Y.Z' could not be found`. Make sure the tag matches
the `version` in `Cargo.toml`, then create and push it:

```sh
git tag -a v0.2.0 -m "Release v0.2.0"
git push origin main
git push origin v0.2.0
```

Pushing the tag triggers the release automatically. Alternatively, use *Run workflow* in the
Actions tab with a tag that is already on the remote; the workflow verifies that the tag exists
before checkout and fails with an actionable hint if it does not.

## License

[MIT](LICENSE)
