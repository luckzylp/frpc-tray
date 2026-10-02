use std::sync::{Arc, Mutex};
use std::process::Child;

use tray_icon::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tray_icon::Icon;

use crate::config::{list_configs, Settings};

pub const ID_START: &str = "start";
pub const ID_STOP: &str = "stop";
pub const ID_AUTOSTART: &str = "autostart";
pub const ID_CHECK_UPDATE: &str = "check_update";
pub const ID_REDOWNLOAD: &str = "redownload";
pub const ID_QUIT: &str = "quit";
pub const ID_CONFIG_PREFIX: &str = "cfg:";

pub struct AppState {
    pub settings: Settings,
    pub child: Option<Child>,
    pub frpc_path: Option<std::path::PathBuf>,
    pub current_config: Option<String>,
}

impl AppState {
    pub fn new(settings: Settings, frpc_path: Option<std::path::PathBuf>) -> Self {
        Self {
            settings,
            child: None,
            frpc_path,
            current_config: None,
        }
    }

    pub fn is_running(&self) -> bool {
        self.child.is_some()
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
        cfg_items.push(Box::new(MenuItem::with_id("no_config", "(无配置文件)", true, None)));
    } else {
        for name in &configs {
            let id = format!("{}{}", ID_CONFIG_PREFIX, name);
            let checked = current.map_or(false, |c| c == name.as_str());
            cfg_items.push(Box::new(CheckMenuItem::with_id(&id, name, true, checked, None)));
        }
    }
    let cfg_refs: Vec<&dyn tray_icon::menu::IsMenuItem> = cfg_items.iter().map(|b| b.as_ref()).collect();
    let submenu = Submenu::with_items("配置", true, &cfg_refs)
        .unwrap_or_else(|_| Submenu::new("配置", true));
    let _ = menu.append(&submenu);

    let _ = menu.append(&PredefinedMenuItem::separator());

    let autostart = CheckMenuItem::with_id(
        ID_AUTOSTART,
        "开机自启",
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
