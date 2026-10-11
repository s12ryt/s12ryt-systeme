//! journalctl 查詢邏輯測試 — 過濾器、優先級/時間參數解析、輸出格式化。

use systeme_journal::entry::JournalEntry;
use systeme_journal::filter::{parse_datetime_arg, parse_priority, JournalFilter};
use systeme_journal::format::{format_entry, OutputFormat};

#[allow(clippy::too_many_arguments)]
fn mk(
    message: &str,
    priority: u8,
    ts: u64,
    unit: Option<&str>,
    pid: Option<u32>,
    identifier: Option<&str>,
) -> JournalEntry {
    JournalEntry {
        message: message.to_string(),
        priority,
        timestamp_us: ts,
        unit: unit.map(|s| s.to_string()),
        pid,
        identifier: identifier.map(|s| s.to_string()),
    }
}

// ---------- 過濾 ----------

#[test]
fn filter_no_constraints_matches_all() {
    let f = JournalFilter::default();
    assert!(f.matches(&mk("anything", 7, 42, None, None, None)));
}

#[test]
fn filter_unit_matches_unit_field() {
    let f = JournalFilter {
        units: vec!["nginx.service".to_string()],
        ..Default::default()
    };
    assert!(f.matches(&mk("m", 6, 0, Some("nginx.service"), None, None)));
    assert!(!f.matches(&mk("m", 6, 0, Some("apache.service"), None, None)));
}

#[test]
fn filter_unit_matches_identifier_field() {
    let f = JournalFilter {
        units: vec!["myapp".to_string()],
        ..Default::default()
    };
    assert!(f.matches(&mk("m", 6, 0, None, None, Some("myapp"))));
    assert!(!f.matches(&mk("m", 6, 0, None, None, Some("other"))));
}

#[test]
fn filter_unit_multiple_any_of() {
    let f = JournalFilter {
        units: vec!["a.service".to_string(), "b.service".to_string()],
        ..Default::default()
    };
    assert!(f.matches(&mk("m", 6, 0, Some("b.service"), None, None)));
    assert!(!f.matches(&mk("m", 6, 0, Some("c.service"), None, None)));
}

#[test]
fn filter_priority_max_inclusive() {
    let f = JournalFilter {
        priority_max: Some(3),
        ..Default::default()
    };
    assert!(f.matches(&mk("m", 3, 0, None, None, None)));
    assert!(!f.matches(&mk("m", 4, 0, None, None, None)));
}

#[test]
fn filter_since_inclusive() {
    let f = JournalFilter {
        since_us: Some(100),
        ..Default::default()
    };
    assert!(f.matches(&mk("m", 6, 100, None, None, None)));
    assert!(!f.matches(&mk("m", 6, 99, None, None, None)));
}

#[test]
fn filter_until_inclusive() {
    let f = JournalFilter {
        until_us: Some(200),
        ..Default::default()
    };
    assert!(f.matches(&mk("m", 6, 200, None, None, None)));
    assert!(!f.matches(&mk("m", 6, 201, None, None, None)));
}

#[test]
fn filter_combined_and_semantics() {
    let f = JournalFilter {
        units: vec!["a.service".to_string()],
        priority_max: Some(4),
        since_us: Some(10),
        until_us: Some(20),
        ..Default::default()
    };
    assert!(f.matches(&mk("m", 3, 15, Some("a.service"), None, None)));
    assert!(!f.matches(&mk("m", 3, 25, Some("a.service"), None, None))); // 超出 until
    assert!(!f.matches(&mk("m", 5, 15, Some("a.service"), None, None))); // 優先級太高
    assert!(!f.matches(&mk("m", 3, 15, Some("b.service"), None, None))); // 單元不匹配
}

#[test]
fn filter_pattern_substring_case_insensitive() {
    let f = JournalFilter {
        patterns: vec!["ERROR".to_string()],
        ..Default::default()
    };
    assert!(f.matches(&mk("disk error occurred", 6, 0, None, None, None)));
    assert!(!f.matches(&mk("all good", 6, 0, None, None, None)));
}

#[test]
fn filter_reject_nonmatching() {
    let f = JournalFilter {
        units: vec!["x.service".to_string()],
        ..Default::default()
    };
    assert!(!f.matches(&mk("m", 6, 0, None, None, None)));
}

// ---------- 優先級解析 ----------

