//! 查詢執行: 過濾 → 截尾 → 排序 → 格式化。

use crate::cli::Query;
use systeme_journal::entry::JournalEntry;
use systeme_journal::filter::JournalFilter;
use systeme_journal::format::format_entry;

/// 對記憶體中的條目執行一次查詢, 返回渲染後的多行文本
/// (每行以 `\n` 結尾; 無結果返回空串)。
pub fn run_query(entries: &[JournalEntry], q: &Query) -> String {
    let filter = JournalFilter {
        units: q.units.clone(),
        priority_max: q.priority_max,
        since_us: q.since,
        until_us: q.until,
        patterns: q.patterns.clone(),
    };
    let mut selected: Vec<&JournalEntry> = entries.iter().filter(|e| filter.matches(e)).collect();

    if let Some(n) = q.lines {
        let start = selected.len().saturating_sub(n);
        selected = selected.split_off(start);
    }
    if q.reverse {
        selected.reverse();
    }

    selected
        .iter()
        .map(|e| format_entry(e, q.format))
        .map(|mut line| {
            line.push('\n');
            line
        })
        .collect()
}
