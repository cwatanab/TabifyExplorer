use std::time::{Duration, Instant};
use windows::core::{Interface, GUID, PCWSTR, VARIANT};
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Gdi::{
    RedrawWindow, HRGN, RDW_ALLCHILDREN, RDW_INVALIDATE, RDW_UPDATENOW,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, IServiceProvider, CLSCTX_ALL,
    COINIT_APARTMENTTHREADED,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, VK_D,
    VK_MENU, VK_RETURN,
};
use windows::Win32::UI::Shell::{
    Folder2, IShellBrowser, IShellFolderViewDual, IShellWindows, IWebBrowser2, SHParseDisplayName,
    ShellWindows, SBSP_ABSOLUTE, SBSP_SAMEBROWSER,
};
use windows::Win32::UI::WindowsAndMessaging::{GetAncestor, GA_ROOT};

use crate::path_resolver::{are_paths_equivalent, parse_location_url};
use crate::{info, warn};

const SID_S_TOP_LEVEL_BROWSER: GUID = GUID::from_u128(0x4c96be40_915c_11cf_99d3_00aa004fe881);

/// 指定 HWND の最上位ルートウィンドウ HWND を取得します。
pub fn get_root_hwnd(hwnd: HWND) -> HWND {
    if hwnd.0 == 0 {
        return hwnd;
    }
    let root = unsafe { GetAncestor(hwnd, GA_ROOT) };
    if root.0 == 0 {
        hwnd
    } else {
        root
    }
}

