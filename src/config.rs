use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

pub fn base_dir() -> PathBuf {
    let exe = std::env::current_exe().expect("cannot get current_exe");
    let dir = exe.parent().expect("exe has no parent").to_path_buf();
    dir
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
