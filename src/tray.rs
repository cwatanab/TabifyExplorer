use windows::core::PCWSTR;
use windows::Win32::Foundation::{ERROR_ACCESS_DENIED, HWND, LPARAM, WPARAM};
use windows::Win32::UI::Shell::{
    ShellExecuteW, Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE,
    NIM_SETVERSION, NOTIFYICON_VERSION_4, NOTIFYICONDATAW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    ChangeWindowMessageFilterEx, CreatePopupMenu, DestroyMenu, GetCursorPos, InsertMenuW,
    MessageBoxW, PostQuitMessage, SetForegroundWindow, TrackPopupMenu, CHANGEFILTERSTRUCT,
    HICON, MB_ICONINFORMATION, MB_OK, MF_BYPOSITION, MF_CHECKED, MF_SEPARATOR, MF_STRING,
    MF_UNCHECKED, MSGFLT_ALLOW, SW_SHOWNORMAL, TPM_BOTTOMALIGN, TPM_LEFTALIGN, WM_RBUTTONUP,
};

pub const WM_TRAYICON: u32 = windows::Win32::UI::WindowsAndMessaging::WM_USER + 1;
pub const ID_TRAY_EXIT: usize = 1001;
pub const ID_TRAY_ABOUT: usize = 1002;
pub const ID_TRAY_LOG: usize = 1003;
pub const ID_TRAY_UNIFY_VIEW: usize = 1004;
pub const ID_TRAY_AUTO_START: usize = 1005;
pub const ID_TRAY_ENABLE_LOG: usize = 1006;

fn allow_tray_callback_message(hwnd: HWND) {
    unsafe {
        let mut filter = CHANGEFILTERSTRUCT {
            cbSize: std::mem::size_of::<CHANGEFILTERSTRUCT>() as u32,
            ..Default::default()
        };

        let res_ex = ChangeWindowMessageFilterEx(hwnd, WM_TRAYICON, MSGFLT_ALLOW, Some(&mut filter));
        let res_global = windows::Win32::UI::WindowsAndMessaging::ChangeWindowMessageFilter(
            WM_TRAYICON,
            windows::Win32::UI::WindowsAndMessaging::MSGFLT_ADD,
        );

        match (res_ex, res_global) {
            (Ok(()), _) => {
                crate::info!(
                    "ChangeWindowMessageFilterEx 成功: WM_TRAYICON を許可しました。(hwnd={:?})",
                    hwnd
                );
            }
            (Err(e), Ok(())) => {
                crate::info!(
                    "ChangeWindowMessageFilterEx は失敗 ({}) しましたが、ChangeWindowMessageFilter (MSGFLT_ADD) で WM_TRAYICON をグローバル許可しました。(hwnd={:?})",
                    e,
                    hwnd
                );
            }
            (Err(e1), Err(e2)) => {
                crate::warn!(
                    "WM_TRAYICON のメッセージフィルター設定に失敗しました。(Ex: {}, Global: {}, hwnd={:?})",
                    e1,
                    e2,
                    hwnd
                );
            }
        }
    }
}

