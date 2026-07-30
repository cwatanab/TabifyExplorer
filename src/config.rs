use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

use windows::Win32::System::Registry::{
    RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegSetValueExW, HKEY_CURRENT_USER, KEY_SET_VALUE,
    REG_SZ,
};

static UNIFY_VIEW_MODE: AtomicBool = AtomicBool::new(true);
static AUTO_START: AtomicBool = AtomicBool::new(false);
static ENABLE_LOG: AtomicBool = AtomicBool::new(false);

const RUN_KEY_PATH: windows::core::PCWSTR =
    windows::core::w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
const APP_REG_NAME: windows::core::PCWSTR = windows::core::w!("TabifyExplorer");

fn get_config_path() -> Option<PathBuf> {
    std::env::current_exe().ok().map(|mut p| {
        p.set_extension("toml");
        p
    })
}

pub fn is_unify_view_mode_enabled() -> bool {
    UNIFY_VIEW_MODE.load(Ordering::Relaxed)
}

pub fn set_unify_view_mode_enabled(enabled: bool) {
    UNIFY_VIEW_MODE.store(enabled, Ordering::Relaxed);
    save_config();
}

pub fn toggle_unify_view_mode() -> bool {
    let new_val = !is_unify_view_mode_enabled();
    set_unify_view_mode_enabled(new_val);
    new_val
}

pub fn is_auto_start_enabled() -> bool {
    AUTO_START.load(Ordering::Relaxed)
}

pub fn set_auto_start_enabled(enabled: bool) {
    AUTO_START.store(enabled, Ordering::Relaxed);
    set_auto_start_in_registry(enabled);
    save_config();
}

pub fn toggle_auto_start() -> bool {
    let new_val = !is_auto_start_enabled();
    set_auto_start_enabled(new_val);
    new_val
}

pub fn is_log_enabled() -> bool {
    ENABLE_LOG.load(Ordering::Relaxed)
}

pub fn set_log_enabled(enabled: bool) {
    ENABLE_LOG.store(enabled, Ordering::Relaxed);
    if enabled {
        crate::logger::init_logger();
    }
    save_config();
}

pub fn toggle_log_enabled() -> bool {
    let new_val = !is_log_enabled();
    set_log_enabled(new_val);
    new_val
}

fn set_auto_start_in_registry(enable: bool) {
    unsafe {
        let mut key = Default::default();
        if RegOpenKeyExW(HKEY_CURRENT_USER, RUN_KEY_PATH, 0, KEY_SET_VALUE, &mut key).is_ok() {
            if enable {
                if let Ok(exe_path) = std::env::current_exe() {
                    let path_quoted = format!("\"{}\"\0", exe_path.display());
                    let u16_buf: Vec<u16> = path_quoted.encode_utf16().collect();
                    let bytes = std::slice::from_raw_parts(
                        u16_buf.as_ptr() as *const u8,
                        u16_buf.len() * 2,
                    );
                    let _ = RegSetValueExW(key, APP_REG_NAME, 0, REG_SZ, Some(bytes));
                }
            } else {
                let _ = RegDeleteValueW(key, APP_REG_NAME);
            }
            let _ = RegCloseKey(key);
        }
    }
}

pub fn load_config() {
    // 旧 json 設定ファイルがあれば削除
    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(parent) = exe_path.parent() {
            let old_json = parent.join("tabify_config.json");
            if old_json.exists() {
                let _ = fs::remove_file(old_json);
            }
        }
    }

    if let Some(path) = get_config_path() {
        if path.exists() {
            if let Ok(content) = fs::read_to_string(&path) {
                if content.contains("unify_view_mode = false") {
                    UNIFY_VIEW_MODE.store(false, Ordering::Relaxed);
                } else {
                    UNIFY_VIEW_MODE.store(true, Ordering::Relaxed);
                }

                if content.contains("auto_start = true") {
                    AUTO_START.store(true, Ordering::Relaxed);
                    set_auto_start_in_registry(true);
                } else {
                    AUTO_START.store(false, Ordering::Relaxed);
                }

                if content.contains("enable_log = true") {
                    ENABLE_LOG.store(true, Ordering::Relaxed);
                } else {
                    ENABLE_LOG.store(false, Ordering::Relaxed);
                }
            }
        } else {
            // 初回起動時: unify_view_mode=true (デフォルトON), auto_start=false, enable_log=false (デフォルトOFF) で TOML 生成
            save_config();
        }
    }
}

fn save_config() {
    if let Some(path) = get_config_path() {
        let unify = is_unify_view_mode_enabled();
        let auto_start = is_auto_start_enabled();
        let log_enabled = is_log_enabled();
        let content = format!(
            "# TabifyExplorer 設定ファイル\nunify_view_mode = {}\nauto_start = {}\nenable_log = {}\n",
            unify, auto_start, log_enabled
        );
        let _ = fs::write(path, content);
    }
}
