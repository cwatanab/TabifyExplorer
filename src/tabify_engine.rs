use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use windows::core::{Interface, VARIANT};
use windows::Win32::Foundation::{BOOL, HWND, LPARAM, RECT};
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, COINIT_APARTMENTTHREADED};
use windows::Win32::UI::WindowsAndMessaging::{EnumWindows, IsWindow};

use crate::com_navigator;
use crate::path_resolver;
use crate::uia_tab_creator;
use crate::window_controller;
use crate::{error, info, warn};

unsafe extern "system" fn enum_existing_explorer_callback(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let hwnds = &mut *(lparam.0 as *mut Vec<isize>);
    if window_controller::is_normal_visible_explorer_window(hwnd, None) {
        hwnds.push(hwnd.0);
    }
    BOOL(1)
}

#[derive(Clone)]
pub struct TabifyEngine {
    known_explorer_hwnds: Arc<Mutex<HashSet<isize>>>,
    processing_hwnds: Arc<Mutex<HashSet<isize>>>,
}

impl Default for TabifyEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl TabifyEngine {
    pub fn new() -> Self {
        Self {
            known_explorer_hwnds: Arc::new(Mutex::new(HashSet::new())),
            processing_hwnds: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    pub fn register_existing_windows(&self) {
        let mut hwnds = Vec::new();
        unsafe {
            let _ = EnumWindows(
                Some(enum_existing_explorer_callback),
                LPARAM(&mut hwnds as *mut Vec<isize> as isize),
            );
        }
        let mut known = self.known_explorer_hwnds.lock().unwrap();
        for hwnd in hwnds {
            info!("既存のエクスプローラーウィンドウを登録: HWND {}", hwnd);
            known.insert(hwnd);
        }
    }

    pub fn start_background_watcher(self: Arc<Self>) {
        std::thread::spawn(move || {
            unsafe {
                let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            }
            let mut tick = 0u32;
            loop {
                std::thread::sleep(Duration::from_millis(150));
                window_controller::update_mouse_drag_state();

                tick += 1;
                if tick.is_multiple_of(50) {
                    self.cleanup_dead_hwnds();
                }

                let mut hwnds = Vec::new();
                unsafe {
                    if let Ok(shell_windows) =
                        CoCreateInstance::<_, windows::Win32::UI::Shell::IShellWindows>(
                            &windows::Win32::UI::Shell::ShellWindows,
                            None,
                            windows::Win32::System::Com::CLSCTX_ALL,
                        )
                    {
                        if let Ok(count) = shell_windows.Count() {
                            for i in 0..count {
                                let var = VARIANT::from(i);
                                if let Ok(dispatch) = shell_windows.Item(&var) {
                                    if let Ok(browser) =
                                        dispatch.cast::<windows::Win32::UI::Shell::IWebBrowser2>()
                                    {
                                        if let Ok(hwnd_val) = browser.HWND() {
                                            if hwnd_val.0 != 0 {
                                                let root =
                                                    com_navigator::get_root_hwnd(HWND(hwnd_val.0));
                                                let is_visible = window_controller::is_normal_visible_explorer_window(root, None);
                                                if is_visible && !hwnds.contains(&root.0) {
                                                    hwnds.push(root.0);
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                for hwnd_val in hwnds {
                    let is_known = {
                        let known = self.known_explorer_hwnds.lock().unwrap();
                        known.contains(&hwnd_val)
                    };

                    let is_processing = {
                        let processing = self.processing_hwnds.lock().unwrap();
                        processing.contains(&hwnd_val)
                    };

                    if !is_known && !is_processing {
                        info!("IShellWindows バックグラウンドウォッチャーで新規エクスプローラー HWND を検知: HWND {}", hwnd_val);
                        self.process_window(hwnd_val);
                    }
                }
            }
        });
    }

    fn cleanup_dead_hwnds(&self) {
        let mut known = self.known_explorer_hwnds.lock().unwrap();
        let before = known.len();
        known.retain(|&hwnd_val| unsafe { IsWindow(HWND(hwnd_val)).as_bool() });
        let removed = before - known.len();
        if removed > 0 {
            info!(
                "非存在 HWND {} 件を登録リストから削除しました (残り: {})",
                removed,
                known.len()
            );
        }
    }

    fn register_known_hwnd(&self, hwnd_val: isize) {
        let mut known = self.known_explorer_hwnds.lock().unwrap();
        known.insert(hwnd_val);
    }

    fn restore_window_and_register(&self, hwnd: HWND, original_rect: Option<RECT>) {
        window_controller::restore_window(hwnd, original_rect);
        self.register_known_hwnd(hwnd.0);
    }

    fn resolve_target_folder_path(new_hwnd: HWND) -> Option<String> {
        const WINDOW_PATH_POLL_ATTEMPTS_WITH_CMDLINE: usize = 150;
        const WINDOW_PATH_POLL_ATTEMPTS_WITHOUT_CMDLINE: usize = 500;

        let mut cmdline_path: Option<String> = None;

        if let Some(cmd_line) = crate::process_info::get_command_line_from_hwnd(new_hwnd) {
            if let Some(path) = crate::process_info::extract_folder_from_cmdline(&cmd_line) {
                info!("プロセス PEB コマンドラインから候補パスを抽出: '{}'", path);
                cmdline_path = Some(path);
            }
        }

        let poll_attempts = if cmdline_path.is_some() {
            WINDOW_PATH_POLL_ATTEMPTS_WITH_CMDLINE
        } else {
            WINDOW_PATH_POLL_ATTEMPTS_WITHOUT_CMDLINE
        };

        let mut last_home_path: Option<String> = None;
        for _attempt in 1..=poll_attempts {
            if let Some(path) = com_navigator::get_window_path(new_hwnd) {
                if !path.is_empty() && path_resolver::is_navigable_folder(&path) {
                    if path_resolver::is_home_path(&path) {
                        last_home_path = Some(path);
                    } else {
                        if let Some(candidate) = cmdline_path.as_deref() {
                            if !path_resolver::are_paths_equivalent(candidate, &path) {
                                info!(
                                    "PEB 候補パス '{}' ではなく COM 取得パス '{}' を採用します",
                                    candidate, path
                                );
                            }
                        }

                        return Some(path);
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(2));
        }

        if let Some(path) = cmdline_path {
            info!(
                "COM から確定パスを取得できなかったため、PEB 候補パスを採用します: '{}'",
                path
            );
            return Some(path);
        }

        last_home_path
    }

    fn find_new_tab_browser(
        target_hwnd: HWND,
        all_tabs: &[com_navigator::TabInfo],
    ) -> Option<windows::Win32::UI::Shell::IWebBrowser2> {
        let before_ptrs: HashSet<*mut std::ffi::c_void> =
            all_tabs.iter().map(|tab| tab.browser.as_raw()).collect();

        for _attempt in 0..50 {
            let tabs_after = com_navigator::get_all_tabs(target_hwnd);

            for tab_after in &tabs_after {
                let ptr = tab_after.browser.as_raw();
                if !before_ptrs.contains(&ptr) {
                    return Some(tab_after.browser.clone());
                }
            }

            if tabs_after.len() > all_tabs.len() {
                if let Some(last_tab) = tabs_after.last() {
                    return Some(last_tab.browser.clone());
                }
            }

            std::thread::sleep(Duration::from_millis(2));
        }

        None
    }

    pub fn process_window(&self, hwnd_val: isize) {
        {
            let known = self.known_explorer_hwnds.lock().unwrap();
            if known.len() > 20 {
                drop(known);
                self.cleanup_dead_hwnds();
            }
        }

        let new_hwnd = HWND(hwnd_val);

        if !window_controller::is_explorer_window(new_hwnd) {
            return;
        }

        info!("process_window イベント受領: HWND {}", hwnd_val);

        {
            let mut processing = self.processing_hwnds.lock().unwrap();
            if processing.contains(&hwnd_val) {
                info!("すでに処理中の HWND のためスキップ: HWND {}", hwnd_val);
                return;
            }
            processing.insert(hwnd_val);
        }

        let _processing_guard = ProcessingGuard {
            hwnds: Arc::clone(&self.processing_hwnds),
            hwnd_val,
        };

        {
            let known = self.known_explorer_hwnds.lock().unwrap();
            if known.contains(&hwnd_val) {
                info!(
                    "既知の HWND リストに含まれるためスキップ: HWND {}",
                    hwnd_val
                );
                return;
            }
        }

        info!("新規エクスプローラー検知: HWND {}", hwnd_val);

        let known_list: Vec<HWND> = {
            let known = self.known_explorer_hwnds.lock().unwrap();
            known.iter().map(|&val| HWND(val)).collect()
        };

        // 統合対象の既存エクスプローラーウィンドウを検索
        let target_hwnd = match window_controller::find_existing_explorer_window(
            &known_list,
            new_hwnd,
        ) {
            Some(target) => target,
            None => {
                info!(
                    "統合先の既存エクスプローラーが存在しないため、ベースウィンドウとして保持します (HWND: {})",
                    hwnd_val
                );
                self.register_known_hwnd(hwnd_val);
                return;
            }
        };

        info!("統合先ウィンドウ確定: HWND {}", target_hwnd.0);

        // 新規ウィンドウを最優先で即時フリッカーレス非表示化 (統合先が存在する場合のみ)
        let original_rect = window_controller::hide_window(new_hwnd);
        if original_rect.is_none() {
            error!(
                "ウィンドウ矩形の取得または非表示化に失敗しました: HWND {}",
                hwnd_val
            );
            self.register_known_hwnd(hwnd_val);
            return;
        }

        let drag_bypass_threshold = Duration::from_millis(1200);

        // Shift キー押下によるバイパス判定
        if window_controller::is_shift_key_pressed() {
            info!(
                "[バイパス] Shift キー検知のためタブ化をスキップして復元します (HWND: {})",
                hwnd_val
            );
            self.restore_window_and_register(new_hwnd, original_rect);
            return;
        }

        // マウス左ボタン押下 (タブのドラッグ＆ドロップ操作中/直後) 判定
        if window_controller::is_drag_and_drop_active_or_recent(drag_bypass_threshold) {
            info!(
                "[バイパス] ドラッグ＆ドロップ操作中/直後 (カーソル移動を伴うD&D) 検知のためタブ化をスキップして復元します (HWND: {})",
                hwnd_val
            );
            self.restore_window_and_register(new_hwnd, original_rect);
            return;
        }

        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        }

        let folder_path = match Self::resolve_target_folder_path(new_hwnd) {
            Some(p) => p,
            None => {
                warn!(
                    "フォルダパスを取得できなかったため表示を復元します (HWND: {})",
                    hwnd_val
                );
                self.restore_window_and_register(new_hwnd, original_rect);
                return;
            }
        };

        info!("目的フォルダパス確定: '{}'", folder_path);

        // 既存タブが存在するか確認
        let all_tabs = com_navigator::get_all_tabs(target_hwnd);
        let mut existing_tab_name = None;
        for tab in &all_tabs {
            if path_resolver::are_paths_equivalent(&tab.path, &folder_path) {
                existing_tab_name = Some(tab.location_name.clone());
                break;
            }
        }

        // 既存タブと同等のパスを持つウィンドウが検出された場合：
        // タブのドラッグアウト（切り離し）による新ウィンドウ生成であるため、新規ウィンドウを強制破棄せず独立復元・保持する
        if existing_tab_name.is_some()
            && window_controller::is_drag_and_drop_active_or_recent(drag_bypass_threshold)
        {
            info!(
                "[バイパス] ドラッグアウト分離検知 (既存タブと同等パス: '{}') のため統合をスキップして復元します (HWND: {})",
                folder_path, hwnd_val
            );
            self.restore_window_and_register(new_hwnd, original_rect);
            return;
        }

        if let Err(e) = uia_tab_creator::create_new_tab_via_uia(target_hwnd) {
            error!("新規タブ作成エラー: {}. 表示復元 (HWND: {})", e, hwnd_val);
            self.restore_window_and_register(new_hwnd, original_rect);
            return;
        }

        // スリープ待機を完全除去（0ms 即時連動）
        uia_tab_creator::activate_last_tab(target_hwnd);

        let parent_view_mode = if crate::config::is_unify_view_mode_enabled() {
            com_navigator::get_window_view_mode(target_hwnd)
        } else {
            None
        };

        let mut com_nav_success = false;
        if let Some(browser) = Self::find_new_tab_browser(target_hwnd, &all_tabs) {
            if com_navigator::navigate_via_com(&browser, &folder_path).is_ok() {
                com_nav_success = true;
            }
        }

        if !com_nav_success {
            info!("COM 経由でのナビゲーション対象が見つからないか失敗したため、アドレスバー経由での遷移にフォールバックします。");
            if let Err(e) = com_navigator::navigate_via_address_bar(target_hwnd, &folder_path) {
                error!(
                    "アドレスバー遷移エラー: {}. 表示復元 (HWND: {})",
                    e, hwnd_val
                );
                self.restore_window_and_register(new_hwnd, original_rect);
                return;
            }
        }

        if let Some(vm) = parent_view_mode {
            com_navigator::apply_view_mode_to_window(target_hwnd, vm);
            info!(
                "親ウィンドウの表示形式 (ViewMode={}) を自動統一適用しました",
                vm
            );
        }

        info!(
            "タブ統合成功: HWND {} ('{}') -> HWND {}",
            hwnd_val, folder_path, target_hwnd.0
        );
        window_controller::close_window(new_hwnd);
    }
}

struct ProcessingGuard {
    hwnds: Arc<Mutex<HashSet<isize>>>,
    hwnd_val: isize,
}

impl Drop for ProcessingGuard {
    fn drop(&mut self) {
        if let Ok(mut lock) = self.hwnds.lock() {
            lock.remove(&self.hwnd_val);
        }
    }
}