pub fn add_tray_icon(hwnd: HWND) -> bool {
    unsafe {
        allow_tray_callback_message(hwnd);

        let mut nid = NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: hwnd,
            uID: 1,
            uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP,
            uCallbackMessage: WM_TRAYICON,
            ..Default::default()
        };

        let hinst =
            windows::Win32::System::LibraryLoader::GetModuleHandleW(None).unwrap_or_default();
        let mut hicon = HICON::default();

        // 1. Try LoadIconW from executable resource
        if let Ok(icon) = windows::Win32::UI::WindowsAndMessaging::LoadIconW(
            hinst,
            PCWSTR(1 as *const u16),
        ) {
            if !icon.is_invalid() {
                crate::info!("リソース ID 1 から LoadIconW でアイコンを取得しました。");
                hicon = icon;
            }
        }

        // 2. Try LoadImageW from executable resource
        if hicon.is_invalid() {
            #[allow(clippy::manual_dangling_ptr)]
            if let Ok(handle) = windows::Win32::UI::WindowsAndMessaging::LoadImageW(
                hinst,
                PCWSTR(1 as *const u16),
                windows::Win32::UI::WindowsAndMessaging::IMAGE_ICON,
                16,
                16,
                windows::Win32::UI::WindowsAndMessaging::LR_DEFAULTCOLOR,
            ) {
                if !handle.is_invalid() {
                    crate::info!("リソース ID 1 から LoadImageW でアイコンを取得しました。");
                    hicon = HICON(handle.0);
                }
            }
        }

        // 3. Fallback to app.ico file in exe dir or current dir
        if hicon.is_invalid() {
            let mut candidate_paths = Vec::new();
            if let Some(exe_dir) = crate::logger::get_exe_dir() {
                candidate_paths.push(exe_dir.join("app.ico"));
            }
            candidate_paths.push(std::path::PathBuf::from("app.ico"));

            for path_buf in candidate_paths {
                let path_str = path_buf.to_string_lossy().to_string() + "\0";
                let path: Vec<u16> = path_str.encode_utf16().collect();
                if let Ok(handle) = windows::Win32::UI::WindowsAndMessaging::LoadImageW(
                    windows::Win32::Foundation::HINSTANCE(0),
                    PCWSTR(path.as_ptr()),
                    windows::Win32::UI::WindowsAndMessaging::IMAGE_ICON,
                    0,
                    0,
                    windows::Win32::UI::WindowsAndMessaging::LR_DEFAULTSIZE
                        | windows::Win32::UI::WindowsAndMessaging::LR_SHARED
                        | windows::Win32::UI::WindowsAndMessaging::LR_LOADFROMFILE,
                ) {
                    if !handle.is_invalid() {
                        crate::info!(
                            "{} から LoadImageW でアイコンを取得しました。",
                            path_buf.display()
                        );
                        hicon = HICON(handle.0);
                        break;
                    }
                }
            }
        }

        // 4. Fallback to system default application icon
        if hicon.is_invalid() {
            crate::warn!("カスタムアイコンの読み込みに失敗したため、システム標準アイコンをフォールバックとして使用します。");
            hicon = windows::Win32::UI::WindowsAndMessaging::LoadIconW(
                windows::Win32::Foundation::HINSTANCE(0),
                windows::Win32::UI::WindowsAndMessaging::IDI_APPLICATION,
            )
            .unwrap_or_default();
        }

        nid.hIcon = hicon;

        let tip = "TabifyExplorer\0".encode_utf16().collect::<Vec<u16>>();
        for (i, &c) in tip.iter().enumerate().take(127) {
            nid.szTip[i] = c;
        }

        let mut success = false;
        let max_retries = 3;

        for attempt in 1..=max_retries {
            if attempt > 1 {
                allow_tray_callback_message(hwnd);
            }

            if Shell_NotifyIconW(NIM_ADD, &nid).as_bool() {
                success = true;
                if attempt > 1 {
                    crate::info!(
                        "トレイアイコンの追加 (NIM_ADD) に試行 {} 回目で成功しました。(hwnd={:?})",
                        attempt,
                        hwnd
                    );
                } else {
                    crate::info!("トレイアイコンの追加 (NIM_ADD) に成功しました。(hwnd={:?})", hwnd);
                }

                nid.Anonymous.uVersion = NOTIFYICON_VERSION_4;
                if Shell_NotifyIconW(NIM_SETVERSION, &nid).as_bool() {
                    crate::info!(
                        "トレイアイコンのバージョン設定 (NIM_SETVERSION / NOTIFYICON_VERSION_4) に成功しました。(hwnd={:?})",
                        hwnd
                    );
                } else {
                    let err = windows::Win32::Foundation::GetLastError();
                    crate::warn!(
                        "トレイアイコンのバージョン設定 (NIM_SETVERSION) に失敗しました。(hwnd={:?}, GetLastError={:?})",
                        hwnd,
                        err
                    );
                }
                break;
            } else {
                let err = windows::Win32::Foundation::GetLastError();
                if attempt < max_retries {
                    crate::warn!(
                        "トレイアイコンの追加 (NIM_ADD) 試行 {}/{} 失敗 (hwnd={:?}, GetLastError={:?})。{}ms 後にリトライします。",
                        attempt,
                        max_retries,
                        hwnd,
                        err,
                        150 * attempt
                    );
                    std::thread::sleep(std::time::Duration::from_millis(150 * attempt as u64));
                } else {
                    if err == ERROR_ACCESS_DENIED {
                        crate::error!(
                            "トレイアイコンの追加 (NIM_ADD) に最終失敗しました。(hwnd={:?}, GetLastError={:?})。ERROR_ACCESS_DENIED のため、Explorer と権限レベルが異なる状態（管理者実行など）で通知領域コールバックが拒否されている可能性があります。通常権限での起動を確認してください。",
                            hwnd,
                            err
                        );
                    } else {
                        crate::error!(
                            "トレイアイコンの追加 (NIM_ADD) に最終失敗しました。(hwnd={:?}, GetLastError={:?})",
                            hwnd,
                            err
                        );
                    }
                }
            }
        }

        success
    }
}

