use std::thread;
use std::time::Duration;
use windows::Win32::Foundation::{BOOL, HWND};
use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, SetFocus, INPUT, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, VK_CONTROL, VK_9,
    VK_T,
};
use windows::Win32::UI::WindowsAndMessaging::{GetWindowThreadProcessId, SetForegroundWindow};

use crate::info;

use windows::core::{Interface, VARIANT};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED,
};
use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, IUIAutomationInvokePattern, TreeScope_Subtree,
    UIA_AutomationIdPropertyId, UIA_InvokePatternId, UIA_NamePropertyId,
};

/// 指定したエクスプローラーウィンドウ (target_hwnd) に UIA / Ctrl+T でタブを追加します。
pub fn create_new_tab_via_uia(target_hwnd: HWND) -> Result<(), String> {
    if target_hwnd.0 == 0 {
        return Err("Target HWND is invalid (0)".to_string());
    }

    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        if let Ok(automation) =
            CoCreateInstance::<_, IUIAutomation>(&CUIAutomation, None, CLSCTX_INPROC_SERVER)
        {
            if let Ok(root_elem) = automation.ElementFromHandle(target_hwnd) {
                // 1. AutomationId "AddButton" で試行
                let bstr_id = windows::core::BSTR::from("AddButton");
                let var_id = VARIANT::from(bstr_id);
                if let Ok(cond) =
                    automation.CreatePropertyCondition(UIA_AutomationIdPropertyId, &var_id)
                {
                    if let Ok(elem) = root_elem.FindFirst(TreeScope_Subtree, &cond) {
                        if let Ok(pattern_unk) = elem.GetCurrentPattern(UIA_InvokePatternId) {
                            if let Ok(invoke_pattern) =
                                pattern_unk.cast::<IUIAutomationInvokePattern>()
                            {
                                if invoke_pattern.Invoke().is_ok() {
                                    info!(
                                        "UIA InvokePattern 経由で 'AddButton' のクリックに成功しました (HWND: {})",
                                        target_hwnd.0
                                    );
                                    return Ok(());
                                }
                            }
                        }
                    }
                }

                // 2. Name "新しいタブ" または "New tab" で試行
                for name in &["新しいタブ", "New tab", "新しいタブを追加", "Add new tab"] {
                    let bstr_name = windows::core::BSTR::from(*name);
                    let var_name = VARIANT::from(bstr_name);
                    if let Ok(cond) =
                        automation.CreatePropertyCondition(UIA_NamePropertyId, &var_name)
                    {
                        if let Ok(elem) = root_elem.FindFirst(TreeScope_Subtree, &cond) {
                            if let Ok(pattern_unk) = elem.GetCurrentPattern(UIA_InvokePatternId) {
                                if let Ok(invoke_pattern) =
                                    pattern_unk.cast::<IUIAutomationInvokePattern>()
                                {
                                    if invoke_pattern.Invoke().is_ok() {
                                        info!(
                                            "UIA InvokePattern 経由で '{}' ボタンのクリックに成功しました (HWND: {})",
                                            name, target_hwnd.0
                                        );
                                        return Ok(());
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    info!(
        "UIA ボタン探査から Ctrl+T ショートカットキー送信へフォールバックします (HWND: {})",
        target_hwnd.0
    );
    send_ctrl_t(target_hwnd);
    Ok(())
}

pub fn release_modifier_keys() {
    unsafe {
        use windows::Win32::UI::Input::KeyboardAndMouse::{
            GetAsyncKeyState, VK_LWIN, VK_MENU, VK_RWIN,
        };

        let mut release_inputs = Vec::new();

        if (GetAsyncKeyState(VK_LWIN.0 as i32) as u16 & 0x8000) != 0 {
            release_inputs.push(INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: windows::Win32::UI::Input::KeyboardAndMouse::INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: VK_LWIN,
                        dwFlags: KEYEVENTF_KEYUP,
                        ..Default::default()
                    },
                },
            });
        }
        if (GetAsyncKeyState(VK_RWIN.0 as i32) as u16 & 0x8000) != 0 {
            release_inputs.push(INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: windows::Win32::UI::Input::KeyboardAndMouse::INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: VK_RWIN,
                        dwFlags: KEYEVENTF_KEYUP,
                        ..Default::default()
                    },
                },
            });
        }
        if (GetAsyncKeyState(VK_MENU.0 as i32) as u16 & 0x8000) != 0 {
            release_inputs.push(INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: windows::Win32::UI::Input::KeyboardAndMouse::INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: VK_MENU,
                        dwFlags: KEYEVENTF_KEYUP,
                        ..Default::default()
                    },
                },
            });
        }

        if !release_inputs.is_empty() {
            SendInput(&release_inputs, std::mem::size_of::<INPUT>() as i32);
            thread::sleep(Duration::from_millis(20));
        }
    }
}

