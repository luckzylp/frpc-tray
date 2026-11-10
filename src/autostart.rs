//! Per-user autostart integration.
//!
//! - Windows: `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`
//! - Linux / BSD: an XDG `~/.config/autostart/frpc-tray.desktop` entry
//! - macOS: a `~/Library/LaunchAgents/com.frpc-tray.plist` launch agent
//!
//! Enabling autostart takes effect at the next login; the running app is not
//! launched a second time (the single-instance guard would reject it anyway).

#[cfg(windows)]
mod platform {
    use std::env;

    use winreg::RegKey;
    use winreg::enums::*;

    const AUTOSTART_NAME: &str = "frpc-tray";
    const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

    pub fn is_autostart() -> bool {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        match hkcu.open_subkey_with_flags(RUN_KEY, KEY_READ) {
            Ok(key) => key.get_value::<String, _>(AUTOSTART_NAME).is_ok(),
            Err(_) => false,
        }
    }

    pub fn set_autostart(enable: bool) {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let (key, _disp) = match hkcu.create_subkey_with_flags(RUN_KEY, KEY_WRITE) {
            Ok(v) => v,
            Err(_) => return,
        };
        if enable {
            if let Ok(exe) = env::current_exe() {
                let exe_str = exe.to_string_lossy().to_string();
                let _ = key.set_value(AUTOSTART_NAME, &exe_str);
            }
        } else {
            let _ = key.delete_value(AUTOSTART_NAME);
        }
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use std::fs;
    use std::path::PathBuf;

    const DESKTOP_FILE: &str = "frpc-tray.desktop";

    fn desktop_path() -> PathBuf {
        let config_home = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                std::env::var_os("HOME")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| PathBuf::from("."))
                    .join(".config")
            });
        config_home.join("autostart").join(DESKTOP_FILE)
    }

    pub fn is_autostart() -> bool {
        desktop_path().exists()
    }

    pub fn set_autostart(enable: bool) {
        let path = desktop_path();
        if !enable {
            let _ = fs::remove_file(path);
            return;
        }
        let Ok(exe) = std::env::current_exe() else {
            return;
        };
        if let Some(parent) = path.parent() {
            if fs::create_dir_all(parent).is_err() {
                return;
            }
        }
        // Quoting the whole path is required by the Desktop Entry spec when it
        // contains spaces; inner quotes/backslashes have to be escaped.
        let exe = exe
            .to_string_lossy()
            .replace('\\', "\\\\")
            .replace('"', "\\\"");
        let content = format!(
            "[Desktop Entry]\n\
             Type=Application\n\
             Version=1.0\n\
             Name=frpc-tray\n\
             Comment=frp client tray manager\n\
             Exec=\"{}\"\n\
             Terminal=false\n\
             Hidden=false\n\
             X-GNOME-Autostart-enabled=true\n",
            exe
        );
        let _ = fs::write(path, content);
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use std::fs;
    use std::path::PathBuf;

    const LABEL: &str = "com.frpc-tray";

    fn plist_path() -> Option<PathBuf> {
        let home = std::env::var_os("HOME")?;
        Some(
            PathBuf::from(home)
                .join("Library/LaunchAgents")
                .join(format!("{}.plist", LABEL)),
        )
    }

    fn xml_escape(s: &str) -> String {
        s.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
            .replace('\'', "&apos;")
    }

    pub fn is_autostart() -> bool {
        plist_path().is_some_and(|p| p.exists())
    }

    pub fn set_autostart(enable: bool) {
        let Some(path) = plist_path() else {
            return;
        };
        if !enable {
            let _ = fs::remove_file(path);
            return;
        }
        let Ok(exe) = std::env::current_exe() else {
            return;
        };
        if let Some(parent) = path.parent() {
            if fs::create_dir_all(parent).is_err() {
                return;
            }
        }
        let exe = xml_escape(&exe.to_string_lossy());
        let content = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
             <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
             <plist version=\"1.0\">\n\
             <dict>\n\
               <key>Label</key>\n\
               <string>{}</string>\n\
               <key>ProgramArguments</key>\n\
               <array>\n\
                 <string>{}</string>\n\
               </array>\n\
               <key>RunAtLoad</key>\n\
               <true/>\n\
               <key>KeepAlive</key>\n\
               <false/>\n\
               <key>ProcessType</key>\n\
               <string>Background</string>\n\
             </dict>\n\
             </plist>\n",
            LABEL, exe
        );
        let _ = fs::write(path, content);
    }
}

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
mod platform {
    pub fn is_autostart() -> bool {
        false
    }

    pub fn set_autostart(_enable: bool) {}
}

pub use platform::{is_autostart, set_autostart};
