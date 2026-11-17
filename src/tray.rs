use std::collections::HashSet;
use std::process::Child;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use tray_icon::Icon;
use tray_icon::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};

use crate::config::{Settings, list_configs};
use crate::frpconf;
use crate::stun::{self, ProbeResult};

pub const ID_START: &str = "start";
pub const ID_STOP: &str = "stop";
pub const ID_AUTOSTART: &str = "autostart";
pub const ID_CHECK_UPDATE: &str = "check_update";
pub const ID_REDOWNLOAD: &str = "redownload";
pub const ID_QUIT: &str = "quit";
pub const ID_CONFIG_PREFIX: &str = "cfg:";
pub const ID_STUN_REFRESH: &str = "stun_refresh";
pub const ID_STUN_RESTORE: &str = "stun_restore";
pub const ID_STUN_ADD_NODE: &str = "stun_add_node";
pub const ID_STUN_ADD_SOURCE: &str = "stun_add_source";
pub const ID_STUN_EDIT_NODES: &str = "stun_edit_nodes";
pub const ID_STUN_EDIT_SOURCES: &str = "stun_edit_sources";
pub const ID_STUN_OPEN_DIR: &str = "stun_open_dir";
pub const ID_STUN_PREFIX: &str = "stun:";

/// How many probed servers are listed in the menu; the rest stays in the cache.
const STUN_LIST_LIMIT: usize = 12;

pub struct AppState {
    pub settings: Settings,
    pub child: Option<Child>,
    pub frpc_path: Option<std::path::PathBuf>,
    pub current_config: Option<String>,
    pub stun: stun::StunState,
}

impl AppState {
    pub fn new(settings: Settings, frpc_path: Option<std::path::PathBuf>) -> Self {
        Self {
            settings,
            child: None,
            frpc_path,
            current_config: None,
            stun: stun::StunState::default(),
        }
    }

    pub fn is_running(&self) -> bool {
        self.child.is_some()
    }

    /// The config file in effect: the user's choice, or the default one.
    pub fn resolved_config(&self) -> Option<String> {
        self.current_config
            .clone()
            .or_else(|| crate::config::default_config(&list_configs()))
    }
}

pub type SharedState = Arc<Mutex<AppState>>;

const TRAY_IDLE: &[u8] = include_bytes!("assets/tray_idle.rgba");
const TRAY_RUNNING: &[u8] = include_bytes!("assets/tray_running.rgba");
const TRAY_STOPPED: &[u8] = include_bytes!("assets/tray_stopped.rgba");
const TRAY_SIZE: u32 = 64;

fn icon_from_bytes(rgba: &'static [u8]) -> Icon {
    Icon::from_rgba(rgba.to_vec(), TRAY_SIZE, TRAY_SIZE).expect("invalid embedded tray icon")
}

pub fn icon_for_state(state: &AppState) -> Icon {
    if state.is_running() {
        icon_from_bytes(TRAY_RUNNING)
    } else if state.frpc_path.is_some() {
        icon_from_bytes(TRAY_IDLE)
    } else {
        icon_from_bytes(TRAY_STOPPED)
    }
}

