//! mount / automount / swap 單元配置層 — 純邏輯解析與單元名推導。
//!
//! 真實 mount(2) / autofs / swapon 動作由 daemon 層 (cfg(unix)) 實現。

use systeme_units::parser::{parse_bool, ParsedUnit};
use systeme_units::specifier::escape_unit_str;

use crate::service::{parse_time_span, TimeSpan};

/// [Mount] 段解析結果。
#[derive(Debug, Clone, PartialEq)]
#[derive(Default)]
pub struct MountConfig {
    pub what: Option<String>,
    pub where_: Option<String>,
    pub fs_type: Option<String>,
    /// 掛載選項 (逗號拆分, 多行累積)
    pub options: Vec<String>,
    pub lazy_unmount: bool,
    pub directory_mode: Option<String>,
    pub warnings: Vec<String>,
}


impl MountConfig {
    pub fn from_parsed(unit: &ParsedUnit) -> Self {
        let mut cfg = Self::default();
        let Some(section) = unit.section("Mount") else {
            return cfg;
        };
        for entry in &section.entries {
            let v = entry.value.trim();
            match entry.key.as_str() {
                "What" => cfg.what = Some(v.to_string()),
                "Where" => cfg.where_ = Some(v.to_string()),
                "Type" => cfg.fs_type = Some(v.to_string()),
                "Options" => {
                    if v.is_empty() {
                        cfg.options.clear();
                    } else {
                        cfg.options.extend(v.split(',').map(|s| s.to_string()));
                    }
                }
                "LazyUnmount" => match parse_bool(v) {
                    Some(b) => cfg.lazy_unmount = b,
                    None => cfg
                        .warnings
                        .push(format!("LazyUnmount value not boolean, ignoring: {v:?}")),
                },
                "DirectoryMode" => cfg.directory_mode = Some(v.to_string()),
                _ => {}
            }
        }
        cfg
    }
}

/// [Swap] 段解析結果。
#[derive(Debug, Clone, PartialEq)]
#[derive(Default)]
pub struct SwapConfig {
    pub what: Option<String>,
    pub priority: Option<i32>,
    pub options: Vec<String>,
    pub warnings: Vec<String>,
}


impl SwapConfig {
    pub fn from_parsed(unit: &ParsedUnit) -> Self {
        let mut cfg = Self::default();
        let Some(section) = unit.section("Swap") else {
            return cfg;
        };
        for entry in &section.entries {
            let v = entry.value.trim();
            match entry.key.as_str() {
                "What" => cfg.what = Some(v.to_string()),
                "Priority" => match v.parse::<i32>() {
                    Ok(p) => cfg.priority = Some(p),
                    Err(_) => cfg
                        .warnings
                        .push(format!("Priority value not a number, ignoring: {v:?}")),
                },
                "Options" => {
                    if v.is_empty() {
                        cfg.options.clear();
                    } else {
                        cfg.options.extend(v.split(',').map(|s| s.to_string()));
                    }
                }
                _ => {}
            }
        }
        cfg
    }
}

/// [Automount] 段解析結果。
#[derive(Debug, Clone, PartialEq)]
#[derive(Default)]
pub struct AutoMountConfig {
    pub where_: Option<String>,
    pub directory_mode: Option<String>,
    pub timeout_idle_sec: Option<TimeSpan>,
    pub warnings: Vec<String>,
}


impl AutoMountConfig {
    pub fn from_parsed(unit: &ParsedUnit) -> Self {
        let mut cfg = Self::default();
        let Some(section) = unit.section("Automount") else {
            return cfg;
        };
        for entry in &section.entries {
            let v = entry.value.trim();
            match entry.key.as_str() {
                "Where" => cfg.where_ = Some(v.to_string()),
                "DirectoryMode" => cfg.directory_mode = Some(v.to_string()),
                "TimeoutIdleSec" => match parse_time_span(v) {
                    Ok(ts) => cfg.timeout_idle_sec = Some(ts),
                    Err(()) => cfg
                        .warnings
                        .push(format!("TimeoutIdleSec value invalid, ignoring: {v:?}")),
                },
                _ => {}
            }
        }
        cfg
    }
}

/// 路徑 → 規範化 escape 主體: 剝前導/尾斜杠後 escape, 根返回 "-"。
fn path_body(path: &str) -> String {
    let trimmed = path.trim_end_matches('/');
    let stripped = trimmed.strip_prefix('/').unwrap_or(trimmed);
    if stripped.is_empty() {
        "-".to_string()
    } else {
        escape_unit_str(stripped, true)
    }
}

/// 掛載點路徑 → mount 單元名 (`/var/www` → `var-www.mount`, `/` → `-.mount`)。
pub fn mount_unit_name(where_: &str) -> String {
    format!("{}.mount", path_body(where_))
}

/// swap 設備/文件 → swap 單元名 (`/swapfile` → `swapfile.swap`)。
pub fn swap_unit_name(what: &str) -> String {
    format!("{}.swap", path_body(what))
}

/// 掛載點路徑 → automount 單元名。
pub fn automount_unit_name(where_: &str) -> String {
    format!("{}.automount", path_body(where_))
}

/// automount 單元 → 其觸發的 mount 單元 (同名剝換)。
pub fn automount_target(automount_unit: &str) -> String {
    automount_unit
        .strip_suffix(".automount")
        .map(|base| format!("{base}.mount"))
        .unwrap_or_else(|| automount_unit.to_string())
}
