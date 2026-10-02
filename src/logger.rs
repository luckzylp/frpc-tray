use std::fs::OpenOptions;
use std::io::Write;
use std::sync::Mutex;

use crate::config::logs_dir;

static LOG_LOCK: Mutex<()> = Mutex::new(());

/// Append a line to `<exe_dir>/frp/logs/frpc-tray.log`. Best-effort; errors
/// are swallowed so logging never breaks app flow.
pub fn log(msg: &str) {
    let path = logs_dir().join("frpc-tray.log");
    let _guard = LOG_LOCK.lock();
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(f, "{}", msg);
    }
}
