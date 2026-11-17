//! Describes the frp release artifact that matches the OS / CPU this binary
//! was built for. Windows ships a `.zip` with `frpc.exe`; Linux and macOS ship
//! a `.tar.gz` with an extension-less `frpc` binary.

use std::path::Path;

/// `frpc.exe` on Windows, `frpc` everywhere else.
#[cfg(windows)]
pub const BINARY_NAME: &str = "frpc.exe";
#[cfg(not(windows))]
pub const BINARY_NAME: &str = "frpc";

/// frp publishes Windows builds as `.zip`, all other platforms as `.tar.gz`.
#[cfg(windows)]
pub const ARCHIVE_EXT: &str = "zip";
#[cfg(not(windows))]
pub const ARCHIVE_EXT: &str = "tar.gz";

/// The `<os>_<arch>` suffix used by fatedier/frp release assets.
fn asset_target() -> Result<&'static str, String> {
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;
    let target = match (os, arch) {
        ("windows", "x86_64") => "windows_amd64",
        ("windows", "aarch64") => "windows_arm64",
        ("linux", "x86_64") => "linux_amd64",
        ("linux", "aarch64") => "linux_arm64",
        ("linux", "arm") => "linux_arm",
        ("linux", "loongarch64") => "linux_loong64",
        ("linux", "riscv64") => "linux_riscv64",
        ("macos", "x86_64") => "darwin_amd64",
        ("macos", "aarch64") => "darwin_arm64",
        _ => {
            return Err(format!(
                "平台 {}-{} 暂无对应的 frp 预编译包，请手动把 {} 放到 frp/ 目录",
                os, arch, BINARY_NAME
            ));
        }
    };
    Ok(target)
}

/// `frp_<version>_<os>_<arch>`
pub fn archive_stem(version: &str) -> Result<String, String> {
    Ok(format!("frp_{}_{}", version, asset_target()?))
}

/// `frp_<version>_<os>_<arch>.zip` / `.tar.gz`
pub fn archive_file_name(version: &str) -> Result<String, String> {
    Ok(format!("{}.{}", archive_stem(version)?, ARCHIVE_EXT))
}

pub fn download_url(version: &str) -> Result<String, String> {
    Ok(format!(
        "https://github.com/fatedier/frp/releases/download/v{}/{}",
        version,
        archive_file_name(version)?
    ))
}

/// Expected path of the frpc binary inside the release archive.
pub fn archive_entry(version: &str) -> Result<String, String> {
    Ok(format!("{}/{}", archive_stem(version)?, BINARY_NAME))
}

/// Accept the exact expected entry, plus a tolerant `*/frpc[.exe]` fallback in
/// case frp changes its top-level directory name in a future release.
pub fn is_frpc_entry(name: &str, expected: &str) -> bool {
    name == expected || name.ends_with(&format!("/{}", BINARY_NAME))
}

/// Open a file or folder with the platform's default handler, so the user can
/// edit `stun/sources.txt` without the app needing a text editor of its own.
pub fn open_path(path: &Path) -> Result<(), String> {
    let program = if cfg!(windows) {
        "explorer"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };

    let mut cmd = std::process::Command::new(program);
    cmd.arg(path);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    cmd.spawn()
        .map(|_| ())
        .map_err(|e| format!("无法启动 {}: {}", program, e))
}

/// Ask for one line of text with a native dialog.
///
/// The app has no window of its own, so the text input is delegated to a small
/// platform helper: PowerShell's InputBox on Windows, `osascript` on macOS and
/// `zenity` / `kdialog` on Linux. Returns `Ok(None)` when the user cancels; an
/// `Err` means no usable helper exists, in which case the caller falls back to
/// editing the file by hand.
pub fn prompt_text(title: &str, message: &str, default: &str) -> Result<Option<String>, String> {
    let title = one_line(title);
    let message = one_line(message);
    let default = one_line(default);

    #[cfg(windows)]
    {
        prompt_windows(&title, &message, &default)
    }
    #[cfg(target_os = "macos")]
    {
        prompt_macos(&title, &message, &default)
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        prompt_linux(&title, &message, &default)
    }
    #[cfg(not(any(windows, unix)))]
    {
        let _ = (title, message, default);
        Err("当前平台没有可用的输入对话框".to_string())
    }
}

#[allow(dead_code)]
fn one_line(text: &str) -> String {
    text.replace(['\r', '\n'], " ")
}

/// PowerShell script that builds the dialog form. Everything up to (but not
/// including) `ShowDialog()`; the caller appends the ending, which lets the
/// smoke test drive the form automatically.
#[cfg(windows)]
fn windows_prompt_script(title: &str, message: &str, default: &str) -> String {
    // Single quotes are escaped by doubling them in PowerShell literals.
    let quote = |s: &str| s.replace('\'', "''");
    // A WinForms form instead of VisualBasic's InputBox: it can be marked
    // TopMost, so the dialog is not lost behind other windows when the tray app
    // itself has no foreground window.
    format!(
        "[Console]::OutputEncoding=[Text.Encoding]::UTF8; \
         Add-Type -AssemblyName System.Windows.Forms; \
         $f = New-Object System.Windows.Forms.Form; \
         $f.Text = '{title}'; $f.TopMost = $true; $f.StartPosition = 'CenterScreen'; \
         $f.FormBorderStyle = 'FixedDialog'; $f.MaximizeBox = $false; $f.MinimizeBox = $false; \
         $f.Width = 540; $f.Height = 190; \
         $l = New-Object System.Windows.Forms.Label; $l.Text = '{message}'; \
         $l.Left = 16; $l.Top = 16; $l.MaximumSize = New-Object System.Drawing.Size(500, 0); \
         $l.AutoSize = $true; [void]$f.Controls.Add($l); \
         $b = New-Object System.Windows.Forms.TextBox; $b.Text = '{default}'; \
         $b.Left = 16; $b.Top = 62; $b.Width = 495; $b.SelectAll(); \
         [void]$f.Controls.Add($b); \
         $ok = New-Object System.Windows.Forms.Button; $ok.Text = 'OK'; \
         $ok.Left = 340; $ok.Top = 100; $ok.Width = 80; $ok.DialogResult = 'OK'; \
         [void]$f.Controls.Add($ok); \
         $no = New-Object System.Windows.Forms.Button; $no.Text = '取消'; \
         $no.Left = 430; $no.Top = 100; $no.Width = 80; $no.DialogResult = 'Cancel'; \
         [void]$f.Controls.Add($no); \
         $f.AcceptButton = $ok; $f.CancelButton = $no; ",
        title = quote(title),
        message = quote(message),
        default = quote(default)
    )
}

