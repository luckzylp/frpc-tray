// Hide the console window on Windows release builds only. On Unix there is no
// such subsystem attribute, so the whole cfg_attr must be gated too.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod autostart;
mod config;
mod frpc;
mod logger;
mod platform;
mod tray;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use tao::event_loop::{ControlFlow, EventLoop};
use tray_icon::TrayIconBuilder;
use tray_icon::menu::MenuEvent;

use config::Settings;
use frpc::{FRP_VERSION, check_update, ensure_frpc, start_frpc, stop_frpc};
use platform::BINARY_NAME;
use tray::{
    AppState, ID_AUTOSTART, ID_CHECK_UPDATE, ID_CONFIG_PREFIX, ID_QUIT, ID_REDOWNLOAD, ID_START,
    ID_STOP, SharedState, build_menu,
};

fn main() {
    let instance = match single_instance::SingleInstance::new(&single_instance_name()) {
        Ok(i) => i,
        Err(_) => return,
    };
    if !instance.is_single() {
        return;
    }

    let mut settings = Settings::load();

    let current_ver = settings
        .frpc_version
        .clone()
        .unwrap_or_else(|| FRP_VERSION.to_string());
    if settings.frpc_version.is_none() {
        settings.frpc_version = Some(FRP_VERSION.to_string());
        settings.save();
    }

    logger::log(&format!("frpc-tray 启动，frp 版本 {}", current_ver));

    let configs = config::list_configs();
    let initial_current = settings
        .last_config
        .clone()
        .or_else(|| config::default_config(&configs));

    let state = Arc::new(Mutex::new(AppState::new(settings, None)));
    {
        let mut s = state.lock().unwrap();
        s.current_config = initial_current;
    }

    let (dl_tx, dl_rx) = std::sync::mpsc::channel::<Result<PathBuf, String>>();
    ensure_frpc_async(&dl_tx, &current_ver);

    silent_check_update_async(&state, &current_ver);

    #[allow(unused_mut)]
    let mut event_loop = EventLoop::new();
    // A tray-only app should not occupy a Dock icon / menu bar on macOS.
    #[cfg(target_os = "macos")]
    {
        use tao::platform::macos::{ActivationPolicy, EventLoopExtMacOS};
        event_loop.set_activation_policy(ActivationPolicy::Accessory);
    }

    let (icon, menu) = {
        let s = state.lock().unwrap();
        let menu = build_menu(&s);
        (tray::icon_for_state(&s), menu)
    };

    let tray = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip("frpc-tray")
        .with_icon(icon)
        .build()
        .expect("failed to create tray icon");

    let state_clone = state.clone();
    let tray_clone = tray;

    event_loop.run(move |_event, _target, control_flow| {
        *control_flow = ControlFlow::WaitUntil(Instant::now() + Duration::from_millis(200));

        if let Some(status) = reap_exited_frpc(&state_clone) {
            logger::log(&format!("frpc 已退出 ({})，详见 frpc.log", status));
            rebuild(&tray_clone, &state_clone);
        }

        if let Ok(result) = dl_rx.try_recv() {
            match result {
                Ok(path) => {
                    logger::log(&format!("{} 就绪: {}", BINARY_NAME, path.display()));
                    if let Ok(mut s) = state_clone.lock() {
                        s.frpc_path = Some(path);
                    }
                    rebuild(&tray_clone, &state_clone);
                }
                Err(e) => {
                    logger::log(&format!("获取 {} 失败: {}", BINARY_NAME, e));
                }
            }
        }

        while let Ok(menu_event) = MenuEvent::receiver().try_recv() {
            let id = menu_event.id.0.clone();
            handle_menu_event(&id, &state_clone, &tray_clone, control_flow);
        }
    });
}

/// `single-instance` uses a named mutex on Windows, an abstract socket on
/// Linux, but a **file path** on macOS (it flock()s that file). Using the same
/// bare name everywhere would create a stray file in the working directory on
/// macOS and break when the CWD is read-only.
fn single_instance_name() -> String {
    #[cfg(target_os = "macos")]
    {
        return config::base_dir()
            .join("frpc-tray.lock")
            .to_string_lossy()
            .into_owned();
    }
    #[cfg(not(target_os = "macos"))]
    {
        "frpc-tray-single-instance".to_string()
    }
}

