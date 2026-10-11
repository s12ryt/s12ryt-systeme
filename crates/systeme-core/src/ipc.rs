//! IPC 協議類型: 守護進程與 systemctl 客戶端共享的請求/響應 (serde_json over Unix socket)。
//!
//! 協議為自有 JSON 行協議 (每行一條消息), D-Bus 兼容層後期另行提供。

use serde::{Deserialize, Serialize};

use crate::manager::UnitEntry;

// ─── 請求 ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Request {
    /// 列出已載入單元
    ListUnits,
    Status { unit: String },
    Start { unit: String },
    Stop { unit: String },
    Restart { unit: String },
    Reload { unit: String },
    IsActive { unit: String },
    /// 輸出單元文件內容 (含 drop-in 疊加後)
    Cat { unit: String },
    Enable { unit: String },
    Disable { unit: String },
    Mask { unit: String },
    Unmask { unit: String },
    /// 重新掃描單元目錄
    DaemonReload,
}

// ─── 響應 ────────────────────────────────────────────────────────────

/// 單元摘要 (list-units / status 共用)。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnitInfo {
    pub name: String,
    pub description: String,
    /// "loaded" | "masked" | "not-found"
    pub load_state: String,
    /// "inactive" | "activating" | "active" | "deactivating" | "auto-restart" | "failed"
    pub active_state: String,
    pub main_pid: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Response {
    Ok,
    Units(Vec<UnitInfo>),
    UnitStatus(UnitInfo),
    /// is-active 結果
    Active(bool),
    Cat { name: String, text: String },
    Error { message: String },
    NotImplemented { request: String },
}

// ─── 轉換 ────────────────────────────────────────────────────────────

/// 管理器單元條目 → IPC 展示信息
pub fn unit_info_of(entry: &UnitEntry) -> UnitInfo {
    UnitInfo {
        name: entry.name.clone(),
        description: entry.description().to_string(),
        load_state: if entry.masked { "masked" } else { "loaded" }.to_string(),
        active_state: entry.runtime.state.as_str().to_string(),
        main_pid: entry.runtime.main_pid,
    }
}
