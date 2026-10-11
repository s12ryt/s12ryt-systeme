//! 日誌條目數據模型。

use serde::{Deserialize, Serialize};

/// journald 默認優先級: info (6)。
pub const DEFAULT_PRIORITY: u8 = 6;

/// 單條日誌 (已從 native/syslog 協議歸一化)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JournalEntry {
    /// MESSAGE=
    pub message: String,
    /// syslog 嚴重度 0-7 (emerg..debug)
    pub priority: u8,
    /// __REALTIME_TIMESTAMP (unix 微秒; 缺 0)
    pub timestamp_us: u64,
    /// _SYSTEMD_UNIT=
    pub unit: Option<String>,
    /// _PID=
    pub pid: Option<u32>,
    /// SYSLOG_IDENTIFIER=
    pub identifier: Option<String>,
}

/// 優先級數字 → syslog 名稱 (超出鉗制到 debug)。
pub fn priority_name(p: u8) -> &'static str {
    const NAMES: [&str; 8] = ["emerg", "alert", "crit", "err", "warning", "notice", "info", "debug"];
    NAMES[(p.min(7)) as usize]
}
