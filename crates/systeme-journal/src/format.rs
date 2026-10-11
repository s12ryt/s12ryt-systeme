//! journalctl 輸出格式化 — short (syslog 風格近似) / json / cat。
//!
//! 注: systemd short 格式含 hostname,本模型未存 host,輸出省略 host 字段。

use crate::entry::JournalEntry;

/// 輸出格式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    /// `Oct 11 10:00:01 tag[pid]: message` (無 host 的近似)。
    Short,
    /// 每行一條 JSON。
    Json,
    /// 僅消息原文。
    Cat,
}

impl OutputFormat {
    /// 從 `-o` 參數值解析。
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "short" => Some(Self::Short),
            "json" => Some(Self::Json),
            "cat" => Some(Self::Cat),
            _ => None,
        }
    }
}

/// 按格式渲染單條日誌。
pub fn format_entry(e: &JournalEntry, fmt: OutputFormat) -> String {
    match fmt {
        OutputFormat::Short => {
            let ts = chrono::DateTime::from_timestamp_micros(e.timestamp_us as i64)
                .map(|d| d.naive_utc().format("%b %e %H:%M:%S").to_string())
                .unwrap_or_default();
            let tag = e.identifier.as_deref().or(e.unit.as_deref());
            match (tag, e.pid) {
                (Some(t), Some(p)) => format!("{ts} {t}[{p}]: {}", e.message),
                (Some(t), None) => format!("{ts} {t}: {}", e.message),
                (None, _) => format!("{ts} {}", e.message),
            }
        }
        OutputFormat::Json => serde_json::to_string(e).unwrap_or_default(),
        OutputFormat::Cat => e.message.clone(),
    }
}
