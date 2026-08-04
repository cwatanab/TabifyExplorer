use windows::Win32::Foundation::{BOOL, COLORREF, HWND, LPARAM, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{
    DwmGetWindowAttribute, DwmSetWindowAttribute, DWMWA_CLOAK, DWMWA_CLOAKED,
    DWMWA_TRANSITIONS_FORCEDISABLED,
};
use windows::Win32::Graphics::Gdi::{
    RedrawWindow, HRGN, RDW_ALLCHILDREN, RDW_INVALIDATE, RDW_UPDATENOW,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON, VK_SHIFT};
use windows::Win32::UI::WindowsAndMessaging::*;

/// 指定された HWND が Explorer ("CabinetWClass") か判定します。
pub fn is_explorer_window(hwnd: HWND) -> bool {
    if hwnd.0 == 0 || unsafe { !IsWindow(hwnd).as_bool() } {
        return false;
    }
    let mut buf = [0u16; 256];
    let len = unsafe { GetClassNameW(hwnd, &mut buf) };
    if len == 0 {
        return false;
    }
    let class_name = String::from_utf16_lossy(&buf[..len as usize]);
    class_name.eq_ignore_ascii_case("CabinetWClass")
}

/// 通常表示されている Explorer ウィンドウか判定します。
pub fn is_normal_visible_explorer_window(hwnd: HWND, ignore_hwnd: Option<HWND>) -> bool {
    if hwnd.0 == 0 || ignore_hwnd == Some(hwnd) {
        return false;
    }
    if !is_explorer_window(hwnd) {
        return false;
    }
    if unsafe { !IsWindowVisible(hwnd).as_bool() } {
        return false;
    }
    if unsafe { IsIconic(hwnd).as_bool() } {
        return false;
    }

    let mut is_cloaked: u32 = 0;
    let hr = unsafe {
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_CLOAKED,
            &mut is_cloaked as *mut _ as _,
            std::mem::size_of::<u32>() as u32,
        )
    };
    if hr.is_ok() && is_cloaked != 0 {
        return false;
    }

    let mut rect = RECT::default();
    if unsafe { GetWindowRect(hwnd, &mut rect).is_ok() } {
        if rect.left <= -30000 || rect.top <= -30000 {
            return false;
        }
    } else {
        return false;
    }

    true
}

/// 指定 HWND の最上位ルートウィンドウを取得します。
pub fn get_root_hwnd(hwnd: HWND) -> HWND {
    if hwnd.0 == 0 {
        return hwnd;
    }
    let root = unsafe { GetAncestor(hwnd, GA_ROOT) };
    if root.0 != 0 {
        root
    } else {
        hwnd
    }
}

/// 統合対象となる既存のエクスプローラーウィンドウを検索します。
pub fn find_existing_explorer_window(_known_hwnds: &[HWND], current_hwnd: HWND) -> Option<HWND> {
    struct EnumContext {
        current_root: HWND,
        found_hwnd: Option<HWND>,
        checked_count: usize,
    }

    unsafe extern "system" fn enum_windows_callback(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let ctx = &mut *(lparam.0 as *mut EnumContext);
        let root = get_root_hwnd(hwnd);

        if !is_explorer_window(hwnd) {
            return BOOL(1);
        }

        ctx.checked_count += 1;

        // 自分自身、または自分と同じルートウィンドウは除外
        if hwnd == ctx.current_root || root == ctx.current_root {
            return BOOL(1);
        }

        if is_normal_visible_explorer_window(hwnd, None) {
            ctx.found_hwnd = Some(hwnd);
            return BOOL(0);
        }
        BOOL(1)
    }

    let current_root = get_root_hwnd(current_hwnd);
    let mut ctx = EnumContext {
        current_root,
        found_hwnd: None,
        checked_count: 0,
    };
    unsafe {
        let _ = EnumWindows(
            Some(enum_windows_callback),
            LPARAM(&mut ctx as *mut EnumContext as isize),
        );
    }

    if let Some(found) = ctx.found_hwnd {
        crate::info!("[統合先探査結果] 既存ウィンドウ発見: HWND {}", found.0);
    } else {
        crate::info!(
            "[統合先探査結果] 既存ウィンドウなし (チェック数: {})",
            ctx.checked_count
        );
    }

    ctx.found_hwnd
}

use std::collections::HashMap;
use std::sync::Mutex;

static SAVED_RECTS: Mutex<Option<HashMap<isize, RECT>>> = Mutex::new(None);