pub fn build_menu(state: &AppState) -> Menu {
    let menu = Menu::new();

    let running = state.is_running();
    let start = MenuItem::with_id(ID_START, "启动 frpc", !running, None);
    let stop = MenuItem::with_id(ID_STOP, "停止 frpc", running, None);

    let _ = menu.append(&start);
    let _ = menu.append(&stop);
    let _ = menu.append(&PredefinedMenuItem::separator());

    let configs = list_configs();
    let current = state.current_config.as_deref();
    let mut cfg_items: Vec<Box<dyn tray_icon::menu::IsMenuItem>> = Vec::new();
    if configs.is_empty() {
        cfg_items.push(Box::new(MenuItem::with_id(
            "no_config",
            "(无配置文件)",
            true,
            None,
        )));
    } else {
        for name in &configs {
            let id = format!("{}{}", ID_CONFIG_PREFIX, name);
            let checked = current.is_some_and(|c| c == name.as_str());
            cfg_items.push(Box::new(CheckMenuItem::with_id(
                &id, name, true, checked, None,
            )));
        }
    }
    let cfg_refs: Vec<&dyn tray_icon::menu::IsMenuItem> =
        cfg_items.iter().map(|b| b.as_ref()).collect();
    let submenu =
        Submenu::with_items("配置", true, &cfg_refs).unwrap_or_else(|_| Submenu::new("配置", true));
    let _ = menu.append(&submenu);

    let _ = menu.append(&build_stun_submenu(state));

    let _ = menu.append(&PredefinedMenuItem::separator());

    let autostart = CheckMenuItem::with_id(
        ID_AUTOSTART,
        "自动启动",
        true,
        crate::autostart::is_autostart(),
        None,
    );
    let _ = menu.append(&autostart);

    let _ = menu.append(&PredefinedMenuItem::separator());

    let check_update = MenuItem::with_id(ID_CHECK_UPDATE, "检查更新", true, None);
    let _ = menu.append(&check_update);

    let redownload = MenuItem::with_id(ID_REDOWNLOAD, "重新下载 frpc", true, None);
    let _ = menu.append(&redownload);

    let _ = menu.append(&PredefinedMenuItem::separator());

    let quit = MenuItem::with_id(ID_QUIT, "退出", true, None);
    let _ = menu.append(&quit);

    menu
}

/// `STUN 服务器` submenu: current value of the active config, the fastest
/// probed servers with their quality, a manual refresh, and the entry points to
/// customise the source list.
fn build_stun_submenu(state: &AppState) -> Submenu {
    let current = state
        .resolved_config()
        .map(|name| crate::config::configs_dir().join(name))
        .and_then(|path| frpconf::read_stun_server(&path));

    let mut items: Vec<Box<dyn tray_icon::menu::IsMenuItem>> = Vec::new();

    items.push(Box::new(MenuItem::with_id(
        "stun_status",
        current_status_label(&current, &state.stun.results),
        false,
        None,
    )));

    let usable = state.stun.results.iter().filter(|r| r.is_usable()).count();
    if state.stun.probing {
        items.push(Box::new(MenuItem::with_id(
            "stun_progress",
            "正在测速，请稍候…",
            false,
            None,
        )));
    } else if !state.stun.results.is_empty() {
        items.push(Box::new(MenuItem::with_id(
            "stun_summary",
            format!(
                "候选 {} 个 · 可用 {} 个 · {}",
                state.stun.results.len(),
                usable,
                ago_text(state.stun.updated_at)
            ),
            false,
            None,
        )));
    }
    if let Some(note) = state.stun.notes.first() {
        items.push(Box::new(MenuItem::with_id(
            "stun_note",
            truncate(note, 46),
            false,
            None,
        )));
    }

    items.push(Box::new(PredefinedMenuItem::separator()));

    // Custom nodes are always listed, the rest of the slots go to the fastest
    // public servers.
    let custom_count = state.stun.results.iter().filter(|r| r.custom).count();
    let mut public_budget = STUN_LIST_LIMIT.saturating_sub(custom_count);
    let mut shown: Vec<&ProbeResult> = Vec::new();
    for result in &state.stun.results {
        if result.custom {
            shown.push(result);
        } else if public_budget > 0 && result.is_usable() {
            shown.push(result);
            public_budget -= 1;
        }
    }
    let width = shown
        .iter()
        .map(|r| server_head(r).chars().count())
        .max()
        .unwrap_or(20)
        .max(24);

    let mut listed: HashSet<&str> = HashSet::new();
    for result in &shown {
        listed.insert(result.addr.as_str());
        let checked = current.as_deref() == Some(result.addr.as_str());
        items.push(Box::new(CheckMenuItem::with_id(
            format!("{}{}", ID_STUN_PREFIX, result.addr),
            server_label(result, width),
            true,
            checked,
            None,
        )));
    }

    // Keep a manually configured server visible even when it is not among the
    // fastest ones, otherwise the menu would hide the active choice.
    if let Some(addr) = current.as_deref()
        && !listed.contains(addr)
    {
        let label = match state.stun.results.iter().find(|r| r.addr == addr) {
            Some(result) => server_label(result, width),
            None => format!("{:<width$}  未测速", format!("  {}", addr), width = width),
        };
        items.push(Box::new(CheckMenuItem::with_id(
            format!("{}{}", ID_STUN_PREFIX, addr),
            label,
            true,
            true,
            None,
        )));
    }

    if state.stun.results.is_empty() && !state.stun.probing {
        items.push(Box::new(MenuItem::with_id(
            "stun_empty",
            "(尚无测速结果，点击「测速并刷新」)",
            false,
            None,
        )));
    }

    items.push(Box::new(PredefinedMenuItem::separator()));

    items.push(Box::new(MenuItem::with_id(
        ID_STUN_REFRESH,
        "测速并刷新列表",
        !state.stun.probing,
        None,
    )));
    items.push(Box::new(MenuItem::with_id(
        ID_STUN_ADD_NODE,
        "添加自定义节点…",
        true,
        None,
    )));
    items.push(Box::new(MenuItem::with_id(
        ID_STUN_ADD_SOURCE,
        "添加自定义源…",
        true,
        None,
    )));

    items.push(Box::new(PredefinedMenuItem::separator()));

    items.push(Box::new(MenuItem::with_id(
        ID_STUN_EDIT_NODES,
        "编辑节点列表…",
        true,
        None,
    )));
    items.push(Box::new(MenuItem::with_id(
        ID_STUN_EDIT_SOURCES,
        "编辑自定义源…",
        true,
        None,
    )));
    items.push(Box::new(MenuItem::with_id(
        ID_STUN_OPEN_DIR,
        "打开 STUN 目录",
        true,
        None,
    )));

    items.push(Box::new(PredefinedMenuItem::separator()));

    items.push(Box::new(MenuItem::with_id(
        ID_STUN_RESTORE,
        "恢复默认（注释 STUN 项）",
        current.is_some(),
        None,
    )));

    let refs: Vec<&dyn tray_icon::menu::IsMenuItem> = items.iter().map(|b| b.as_ref()).collect();
    Submenu::with_items("STUN 服务器", true, &refs)
        .unwrap_or_else(|_| Submenu::new("STUN 服务器", true))
}