fn handle_menu_event(
    id: &str,
    state: &SharedState,
    tray: &tray_icon::TrayIcon,
    control_flow: &mut ControlFlow,
) {
    if id == ID_START {
        start_command(state);
        rebuild(tray, state);
    } else if id == ID_STOP {
        stop_command(state);
        rebuild(tray, state);
    } else if id == ID_AUTOSTART {
        let now = autostart::is_autostart();
        autostart::set_autostart(!now);
        logger::log(&format!("自动启动已{}", if !now { "开启" } else { "关闭" }));
        rebuild(tray, state);
    } else if id == ID_CHECK_UPDATE {
        check_update_command(state);
    } else if id == ID_REDOWNLOAD {
        redownload_command(state);
    } else if id == ID_QUIT {
        quit_command(state, control_flow);
    } else if let Some(name) = id.strip_prefix(ID_CONFIG_PREFIX) {
        switch_config(state, name.to_string());
        rebuild(tray, state);
    }
}

fn rebuild(tray: &tray_icon::TrayIcon, state: &SharedState) {
    let (menu, icon) = {
        let s = state.lock().unwrap();
        let menu = build_menu(&s);
        let icon = tray::icon_for_state(&s);
        (menu, icon)
    };
    tray.set_menu(Some(Box::new(menu)));
    let _ = tray.set_icon(Some(icon));
}

fn reap_exited_frpc(state: &SharedState) -> Option<std::process::ExitStatus> {
    let mut s = state.lock().ok()?;
    let status = match s.child.as_mut() {
        Some(child) => match child.try_wait() {
            Ok(Some(status)) => Some(status),
            Ok(None) => None,
            Err(e) => {
                logger::log(&format!("读取 frpc 进程状态失败: {}", e));
                None
            }
        },
        None => None,
    };
    let status = status?;
    s.child = None;
    Some(status)
}

fn start_command(state: &SharedState) {
    let mut s = state.lock().unwrap();
    if s.is_running() {
        logger::log("frpc 已在运行中");
        return;
    }
    let exe = match &s.frpc_path {
        Some(p) => p.clone(),
        None => {
            logger::log(&format!("{} 不可用，请先重新下载", BINARY_NAME));
            return;
        }
    };
    let config_name = match &s.current_config {
        Some(c) => c.clone(),
        None => match config::default_config(&config::list_configs()) {
            Some(c) => c.clone(),
            None => {
                logger::log("没有可用的配置文件");
                return;
            }
        },
    };
    let config_path = config::configs_dir().join(&config_name);
    if !config_path.exists() {
        logger::log(&format!("配置文件不存在: {}", config_path.display()));
        return;
    }
    match start_frpc(&exe, &config_path) {
        Ok(child) => {
            s.child = Some(child);
        }
        Err(e) => {
            logger::log(&e);
        }
    }
}

fn stop_command(state: &SharedState) {
    let mut s = state.lock().unwrap();
    if let Some(mut child) = s.child.take() {
        stop_frpc(&mut child);
    } else {
        logger::log("frpc 未在运行");
    }
}

fn switch_config(state: &SharedState, name: String) {
    let was_running;
    {
        let mut s = state.lock().unwrap();
        was_running = s.is_running();
        s.current_config = Some(name.clone());
        s.settings.last_config = Some(name.clone());
        s.settings.save();
    }

    logger::log(&format!("切换配置: {}", name));

    if was_running {
        stop_command(state);
        start_command(state);
    }
}

fn check_update_command(state: &SharedState) {
    logger::log("正在检查更新...");
    let state = state.clone();
    thread::spawn(move || {
        let current_ver = {
            let s = state.lock().unwrap();
            s.settings
                .frpc_version
                .clone()
                .unwrap_or_else(|| FRP_VERSION.to_string())
        };

        match check_update(&current_ver) {
            Ok(Some(new_ver)) => {
                logger::log(&format!("发现新版本 v{}，开始更新...", new_ver));
                if let Err(e) = do_update(&new_ver, &state) {
                    logger::log(&format!("更新失败: {}", e));
                }
            }
            Ok(None) => {
                logger::log(&format!("已是最新版本 (v{})", current_ver));
            }
            Err(e) => {
                logger::log(&format!("检查更新失败: {}", e));
            }
        }
    });
}

