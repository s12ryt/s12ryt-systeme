//! # systeme-units
//!
//! systemd 單元文件 (.service/.timer/.socket/...) 解析庫。
//!
//! 設計目標: **高兼容** —— 能直接解析發行版自帶的真實單元文件,
//! 未知 section/key 只降級為警告, 決不報錯 (見 agent/question.md 驗收標準)。

pub mod parser;
pub mod search;
pub mod specifier;
pub mod types;

pub use parser::{parse_bool, parse_unit, Entry, ParseWarning, ParsedUnit, Section};
pub use search::{
    find_unit, load_unit, system_search_paths, user_search_paths, FoundUnit, Loaded, MemFs, OsFs,
    SearchPaths, UnitFs,
};
pub use specifier::{
    escape_unit_str, expand_env_vars, expand_specifiers, split_instance, unescape_unit_path,
    unescape_unit_str, Expansion, SpecContext,
};
pub use types::{is_valid_unit_name, UnitType};