pub fn send_ctrl_t(target_hwnd: HWND) {
    unsafe {
        release_modifier_keys();

        let current_thread_id = GetCurrentThreadId();
        let target_thread_id = GetWindowThreadProcessId(target_hwnd, None);

        if target_thread_id != 0 && current_thread_id != target_thread_id {
            let _ = AttachThreadInput(current_thread_id, target_thread_id, BOOL(1));
        }

        let _ = SetForegroundWindow(target_hwnd);
        let _ = SetFocus(target_hwnd);
        thread::sleep(Duration::from_millis(30));

        let inputs = [
            INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: windows::Win32::UI::Input::KeyboardAndMouse::INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: VK_CONTROL,
                        ..Default::default()
                    },
                },
            },
            INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: windows::Win32::UI::Input::KeyboardAndMouse::INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: VK_T,
                        ..Default::default()
                    },
                },
            },
            INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: windows::Win32::UI::Input::KeyboardAndMouse::INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: VK_T,
                        dwFlags: KEYEVENTF_KEYUP,
                        ..Default::default()
                    },
                },
            },
            INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: windows::Win32::UI::Input::KeyboardAndMouse::INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: VK_CONTROL,
                        dwFlags: KEYEVENTF_KEYUP,
                        ..Default::default()
                    },
                },
            },
        ];
        SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
        thread::sleep(Duration::from_millis(30));

        if target_thread_id != 0 && current_thread_id != target_thread_id {
            let _ = AttachThreadInput(current_thread_id, target_thread_id, BOOL(0));
        }
    }
}

/// 一番右端（最新）のタブをアクティブ選択状態にします (Ctrl + 9)。
pub fn activate_last_tab(target_hwnd: HWND) {
    unsafe {
        let current_thread_id = GetCurrentThreadId();
        let target_thread_id = GetWindowThreadProcessId(target_hwnd, None);

        if target_thread_id != 0 && current_thread_id != target_thread_id {
            let _ = AttachThreadInput(current_thread_id, target_thread_id, BOOL(1));
        }

        let _ = SetForegroundWindow(target_hwnd);
        let _ = SetFocus(target_hwnd);
        thread::sleep(Duration::from_millis(30));

        let inputs = [
            INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: windows::Win32::UI::Input::KeyboardAndMouse::INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: VK_CONTROL,
                        ..Default::default()
                    },
                },
            },
            INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: windows::Win32::UI::Input::KeyboardAndMouse::INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: VK_9,
                        ..Default::default()
                    },
                },
            },
            INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: windows::Win32::UI::Input::KeyboardAndMouse::INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: VK_9,
                        dwFlags: KEYEVENTF_KEYUP,
                        ..Default::default()
                    },
                },
            },
            INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: windows::Win32::UI::Input::KeyboardAndMouse::INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: VK_CONTROL,
                        dwFlags: KEYEVENTF_KEYUP,
                        ..Default::default()
                    },
                },
            },
        ];
        SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
        thread::sleep(Duration::from_millis(30));

        if target_thread_id != 0 && current_thread_id != target_thread_id {
            let _ = AttachThreadInput(current_thread_id, target_thread_id, BOOL(0));
        }

        info!(
            "Ctrl+9 ショートカットキーを送信し、最新タブをアクティブ化しました (HWND: {})",
            target_hwnd.0
        );
    }
}
