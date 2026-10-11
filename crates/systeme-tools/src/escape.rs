//! systemd-escape 子集: 單元名轉義/反轉義 CLI 邏輯。
//!
//! 複用 systeme-units specifier 的 escape_unit_str / unescape_unit_path。

use systeme_units::specifier::{escape_unit_str, unescape_unit_path};

/// systemd-escape 主邏輯。`args` 為位置參數, `flags` 為選項 (--suffix=, --template=, --unescape, --instance)。
pub fn run(args: &[String], flags: &[String]) -> String {
    let unescape_mode = flags.iter().any(|f| f == "--unescape" || f == "-u");
    let instance_mode = flags.iter().any(|f| f == "--instance" || f == "-i");
    let suffix = flags
        .iter()
        .find_map(|f| f.strip_prefix("--suffix=").map(|s| s.to_string()))
        .or_else(|| {
            flags
                .iter()
                .position(|f| f == "--suffix" || f == "-S")
                .and_then(|i| flags.get(i + 1).cloned())
        });
    let template = flags
        .iter()
        .find_map(|f| f.strip_prefix("--template=").map(|s| s.to_string()))
        .or_else(|| {
            flags
                .iter()
                .position(|f| f == "--template" || f == "-T")
                .and_then(|i| flags.get(i + 1).cloned())
        });

    let joined = args.concat();
    if joined.is_empty() {
        return String::new();
    }

    if unescape_mode {
        // 剝 --suffix 後綴再反轉義; 結果補前導 '/' (systemd 語義: 反轉義結果是絕對路徑)
        let mut name = joined.clone();
        if let Some(suf) = &suffix {
            let dot_suf = format!(".{suf}");
            if let Some(stripped) = name.strip_suffix(&dot_suf) {
                name = stripped.to_string();
            }
        } else if instance_mode {
            // --instance: 取 @ 後的實例部分反轉義
            if let Some((_, inst)) = name.split_once('@') {
                name = inst.to_string();
            }
        }
        let mut out = unescape_unit_path(&name);
        if !out.starts_with('/') {
            out.insert(0, '/');
        }
        return out;
    }

    // 轉義: 參數以 '/' 開頭 → 路徑模式 (剝前導 '/'、'/'→'-'); 否則模板名模式
    let path_mode = joined.starts_with('/');
    let body = if path_mode {
        let trimmed = joined.trim_end_matches('/');
        let stripped = trimmed.strip_prefix('/').unwrap_or(trimmed);
        escape_unit_str(stripped, true)
    } else {
        escape_unit_str(&joined, false)
    };

    if let Some(tpl) = &template {
        // fsck@.service + /dev/sda1 → fsck@dev-sda1.service
        if let Some((prefix, suffix_part)) = tpl.split_once('@') {
            return format!("{prefix}@{body}{suffix_part}");
        }
        return format!("{tpl}{body}");
    }

    match suffix {
        Some(suf) => format!("{body}.{suf}"),
        None => body,
    }
}
