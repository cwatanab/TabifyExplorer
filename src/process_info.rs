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