pub fn remove_tray_icon(hwnd: HWND) {
    unsafe {
        let nid = NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: hwnd,
            uID: 1,
            ..Default::default()
        };
        let _ = Shell_NotifyIconW(NIM_DELETE, &nid);
    }
}

use std::sync::Mutex;
use std::time::{Duration, Instant};

static LAST_MENU_TIME: Mutex<Option<Instant>> = Mutex::new(None);

pub fn handle_tray_message(hwnd: HWND, lparam: LPARAM) {
    let msg = (lparam.0 as u32) & 0xFFFF;

    const WM_CONTEXTMENU_U32: u32 = windows::Win32::UI::WindowsAndMessaging::WM_CONTEXTMENU;
    const NIN_SELECT: u32 = windows::Win32::UI::WindowsAndMessaging::WM_USER;
    const NIN_KEYSELECT: u32 = windows::Win32::UI::WindowsAndMessaging::WM_USER + 1;
    const TPM_RETURNCMD_VAL: windows::Win32::UI::WindowsAndMessaging::TRACK_POPUP_MENU_FLAGS =
        windows::Win32::UI::WindowsAndMessaging::TPM_RETURNCMD;

    let should_show_menu = matches!(
        msg,
        WM_RBUTTONUP
            | WM_CONTEXTMENU_U32
            | windows::Win32::UI::WindowsAndMessaging::WM_LBUTTONUP
            | NIN_SELECT
            | NIN_KEYSELECT
    );

    if should_show_menu {
        // 300ms 以内の連続メニュー表示を制御して二重ポップアップを防止
        if let Ok(mut guard) = LAST_MENU_TIME.lock() {
            let now = Instant::now();
            if let Some(last_time) = *guard {
                if now.duration_since(last_time) < Duration::from_millis(300) {
                    return;
                }
            }
            *guard = Some(now);
        }

        unsafe {
            let mut pt = windows::Win32::Foundation::POINT::default();
            let _ = GetCursorPos(&mut pt);

            let menu = CreatePopupMenu().unwrap_or_default();
            if !menu.is_invalid() {
                let unify_check_flag = if crate::config::is_unify_view_mode_enabled() {
                    MF_CHECKED
                } else {
                    MF_UNCHECKED
                };
                let auto_start_check_flag = if crate::config::is_auto_start_enabled() {
                    MF_CHECKED
                } else {
                    MF_UNCHECKED
                };
                let log_check_flag = if crate::config::is_log_enabled() {
                    MF_CHECKED
                } else {
                    MF_UNCHECKED
                };

                let _ = InsertMenuW(
                    menu,
                    0,
                    MF_BYPOSITION | MF_STRING | unify_check_flag,
                    ID_TRAY_UNIFY_VIEW,
                    windows::core::w!("表示形式を親ウィンドウに統一"),
                );
                let _ = InsertMenuW(
                    menu,
                    1,
                    MF_BYPOSITION | MF_STRING | auto_start_check_flag,
                    ID_TRAY_AUTO_START,
                    windows::core::w!("Windows 起動時に自動起動"),
                );
                let _ = InsertMenuW(
                    menu,
                    2,
                    MF_BYPOSITION | MF_SEPARATOR,
                    0,
                    windows::core::w!(""),
                );
                let _ = InsertMenuW(
                    menu,
                    3,
                    MF_BYPOSITION | MF_STRING | log_check_flag,
                    ID_TRAY_ENABLE_LOG,
                    windows::core::w!("ログ出力を有効化"),
                );
                let _ = InsertMenuW(
                    menu,
                    4,
                    MF_BYPOSITION | MF_STRING,
                    ID_TRAY_LOG,
                    windows::core::w!("ログファイルを開く"),
                );
                let _ = InsertMenuW(
                    menu,
                    5,
                    MF_BYPOSITION | MF_SEPARATOR,
                    0,
                    windows::core::w!(""),
                );
                let _ = InsertMenuW(
                    menu,
                    6,
                    MF_BYPOSITION | MF_STRING,
                    ID_TRAY_ABOUT,
                    windows::core::w!("バージョン情報"),
                );
                let _ = InsertMenuW(
                    menu,
                    7,
                    MF_BYPOSITION | MF_SEPARATOR,
                    0,
                    windows::core::w!(""),
                );
                let _ = InsertMenuW(
                    menu,
                    8,
                    MF_BYPOSITION | MF_STRING,
                    ID_TRAY_EXIT,
                    windows::core::w!("終了"),
                );

                let _ = SetForegroundWindow(hwnd);
                let cmd = TrackPopupMenu(
                    menu,
                    TPM_BOTTOMALIGN | TPM_LEFTALIGN | TPM_RETURNCMD_VAL,
                    pt.x,
                    pt.y,
                    0,
                    hwnd,
                    None,
                );
                let _ = DestroyMenu(menu);

                if cmd.0 != 0 {
                    handle_menu_command(hwnd, WPARAM(cmd.0 as usize));
                }
            }
        }
    }
}

