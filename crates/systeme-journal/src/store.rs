//! 日誌存儲 — JournalSink 抽象、內存/JSONL 後端、per-source 限流。

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::rc::Rc;

use crate::entry::JournalEntry;
use crate::rate::RateLimiter;

/// 存儲後端抽象 (便於測試與未來換後端)。
pub trait JournalSink {
    /// 追加一條日誌;失敗時返回錯誤描述。
    fn append(&mut self, entry: &JournalEntry) -> Result<(), String>;
}

/// 內存後端 — 測試用;`Clone` 後共享同一份條目 (Rc)。
#[derive(Debug, Clone, Default)]
pub struct MemorySink {
    /// 已追加的條目 (RefCell 供 clone 端觀察)。
    pub entries: Rc<RefCell<Vec<JournalEntry>>>,
}

impl JournalSink for MemorySink {
    fn append(&mut self, entry: &JournalEntry) -> Result<(), String> {
        self.entries.borrow_mut().push(entry.clone());
        Ok(())
    }
}

/// JSONL 文件後端 — 每行一條 JSON,追加寫入。
pub struct JsonlSink {
    file: File,
}

impl JsonlSink {
    /// 打開 (或建立) JSONL 文件用於追加。
    pub fn create(path: &Path) -> Result<Self, String> {
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .map_err(|e| format!("open {path:?}: {e}"))?;
        Ok(Self { file })
    }
}

impl JournalSink for JsonlSink {
    fn append(&mut self, entry: &JournalEntry) -> Result<(), String> {
        let line = serde_json::to_string(entry).map_err(|e| e.to_string())?;
        writeln!(self.file, "{line}").map_err(|e| e.to_string())?;
        self.file.flush().map_err(|e| e.to_string())
    }
}

/// 讀取整個 JSONL 文件;不存在或壞行靜默跳過 (容忍部分寫入損壞)。
pub fn read_jsonl(path: &Path) -> Vec<JournalEntry> {
    let Ok(file) = File::open(path) else {
        return Vec::new();
    };
    BufReader::new(file)
        .lines()
        .map_while(Result::ok)
        .filter_map(|line| serde_json::from_str(&line).ok())
        .collect()
}

/// 日誌倉庫 — 前置 per-source 限流再寫入 sink。
///
/// 來源鍵優先級: unit → identifier → `"_unknown"`。
pub struct JournalStore {
    sink: Box<dyn JournalSink>,
    limiters: BTreeMap<String, RateLimiter>,
    burst: usize,
    interval_ms: u64,
    dropped_count: u64,
}

impl JournalStore {
    /// 建立倉庫:每來源每 `interval_ms` 毫秒最多 `burst` 條。
    pub fn new(sink: Box<dyn JournalSink>, burst: usize, interval_ms: u64) -> Self {
        Self {
            sink,
            limiters: BTreeMap::new(),
            burst,
            interval_ms,
            dropped_count: 0,
        }
    }

    fn source_key(entry: &JournalEntry) -> &str {
        entry
            .unit
            .as_deref()
            .or(entry.identifier.as_deref())
            .unwrap_or("_unknown")
    }

    /// 提交一條日誌;被限流或寫入失敗時返回 false。
    pub fn submit(&mut self, entry: &JournalEntry, now_ms: u64) -> bool {
        let key = Self::source_key(entry).to_string();
        let limiter = self
            .limiters
            .entry(key)
            .or_insert_with(|| RateLimiter::new(self.burst, self.interval_ms));
        if !limiter.allow(now_ms) {
            self.dropped_count += 1;
            return false;
        }
        self.sink.append(entry).is_ok()
    }

    /// 因限流被丟棄的條目總數。
    pub fn dropped(&self) -> u64 {
        self.dropped_count
    }
}
