#![windows_subsystem = "windows"]

use std::sync::mpsc;
use std::sync::Arc;
use std::thread;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
use windows::Win32::System::Threading::{
    CreateMutexW, OpenProcess, TerminateProcess, PROCESS_TERMINATE,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, RegisterClassW,
    TranslateMessage, MSG, WM_COMMAND, WNDCLASSW, WS_OVERLAPPEDWINDOW,
};

use TabifyExplorer::detector::WinEventHookManager;
use TabifyExplorer::process_info;
use TabifyExplorer::tabify_engine::TabifyEngine;
use TabifyExplorer::{error, info, logger, tray, warn};

fn ensure_single_instance() -> Option<HANDLE> {
    let mutex_name: Vec<u16> = "Global\\TabifyExplorer_SingleInstance\0"
        .encode_utf16()
        .collect();
    unsafe {
        kill_previous_instances_fast();
        std::thread::sleep(std::time::Duration::from_millis(100));
        let handle = CreateMutexW(None, true, PCWSTR(mutex_name.as_ptr())).ok();
        handle
    }
}

fn kill_previous_instances_fast() {
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    let current_pid = std::process::id();
    unsafe {
        let snapshot = match CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) {
            Ok(h) => h,
            Err(_) => return,
        };
        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        if Process32FirstW(snapshot, &mut entry).is_ok() {
            loop {
                let name = String::from_utf16_lossy(
                    &entry.szExeFile[..entry
                        .szExeFile
                        .iter()
                        .position(|&c| c == 0)
                        .unwrap_or(entry.szExeFile.len())],
                );
                let name_lower = name.to_lowercase();
                if (name_lower.contains("tabify_explorer") || name_lower.contains("tabifyexplorer"))
                    && entry.th32ProcessID != current_pid
                {
                    if let Ok(proc_handle) =
                        OpenProcess(PROCESS_TERMINATE, false, entry.th32ProcessID)
                    {
                        let _ = TerminateProcess(proc_handle, 1);
                        let _ = CloseHandle(proc_handle);
                    }
                }
                if Process32NextW(snapshot, &mut entry).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snapshot);
    }
}

unsafe extern "system" fn tray_window_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if msg == tray::WM_TRAYICON {
        tray::handle_tray_message(hwnd, lparam);
        LRESULT(0)
    } else if msg == WM_COMMAND {
        tray::handle_menu_command(hwnd, wparam);
        LRESULT(0)
    } else {
        DefWindowProcW(hwnd, msg, wparam, lparam)
    }
}

fn create_tray_window() -> HWND {
    unsafe {
        let hinstance = windows::Win32::System::LibraryLoader::GetModuleHandleW(None).unwrap_or_default();
        let hinstance_hinstance = windows::Win32::Foundation::HINSTANCE(hinstance.0);
        let class_name: Vec<u16> = "TabifyTrayClass\0".encode_utf16().collect();
        let wc = WNDCLASSW {
            lpfnWndProc: Some(tray_window_proc),
            hInstance: hinstance_hinstance,
            lpszClassName: PCWSTR(class_name.as_ptr()),
            ..Default::default()
        };
        let atom = RegisterClassW(&wc);
        if atom == 0 {
            let err = windows::Win32::Foundation::GetLastError();
            warn!("RegisterClassW が 0 を返しました (atom=0, GetLastError={:?})", err);
        } else {
            info!("RegisterClassW 成功 (atom={})", atom);
        }

        let hwnd = CreateWindowExW(
            windows::Win32::UI::WindowsAndMessaging::WINDOW_EX_STYLE(0),
            PCWSTR(class_name.as_ptr()),
            PCWSTR(class_name.as_ptr()),
            WS_OVERLAPPEDWINDOW,
            0,
            0,
            0,
            0,
            HWND::default(),
            windows::Win32::UI::WindowsAndMessaging::HMENU::default(),
            hinstance_hinstance,
            None,
        );

        if hwnd.0 == 0 {
            let err = windows::Win32::Foundation::GetLastError();
            error!("CreateWindowExW 失敗: (hwnd=0, GetLastError={:?})", err);
        } else {
            info!("CreateWindowExW 成功: (hwnd={:?})", hwnd);
        }

        hwnd
    }
}

fn main() {
    std::panic::set_hook(Box::new(|info| {
        error!("Unhandled panic: {:?}", info);
        use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};
        let title: Vec<u16> = "TabifyExplorer 致命的エラー\0".encode_utf16().collect();
        let msg = format!("予期しないエラーが発生しました:\n{}\0", info);
        let msg_u16: Vec<u16> = msg.encode_utf16().collect();
        unsafe {
            MessageBoxW(
                None,
                PCWSTR(msg_u16.as_ptr()),
                PCWSTR(title.as_ptr()),
                MB_OK | MB_ICONERROR,
            );
        }
    }));

    logger::init_logger();
    info!("=== TabifyExplorer アプリケーション起動 ===");
    process_info::log_current_process_security_context();
    let _mutex_handle = ensure_single_instance();
    info!("ensure_single_instance 完了");

    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }

    let engine = Arc::new(TabifyEngine::new());
    engine.register_existing_windows();
    Arc::clone(&engine).start_background_watcher();

    let (tx, rx) = mpsc::channel::<isize>();

    let engine_clone = Arc::clone(&engine);
    thread::spawn(move || {
        while let Ok(hwnd_val) = rx.recv() {
            let engine = Arc::clone(&engine_clone);
            thread::spawn(move || {
                engine.process_window(hwnd_val);
            });
        }
    });

    let _hook_manager = match WinEventHookManager::new(tx) {
        Ok(manager) => Some(manager),
        Err(e) => {
            warn!("WinEventHookManager の初期化に失敗したため、バックグラウンドウォッチャーのみで常駐を継続します: {}", e);
            None
        }
    };

    let tray_hwnd = create_tray_window();
    if tray_hwnd.0 != 0 {
        info!("tray_hwnd 有効 ({:?})。add_tray_icon を呼び出します。", tray_hwnd);
        tray::add_tray_icon(tray_hwnd);
    } else {
        error!("tray_hwnd が無効 (0) のため、add_tray_icon をスキップしました。");
    }

    info!("Entering main Win32 message loop.");

    unsafe {
        let mut msg = MSG::default();
        loop {
            let res = GetMessageW(&mut msg, None, 0, 0);
            if res.0 == 0 || res.0 == -1 {
                info!("GetMessageW 終了メッセージ受領 (res={})", res.0);
                break;
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }

    if tray_hwnd.0 != 0 {
        tray::remove_tray_icon(tray_hwnd);
    }

    info!("TabifyExplorer shutting down cleanly.");
}
