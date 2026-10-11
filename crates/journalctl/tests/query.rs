//! journalctl 查詢執行測試 (RED 先行)。

use journalctl::cli::Query;
use journalctl::query::run_query;
use systeme_journal::entry::JournalEntry;
use systeme_journal::format::OutputFormat;

fn entry(unit: Option<&str>, ident: Option<&str>, msg: &str, ts_us: u64) -> JournalEntry {
    JournalEntry {
        message: msg.to_string(),
        priority: 6,
        timestamp_us: ts_us,
        unit: unit.map(|s| s.to_string()),
        pid: ident.map(|_| 123),
        identifier: ident.map(|s| s.to_string()),
    }
}

fn query(q: Query, entries: &[JournalEntry]) -> String {
    run_query(entries, &q)
}

#[test]
fn filters_by_unit() {
    let entries = vec![
        entry(Some("a.service"), None, "from a", 1_000),
        entry(Some("b.service"), None, "from b", 2_000),
    ];
    let q = Query {
        units: vec!["a.service".to_string()],
        ..Default::default()
    };
    let out = query(q, &entries);
    assert!(out.contains("from a"));
    assert!(!out.contains("from b"));
}

#[test]
fn tail_n_keeps_last_lines() {
    let entries: Vec<JournalEntry> = (0..10)
        .map(|i| entry(Some("a.service"), None, &format!("msg-{i}"), i))
        .collect();
    let q = Query {
        lines: Some(3),
        ..Default::default()
    };
    let out = query(q, &entries);
    assert!(out.contains("msg-7"));
    assert!(out.contains("msg-8"));
    assert!(out.contains("msg-9"));
    assert!(!out.contains("msg-0"));
    assert!(!out.contains("msg-6"));
}

#[test]
fn reverse_orders_newest_first() {
    let entries = vec![
        entry(Some("a.service"), None, "first", 1_000),
        entry(Some("a.service"), None, "second", 2_000),
    ];
    let q = Query {
        reverse: true,
        ..Default::default()
    };
    let out = query(q, &entries);
    let second_pos = out.find("second").unwrap();
    let first_pos = out.find("first").unwrap();
    assert!(second_pos < first_pos);
}

#[test]
fn short_format_output() {
    let entries = vec![entry(Some("a.service"), Some("smartd"), "disk failing", 0)];
    let out = query(Query::default(), &entries);
    // 無時間戳 (epoch) + tag[pid]: message
    assert!(out.contains("smartd[123]: disk failing"));
}

#[test]
fn json_format_output() {
    let entries = vec![entry(Some("a.service"), None, "hello json", 0)];
    let q = Query {
        format: OutputFormat::Json,
        ..Default::default()
    };
    let out = query(q, &entries);
    assert!(out.contains("\"message\":\"hello json\""));
}

#[test]
fn cat_format_output() {
    let entries = vec![entry(Some("a.service"), None, "raw line", 0)];
    let q = Query {
        format: OutputFormat::Cat,
        ..Default::default()
    };
    let out = query(q, &entries);
    assert!(out.contains("raw line"));
    assert!(!out.contains("["));
}

#[test]
fn no_match_returns_empty() {
    let entries = vec![entry(Some("a.service"), None, "hello", 0)];
    let q = Query {
        units: vec!["zzz.service".to_string()],
        ..Default::default()
    };
    let out = query(q, &entries);
    assert!(out.is_empty());
}
