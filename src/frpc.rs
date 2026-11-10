use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

use flate2::read::GzDecoder;

use crate::config::frpc_dir;
use crate::logger;
use crate::platform::{self, BINARY_NAME};

pub const FRP_VERSION: &str = "0.71.0";
const GITHUB_API_URL: &str = "https://api.github.com/repos/fatedier/frp/releases/latest";

fn debug_log(msg: &str) {
    use std::io::Write;
    let path = frpc_dir().join("debug.log");
    if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(f, "{}", msg);
    }
}

pub fn frpc_exe_path() -> PathBuf {
    frpc_dir().join(BINARY_NAME)
}

pub fn ensure_frpc(version: &str) -> Result<PathBuf, String> {
    debug_log("ensure_frpc: start");
    let exe = frpc_exe_path();
    if exe.exists() {
        debug_log("ensure_frpc: binary already exists");
        make_executable(&exe)?;
        return Ok(exe);
    }

    let archive_path = frpc_dir().join(platform::archive_file_name(version)?);
    let url = platform::download_url(version)?;

    debug_log(&format!("ensure_frpc: downloading v{} ({})", version, url));
    download_internal(&url, &archive_path)?;
    debug_log("ensure_frpc: extracting");
    extract_frpc(&archive_path, &exe, version)?;
    debug_log("ensure_frpc: removing archive");
    let _ = fs::remove_file(&archive_path);

    if !exe.exists() {
        return Err(format!("解压后未找到 {}", BINARY_NAME));
    }
    // frp's tar.gz preserves the executable bit, but a plain copy/extract on
    // Unix may not. Make sure the binary can actually be spawned.
    make_executable(&exe)?;
    debug_log("ensure_frpc: done");
    Ok(exe)
}

pub fn check_update(current_version: &str) -> Result<Option<String>, String> {
    debug_log(&format!("check_update: current={}", current_version));
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .connect_timeout(std::time::Duration::from_secs(10))
        .user_agent("frpc-tray")
        .build()
        .map_err(|e| format!("创建 HTTP 客户端失败: {}", e))?;

    let resp = client
        .get(GITHUB_API_URL)
        .header("User-Agent", "frpc-tray")
        .send()
        .map_err(|e| format!("GitHub API 请求失败: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("GitHub API 返回 HTTP {}", resp.status()));
    }

    let body: serde_json::Value = resp
        .json()
        .map_err(|e| format!("解析 GitHub API 响应失败: {}", e))?;

    let tag_name = body
        .get("tag_name")
        .and_then(|v| v.as_str())
        .ok_or("GitHub API 响应中无 tag_name")?;

    let latest = tag_name.strip_prefix('v').unwrap_or(tag_name);
    debug_log(&format!("check_update: latest={}", latest));

    if is_newer(latest, current_version) {
        Ok(Some(latest.to_string()))
    } else {
        Ok(None)
    }
}

fn is_newer(latest: &str, current: &str) -> bool {
    let parse =
        |s: &str| -> Vec<u64> { s.split('.').filter_map(|p| p.parse::<u64>().ok()).collect() };
    let l = parse(latest);
    let c = parse(current);
    for i in 0..l.len().max(c.len()) {
        let lv = l.get(i).copied().unwrap_or(0);
        let cv = c.get(i).copied().unwrap_or(0);
        if lv > cv {
            return true;
        }
        if lv < cv {
            return false;
        }
    }
    false
}

pub fn download_internal(url: &str, dest: &Path) -> Result<(), String> {
    debug_log(&format!("download: start url={}", url));
    match download_reqwest(url, dest) {
        Ok(()) => {
            debug_log("download: reqwest ok");
            return Ok(());
        }
        Err(e) => debug_log(&format!("download: reqwest failed ({}), trying curl", e)),
    }
    match download_curl(url, dest) {
        Ok(()) => {
            debug_log("download: curl ok");
            return Ok(());
        }
        Err(e) => debug_log(&format!("download: curl failed ({})", e)),
    }

    #[cfg(windows)]
    {
        debug_log("download: trying powershell");
        download_powershell(url, dest)
    }
    #[cfg(not(windows))]
    {
        Err("下载失败: reqwest 与 curl 均不可用，请检查网络或手动放置 frpc".into())
    }
}

fn download_reqwest(url: &str, dest: &Path) -> Result<(), String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .connect_timeout(std::time::Duration::from_secs(10))
        .user_agent("frpc-tray")
        .build()
        .map_err(|e| format!("创建 HTTP 客户端失败: {}", e))?;
    let resp = client
        .get(url)
        .send()
        .map_err(|e| format!("下载失败: {}", e))?;
    if !resp.status().is_success() {
        return Err(format!("下载失败: HTTP {}", resp.status()));
    }
    let bytes = resp
        .bytes()
        .map_err(|e| format!("读取下载内容失败: {}", e))?;
    fs::write(dest, &bytes).map_err(|e| format!("写入下载文件失败: {}", e))?;
    Ok(())
}

