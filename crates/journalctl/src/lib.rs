//! journalctl — 日誌查詢 CLI 庫層。
//!
//! 二進制入口見 `src/main.rs`; 本庫暴露 CLI 解析 (`cli`) 與
//! 查詢執行 (`query`) 供測試與復用。

pub mod cli;
pub mod query;

pub use cli::{parse_args, Query};
pub use query::run_query;