fn save_rect(hwnd_val: isize, rect: RECT) {
    if let Ok(mut guard) = SAVED_RECTS.lock() {
        let map = guard.get_or_insert_with(HashMap::new);
        map.entry(hwnd_val).or_insert(rect);
    }
}

fn get_and_remove_saved_rect(hwnd_val: isize) -> Option<RECT> {
    if let Ok(mut guard) = SAVED_RECTS.lock() {
        if let Some(map) = guard.as_mut() {
            return map.remove(&hwnd_val);
        }
    }
    None
}

fn get_saved_rect(hwnd_val: isize) -> Option<RECT> {
    if let Ok(guard) = SAVED_RECTS.lock() {
        if let Some(map) = guard.as_ref() {
            return map.get(&hwnd_val).copied();
        }
    }
    None
}

/// 新規検出したウィンドウをフリッカーレスで即時隠蔽および画面外待避します。
pub fn hide_window(hwnd: HWND) -> Option<RECT> {
    if hwnd.0 == 0 || unsafe { !IsWindow(hwnd).as_bool() } {
        return None;
    }

    let saved = get_saved_rect(hwnd.0);
    let rect = if let Some(r) = saved {
        r
    } else {
        let mut r = RECT::default();
        if unsafe { GetWindowRect(hwnd, &mut r).is_ok() } {
            if r.left > -10000 && r.top > -10000 {
                save_rect(hwnd.0, r);
            }
            r
        } else {
            RECT::default()
        }
    };

    unsafe {
        // 1. DWM レベルで即時クローク (描画・アニメーションを完全に停止してフリッカーをカット)
        let is_cloaked: BOOL = BOOL(1);
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_CLOAK,
            &is_cloaked as *const _ as _,
            std::mem::size_of::<BOOL>() as u32,
        );

        // 2. ウィンドウアニメーション無効化
        let disable_anim: BOOL = BOOL(1);
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_TRANSITIONS_FORCEDISABLED,
            &disable_anim as *const _ as _,
            std::mem::size_of::<BOOL>() as u32,
        );

        // 3. OSのウィンドウ表示状態を非表示へ
        let _ = ShowWindow(hwnd, SW_HIDE);

        // 4. 画面外退避
        let width = rect.right - rect.left;
        let height = rect.bottom - rect.top;
        let _ = SetWindowPos(
            hwnd,
            HWND::default(),
            -32000,
            -32000,
            if width > 0 { width } else { 800 },
            if height > 0 { height } else { 600 },
            SWP_NOZORDER | SWP_NOACTIVATE | SWP_HIDEWINDOW,
        );

        // 5. WM_SETREDRAW 0
        let _ = SendMessageW(hwnd, WM_SETREDRAW, WPARAM(0), LPARAM(0));

        // WS_EX_LAYERED アルファ値 0 (完全透明) 設定
        let ex_style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        let new_ex_style = ex_style | (WS_EX_LAYERED.0 as isize);
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, new_ex_style);
        let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), 0, LWA_ALPHA);
    }

    Some(rect)
}

/// ウィンドウの位置・表示状態を元に戻します (フォールバック用)。
pub fn restore_window(hwnd: HWND, original_rect: Option<RECT>) {
    if unsafe { !IsWindow(hwnd).as_bool() } {
        get_and_remove_saved_rect(hwnd.0);
        return;
    }

    let rect = match original_rect {
        Some(r) if r.left > -10000 && r.top > -10000 => {
            get_and_remove_saved_rect(hwnd.0);
            r
        }
        _ => match get_and_remove_saved_rect(hwnd.0) {
            Some(r) if r.left > -10000 && r.top > -10000 => r,
            _ => RECT {
                left: 100,
                top: 100,
                right: 900,
                bottom: 700,
            },
        },
    };

    let width = if rect.right - rect.left > 100 { rect.right - rect.left } else { 800 };
    let height = if rect.bottom - rect.top > 100 { rect.bottom - rect.top } else { 600 };
    let left = if rect.left > -10000 { rect.left } else { 100 };
    let top = if rect.top > -10000 { rect.top } else { 100 };

    unsafe {
        // 1. WM_SETREDRAW 1
        let _ = SendMessageW(hwnd, WM_SETREDRAW, WPARAM(1), LPARAM(0));

        // 2. SetWindowPos (元の位置へ)
        let _ = SetWindowPos(
            hwnd,
            HWND::default(),
            left,
            top,
            width,
            height,
            SWP_NOZORDER | SWP_FRAMECHANGED | SWP_SHOWWINDOW,
        );

        // 3. ShowWindow(hwnd, SW_SHOW)
        let _ = ShowWindow(hwnd, SW_SHOW);

        // 4. DWMWA_CLOAKED = FALSE
        let uncloak: BOOL = BOOL(0);
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_CLOAK,
            &uncloak as *const _ as _,
            std::mem::size_of::<BOOL>() as u32,
        );

        let enable_anim: BOOL = BOOL(0);
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_TRANSITIONS_FORCEDISABLED,
            &enable_anim as *const _ as _,
            std::mem::size_of::<BOOL>() as u32,
        );

        let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), 255, LWA_ALPHA);

        let ex_style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        let new_ex_style = ex_style & !(WS_EX_LAYERED.0 as isize);
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, new_ex_style);

        // 5. RedrawWindow でクリーンに復元
        let _ = RedrawWindow(
            hwnd,
            None,
            HRGN::default(),
            RDW_INVALIDATE | RDW_UPDATENOW | RDW_ALLCHILDREN,
        );
    }
}

