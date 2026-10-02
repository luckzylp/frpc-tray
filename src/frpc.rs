use std::fs;
use std::io::{self, BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

use crate::config::frpc_dir;
use crate::logger;

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
    frpc_dir().join("frpc.exe")
}

fn download_url(version: &str) -> String {
    format!(
        "https://github.com/fatedier/frp/releases/download/v{}/frp_{}_windows_amd64.zip",
        version, version
    )
}

fn zip_entry_name(version: &str) -> String {
    format!("frp_{}_windows_amd64/frpc.exe", version)
}

pub fn ensure_frpc(version: &str) -> Result<PathBuf, String> {
    debug_log("ensure_frpc: start");
    let exe = frpc_exe_path();
    if exe.exists() {
        debug_log("ensure_frpc: exe already exists");
        return Ok(exe);
    }

    let zip_path = frpc_dir().join("frp_download.zip");

    debug_log(&format!("ensure_frpc: downloading v{}", version));
    download_internal(&download_url(version), &zip_path)?;
    debug_log("ensure_frpc: extracting");
    extract_frpc(&zip_path, &exe, &zip_entry_name(version))?;
    debug_log("ensure_frpc: removing zip");
    let _ = fs::remove_file(&zip_path);

    if !exe.exists() {
        return Err("解压后未找到 frpc.exe".into());
    }
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
    let parse = |s: &str| -> Vec<u64> {
        s.split('.')
            .filter_map(|p| p.parse::<u64>().ok())
            .collect()
    };
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
            Ok(())
        }
        Err(e) => {
            debug_log(&format!("download: reqwest failed ({}), trying curl", e));
            match download_curl(url, dest) {
                Ok(()) => {
                    debug_log("download: curl ok");
                    Ok(())
                }
                Err(e2) => {
                    debug_log(&format!("download: curl failed ({}), trying powershell", e2));
                    download_powershell(url, dest)
                }
            }
        }
    }
}

fn download_reqwest(url: &str, dest: &Path) -> Result<(), String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .connect_timeout(std::time::Duration::from_secs(10))
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
    fs::write(dest, &bytes).map_err(|e| format!("写入 zip 失败: {}", e))?;
    Ok(())
}

fn download_curl(url: &str, dest: &Path) -> Result<(), String> {
    let curl = which_curl();
    let mut cmd = std::process::Command::new(&curl);
    cmd.args(["-sSL", "--connect-timeout", "30", "--max-time", "300"])
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
        return Err(format!("curl 下载失败: exit={} stderr={}", output.status, stderr.trim()));
    }
    if !dest.exists() || dest.metadata().map(|m| m.len()).unwrap_or(0) < 100_000 {
        return Err("curl 下载结果过小或不存在".into());
    }
    Ok(())
}

fn which_curl() -> String {
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
        #[cfg(windows)]
        cmd.creation_flags(CREATE_NO_WINDOW);
        if cmd
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
        {
            return candidate.to_string();
        }
    }
    "curl".to_string()
}

fn download_powershell(url: &str, dest: &Path) -> Result<(), String> {
    let ps_cmd = format!(
        "Invoke-WebRequest -Uri '{}' -OutFile '{}' -UseBasicParsing",
        url,
        dest.display()
    );
    let mut ps = std::process::Command::new("powershell");
    ps.args(["-NoProfile", "-NonInteractive", "-Command", &ps_cmd]);
    #[cfg(windows)]
    ps.creation_flags(CREATE_NO_WINDOW);
    let output = ps
        .output()
        .map_err(|e| format!("powershell 启动失败: {}", e))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("powershell 下载失败: exit={} stderr={}", output.status, stderr.trim()));
    }
    if !dest.exists() || dest.metadata().map(|m| m.len()).unwrap_or(0) < 100_000 {
        return Err("powershell 下载结果过小或不存在".into());
    }
    Ok(())
}

pub fn extract_frpc(zip_path: &Path, dest: &Path, entry_name: &str) -> Result<(), String> {
    let file = fs::File::open(zip_path).map_err(|e| format!("打开 zip 失败: {}", e))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| format!("解析 zip 失败: {}", e))?;

    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| format!("读取 zip 条目失败: {}", e))?;
        if entry.name() == entry_name {
            let mut out =
                fs::File::create(dest).map_err(|e| format!("创建 frpc.exe 失败: {}", e))?;
            io::copy(&mut entry, &mut out).map_err(|e| format!("解压 frpc.exe 失败: {}", e))?;
            return Ok(());
        }
    }
    Err(format!("zip 中未找到 {}", entry_name))
}

pub fn start_frpc(exe: &Path, config_path: &Path) -> Result<Child, String> {
    let mut cmd = Command::new(exe);
    cmd.arg("-c")
        .arg(config_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("启动 frpc 失败: {}", e))?;

    logger::log(&format!("frpc 已启动 (pid={})", child.id()));

    if let Some(stdout) = child.stdout.take() {
        thread::spawn(move || {
            let reader = BufReader::new(stdout);
            for line in reader.lines() {
                if let Ok(line) = line {
                    logger::log(&line);
                }
            }
        });
    }
    if let Some(stderr) = child.stderr.take() {
        thread::spawn(move || {
            let reader = BufReader::new(stderr);
            for line in reader.lines() {
                if let Ok(line) = line {
                    logger::log(&line);
                }
            }
        });
    }

    Ok(child)
}

pub fn stop_frpc(child: &mut Child) {
    let pid = child.id();
    if child.kill().is_ok() {
        logger::log(&format!("frpc 已停止 (pid={})", pid));
    } else {
        let mut tk = Command::new("taskkill");
        tk.args(["/F", "/T", "/PID"]).arg(pid.to_string());
        #[cfg(windows)]
        tk.creation_flags(CREATE_NO_WINDOW);
        let _ = tk.output();
        logger::log(&format!("frpc 已停止 (taskkill pid={})", pid));
    }
    let _ = child.wait();
}