fn do_update(new_ver: &str, state: &SharedState) -> Result<(), String> {
    let was_running = state.lock().unwrap().is_running();

    if was_running {
        logger::log("更新前停止 frpc...");
        let mut s = state.lock().unwrap();
        if let Some(mut child) = s.child.take() {
            stop_frpc(&mut child);
        }
        drop(s);
    }

    let exe_path = frpc::frpc_exe_path();
    let new_path = config::frpc_dir().join(format!("{}.new", BINARY_NAME));
    let bak_path = config::frpc_dir().join(format!("{}.bak", BINARY_NAME));
    let archive_path = config::frpc_dir().join(format!("frp_update.{}", platform::ARCHIVE_EXT));

    let url = platform::download_url(new_ver)?;
    logger::log(&format!("下载 v{}", new_ver));
    frpc::download_internal(&url, &archive_path)?;

    frpc::extract_frpc(&archive_path, &new_path, new_ver)?;
    let _ = std::fs::remove_file(&archive_path);

    if !new_path.exists() {
        return Err(format!("解压后未找到 {}", BINARY_NAME));
    }

    let _ = std::fs::remove_file(&bak_path);
    if exe_path.exists() {
        let _ = std::fs::rename(&exe_path, &bak_path);
    }
    if std::fs::rename(&new_path, &exe_path).is_err() {
        if bak_path.exists() {
            let _ = std::fs::rename(&bak_path, &exe_path);
        }
        return Err(format!("替换 {} 失败", BINARY_NAME));
    }
    let _ = std::fs::remove_file(&bak_path);

    {
        let mut s = state.lock().unwrap();
        s.settings.frpc_version = Some(new_ver.to_string());
        s.settings.save();
        s.frpc_path = Some(exe_path.clone());
    }

    logger::log(&format!("已更新到 v{}", new_ver));

    if was_running {
        logger::log("更新后重启 frpc...");
        start_command(state);
    }

    Ok(())
}

fn redownload_command(state: &SharedState) {
    logger::log("开始重新下载 frpc...");
    let exe_path = frpc::frpc_exe_path();
    let _ = std::fs::remove_file(&exe_path);
    let state = state.clone();
    let ver = {
        let s = state.lock().unwrap();
        s.settings
            .frpc_version
            .clone()
            .unwrap_or_else(|| FRP_VERSION.to_string())
    };
    thread::spawn(move || match ensure_frpc(&ver) {
        Ok(p) => {
            logger::log(&format!("frpc 下载完成: {}", p.display()));
            if let Ok(mut s) = state.lock() {
                s.frpc_path = Some(p);
            }
        }
        Err(e) => {
            logger::log(&format!("下载失败: {}", e));
        }
    });
}

fn quit_command(state: &SharedState, _control_flow: &mut ControlFlow) {
    let mut s = state.lock().unwrap();
    if let Some(mut child) = s.child.take() {
        stop_frpc(&mut child);
    }
    s.settings.save();
    drop(s);
    logger::log("frpc-tray 退出");
    std::process::exit(0);
}

fn ensure_frpc_async(tx: &std::sync::mpsc::Sender<Result<PathBuf, String>>, version: &str) {
    let tx = tx.clone();
    let version = version.to_string();
    thread::spawn(move || match ensure_frpc(&version) {
        Ok(p) => {
            logger::log(&format!("{} 就绪: {}", BINARY_NAME, p.display()));
            let _ = tx.send(Ok(p));
        }
        Err(e) => {
            logger::log(&format!("获取 {} 失败: {}", BINARY_NAME, e));
            let _ = tx.send(Err(e));
        }
    });
}

fn silent_check_update_async(_state: &SharedState, current_ver: &str) {
    let current_ver = current_ver.to_string();
    thread::spawn(move || {
        thread::sleep(Duration::from_secs(3));
        match check_update(&current_ver) {
            Ok(Some(new_ver)) => {
                logger::log(&format!(
                    "发现新版本 v{}，可在菜单中点击「检查更新」进行升级",
                    new_ver
                ));
            }
            Ok(None) => {
                // silent, no log
            }
            Err(e) => {
                logger::log(&format!("静默检查更新失败: {}", e));
            }
        }
    });
}