/// 指定したウィンドウを破棄します (WM_CLOSE 送信)。
pub fn close_window(hwnd: HWND) {
    if unsafe { IsWindow(hwnd).as_bool() } {
        unsafe {
            let uncloak: BOOL = BOOL(0);
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_CLOAK,
                &uncloak as *const _ as _,
                std::mem::size_of::<BOOL>() as u32,
            );
            let _ = SendMessageW(hwnd, WM_CLOSE, WPARAM(0), LPARAM(0));
            if IsWindow(hwnd).as_bool() {
                let _ = PostMessageW(hwnd, WM_CLOSE, WPARAM(0), LPARAM(0));
            }
        }
    }
}

/// Shiftキーが押下されているか最上位ビットで判定します。
pub fn is_shift_key_pressed() -> bool {
    unsafe { (GetAsyncKeyState(VK_SHIFT.0 as i32) as u16 & 0x8000) != 0 }
}

use std::time::{Duration, Instant};

struct MouseDragState {
    is_down: bool,
    down_pos: POINT,
    is_dragging: bool,
    last_drag_time: Option<Instant>,
}

static MOUSE_DRAG_STATE: Mutex<MouseDragState> = Mutex::new(MouseDragState {
    is_down: false,
    down_pos: POINT { x: 0, y: 0 },
    is_dragging: false,
    last_drag_time: None,
});

/// マウスの状態を更新し、ドラッグ＆ドロップ（長距離のマウス移動を伴うクリック）操作を追跡します。
pub fn update_mouse_drag_state() {
    let is_down = unsafe { (GetAsyncKeyState(VK_LBUTTON.0 as i32) as u16 & 0x8000) != 0 };
    let mut current_pos = POINT { x: 0, y: 0 };
    unsafe {
        let _ = GetCursorPos(&mut current_pos);
    }

    if let Ok(mut state) = MOUSE_DRAG_STATE.lock() {
        if is_down {
            if !state.is_down {
                state.is_down = true;
                state.down_pos = current_pos;
                state.is_dragging = false;
            } else {
                let dx = (current_pos.x - state.down_pos.x).abs();
                let dy = (current_pos.y - state.down_pos.y).abs();
                if dx > 15 || dy > 15 {
                    state.is_dragging = true;
                    state.last_drag_time = Some(Instant::now());
                }
            }
        } else {
            if state.is_down {
                if state.is_dragging {
                    state.last_drag_time = Some(Instant::now());
                }
                state.is_down = false;
                state.is_dragging = false;
            }
        }
    }
}

/// マウス左ボタンが押下されているか判定します。
pub fn is_left_mouse_button_down() -> bool {
    update_mouse_drag_state();
    unsafe { (GetAsyncKeyState(VK_LBUTTON.0 as i32) as u16 & 0x8000) != 0 }
}

/// タブのドラッグ＆ドロップ操作中、またはドラッグ＆ドロップ直後（指定時間内）であるか判定します。
pub fn is_drag_and_drop_active_or_recent(threshold: Duration) -> bool {
    update_mouse_drag_state();
    if let Ok(state) = MOUSE_DRAG_STATE.lock() {
        if state.is_down && state.is_dragging {
            return true;
        }
        if let Some(last_time) = state.last_drag_time {
            if last_time.elapsed() < threshold {
                return true;
            }
        }
    }
    false
}
