use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

static UNIFY_VIEW_MODE: AtomicBool = AtomicBool::new(false);

fn get_config_path() -> Option<PathBuf> {
    std::env::current_exe().ok().and_then(|p| p.parent().map(|dir| dir.join("tabify_config.json")))
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

pub fn load_config() {
    if let Some(path) = get_config_path() {
        if path.exists() {
            if let Ok(content) = fs::read_to_string(&path) {
                if content.contains("\"unify_view_mode\": true") || content.contains("\"unify_view_mode\":true") {
                    UNIFY_VIEW_MODE.store(true, Ordering::Relaxed);
                } else {
                    UNIFY_VIEW_MODE.store(false, Ordering::Relaxed);
                }
            }
        }
    }
}

fn save_config() {
    if let Some(path) = get_config_path() {
        let enabled = is_unify_view_mode_enabled();
        let content = format!("{{\n  \"unify_view_mode\": {}\n}}\n", enabled);
        let _ = fs::write(path, content);
    }
}