/// Ending of the prompt script: print the text box on OK, nothing on cancel.
#[cfg(windows)]
const PROMPT_SCRIPT_END: &str = "if ($f.ShowDialog() -eq 'OK') { $b.Text } else { '' }";

#[cfg(windows)]
fn prompt_windows(title: &str, message: &str, default: &str) -> Result<Option<String>, String> {
    use std::os::windows::process::CommandExt;

    let script = format!(
        "{}{}",
        windows_prompt_script(title, message, default),
        PROMPT_SCRIPT_END
    );

    let mut cmd = std::process::Command::new("powershell");
    // -STA: WinForms needs a single threaded apartment.
    cmd.args(["-NoProfile", "-STA", "-Command", &script]);
    cmd.creation_flags(0x0800_0000);
    let output = cmd
        .output()
        .map_err(|e| format!("无法启动 powershell: {}", e))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("输入对话框失败: {}", stderr.trim()));
    }
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok(if text.is_empty() { None } else { Some(text) })
}

#[cfg(target_os = "macos")]
fn prompt_macos(title: &str, message: &str, default: &str) -> Result<Option<String>, String> {
    let quote = |s: &str| s.replace('\\', "\\\\").replace('"', "\\\"");
    let script = format!(
        "text returned of (display dialog \"{}\" with title \"{}\" default answer \"{}\")",
        quote(message),
        quote(title),
        quote(default)
    );
    let output = std::process::Command::new("osascript")
        .args(["-e", &script])
        .output()
        .map_err(|e| format!("无法启动 osascript: {}", e))?;
    if !output.status.success() {
        // AppleScript reports a cancel as "User canceled. (-128)".
        return Ok(None);
    }
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok(if text.is_empty() { None } else { Some(text) })
}

#[cfg(all(unix, not(target_os = "macos")))]
fn prompt_linux(title: &str, message: &str, default: &str) -> Result<Option<String>, String> {
    let read = |out: std::process::Output| -> Option<String> {
        let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if text.is_empty() { None } else { Some(text) }
    };

    match std::process::Command::new("zenity")
        .args(["--entry", "--title", title, "--text", message])
        .arg(format!("--entry-text={}", default))
        .output()
    {
        Ok(out) if out.status.success() => return Ok(read(out)),
        Ok(_) => return Ok(None),
        Err(_) => {}
    }

    match std::process::Command::new("kdialog")
        .args(["--title", title, "--inputbox", message, default])
        .output()
    {
        Ok(out) if out.status.success() => Ok(read(out)),
        Ok(_) => Ok(None),
        Err(_) => {
            Err("未找到 zenity 或 kdialog，请安装其中一个，或直接编辑 stun/servers.txt".to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asset_names_are_platform_specific() {
        let stem = archive_stem("0.71.0").unwrap();
        assert!(stem.starts_with("frp_0.71.0_"));
        assert_eq!(
            download_url("0.71.0").unwrap(),
            format!(
                "https://github.com/fatedier/frp/releases/download/v0.71.0/{}.{}",
                stem, ARCHIVE_EXT
            )
        );
        assert_eq!(
            archive_entry("0.71.0").unwrap(),
            format!("{}/{}", stem, BINARY_NAME)
        );
    }

    #[test]
    fn entry_matching_is_tolerant() {
        assert!(is_frpc_entry(
            &format!("a/b/{}", BINARY_NAME),
            "does/not/match"
        ));
        assert!(!is_frpc_entry("a/b/frps", "does/not/match"));
    }

    /// Drives the real prompt dialog automatically (it flashes for a moment and
    /// closes itself), verifying the script and the stdout round trip:
    /// `cargo test -- --ignored --nocapture prompt_dialog`
    #[cfg(windows)]
    #[test]
    #[ignore]
    fn prompt_dialog_round_trip() {
        // A single quote in the message checks the PowerShell escaping.
        let script = format!(
            "{} $f.Add_Shown({{ $b.Text = 'stun.auto:3478'; $f.DialogResult = 'OK'; $f.Close() }}); {}",
            windows_prompt_script("标题 '测试'", "输入地址", "默认值"),
            PROMPT_SCRIPT_END
        );
        let output = std::process::Command::new("powershell")
            .args(["-NoProfile", "-STA", "-Command", &script])
            .output()
            .expect("spawn powershell");
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        println!(
            "status={} stdout={:?} stderr={:?}",
            output.status,
            stdout,
            String::from_utf8_lossy(&output.stderr).trim()
        );
        assert!(output.status.success());
        assert_eq!(stdout, "stun.auto:3478");
    }
}