/// 指定したエクスプローラーウィンドウ (target_hwnd) の現在のフォルダパスを COM 経由で取得します。
pub fn get_window_path(target_hwnd: HWND) -> Option<String> {
    if target_hwnd.0 == 0 {
        return None;
    }
    let target_root = get_root_hwnd(target_hwnd);

    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let shell_windows: IShellWindows =
            CoCreateInstance(&ShellWindows, None, CLSCTX_ALL).ok()?;
        let count = shell_windows.Count().ok()?;

        for i in 0..count {
            let var = VARIANT::from(i);
            let dispatch = match shell_windows.Item(&var) {
                Ok(d) => d,
                Err(_) => continue,
            };

            let browser: IWebBrowser2 = match dispatch.cast() {
                Ok(b) => b,
                Err(_) => continue,
            };

            let hwnd_val = match browser.HWND() {
                Ok(h) => h,
                Err(_) => continue,
            };

            let browser_root = get_root_hwnd(HWND(hwnd_val.0));

            if hwnd_val.0 as isize == target_hwnd.0
                || browser_root.0 as isize == target_root.0
                || browser_root.0 as isize == target_hwnd.0
                || hwnd_val.0 as isize == target_root.0
            {
                // 1. Document (IShellFolderViewDual) から実際のフォルダパスを取得 (最優先)
                if let Ok(doc_dispatch) = browser.Document() {
                    if let Ok(folder_view) = doc_dispatch.cast::<IShellFolderViewDual>() {
                        if let Ok(folder) = folder_view.Folder() {
                            if let Ok(folder2) = folder.cast::<Folder2>() {
                                if let Ok(folder_item) = folder2.Self_() {
                                    if let Ok(bstr_path) = folder_item.Path() {
                                        let path_str = bstr_path.to_string();
                                        if !path_str.is_empty() {
                                            return Some(path_str);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                // 2. LocationURL からのパース取得
                let location_url = browser.LocationURL().ok().unwrap_or_default();
                let location_str = location_url.to_string();

                if !location_str.is_empty() {
                    if let Some(parsed) = parse_location_url(&location_str) {
                        return Some(parsed);
                    }
                }
            }
        }
    }

    None
}

use std::sync::atomic::{AtomicUsize, Ordering};
static NEXT_SESSION_ID: AtomicUsize = AtomicUsize::new(1);

/// target_hwnd に属する全タブにセッション固有の ID を付与し、既存タブをマーキングします。
pub fn snapshot_tab_ids(target_hwnd: HWND) -> usize {
    let session_id = NEXT_SESSION_ID.fetch_add(1, Ordering::SeqCst);
    if target_hwnd.0 == 0 {
        return session_id;
    }
    let target_root = get_root_hwnd(target_hwnd);
    let bstr_key = windows::core::BSTR::from("Tabify_SessionID");
    let var_val = VARIANT::from(session_id as i32);

    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        if let Ok(shell_windows) =
            CoCreateInstance::<_, IShellWindows>(&ShellWindows, None, CLSCTX_ALL)
        {
            if let Ok(count) = shell_windows.Count() {
                for i in 0..count {
                    let var = VARIANT::from(i);
                    if let Ok(dispatch) = shell_windows.Item(&var) {
                        if let Ok(browser) = dispatch.cast::<IWebBrowser2>() {
                            if let Ok(hwnd_val) = browser.HWND() {
                                let browser_root = get_root_hwnd(HWND(hwnd_val.0));
                                if hwnd_val.0 as isize == target_hwnd.0
                                    || browser_root.0 as isize == target_root.0
                                    || browser_root.0 as isize == target_hwnd.0
                                    || hwnd_val.0 as isize == target_root.0
                                {
                                    let _ = browser.PutProperty(&bstr_key, &var_val);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    session_id
}

/// UIA で新規タブ作成後、IShellWindows をリトライ監視して新しく追加された IWebBrowser2 タブを取得します。
pub fn find_new_tab_browser(
    target_hwnd: HWND,
    current_session_id: usize,
    timeout_ms: u64,
) -> Option<IWebBrowser2> {
    let target_root = get_root_hwnd(target_hwnd);
    let start = Instant::now();
    let timeout = Duration::from_millis(timeout_ms);
    let bstr_key = windows::core::BSTR::from("Tabify_SessionID");

    loop {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            if let Ok(shell_windows) =
                CoCreateInstance::<_, IShellWindows>(&ShellWindows, None, CLSCTX_ALL)
            {
                if let Ok(count) = shell_windows.Count() {
                    for i in (0..count).rev() {
                        let var = VARIANT::from(i);
                        if let Ok(dispatch) = shell_windows.Item(&var) {
                            if let Ok(browser) = dispatch.cast::<IWebBrowser2>() {
                                if let Ok(hwnd_val) = browser.HWND() {
                                    let browser_root = get_root_hwnd(HWND(hwnd_val.0));
                                    if hwnd_val.0 as isize == target_hwnd.0
                                        || browser_root.0 as isize == target_root.0
                                        || browser_root.0 as isize == target_hwnd.0
                                        || hwnd_val.0 as isize == target_root.0
                                    {
                                        let tab_session_id = match browser.GetProperty(&bstr_key) {
                                            Ok(var_res) => {
                                                i32::try_from(&var_res).unwrap_or(-1) as usize
                                            }
                                            Err(_) => 0,
                                        };

                                        if tab_session_id != current_session_id {
                                            info!(
                                                "新しく追加されたタブの IWebBrowser2 を確実に検出しました (HWND: {}, Index: {}, Session: {})",
                                                hwnd_val.0, i, current_session_id
                                            );
                                            let _ = browser.PutProperty(
                                                &bstr_key,
                                                &VARIANT::from(current_session_id as i32),
                                            );
                                            return Some(browser);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        if start.elapsed() >= timeout {
            break;
        }
        std::thread::sleep(Duration::from_millis(15));
    }

    warn!(
        "新規タブの IWebBrowser2 検出タイムアウト (HWND: {}, Session: {})",
        target_hwnd.0, current_session_id
    );
    None
}

/// アドレスバー操作 (Alt+D -> Unicode パス送信 -> Enter) で指定 HWND のエクスプローラーをフォルダへ遷移させます。
pub fn navigate_via_address_bar(target_hwnd: HWND, target_path: &str) -> Result<(), String> {
    unsafe {
        use windows::Win32::Foundation::BOOL;
        use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
        use windows::Win32::UI::Input::KeyboardAndMouse::SetFocus;
        use windows::Win32::UI::WindowsAndMessaging::{
            GetWindowThreadProcessId, SetForegroundWindow,
        };

        crate::uia_tab_creator::release_modifier_keys();

        let current_thread_id = GetCurrentThreadId();
        let target_thread_id = GetWindowThreadProcessId(target_hwnd, None);

        if target_thread_id != 0 && current_thread_id != target_thread_id {
            let _ = AttachThreadInput(current_thread_id, target_thread_id, BOOL(1));
        }

        let _ = SetForegroundWindow(target_hwnd);
        let _ = SetFocus(target_hwnd);
        std::thread::sleep(Duration::from_millis(30));

        // 1. Alt + D (アドレスバー選択)
        let inputs_alt_d = [
            INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: windows::Win32::UI::Input::KeyboardAndMouse::INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: VK_MENU,
                        ..Default::default()
                    },
                },
            },
            INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: windows::Win32::UI::Input::KeyboardAndMouse::INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: VK_D,
                        ..Default::default()
                    },
                },
            },
            INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: windows::Win32::UI::Input::KeyboardAndMouse::INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: VK_D,
                        dwFlags: KEYEVENTF_KEYUP,
                        ..Default::default()
                    },
                },
            },
            INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: windows::Win32::UI::Input::KeyboardAndMouse::INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: VK_MENU,
                        dwFlags: KEYEVENTF_KEYUP,
                        ..Default::default()
                    },
                },
            },
        ];
        SendInput(&inputs_alt_d, std::mem::size_of::<INPUT>() as i32);
        std::thread::sleep(Duration::from_millis(50));

        // 2. パス文字列を KEYEVENTF_UNICODE で送信
        let utf16_chars: Vec<u16> = target_path.encode_utf16().collect();
        let mut key_inputs = Vec::with_capacity(utf16_chars.len() * 2);

        for &ch in &utf16_chars {
            key_inputs.push(INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: windows::Win32::UI::Input::KeyboardAndMouse::INPUT_0 {
                    ki: KEYBDINPUT {
                        wScan: ch,
                        dwFlags: KEYEVENTF_UNICODE,
                        ..Default::default()
                    },
                },
            });
            key_inputs.push(INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: windows::Win32::UI::Input::KeyboardAndMouse::INPUT_0 {
                    ki: KEYBDINPUT {
                        wScan: ch,
                        dwFlags: KEYEVENTF_UNICODE | KEYEVENTF_KEYUP,
                        ..Default::default()
                    },
                },
            });
        }
        SendInput(&key_inputs, std::mem::size_of::<INPUT>() as i32);
        std::thread::sleep(Duration::from_millis(50));

        // 3. Enter キーを送信
        let inputs_enter = [
            INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: windows::Win32::UI::Input::KeyboardAndMouse::INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: VK_RETURN,
                        ..Default::default()
                    },
                },
            },
            INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: windows::Win32::UI::Input::KeyboardAndMouse::INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: VK_RETURN,
                        dwFlags: KEYEVENTF_KEYUP,
                        ..Default::default()
                    },
                },
            },
        ];
        SendInput(&inputs_enter, std::mem::size_of::<INPUT>() as i32);
        std::thread::sleep(Duration::from_millis(50));

        let _ = RedrawWindow(
            target_hwnd,
            None,
            HRGN::default(),
            RDW_INVALIDATE | RDW_UPDATENOW | RDW_ALLCHILDREN,
        );

        if target_thread_id != 0 && current_thread_id != target_thread_id {
            let _ = AttachThreadInput(current_thread_id, target_thread_id, BOOL(0));
        }
    }

    info!(
        "アドレスバー入力経由でのフォルダ遷移を完了しました: '{}'",
        target_path
    );
    Ok(())
}

/// IShellBrowser::BrowseObject または IWebBrowser2.Navigate を呼び出し、目的のフォルダパス文字列へダイレクト遷移させます。
pub fn navigate_browser(
    target_hwnd: HWND,
    browser: &IWebBrowser2,
    target_path: &str,
) -> Result<(), String> {
    // 1. 新規タブの COM オブジェクトが初期化されるまで待機 (ReadyState)
    for _ in 0..15 {
        if let Ok(state) = unsafe { browser.ReadyState() } {
            if state.0 >= 3 {
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(15));
    }

    // 2. IShellBrowser::BrowseObject 遷移試行
    if let Ok(sp) = browser.cast::<IServiceProvider>() {
        if let Ok(shell_browser) =
            unsafe { sp.QueryService::<IShellBrowser>(&SID_S_TOP_LEVEL_BROWSER) }
        {
            let path_wide: Vec<u16> = target_path
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect();
            let mut pidl = std::ptr::null_mut();
            let hr = unsafe {
                SHParseDisplayName(PCWSTR(path_wide.as_ptr()), None, &mut pidl, 0, None)
            };

            if hr.is_ok() && !pidl.is_null() {
                let browse_res = unsafe {
                    shell_browser
                        .BrowseObject(pidl as _, (SBSP_SAMEBROWSER | SBSP_ABSOLUTE) as u32)
                };
                unsafe { CoTaskMemFree(Some(pidl as _)) };

                if browse_res.is_ok() {
                    info!("IShellBrowser::BrowseObject 遷移成功 ('{}')", target_path);
                    if check_single_browser_has_path(browser, target_path) {
                        return Ok(());
                    }
                }
            }
        }
    }

    // 3. IWebBrowser2::Navigate2 遷移試行
    let empty = VARIANT::default();
    let bstr_url = windows::core::BSTR::from(target_path);
    let var_url = VARIANT::from(bstr_url);

    for attempt in 1..=3 {
        let _ = unsafe {
            browser.Navigate2(
                &var_url as *const _,
                Some(&empty as *const _),
                Some(&empty as *const _),
                Some(&empty as *const _),
                Some(&empty as *const _),
            )
        };
        std::thread::sleep(Duration::from_millis(30));
        if check_single_browser_has_path(browser, target_path) {
            info!("IWebBrowser2::Navigate2 遷移成功 ('{}', 試行: {})", target_path, attempt);
            return Ok(());
        }
    }

    info!(
        "新規タブ COM 遷移でのパス変更が確認できなかったため、アドレスバー入力でフォールバック遷移します: '{}'",
        target_path
    );
    navigate_via_address_bar(target_hwnd, target_path)
}

/// 特定の IWebBrowser2 インスタンスが目的のパスに遷移したか直接確認します。
fn check_single_browser_has_path(browser: &IWebBrowser2, target_path: &str) -> bool {
    for _ in 0..10 {
        if let Ok(doc_disp) = unsafe { browser.Document() } {
            if let Ok(folder_view) = doc_disp.cast::<IShellFolderViewDual>() {
                if let Ok(folder) = unsafe { folder_view.Folder() } {
                    if let Ok(folder2) = folder.cast::<Folder2>() {
                        if let Ok(folder_item) = unsafe { folder2.Self_() } {
                            if let Ok(bstr_path) = unsafe { folder_item.Path() } {
                                let path_str = bstr_path.to_string();
                                if are_paths_equivalent(&path_str, target_path) {
                                    return true;
                                }
                            }
                        }
                    }
                }
            }
        }
        if let Ok(loc_bstr) = unsafe { browser.LocationURL() } {
            let loc_str = loc_bstr.to_string();
            if !loc_str.is_empty() {
                if let Some(parsed) = parse_location_url(&loc_str) {
                    if are_paths_equivalent(&parsed, target_path) {
                        return true;
                    }
                }
            }
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}

/// target_hwnd に属する全てのタブのフォルダパス文字列のリストを取得します。
pub fn get_all_window_paths(target_hwnd: HWND) -> Vec<String> {
    let mut paths = Vec::new();
    if target_hwnd.0 == 0 {
        return paths;
    }
    let target_root = get_root_hwnd(target_hwnd);

    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        if let Ok(shell_windows) =
            CoCreateInstance::<_, IShellWindows>(&ShellWindows, None, CLSCTX_ALL)
        {
            if let Ok(count) = shell_windows.Count() {
                for i in 0..count {
                    let var = VARIANT::from(i);
                    if let Ok(dispatch) = shell_windows.Item(&var) {
                        if let Ok(browser) = dispatch.cast::<IWebBrowser2>() {
                            if let Ok(hwnd_val) = browser.HWND() {
                                let browser_root = get_root_hwnd(HWND(hwnd_val.0));
                                if hwnd_val.0 as isize == target_hwnd.0
                                    || browser_root.0 as isize == target_root.0
                                    || browser_root.0 as isize == target_hwnd.0
                                    || hwnd_val.0 as isize == target_root.0
                                {
                                    let mut path_found = false;
                                    if let Ok(doc_dispatch) = browser.Document() {
                                        if let Ok(folder_view) =
                                            doc_dispatch.cast::<IShellFolderViewDual>()
                                        {
                                            if let Ok(folder) = folder_view.Folder() {
                                                if let Ok(folder2) = folder.cast::<Folder2>() {
                                                    if let Ok(folder_item) = folder2.Self_() {
                                                        if let Ok(bstr_path) = folder_item.Path() {
                                                            let path_str = bstr_path.to_string();
                                                            if !path_str.is_empty() {
                                                                paths.push(path_str);
                                                                path_found = true;
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }

                                    if !path_found {
                                        let location_url =
                                            browser.LocationURL().ok().unwrap_or_default();
                                        let location_str = location_url.to_string();
                                        if !location_str.is_empty() {
                                            if let Some(parsed) = parse_location_url(&location_str)
                                            {
                                                paths.push(parsed);
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
    }
    paths
}

/// target_hwnd のいずれかのタブで expected_path が開かれているか検証します。
pub fn verify_target_has_path(target_hwnd: HWND, expected_path: &str, timeout_ms: u64) -> bool {
    let start = Instant::now();
    let timeout = Duration::from_millis(timeout_ms);

    loop {
        let current_paths = get_all_window_paths(target_hwnd);
        for path in &current_paths {
            if are_paths_equivalent(path, expected_path) {
                return true;
            }
        }

        if start.elapsed() >= timeout {
            break;
        }

        std::thread::sleep(Duration::from_millis(10));
    }

    false
}
