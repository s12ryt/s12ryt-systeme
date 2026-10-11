//! journalctl 查詢過濾 — 單元/優先級/時間窗/消息模式。

use crate::entry::JournalEntry;

/// 過濾條件;空集合/None = 不限制。
#[derive(Debug, Clone, Default)]
pub struct JournalFilter {
    /// 匹配 unit 或 SYSLOG_IDENTIFIER (任一命中即可)。
    pub units: Vec<String>,
    /// 最高優先級 (數字越小越嚴重),含本身。
    pub priority_max: Option<u8>,
    /// 起始時間 (微秒,含)。
    pub since_us: Option<u64>,
    /// 結束時間 (微秒,含)。
    pub until_us: Option<u64>,
    /// 消息子串模式 (大小寫不敏感,全部命中才通過)。
    pub patterns: Vec<String>,
}

impl JournalFilter {
    /// 條目是否通過過濾 (AND 語義)。
    pub fn matches(&self, e: &JournalEntry) -> bool {
        if !self.units.is_empty() {
            let hit = e
                .unit
                .as_deref()
                .is_some_and(|u| self.units.iter().any(|x| x == u))
                || e.identifier
                    .as_deref()
                    .is_some_and(|i| self.units.iter().any(|x| x == i));
            if !hit {
                return false;
            }
        }
        if let Some(max) = self.priority_max {
            if e.priority > max {
                return false;
            }
        }
        if let Some(since) = self.since_us {
            if e.timestamp_us < since {
                return false;
            }
        }
        if let Some(until) = self.until_us {
            if e.timestamp_us > until {
                return false;
            }
        }
        for p in &self.patterns {
            if !e.message.to_lowercase().contains(&p.to_lowercase()) {
                return false;
            }
        }
        true
    }
}

/// 優先級名稱表 (systemd 兼容別名)。
const PRIORITY_NAMES: [(&str, u8); 10] = [
    ("emerg", 0),
    ("panic", 0),
    ("alert", 1),
    ("crit", 2),
    ("err", 3),
    ("error", 3),
    ("warning", 4),
    ("warn", 4),
    ("notice", 5),
    ("info", 6),
];

/// 解析優先級參數: 數字 "0".."7" 或名稱 (大小寫不敏感);debug=7 也接受。
pub fn parse_priority(s: &str) -> Option<u8> {
    let s = s.trim();
    if let Ok(n) = s.parse::<i32>() {
        return if (0..=7).contains(&n) {
            Some(n as u8)
        } else {
            None
        };
    }
    let lower = s.to_lowercase();
    if lower == "debug" {
        return Some(7);
    }
    PRIORITY_NAMES
        .iter()
        .find(|(name, _)| *name == lower)
        .map(|(_, p)| *p)
}

/// 解析時間參數為 UTC 微秒時間戳。
///
/// 支持: `2026-10-11 10:00:00`、`2026-10-11T10:00:00`、`2026-10-11 10:00`、`2026-10-11`。
pub fn parse_datetime_arg(s: &str) -> Option<u64> {
    let s = s.trim();
    for fmt in [
        "%Y-%m-%d %H:%M:%S",
        "%Y-%m-%dT%H:%M:%S",
        "%Y-%m-%d %H:%M",
        "%Y-%m-%dT%H:%M",
    ] {
        if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(s, fmt) {
            return Some(dt.and_utc().timestamp_micros() as u64);
        }
    }
    if let Ok(d) = chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        return Some(
            d.and_hms_opt(0, 0, 0)?
                .and_utc()
                .timestamp_micros() as u64,
        );
    }
    None
}
