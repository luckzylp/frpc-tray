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
- **STUN server picker** — probes a live list of public STUN servers (by default from
  [always-online-stun](https://github.com/pradt2/always-online-stun)) with real RFC 5389
  binding requests, shows the latency / quality of the fastest ones, and writes the server you
  pick into the active `frpc` config (`natHoleStunServer` / `nat_hole_stun_server`). Sources are
  user editable.
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
   - `STUN 服务器` — probe public STUN servers, see their quality, and apply the chosen one
     to the active config (see below)
   - `自动启动` — toggle start-on-login
   - `检查更新` — check for and install a newer frp release
   - `重新下载 frpc` — delete and re-fetch the binary
   - `退出` — stop `frpc` and exit

On Linux the tray icon appears in the notification area / panel; on macOS it appears in the
menu bar. macOS builds downloaded from GitHub are unsigned: if Gatekeeper blocks the `.app`,
right-click it and choose **Open**, or run
`xattr -d com.apple.quarantine frpc-tray.app`.

### STUN servers

frp uses a STUN server to learn its public address before attempting NAT hole punching
(`natHoleStunServer` in `frpc.toml`, `[common] nat_hole_stun_server` in the legacy
`frpc.ini`). Public STUN servers vary a lot in reachability and latency from a given network,
so the `STUN 服务器` submenu lets you measure and choose one:

| Menu item | What it does |
| --- | --- |
| `当前: <addr> · 38ms 优 · 出口 1.2.3.4` | Value of the active config plus its measured quality and the public IP the server sees |
| `候选 N 个 · 可用 M 个 · 刚刚测速` | Size of the probed list and how old the result is |
| Server list | Fastest servers, `host:port  <latency>  <grade>`. Clicking one writes it into the active config and restarts `frpc` if it is running. `★` marks your own nodes; they are always shown, even when slow or unreachable |
| `测速并刷新列表` | Re-download every source and probe all candidates again |
| `添加自定义节点…` | Native input dialog: type a `host:port` (a bare host name gets `:3478`). The node is appended to `servers.txt` and probed immediately |
| `添加自定义源…` | Native input dialog for a list URL, a local list file or a single address; appended to `sources.txt` and followed by a full refresh |
| `编辑节点列表…` / `编辑自定义源…` | Open `servers.txt` / `sources.txt` in the default editor (fallback when no dialog helper is available) |
| `打开 STUN 目录` | Open `frp/stun` (source files, node list and result cache) |
| `恢复默认（注释 STUN 项）` | Comment the entry out so frp falls back to its built-in default |

The two `添加…` entries use a small native dialog: a TopMost WinForms form driven by PowerShell on
Windows, `osascript` on macOS and `zenity` (or `kdialog`) on Linux. If none of those is usable,
the app logs it and you can still edit the files directly.

Quality is measured with up to three RFC 5389 binding requests per server (the first packet of
a cold path is allowed to time out, later timeouts are reported as packet loss):

| Grade | Average latency |
| --- | --- |
| 优 | ≤ 100 ms |
| 良 | ≤ 250 ms |
| 一般 | ≤ 500 ms |
| 差 | > 500 ms |
| 不可用 | no valid response |

Results are cached in `frp/stun/cache.json`, so the menu is populated instantly on start and a
fresh probe only runs when the cache is older than six hours (or from the menu). Probing never
blocks the UI: the work happens on a background thread pool (32 workers).

#### How the value is written

Many configs keep a commented list of candidate servers next to the one in use:

```toml
# natHoleStunServer = "stun.hitv.com:3478"
# natHoleStunServer = "stun.cheapvoip.com:3478"
natHoleStunServer = "stun.miwifi.com:3478"
```

Picking a server always reuses the line that is **in effect** (here the `stun.miwifi.com` one) and
leaves the commented candidates untouched. Filling in a commented line while another one is
active would define the key twice, which frp rejects with
`toml: line N: key natHoleStunServer is already defined`.

If a config already contains several active copies (hand edited, or written by an older build),
the app comments the extra ones out before handing the file to `frpc`, and logs
`… 中有 N 处重复的 STUN 项，已注释多余行`. `恢复默认` comments out every active copy.

#### Custom nodes and sources

Two separate files, both created on first run:

**`frp/stun/servers.txt`** — your own STUN nodes, one `host:port` per line, `#` starts a comment.
These are probed first and are always listed in the menu with a `★` marker:

```text
stun.myserver.com:3478
192.168.1.10:3478        # 内网测试服务器
```

**`frp/stun/sources.txt`** — where candidate lists come from; a file can mix three kinds of lines:

```text
# 1) remote list — any URL whose body contains host:port entries
https://raw.githubusercontent.com/pradt2/always-online-stun/master/valid_hosts.txt

# 2) local file — absolute path to a list of host:port entries
/data/stun-servers.txt

# 3) direct entries (same effect as a line in servers.txt)
stun.backup.example.com:3478
```

Remote lists may separate entries with spaces, commas or semicolons and may carry trailing
comments. Duplicates are removed case-insensitively, across nodes and sources. If no source can be
reached and there is no cache yet, your custom nodes plus a small built-in list keep the menu
working offline.

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
    ├── stun/
    │   ├── sources.txt       # STUN server sources (editable, or via the menu)
    │   ├── servers.txt       # your own STUN nodes (★ in the menu)
    │   └── cache.json        # last probe results, reused on the next start
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