fn download_curl(url: &str, dest: &Path) -> Result<(), String> {
    let curl = curl_command();
    let mut cmd = std::process::Command::new(&curl);
    cmd.args(["-fsSL", "--connect-timeout", "30", "--max-time", "300"])
        .arg("-o")
        .arg(dest)
        .arg(url);
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);
    let output = cmd
        .output()
        .map_err(|e| format!("curl 启动失败 ({}): {}", curl, e))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "curl 下载失败: exit={} stderr={}",
            output.status,
            stderr.trim()
        ));
    }
    if !dest.exists() || dest.metadata().map(|m| m.len()).unwrap_or(0) < 100_000 {
        return Err("curl 下载结果过小或不存在".into());
    }
    Ok(())
}

#[cfg(windows)]
fn curl_command() -> String {
    let candidates = [
        "C:\\Program Files\\Git\\mingw64\\bin\\curl.exe",
        "C:\\Program Files (x86)\\Git\\mingw64\\bin\\curl.exe",
        "curl",
        "curl.exe",
        "C:\\Windows\\System32\\curl.exe",
    ];
    for candidate in &candidates {
        let mut cmd = std::process::Command::new(candidate);
        cmd.arg("--version");
        cmd.creation_flags(CREATE_NO_WINDOW);
        if cmd.output().map(|o| o.status.success()).unwrap_or(false) {
            return candidate.to_string();
        }
    }
    "curl".to_string()
}

#[cfg(not(windows))]
fn curl_command() -> String {
    // macOS and most Linux distributions ship curl in PATH.
    "curl".to_string()
}

#[cfg(windows)]
fn download_powershell(url: &str, dest: &Path) -> Result<(), String> {
    let ps_cmd = format!(
        "Invoke-WebRequest -Uri '{}' -OutFile '{}' -UseBasicParsing",
        url,
        dest.display()
    );
    let mut ps = std::process::Command::new("powershell");
    ps.args(["-NoProfile", "-NonInteractive", "-Command", &ps_cmd]);
    ps.creation_flags(CREATE_NO_WINDOW);
    let output = ps
        .output()
        .map_err(|e| format!("powershell 启动失败: {}", e))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "powershell 下载失败: exit={} stderr={}",
            output.status,
            stderr.trim()
        ));
    }
    if !dest.exists() || dest.metadata().map(|m| m.len()).unwrap_or(0) < 100_000 {
        return Err("powershell 下载结果过小或不存在".into());
    }
    Ok(())
}

/// Extract the frpc binary from either a `.zip` (Windows) or a `.tar.gz`
/// (Linux/macOS) frp release archive.
pub fn extract_frpc(archive_path: &Path, dest: &Path, version: &str) -> Result<(), String> {
    let expected = platform::archive_entry(version)?;
    if archive_path.extension().and_then(|s| s.to_str()) == Some("zip") {
        extract_zip(archive_path, dest, &expected)
    } else {
        extract_targz(archive_path, dest, &expected)
    }
}

fn extract_zip(archive_path: &Path, dest: &Path, expected: &str) -> Result<(), String> {
    let file = fs::File::open(archive_path).map_err(|e| format!("打开 zip 失败: {}", e))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| format!("解析 zip 失败: {}", e))?;

    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| format!("读取 zip 条目失败: {}", e))?;
        let name = entry.name().replace('\\', "/");
        if !platform::is_frpc_entry(&name, expected) {
            continue;
        }
        let mut out =
            fs::File::create(dest).map_err(|e| format!("创建 {} 失败: {}", BINARY_NAME, e))?;
        io::copy(&mut entry, &mut out).map_err(|e| format!("解压 {} 失败: {}", BINARY_NAME, e))?;
        drop(out);
        make_executable(dest)?;
        return Ok(());
    }
    Err(format!("zip 中未找到 {}", expected))
}

fn extract_targz(archive_path: &Path, dest: &Path, expected: &str) -> Result<(), String> {
    let file = fs::File::open(archive_path).map_err(|e| format!("打开 tar.gz 失败: {}", e))?;
    let mut archive = tar::Archive::new(GzDecoder::new(file));
    let entries = archive
        .entries()
        .map_err(|e| format!("解析 tar.gz 失败: {}", e))?;

    for entry in entries {
        let mut entry = entry.map_err(|e| format!("读取 tar.gz 条目失败: {}", e))?;
        if !entry.header().entry_type().is_file() {
            continue;
        }
        let name = entry
            .path()
            .map_err(|e| format!("读取 tar.gz 条目名失败: {}", e))?
            .to_string_lossy()
            .replace('\\', "/");
        if !platform::is_frpc_entry(&name, expected) {
            continue;
        }
        let mut out =
            fs::File::create(dest).map_err(|e| format!("创建 {} 失败: {}", BINARY_NAME, e))?;
        io::copy(&mut entry, &mut out).map_err(|e| format!("解压 {} 失败: {}", BINARY_NAME, e))?;
        drop(out);
        make_executable(dest)?;
        return Ok(());
    }
    Err(format!("tar.gz 中未找到 {}", expected))
}

