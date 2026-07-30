use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use windows::core::{Interface, VARIANT};
use windows::Win32::Foundation::{BOOL, HWND, LPARAM};
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
                tick += 1;
                if tick % 50 == 0 {
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
                                                if is_visible {
                                                    if !hwnds.contains(&(root.0 as isize)) {
                                                        hwnds.push(root.0 as isize);
                                                    }
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

        let processing_guard = ProcessingGuard {
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
                let mut known = self.known_explorer_hwnds.lock().unwrap();
                known.insert(hwnd_val);
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
            let mut known = self.known_explorer_hwnds.lock().unwrap();
            known.insert(hwnd_val);
            return;
        }

        // Shift キー押下によるバイパス判定
        if window_controller::is_shift_key_pressed() {
            info!(
                "[バイパス] Shift キー検知のためタブ化をスキップして復元します (HWND: {})",
                hwnd_val
            );
            window_controller::restore_window(new_hwnd, original_rect);
            let mut known = self.known_explorer_hwnds.lock().unwrap();
            known.insert(hwnd_val);
            return;
        }

        // マウス左ボタン長押し (ドラッグ＆ドロップ) 判定
        if window_controller::is_left_mouse_button_down() {
            let mut released = false;
            let check_interval = Duration::from_millis(10);
            let max_checks = 15;

            for _ in 0..max_checks {
                std::thread::sleep(check_interval);
                if !window_controller::is_left_mouse_button_down() {
                    released = true;
                    break;
                }
            }

            if !released {
                info!(
                    "[バイパス] ドラッグ＆ドロップ長押し検知のためスキップして復元します (HWND: {})",
                    hwnd_val
                );
                window_controller::restore_window(new_hwnd, original_rect);
                let mut known = self.known_explorer_hwnds.lock().unwrap();
                known.insert(hwnd_val);
                return;
            }
        }

        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        }

        // 新規ウィンドウが開こうとしている確定フォルダパスを取得 (1. プロセス PEB から 0ms 超即時抽出)
        let mut target_folder_path: Option<String> = None;

        if let Some(cmd_line) = crate::process_info::get_command_line_from_hwnd(new_hwnd) {
            if let Some(p) = crate::process_info::extract_folder_from_cmdline(&cmd_line) {
                info!("プロセス PEB コマンドラインから 0ms 即時パス抽出に成功: '{}'", p);
                target_folder_path = Some(p);
            }
        }

        // 2. コマンドラインからパースできない場合は COM ポート経由で取得
        if target_folder_path.is_none() {
            for _attempt in 1..=100 {
                if let Some(p) = com_navigator::get_window_path(new_hwnd) {
                    if !p.is_empty() && path_resolver::is_navigable_folder(&p) {
                        target_folder_path = Some(p.clone());
                        if !path_resolver::is_home_path(&p) {
                            break;
                        }
                    }
                }
                std::thread::sleep(Duration::from_millis(1));
            }
        }

        let folder_path = match target_folder_path {
            Some(p) => p,
            None => {
                warn!(
                    "フォルダパスを取得できなかったため表示を復元します (HWND: {})",
                    hwnd_val
                );
                window_controller::restore_window(new_hwnd, original_rect);
                let mut known = self.known_explorer_hwnds.lock().unwrap();
                known.insert(hwnd_val);
                return;
            }
        };

        info!("目的フォルダパス確定: '{}'", folder_path);

        if let Err(e) = uia_tab_creator::create_new_tab_via_uia(target_hwnd) {
            error!(
                "新規タブ作成エラー: {}. 表示復元 (HWND: {})",
                e, hwnd_val
            );
            window_controller::restore_window(new_hwnd, original_rect);
            let mut known = self.known_explorer_hwnds.lock().unwrap();
            known.insert(hwnd_val);
            return;
        }

        std::thread::sleep(Duration::from_millis(40));
        uia_tab_creator::activate_last_tab(target_hwnd);
        std::thread::sleep(Duration::from_millis(40));

        if let Err(e) = com_navigator::navigate_via_address_bar(target_hwnd, &folder_path) {
            error!(
                "アドレスバー遷移エラー: {}. 表示復元 (HWND: {})",
                e, hwnd_val
            );
            window_controller::restore_window(new_hwnd, original_rect);
            let mut known = self.known_explorer_hwnds.lock().unwrap();
            known.insert(hwnd_val);
            return;
        }

        info!(
            "タブ統合成功: HWND {} ('{}') -> HWND {}",
            hwnd_val, folder_path, target_hwnd.0
        );
        window_controller::close_window(new_hwnd);

        drop(processing_guard);
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
