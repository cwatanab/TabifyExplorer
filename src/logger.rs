use std::fs::{File, OpenOptions};
use std::io::Write;
use std::sync::Mutex;

static LOGGER: Mutex<Option<File>> = Mutex::new(None);

/// 実行ファイルのディレクトリを取得します。
pub fn get_exe_dir() -> Option<std::path::PathBuf> {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
}

pub fn init_logger() {
    if !crate::config::is_log_enabled() {
        return;
    }
    let log_path = get_exe_dir()
        .map(|d| d.join("TabifyExplorer.log"))
        .unwrap_or_else(|| std::path::PathBuf::from("TabifyExplorer.log"));
    let file = OpenOptions::new().create(true).append(true).open(&log_path);
    if let Ok(f) = file {
        if let Ok(mut guard) = LOGGER.lock() {
            *guard = Some(f);
        }
    }
}

/// ログファイルの絶対パスを取得します。
pub fn get_log_path() -> Option<std::path::PathBuf> {
    get_exe_dir().map(|d| d.join("TabifyExplorer.log"))
}

pub fn log_msg(level: &str, msg: &str) {
    if !crate::config::is_log_enabled() {
        return;
    }
    if let Ok(mut guard) = LOGGER.lock() {
        if let Some(f) = guard.as_mut() {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            let _ = writeln!(f, "[{}] [{}] {}", now, level, msg);
            let _ = f.flush();
        }
    }
}

#[macro_export]
macro_rules! info {
    ($($arg:tt)*) => {
        $crate::logger::log_msg("INFO", &format!($($arg)*))
    };
}

#[macro_export]
macro_rules! error {
    ($($arg:tt)*) => {
        $crate::logger::log_msg("ERROR", &format!($($arg)*))
    };
}

#[macro_export]
macro_rules! warn {
    ($($arg:tt)*) => {
        $crate::logger::log_msg("WARN", &format!($($arg)*))
    };
}