#[cfg(unix)]
fn make_executable(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = fs::metadata(path)
        .map_err(|e| format!("读取 {} 权限失败: {}", path.display(), e))?
        .permissions();
    let mode = perms.mode() | 0o755;
    if mode != perms.mode() {
        perms.set_mode(mode);
        fs::set_permissions(path, perms)
            .map_err(|e| format!("设置 {} 可执行权限失败: {}", path.display(), e))?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) -> Result<(), String> {
    Ok(())
}

const FRPC_LOG_MAX_BYTES: u64 = 2 * 1024 * 1024;

fn open_log(path: &Path) -> io::Result<fs::File> {
    let oversized = path
        .metadata()
        .map(|m| m.len() > FRPC_LOG_MAX_BYTES)
        .unwrap_or(false);
    if oversized {
        let _ = fs::remove_file(path);
    }
    fs::OpenOptions::new().create(true).append(true).open(path)
}

pub fn start_frpc(exe: &Path, config_path: &Path) -> Result<Child, String> {
    let mut cmd = Command::new(exe);
    cmd.arg("-c").arg(config_path);

    // Relative paths in the config resolve against the working directory.
    if let Some(dir) = config_path.parent().filter(|d| !d.as_os_str().is_empty()) {
        cmd.current_dir(dir);
    }

    // frpc writes its output straight to a dedicated log file instead of a
    // pipe, so no reader thread has to be drained to keep frpc unblocked.
    let log_path = crate::config::logs_dir().join("frpc.log");
    match open_log(&log_path) {
        Ok(file) => {
            let stderr_file = file
                .try_clone()
                .map_err(|e| format!("复制日志文件句柄失败: {}", e))?;
            cmd.stdout(Stdio::from(file));
            cmd.stderr(Stdio::from(stderr_file));
        }
        Err(e) => {
            logger::log(&format!(
                "无法打开 frpc 日志文件 ({}): {}",
                log_path.display(),
                e
            ));
            cmd.stdout(Stdio::null());
            cmd.stderr(Stdio::null());
        }
    }

    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);

    let child = cmd.spawn().map_err(|e| format!("启动 frpc 失败: {}", e))?;

    logger::log(&format!("frpc 已启动 (pid={})", child.id()));

    Ok(child)
}

pub fn stop_frpc(child: &mut Child) {
    let pid = child.id();
    if child.kill().is_ok() {
        logger::log(&format!("frpc 已停止 (pid={})", pid));
    } else {
        #[cfg(windows)]
        {
            let mut tk = Command::new("taskkill");
            tk.args(["/F", "/T", "/PID"]).arg(pid.to_string());
            tk.creation_flags(CREATE_NO_WINDOW);
            let _ = tk.output();
            logger::log(&format!("frpc 已停止 (taskkill pid={})", pid));
        }
        #[cfg(not(windows))]
        {
            let mut kill = Command::new("kill");
            kill.args(["-KILL"]).arg(pid.to_string());
            let _ = kill.output();
            logger::log(&format!("frpc 已停止 (kill pid={})", pid));
        }
    }
    let _ = child.wait();
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Verify the tar.gz path end to end with a synthetic frp release archive.
    /// This runs on every platform, so the Linux/macOS extraction code is
    /// exercised even when developing on Windows.
    #[test]
    fn extracts_frpc_from_targz() {
        let dir = std::env::temp_dir().join(format!("frpc-tray-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let archive = dir.join("test.tar.gz");
        let dest = dir.join(BINARY_NAME);
        let payload = b"fake frpc binary";
        let entry = format!("frp_9.9.9_some_os_arch/{}", BINARY_NAME);

        let file = fs::File::create(&archive).unwrap();
        let encoder = flate2::write::GzEncoder::new(file, flate2::Compression::default());
        let mut builder = tar::Builder::new(encoder);
        let mut header = tar::Header::new_gnu();
        header.set_size(payload.len() as u64);
        header.set_mode(0o755);
        header.set_cksum();
        builder
            .append_data(&mut header, &entry, &payload[..])
            .unwrap();
        builder.into_inner().unwrap().finish().unwrap();

        extract_targz(&archive, &dest, &entry).unwrap();
        assert_eq!(fs::read(&dest).unwrap(), payload);

        let _ = fs::remove_dir_all(&dir);
    }
}
