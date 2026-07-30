use std::sync::{mpsc, Mutex};
use std::time::{Duration, Instant};
use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::Accessibility::{SetWinEventHook, UnhookWinEvent, HWINEVENTHOOK};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DeregisterShellHookWindow, RegisterClassW,
    RegisterShellHookWindow, RegisterWindowMessageW, EVENT_OBJECT_CREATE, HSHELL_WINDOWCREATED,
    WINEVENT_OUTOFCONTEXT, WNDCLASSW, WS_OVERLAPPEDWINDOW,
};

struct SendSender(mpsc::Sender<isize>);
unsafe impl Send for SendSender {}
unsafe impl Sync for SendSender {}

static HOOK_SENDER: Mutex<Option<SendSender>> = Mutex::new(None);
static SHELL_HOOK_MSG: Mutex<u32> = Mutex::new(0);
static LAST_SEEN_EVENT: Mutex<Option<(isize, Instant)>> = Mutex::new(None);

/// 同一 HWND への短時間（300ms）以内の二重通知をスキップするデバウンス判定
fn should_send_event(hwnd_val: isize) -> bool {
    if let Ok(mut guard) = LAST_SEEN_EVENT.lock() {
        let now = Instant::now();
        if let Some((last_hwnd, last_time)) = *guard {
            if last_hwnd == hwnd_val && now.duration_since(last_time) < Duration::from_millis(300) {
                return false;
            }
        }
        *guard = Some((hwnd_val, now));
    }
    true
}

/// # Safety
///
/// OS から WinEvent hook として呼び出されます。
pub unsafe extern "system" fn win_event_proc(
    _h_win_event_hook: HWINEVENTHOOK,
    _event: u32,
    hwnd: HWND,
    id_object: i32,
    id_child: i32,
    _dw_event_thread: u32,
    _dwms_event_time: u32,
) {
    if id_object == 0 && id_child == 0 && hwnd.0 != 0 {
        if crate::window_controller::is_explorer_window(hwnd) {
            if should_send_event(hwnd.0) {
                crate::info!("WinEventHook エクスプローラーイベント検知: HWND {}", hwnd.0);
                if let Ok(guard) = HOOK_SENDER.lock() {
                    if let Some(ref wrapper) = *guard {
                        let _ = wrapper.0.send(hwnd.0);
                    }
                }
            }
        }
    }
}

unsafe extern "system" fn shell_hook_window_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let shell_msg = if let Ok(guard) = SHELL_HOOK_MSG.lock() {
        *guard
    } else {
        0
    };

    if shell_msg != 0 && msg == shell_msg {
        let code = wparam.0 as u32;
        if code == HSHELL_WINDOWCREATED as u32 {
            let target_hwnd = HWND(lparam.0);
            if crate::window_controller::is_explorer_window(target_hwnd) {
                if should_send_event(target_hwnd.0) {
                    crate::info!(
                        "ShellHook エクスプローラーウィンドウ作成検知: HWND {}",
                        target_hwnd.0
                    );
                    if let Ok(guard) = HOOK_SENDER.lock() {
                        if let Some(ref wrapper) = *guard {
                            let _ = wrapper.0.send(target_hwnd.0);
                        }
                    }
                }
            }
        }
        LRESULT(0)
    } else {
        DefWindowProcW(hwnd, msg, wparam, lparam)
    }
}

pub struct WinEventHookManager {
    hook_create: HWINEVENTHOOK,
    shell_hook_hwnd: HWND,
}

impl WinEventHookManager {
    pub fn new(sender: mpsc::Sender<isize>) -> Result<Self, String> {
        {
            let mut guard = HOOK_SENDER
                .lock()
                .map_err(|e| format!("Failed to acquire lock for HOOK_SENDER: {}", e))?;
            *guard = Some(SendSender(sender));
        }

        let mut shell_hwnd = HWND::default();

        unsafe {
            let msg_id = RegisterWindowMessageW(w!("SHELLHOOK"));
            if msg_id != 0 {
                if let Ok(mut guard) = SHELL_HOOK_MSG.lock() {
                    *guard = msg_id;
                }

                let class_name: Vec<u16> = "TabifyShellHookClass\0".encode_utf16().collect();
                let wc = WNDCLASSW {
                    lpfnWndProc: Some(shell_hook_window_proc),
                    hInstance: windows::Win32::Foundation::HINSTANCE(0),
                    lpszClassName: windows::core::PCWSTR(class_name.as_ptr()),
                    ..Default::default()
                };
                let _ = RegisterClassW(&wc);

                shell_hwnd = CreateWindowExW(
                    windows::Win32::UI::WindowsAndMessaging::WINDOW_EX_STYLE(0),
                    windows::core::PCWSTR(class_name.as_ptr()),
                    windows::core::PCWSTR(class_name.as_ptr()),
                    WS_OVERLAPPEDWINDOW,
                    0,
                    0,
                    0,
                    0,
                    HWND::default(),
                    windows::Win32::UI::WindowsAndMessaging::HMENU::default(),
                    windows::Win32::Foundation::HINSTANCE(0),
                    None,
                );

                if shell_hwnd.0 != 0 {
                    let _ = RegisterShellHookWindow(shell_hwnd);
                    crate::info!("ShellHookWindow 登録成功: HWND {}", shell_hwnd.0);
                }
            }

            let hook_create = SetWinEventHook(
                EVENT_OBJECT_CREATE,
                EVENT_OBJECT_CREATE,
                None,
                Some(win_event_proc),
                0,
                0,
                WINEVENT_OUTOFCONTEXT,
            );

            Ok(Self {
                hook_create,
                shell_hook_hwnd: shell_hwnd,
            })
        }
    }
}

impl Drop for WinEventHookManager {
    fn drop(&mut self) {
        unsafe {
            if self.shell_hook_hwnd.0 != 0 {
                let _ = DeregisterShellHookWindow(self.shell_hook_hwnd);
            }
            if !self.hook_create.is_invalid() {
                let _ = UnhookWinEvent(self.hook_create);
            }
        }
        if let Ok(mut guard) = HOOK_SENDER.lock() {
            *guard = None;
        }
    }
}
