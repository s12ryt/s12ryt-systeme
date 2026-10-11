//! systeme-journal: journald 核心 — 日誌條目、native/syslog 協議、rate limit、存儲。
//!
//! - [`entry`] — JournalEntry 數據模型與優先級名稱
//! - [`native`] — journald native 協議解析 (文本 + 二進制長度前綴形式)
//! - [`syslog`] — RFC3164 syslog 行解析 (/dev/log datagram)
//! - [`rate`] — 固定窗口速率限制器
//! - [`store`] — JournalStore (per-source 限流) + JSONL sink / 讀取

pub mod entry;
pub mod filter;
pub mod format;
pub mod native;
pub mod rate;
pub mod store;
pub mod syslog;

pub use store::{JournalSink, JournalStore, JsonlSink, MemorySink};

/// 處理一個 /dev/log 數據報: 先按 syslog 行解析, 失敗再按 native 協議解析。
/// 返回是否有條目被接受 (含被限流拒絕的情況 = false)。
pub fn process_datagram(
    payload: &[u8],
    default_year: i32,
    store: &mut JournalStore,
    now_ms: u64,
) -> bool {
    let text = String::from_utf8_lossy(payload);
    let entries = match syslog::parse_syslog(&text, default_year) {
        Some(e) => vec![e],
        None => native::parse_native(payload),
    };
    let mut any = false;
    for e in entries {
        if store.submit(&e, now_ms) {
            any = true;
        }
    }
    any
}

/// 日誌文件路徑: $SYSTEME_JOURNAL > $XDG_RUNTIME_DIR/systeme/journal.jsonl
/// > /run/systeme/journal.jsonl (journald/journalctl 共用)。
pub fn journal_path() -> std::path::PathBuf {
    if let Ok(p) = std::env::var("SYSTEME_JOURNAL") {
        return std::path::PathBuf::from(p);
    }
    if let Ok(xdg) = std::env::var("XDG_RUNTIME_DIR") {
        return std::path::PathBuf::from(xdg).join("systeme/journal.jsonl");
    }
    std::path::PathBuf::from("/run/systeme/journal.jsonl")
}