pub fn handle_menu_command(hwnd: HWND, wparam: WPARAM) {
    let id = wparam.0 & 0xFFFF;
    match id {
        ID_TRAY_UNIFY_VIEW => {
            let enabled = crate::config::toggle_unify_view_mode();
            crate::info!("表示形式統一モードを変更しました: enabled={}", enabled);
        }
        ID_TRAY_AUTO_START => {
            let enabled = crate::config::toggle_auto_start();
            crate::info!("Windows 自動起動モードを変更しました: enabled={}", enabled);
        }
        ID_TRAY_ENABLE_LOG => {
            let enabled = crate::config::toggle_log_enabled();
            crate::info!("ログ出力モードを変更しました: enabled={}", enabled);
        }
        ID_TRAY_EXIT => unsafe {
            PostQuitMessage(0);
        },
        ID_TRAY_ABOUT => {
            unsafe {
                MessageBoxW(
                    hwnd,
                    windows::core::w!("TabifyExplorer v0.1.0 (Rust - Optimized)\n\nWindows 11 の新規エクスプローラーを既存ウィンドウのタブへ自動統合する常駐ツールです。"),
                    windows::core::w!("バージョン情報"),
                    MB_OK | MB_ICONINFORMATION,
                );
            }
        }
        ID_TRAY_LOG => {
            if let Some(log_path) = crate::logger::get_log_path() {
                if !log_path.exists() {
                    let _ = std::fs::File::create(&log_path);
                }
                let path_str = log_path.to_string_lossy().to_string() + "\0";
                let path_u16: Vec<u16> = path_str.encode_utf16().collect();

                unsafe {
                    let res = ShellExecuteW(
                        hwnd,
                        windows::core::w!("open"),
                        PCWSTR(path_u16.as_ptr()),
                        PCWSTR(std::ptr::null()),
                        PCWSTR(std::ptr::null()),
                        SW_SHOWNORMAL,
                    );
                    crate::info!(
                        "ログファイルのオープン (ShellExecuteW) を実行しました。(path={}, res={:?})",
                        log_path.display(),
                        res
                    );
                }
            }
        }
        _ => {}
    }
}
