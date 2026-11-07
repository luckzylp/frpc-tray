use std::env;

use winreg::enums::*;
use winreg::RegKey;

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
