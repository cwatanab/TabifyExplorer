use std::path::Path;

/// Percent-decodes URL strings (e.g. file:///C:/Path%20With%20Spaces -> C:\Path With Spaces).
pub fn url_decode(s: &str) -> String {
    let mut bytes = Vec::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '%' {
            let mut hex = String::new();
            if let Some(&h1) = chars.peek() {
                hex.push(h1);
                chars.next();
            }
            if let Some(&h2) = chars.peek() {
                hex.push(h2);
                chars.next();
            }
            if hex.len() == 2 {
                if let Ok(byte) = u8::from_str_radix(&hex, 16) {
                    bytes.push(byte);
                    continue;
                }
            }
            bytes.push(b'%');
            bytes.extend(hex.bytes());
        } else if c == '+' {
            bytes.push(b' ');
        } else {
            let mut buf = [0; 4];
            bytes.extend(c.encode_utf8(&mut buf).bytes());
        }
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

/// LocationURL (例: file:///C:/path, file://server/share, shell:..., ::{...) をデコードおよびパースします。
pub fn parse_location_url(url: &str) -> Option<String> {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return None;
    }

    let decoded = url_decode(trimmed);

    if let Some(stripped) = decoded.strip_prefix("file:///") {
        let local_path = stripped.replace('/', "\\");
        if !local_path.is_empty() {
            return Some(local_path);
        }
    } else if let Some(stripped) = decoded.strip_prefix("file://") {
        let unc_path = format!("\\\\{}", stripped.replace('/', "\\"));
        if !unc_path.is_empty() {
            return Some(unc_path);
        }
    } else {
        let lower = decoded.to_lowercase();
        if lower.starts_with("shell:") || lower.starts_with("::{") {
            return Some(decoded);
        }
    }

    None
}

/// フォルダが遷移可能な有効なディレクトリ・仮想パスか判定します。
pub fn is_navigable_folder(path: &str) -> bool {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return false;
    }

    // UNC パス (\ や // で始まる)
    if trimmed.starts_with("\\\\") || trimmed.starts_with("//") {
        return true;
    }

    // 実在するディレクトリかチェック
    if Path::new(trimmed).is_dir() {
        return true;
    }

    let lower = trimmed.to_lowercase();

    // shell: で始まる仮想パス
    if lower.starts_with("shell:") {
        return true;
    }

    // CLSID 形式 (::{ で始まる)
    if lower.starts_with("::{") {
        return true;
    }

    // 「PC」「ホーム」「ギャラリー」などの CLSID / 仮想パス名
    matches!(
        lower.as_str(),
        "pc" | "this pc" | "home" | "ホーム" | "gallery" | "ギャラリー"
    )
}

/// 2つのパス文字列が同等か（/ -> \, 末尾 \ 除去, 大小文字無視）判定します。
pub fn are_paths_equivalent(path1: &str, path2: &str) -> bool {
    normalize_path_for_equiv(path1) == normalize_path_for_equiv(path2)
}

fn normalize_path_for_equiv(path: &str) -> String {
    let trimmed = path.trim().replace('/', "\\");
    trimmed.trim_end_matches('\\').to_lowercase()
}

/// ホーム / デフォルト仮想フォルダかチェックします。
pub fn is_home_path(p: &str) -> bool {
    let lower = p.trim().to_lowercase();
    lower == "home"
        || lower == "ホーム"
        || lower == "quick access"
        || lower == "クイック アクセス"
        || lower == "クイックアクセス"
        || lower.contains("::{6dcd978d-6903-4905-885e-812c59d810b8}")
        || lower.contains("::{679f85cb-0220-4080-929b-55a21749c2c1}")
        || lower.contains("::{f874320e-b68e-4156-a30f-211113d6615b}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_are_paths_equivalent() {
        assert!(are_paths_equivalent("C:/Users/test", "C:\\Users\\test\\"));
        assert!(are_paths_equivalent("C:\\Users\\test", "c:\\users\\test"));
        assert!(are_paths_equivalent("shell:Personal", "SHELL:PERSONAL"));
        assert!(are_paths_equivalent("C:\\", "c:"));
        assert!(!are_paths_equivalent(
            "C:\\Users\\test1",
            "C:\\Users\\test2"
        ));
    }

    #[test]
    fn test_parse_location_url() {
        assert_eq!(
            parse_location_url("file:///C:/Users/test"),
            Some("C:\\Users\\test".to_string())
        );
        assert_eq!(
            parse_location_url("file://server/share/folder"),
            Some("\\\\server\\share\\folder".to_string())
        );
        assert_eq!(
            parse_location_url("shell:Personal"),
            Some("shell:Personal".to_string())
        );
        assert_eq!(
            parse_location_url("::{20D04FE0-3AEA-1069-A2D8-08002B30309D}"),
            Some("::{20D04FE0-3AEA-1069-A2D8-08002B30309D}".to_string())
        );
        assert_eq!(parse_location_url(""), None);
    }

    #[test]
    fn test_is_navigable_folder() {
        assert!(is_navigable_folder(
            "::{20D04FE0-3AEA-1069-A2D8-08002B30309D}"
        ));
        assert!(is_navigable_folder(
            "::{6dcd978d-6903-4905-885e-812c59d810b8}"
        ));
        assert!(is_navigable_folder("PC"));
        assert!(is_navigable_folder("This PC"));
        assert!(is_navigable_folder("home"));
        assert!(is_navigable_folder("ホーム"));
        assert!(is_navigable_folder("gallery"));
        assert!(is_navigable_folder("ギャラリー"));
        assert!(is_navigable_folder("shell:Personal"));
        assert!(is_navigable_folder("\\\\server\\share"));
        assert!(is_navigable_folder("//server/share"));
        assert!(is_navigable_folder("C:\\"));
        assert!(!is_navigable_folder(""));
        assert!(!is_navigable_folder("   "));
        assert!(!is_navigable_folder("non_existent_folder_xyz_12345"));
    }
}
