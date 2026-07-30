use std::time::Duration;
use windows::core::{Interface, VARIANT};
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Gdi::{
    RedrawWindow, HRGN, RDW_ALLCHILDREN, RDW_INVALIDATE, RDW_UPDATENOW,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_APARTMENTTHREADED,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, VK_D,
    VK_MENU, VK_RETURN,
};
use windows::Win32::UI::Shell::{
    Folder2, IShellFolderViewDual, IShellWindows, IWebBrowser2, ShellWindows,
};
use windows::Win32::UI::WindowsAndMessaging::{GetAncestor, GA_ROOT};

use crate::info;
use crate::path_resolver::parse_location_url;

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
        std::thread::sleep(Duration::from_millis(15));

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
        std::thread::sleep(Duration::from_millis(20));

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
        std::thread::sleep(Duration::from_millis(20));

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
        std::thread::sleep(Duration::from_millis(20));

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

use windows::Win32::System::Com::IServiceProvider;
use windows::Win32::UI::Shell::{IFolderView, SID_STopLevelBrowser};

pub fn get_window_view_mode(target_hwnd: HWND) -> Option<u32> {
    if target_hwnd.0 == 0 {
        return None;
    }
    unsafe {
        let shell_windows: IShellWindows = CoCreateInstance(&ShellWindows, None, CLSCTX_ALL).ok()?;
        let count = shell_windows.Count().ok()?;

        for i in 0..count {
            let item = match shell_windows.Item(&VARIANT::from(i)) {
                Ok(item) => item,
                Err(_) => continue,
            };

            let web_browser: IWebBrowser2 = match item.cast() {
                Ok(wb) => wb,
                Err(_) => continue,
            };

            let hwnd_val = match web_browser.HWND() {
                Ok(h) => h.0 as isize,
                Err(_) => continue,
            };

            let root_hwnd = GetAncestor(HWND(hwnd_val), GA_ROOT);
            if root_hwnd == target_hwnd || HWND(hwnd_val) == target_hwnd {
                if let Ok(sp) = web_browser.cast::<IServiceProvider>() {
                    if let Ok(fv) = sp.QueryService::<IFolderView>(&SID_STopLevelBrowser) {
                        if let Ok(mode) = fv.GetCurrentViewMode() {
                            return Some(mode);
                        }
                    }
                }
            }
        }
    }
    None
}

pub fn apply_view_mode_to_window(target_hwnd: HWND, view_mode: u32) {
    if target_hwnd.0 == 0 {
        return;
    }
    unsafe {
        let shell_windows: IShellWindows = match CoCreateInstance(&ShellWindows, None, CLSCTX_ALL) {
            Ok(sw) => sw,
            Err(_) => return,
        };
        let count = match shell_windows.Count() {
            Ok(c) => c,
            Err(_) => return,
        };

        for i in 0..count {
            let item = match shell_windows.Item(&VARIANT::from(i)) {
                Ok(item) => item,
                Err(_) => continue,
            };

            let web_browser: IWebBrowser2 = match item.cast() {
                Ok(wb) => wb,
                Err(_) => continue,
            };

            let hwnd_val = match web_browser.HWND() {
                Ok(h) => h.0 as isize,
                Err(_) => continue,
            };

            let root_hwnd = GetAncestor(HWND(hwnd_val), GA_ROOT);
            if root_hwnd == target_hwnd || HWND(hwnd_val) == target_hwnd {
                if let Ok(sp) = web_browser.cast::<IServiceProvider>() {
                    if let Ok(fv) = sp.QueryService::<IFolderView>(&SID_STopLevelBrowser) {
                        let _ = fv.SetCurrentViewMode(view_mode);
                    }
                }
            }
        }
    }
}
