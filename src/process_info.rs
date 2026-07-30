use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::Security::{
    GetSidSubAuthority, GetSidSubAuthorityCount, GetTokenInformation, TokenElevation,
    TokenIntegrityLevel, TOKEN_ELEVATION, TOKEN_MANDATORY_LABEL, TOKEN_QUERY,
};
use windows::Win32::System::SystemServices::{
    SECURITY_MANDATORY_HIGH_RID, SECURITY_MANDATORY_LOW_RID,
    SECURITY_MANDATORY_MEDIUM_PLUS_RID, SECURITY_MANDATORY_MEDIUM_RID,
    SECURITY_MANDATORY_SYSTEM_RID, SECURITY_MANDATORY_UNTRUSTED_RID,
};
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

#[derive(Debug, Clone)]
pub struct ProcessSecurityContext {
    pub elevated: bool,
    pub integrity_rid: u32,
    pub integrity_label: &'static str,
}

fn integrity_label_from_rid(rid: u32) -> &'static str {
    if rid < SECURITY_MANDATORY_UNTRUSTED_RID as u32 {
        "below-untrusted"
    } else if rid < SECURITY_MANDATORY_LOW_RID as u32 {
        "untrusted"
    } else if rid < SECURITY_MANDATORY_MEDIUM_RID as u32 {
        "low"
    } else if rid == SECURITY_MANDATORY_MEDIUM_PLUS_RID as u32 {
        "medium+"
    } else if rid < SECURITY_MANDATORY_HIGH_RID as u32 {
        "medium"
    } else if rid < SECURITY_MANDATORY_SYSTEM_RID as u32 {
        "high"
    } else if rid == SECURITY_MANDATORY_SYSTEM_RID as u32 {
        "system"
    } else {
        "protected-or-higher"
    }
}

pub fn query_current_process_security_context() -> Result<ProcessSecurityContext, String> {
    unsafe {
        let mut token = HANDLE::default();
        OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token)
            .map_err(|e| format!("OpenProcessToken 失敗: {}", e))?;

        let result = (|| {
            let mut elevation = TOKEN_ELEVATION::default();
            let mut ret_len = 0u32;
            GetTokenInformation(
                token,
                TokenElevation,
                Some((&mut elevation as *mut TOKEN_ELEVATION).cast()),
                std::mem::size_of::<TOKEN_ELEVATION>() as u32,
                &mut ret_len,
            )
            .map_err(|e| format!("GetTokenInformation(TokenElevation) 失敗: {}", e))?;

            let mut needed = 0u32;
            let _ = GetTokenInformation(token, TokenIntegrityLevel, None, 0, &mut needed);
            if needed == 0 {
                return Err(
                    "GetTokenInformation(TokenIntegrityLevel) の必要バッファ長が 0 でした。"
                        .to_string(),
                );
            }

            let mut buf = vec![0u8; needed as usize];
            GetTokenInformation(
                token,
                TokenIntegrityLevel,
                Some(buf.as_mut_ptr().cast()),
                needed,
                &mut needed,
            )
            .map_err(|e| format!("GetTokenInformation(TokenIntegrityLevel) 失敗: {}", e))?;

            let tml = std::ptr::read_unaligned(buf.as_ptr() as *const TOKEN_MANDATORY_LABEL);
            let sid = tml.Label.Sid;
            if sid.0.is_null() {
                return Err("TokenIntegrityLevel の SID が null でした。".to_string());
            }

            let sub_authority_count_ptr = GetSidSubAuthorityCount(sid);
            if sub_authority_count_ptr.is_null() {
                return Err("GetSidSubAuthorityCount が null を返しました。".to_string());
            }
            let sub_authority_count = *sub_authority_count_ptr as u32;
            if sub_authority_count == 0 {
                return Err("SID の sub-authority count が 0 でした。".to_string());
            }

            let rid_ptr = GetSidSubAuthority(sid, sub_authority_count - 1);
            if rid_ptr.is_null() {
                return Err("GetSidSubAuthority が null を返しました。".to_string());
            }
            let rid = *rid_ptr;

            Ok(ProcessSecurityContext {
                elevated: elevation.TokenIsElevated != 0,
                integrity_rid: rid,
                integrity_label: integrity_label_from_rid(rid),
            })
        })();

        let _ = CloseHandle(token);
        result
    }
}

pub fn log_current_process_security_context() {
    let exe_path = std::env::current_exe()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "<unknown>".to_string());

    crate::info!(
        "Process context: pid={}, exe={}",
        std::process::id(),
        exe_path
    );

    match query_current_process_security_context() {
        Ok(ctx) => {
            crate::info!(
                "Process security context: elevated={}, integrity={} (RID={})",
                ctx.elevated,
                ctx.integrity_label,
                ctx.integrity_rid
            );

            if ctx.elevated {
                crate::warn!(
                    "現在のプロセスは昇格実行 (elevated=true) です。Explorer が通常権限の場合、トレイアイコン登録やウィンドウメッセージ連携が失敗する可能性があります。"
                );
            }

            match ctx.integrity_label {
                "low" | "untrusted" | "below-untrusted" => {
                    crate::warn!(
                        "現在のプロセス整合性レベルは {} です。Low/Untrusted では ChangeWindowMessageFilterEx が ERROR_ACCESS_DENIED になり得て、トレイアイコン登録も失敗しやすくなります。通常は Explorer から直接起動した medium が想定です。",
                        ctx.integrity_label
                    );
                }
                "high" | "system" | "protected-or-higher" => {
                    crate::warn!(
                        "現在のプロセス整合性レベルは {} です。Explorer が medium の場合、UIPI により通知領域やメッセージ連携で拒否される可能性があります。通常権限での起動を確認してください。",
                        ctx.integrity_label
                    );
                }
                _ => {}
            }
        }
        Err(e) => {
            crate::warn!("Process security context の取得に失敗しました: {}", e);
        }
    }
}

