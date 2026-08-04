use std::thread;
use std::time::Duration;
use windows::Win32::Foundation::{BOOL, HWND};
use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, SetFocus, INPUT, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, VK_CONTROL, VK_9,
};
use windows::Win32::UI::WindowsAndMessaging::{GetWindowThreadProcessId, SetForegroundWindow};

use crate::info;

use windows::core::{Interface, VARIANT};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED,
};
use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, IUIAutomationElement, IUIAutomationInvokePattern, IUIAutomationSelectionItemPattern, TreeScope_Subtree,
    UIA_AutomationIdPropertyId, UIA_InvokePatternId, UIA_NamePropertyId, UIA_SelectionItemPatternId, UIA_ControlTypePropertyId, UIA_TabItemControlTypeId
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

    Err(format!(
        "UIA '新しいタブ' ボタンの検出・クリックに失敗しました (HWND: {})",
        target_hwnd.0
    ))
}

/// 指定したエクスプローラーウィンドウ内のすべてのタブ要素 (UIA) を取得します。
pub fn get_uia_tab_items(target_hwnd: HWND) -> Vec<IUIAutomationElement> {
    let mut items = Vec::new();
    unsafe {
        if let Ok(automation) = CoCreateInstance::<_, IUIAutomation>(&CUIAutomation, None, CLSCTX_INPROC_SERVER) {
            if let Ok(root) = automation.ElementFromHandle(target_hwnd) {
                let var_type = VARIANT::from(UIA_TabItemControlTypeId.0);
                if let Ok(cond) = automation.CreatePropertyCondition(UIA_ControlTypePropertyId, &var_type) {
                    if let Ok(array) = root.FindAll(TreeScope_Subtree, &cond) {
                        if let Ok(len) = array.Length() {
                            for i in 0..len {
                                if let Ok(elem) = array.GetElement(i) {
                                    items.push(elem);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    items
}

/// 指定した名前のタブを UIA 経由でアクティブ化します。
pub fn activate_tab_by_name(target_hwnd: HWND, tab_name: &str) -> Result<(), String> {
    let items = get_uia_tab_items(target_hwnd);
    for elem in items {
        unsafe {
            if let Ok(name_bstr) = elem.CurrentName() {
                let name = name_bstr.to_string();
                if name == tab_name {
                    // Try SelectionItemPattern first
                    if let Ok(pattern_unk) = elem.GetCurrentPattern(UIA_SelectionItemPatternId) {
                        if let Ok(sel_pattern) = pattern_unk.cast::<IUIAutomationSelectionItemPattern>() {
                            if sel_pattern.Select().is_ok() {
                                info!("UIA SelectionItemPattern 経由でタブ '{}' をアクティブ化しました (HWND: {})", tab_name, target_hwnd.0);
                                return Ok(());
                            }
                        }
                    }
                    // Fallback to InvokePattern
                    if let Ok(pattern_unk) = elem.GetCurrentPattern(UIA_InvokePatternId) {
                        if let Ok(inv_pattern) = pattern_unk.cast::<IUIAutomationInvokePattern>() {
                            if inv_pattern.Invoke().is_ok() {
                                info!("UIA InvokePattern 経由でタブ '{}' をアクティブ化しました (HWND: {})", tab_name, target_hwnd.0);
                                return Ok(());
                            }
                        }
                    }
                }
            }
        }
    }
    Err(format!("UIA でタブ '{}' が見つからなかったか、アクティブ化に失敗しました (HWND: {})", tab_name, target_hwnd.0))
}

pub fn release_modifier_keys() {
    unsafe {
        use windows::Win32::UI::Input::KeyboardAndMouse::{
            GetAsyncKeyState, VK_CONTROL, VK_LCONTROL, VK_LSHIFT, VK_LWIN, VK_MENU, VK_RCONTROL,
            VK_RSHIFT, VK_RWIN, VK_SHIFT,
        };

        let mut release_inputs = Vec::new();

        let keys_to_check = [
            VK_CONTROL, VK_LCONTROL, VK_RCONTROL,
            VK_SHIFT, VK_LSHIFT, VK_RSHIFT,
            VK_MENU, VK_LWIN, VK_RWIN,
        ];

        for &vk in &keys_to_check {
            if (GetAsyncKeyState(vk.0 as i32) as u16 & 0x8000) != 0 {
                release_inputs.push(INPUT {
                    r#type: INPUT_KEYBOARD,
                    Anonymous: windows::Win32::UI::Input::KeyboardAndMouse::INPUT_0 {
                        ki: KEYBDINPUT {
                            wVk: vk,
                            dwFlags: KEYEVENTF_KEYUP,
                            ..Default::default()
                        },
                    },
                });
            }
        }

        if !release_inputs.is_empty() {
            SendInput(&release_inputs, std::mem::size_of::<INPUT>() as i32);
            thread::sleep(Duration::from_millis(20));
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

        if target_thread_id != 0 && current_thread_id != target_thread_id {
            let _ = AttachThreadInput(current_thread_id, target_thread_id, BOOL(0));
        }

        info!(
            "Ctrl+9 ショートカットキーを送信し、最新タブをアクティブ化しました (HWND: {})",
            target_hwnd.0
        );
    }
}