#[test]
fn priority_numeric() {
    assert_eq!(parse_priority("3"), Some(3));
    assert_eq!(parse_priority("0"), Some(0));
    assert_eq!(parse_priority("7"), Some(7));
}

#[test]
fn priority_name_err() {
    assert_eq!(parse_priority("err"), Some(3));
    assert_eq!(parse_priority("info"), Some(6));
}

#[test]
fn priority_case_insensitive_and_aliases() {
    assert_eq!(parse_priority("EMERG"), Some(0));
    assert_eq!(parse_priority("Warning"), Some(4));
    assert_eq!(parse_priority("panic"), Some(0)); // emerg 別名
    assert_eq!(parse_priority("error"), Some(3)); // err 別名
    assert_eq!(parse_priority("warn"), Some(4));
}

#[test]
fn priority_bad_none() {
    assert_eq!(parse_priority("banana"), None);
    assert_eq!(parse_priority(""), None);
}

#[test]
fn priority_out_of_range_none() {
    assert_eq!(parse_priority("8"), None);
    assert_eq!(parse_priority("-1"), None);
    assert_eq!(parse_priority("999"), None);
}

// ---------- 時間參數解析 ----------

#[test]
fn datetime_full() {
    assert_eq!(
        parse_datetime_arg("2026-10-11 10:00:00"),
        Some(1_791_712_800_000_000)
    );
}

#[test]
fn datetime_date_only() {
    assert_eq!(
        parse_datetime_arg("2026-10-11"),
        Some(1_791_676_800_000_000)
    );
}

#[test]
fn datetime_iso_t_separator() {
    assert_eq!(
        parse_datetime_arg("2026-10-11T10:00:00"),
        Some(1_791_712_800_000_000)
    );
}

#[test]
fn datetime_bad_none() {
    assert_eq!(parse_datetime_arg("not a date"), None);
    assert_eq!(parse_datetime_arg("2026-13-40 99:99:99"), None);
}

// ---------- 輸出格式 ----------

#[test]
fn format_short_identifier_pid() {
    let e = mk("disk failing", 3, 1_791_712_801_000_000, None, Some(123), Some("smartd"));
    assert_eq!(
        format_entry(&e, OutputFormat::Short),
        "Oct 11 10:00:01 smartd[123]: disk failing"
    );
}

#[test]
fn format_short_no_pid() {
    let e = mk("job done", 5, 1_791_712_801_000_000, None, None, Some("cron"));
    assert_eq!(
        format_entry(&e, OutputFormat::Short),
        "Oct 11 10:00:01 cron: job done"
    );
}

#[test]
fn format_short_fallback_unit_no_ident() {
    let e = mk("worker started", 6, 1_791_712_801_000_000, Some("nginx.service"), None, None);
    assert_eq!(
        format_entry(&e, OutputFormat::Short),
        "Oct 11 10:00:01 nginx.service: worker started"
    );
}

#[test]
fn format_short_no_source() {
    let e = mk("bare message", 6, 1_791_712_801_000_000, None, None, None);
    assert_eq!(
        format_entry(&e, OutputFormat::Short),
        "Oct 11 10:00:01 bare message"
    );
}

#[test]
fn format_json_roundtrip() {
    let e = mk("round trip", 4, 12345, Some("u.service"), Some(9), Some("ident"));
    let line = format_entry(&e, OutputFormat::Json);
    let back: JournalEntry = serde_json::from_str(&line).expect("valid json");
    assert_eq!(back.message, "round trip");
    assert_eq!(back.priority, 4);
    assert_eq!(back.timestamp_us, 12345);
    assert_eq!(back.unit.as_deref(), Some("u.service"));
    assert_eq!(back.pid, Some(9));
    assert_eq!(back.identifier.as_deref(), Some("ident"));
}

#[test]
fn format_cat_message() {
    let e = mk("raw output\nline2", 6, 0, None, None, None);
    assert_eq!(format_entry(&e, OutputFormat::Cat), "raw output\nline2");
}

#[test]
fn output_format_parse() {
    assert_eq!(OutputFormat::parse("short"), Some(OutputFormat::Short));
    assert_eq!(OutputFormat::parse("json"), Some(OutputFormat::Json));
    assert_eq!(OutputFormat::parse("cat"), Some(OutputFormat::Cat));
    assert_eq!(OutputFormat::parse("verbose"), None);
    assert_eq!(OutputFormat::parse(""), None);
}
