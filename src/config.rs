use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

static BASE_DIR: OnceLock<PathBuf> = OnceLock::new();

/// Root folder for all runtime files (the `frp/` tree).
///
/// Portable mode is preferred: files live next to the executable when that
/// directory is writable, which keeps the original Windows behaviour and makes
/// local development easy. When the app is installed into a read-only location
/// (`/usr/bin`, `/Applications`, `C:\Program Files`) we fall back to the
/// per-user data directory. `FRPC_TRAY_HOME` overrides both.
pub fn base_dir() -> PathBuf {
    BASE_DIR.get_or_init(resolve_base_dir).clone()
}

fn resolve_base_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("FRPC_TRAY_HOME") {
        let dir = PathBuf::from(dir);
        let _ = fs::create_dir_all(&dir);
        return dir;
    }

    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()));
    if let Some(dir) = &exe_dir
        && is_writable(dir)
        && !is_app_bundle_dir(dir)
    {
        return dir.clone();
    }

    let dir = user_data_dir();
    let _ = fs::create_dir_all(&dir);
    dir
}

/// A macOS `.app` bundle is often user-writable, but runtime files must not
/// be written inside it (it breaks the bundle and loses state on upgrade).
#[cfg(target_os = "macos")]
fn is_app_bundle_dir(dir: &Path) -> bool {
    dir.to_string_lossy().contains(".app/Contents/")
}

#[cfg(not(target_os = "macos"))]
fn is_app_bundle_dir(_dir: &Path) -> bool {
    false
}

fn is_writable(dir: &Path) -> bool {
    let probe = dir.join(".frpc-tray-write-test");
    match fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&probe)
    {
        Ok(_) => {
            let _ = fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

#[cfg(windows)]
fn user_data_dir() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .or_else(|| std::env::var_os("APPDATA"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("frpc-tray")
}

#[cfg(target_os = "macos")]
fn user_data_dir() -> PathBuf {
    home_dir().join("Library/Application Support/frpc-tray")
}

#[cfg(target_os = "linux")]
fn user_data_dir() -> PathBuf {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home_dir().join(".local/share"))
        .join("frpc-tray")
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
fn user_data_dir() -> PathBuf {
    PathBuf::from(".")
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

pub fn app_dir() -> PathBuf {
    let dir = base_dir().join("frp");
    fs::create_dir_all(&dir).ok();
    dir
}

pub fn frpc_dir() -> PathBuf {
    app_dir()
}

pub fn configs_dir() -> PathBuf {
    let dir = app_dir().join("config");
    fs::create_dir_all(&dir).ok();
    dir
}

pub fn logs_dir() -> PathBuf {
    let dir = app_dir().join("logs");
    fs::create_dir_all(&dir).ok();
    dir
}

pub fn settings_path() -> PathBuf {
    app_dir().join("settings.json")
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Settings {
    pub frpc_version: Option<String>,
    pub last_config: Option<String>,
}

impl Settings {
    pub fn load() -> Self {
        let path = settings_path();
        match fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str(&text).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self) {
        let path = settings_path();
        if let Ok(text) = serde_json::to_string_pretty(self) {
            let _ = fs::write(path, text);
        }
    }
}

pub fn list_configs() -> Vec<String> {
    let dir = configs_dir();
    let mut names = Vec::new();
    if let Ok(entries) = fs::read_dir(&dir) {
        for entry in entries.flatten() {
            if let Ok(name) = entry.file_name().into_string() {
                let lower = name.to_lowercase();
                if lower.ends_with(".toml") || lower.ends_with(".ini") {
                    names.push(name);
                }
            }
        }
    }
    names.sort();
    names
}

pub fn default_config(configs: &[String]) -> Option<String> {
    if let Some(frpc_toml) = configs.iter().find(|c| c.as_str() == "frpc.toml") {
        return Some(frpc_toml.clone());
    }
    configs.first().cloned()
}