/// `当前: stun.host:3478 · 38ms 优 · 出口 1.2.3.4`
fn current_status_label(current: &Option<String>, results: &[ProbeResult]) -> String {
    let Some(addr) = current.as_deref() else {
        return format!("当前: 未设置（frp 默认 {}）", stun::FRP_DEFAULT_SERVER);
    };
    let Some(result) = results.iter().find(|r| r.addr == addr) else {
        return format!("当前: {}", addr);
    };
    match (result.latency_ms, &result.public_ip) {
        (Some(ms), Some(ip)) => {
            format!("当前: {} · {}ms {} · 出口 {}", addr, ms, result.grade(), ip)
        }
        (Some(ms), None) => format!("当前: {} · {}ms {}", addr, ms, result.grade()),
        (None, _) => {
            let reason = result.error.as_deref().unwrap_or("无响应");
            format!("当前: {} · {} ({})", addr, result.grade(), reason)
        }
    }
}

/// `★ host:port` for nodes the user added, indented for the others so the
/// latency column lines up.
fn server_head(result: &ProbeResult) -> String {
    if result.custom {
        format!("★ {}", result.addr)
    } else {
        format!("  {}", result.addr)
    }
}

fn server_label(result: &ProbeResult, width: usize) -> String {
    let mut label = format!("{:<width$}", server_head(result), width = width);
    match result.latency_ms {
        Some(ms) => {
            label.push_str(&format!("  {}ms {}", ms, result.grade()));
            if result.loss > 0 {
                label.push_str(&format!("  丢包 {}/{}", result.loss, result.attempts));
            }
        }
        None => label.push_str("  超时/不可用"),
    }
    label
}

fn ago_text(updated_at: Option<u64>) -> String {
    let Some(updated) = updated_at else {
        return "未测速".to_string();
    };
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(updated);
    let delta = now.saturating_sub(updated);
    if delta < 60 {
        "刚刚测速".to_string()
    } else if delta < 3600 {
        format!("{} 分钟前测速", delta / 60)
    } else if delta < 86_400 {
        format!("{} 小时前测速", delta / 3600)
    } else {
        format!("{} 天前测速", delta / 86_400)
    }
}

fn truncate(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let mut out: String = text.chars().take(max_chars.saturating_sub(1)).collect();
    out.push('…');
    out
}