use windows::Win32::Foundation::{HWND, NTSTATUS};
use windows::Win32::System::Diagnostics::Debug::ReadProcessMemory;
use windows::Win32::System::Threading::{
    OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ,
};
use windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;

#[repr(C)]
struct PROCESS_BASIC_INFORMATION {
    _exit_status: NTSTATUS,
    peb_base_address: *mut std::ffi::c_void,
    _affinity_mask: usize,
    _base_priority: i32,
    _unique_process_id: usize,
    _inherited_from_unique_process_id: usize,
}

#[repr(C)]
struct UNICODE_STRING {
    length: u16,
    maximum_length: u16,
    buffer: *mut u16,
}

#[repr(C)]
struct RTL_USER_PROCESS_PARAMETERS {
    _reserved1: [u8; 16],
    _reserved2: [*mut std::ffi::c_void; 5],
    _current_directory_path: [u8; 24],
    _dll_path: UNICODE_STRING,
    _image_path_name: UNICODE_STRING,
    command_line: UNICODE_STRING,
}

pub fn get_command_line_from_hwnd(hwnd: HWND) -> Option<String> {
    if hwnd.0 == 0 {
        return None;
    }
    unsafe {
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == 0 {
            return None;
        }

        let process_handle =
            OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, false, pid).ok()?;
        let res = extract_cmdline_from_process(process_handle);
        let _ = CloseHandle(process_handle);
        res
    }
}

unsafe fn extract_cmdline_from_process(process: HANDLE) -> Option<String> {
    type FnNtQueryInformationProcess = unsafe extern "system" fn(
        HANDLE,
        u32,
        *mut std::ffi::c_void,
        u32,
        *mut u32,
    ) -> NTSTATUS;

    let ntdll = windows::Win32::System::LibraryLoader::GetModuleHandleW(windows::core::w!(
        "ntdll.dll"
    ))
    .ok()?;
    let proc_addr = windows::Win32::System::LibraryLoader::GetProcAddress(
        ntdll,
        windows::core::s!("NtQueryInformationProcess"),
    )?;
    let nt_query: FnNtQueryInformationProcess = std::mem::transmute(proc_addr);

    let mut pbi = std::mem::zeroed::<PROCESS_BASIC_INFORMATION>();
    let mut return_length = 0u32;
    let status = nt_query(
        process,
        0,
        &mut pbi as *mut _ as _,
        std::mem::size_of::<PROCESS_BASIC_INFORMATION>() as u32,
        &mut return_length,
    );
    if status.0 != 0 || pbi.peb_base_address.is_null() {
        return None;
    }

    let process_parameters_ptr_addr =
        (pbi.peb_base_address as usize + 0x20) as *const std::ffi::c_void;
    let mut process_parameters_ptr = std::ptr::null_mut::<std::ffi::c_void>();
    let mut bytes_read = 0usize;

    if ReadProcessMemory(
        process,
        process_parameters_ptr_addr,
        &mut process_parameters_ptr as *mut _ as _,
        std::mem::size_of::<*mut std::ffi::c_void>(),
        Some(&mut bytes_read),
    )
    .is_err()
        || process_parameters_ptr.is_null()
    {
        return None;
    }

    let mut params = std::mem::zeroed::<RTL_USER_PROCESS_PARAMETERS>();
    if ReadProcessMemory(
        process,
        process_parameters_ptr,
        &mut params as *mut _ as _,
        std::mem::size_of::<RTL_USER_PROCESS_PARAMETERS>(),
        Some(&mut bytes_read),
    )
    .is_err()
    {
        return None;
    }

    if params.command_line.buffer.is_null() || params.command_line.length == 0 {
        return None;
    }

    let len_chars = (params.command_line.length / 2) as usize;
    let mut buffer = vec![0u16; len_chars];

    if ReadProcessMemory(
        process,
        params.command_line.buffer as _,
        buffer.as_mut_ptr() as _,
        params.command_line.length as usize,
        Some(&mut bytes_read),
    )
    .is_err()
    {
        return None;
    }

    Some(String::from_utf16_lossy(&buffer))
}

pub fn extract_folder_from_cmdline(cmd_line: &str) -> Option<String> {
    let mut parts = Vec::new();
    let mut in_quotes = false;
    let mut current = String::new();

    for c in cmd_line.chars() {
        if c == '"' {
            in_quotes = !in_quotes;
            if !current.is_empty() {
                parts.push(current.clone());
                current.clear();
            }
        } else if c == ' ' && !in_quotes {
            if !current.is_empty() {
                parts.push(current.clone());
                current.clear();
            }
        } else {
            current.push(c);
        }
    }
    if !current.is_empty() {
        parts.push(current);
    }

    for part in parts {
        let trimmed = part.trim_matches('"').trim();
        if !trimmed.is_empty()
            && crate::path_resolver::is_navigable_folder(trimmed)
            && !crate::path_resolver::is_home_path(trimmed)
        {
            return Some(trimmed.to_string());
        }
    }

    None
}
