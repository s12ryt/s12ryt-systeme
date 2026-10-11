//! journald 測試 — native 協議解析、syslog 解析、rate limit、JSONL 存儲。

use systeme_journal::entry::{priority_name, JournalEntry};
use systeme_journal::native::parse_native;
use systeme_journal::rate::RateLimiter;
use systeme_journal::store::{read_jsonl, JournalStore, JsonlSink, MemorySink};
use systeme_journal::syslog::parse_syslog;

fn entry(unit: Option<&str>, message: &str) -> JournalEntry {
    JournalEntry {
        message: message.to_string(),
        priority: 6,
        timestamp_us: 0,
        unit: unit.map(|s| s.to_string()),
        pid: None,
        identifier: None,
    }
}

// ---------- native 協議 ----------

#[test]
fn native_basic_fields() {
    let payload = b"MESSAGE=hello world\nPRIORITY=5\n_PID=1234\n_SYSTEMD_UNIT=nginx.service\n\n";
    let entries = parse_native(payload);
    assert_eq!(entries.len(), 1);
    let e = &entries[0];
    assert_eq!(e.message, "hello world");
    assert_eq!(e.priority, 5);
    assert_eq!(e.pid, Some(1234));
    assert_eq!(e.unit.as_deref(), Some("nginx.service"));
}

#[test]
fn native_multiple_entries_split_by_blank_line() {
    let payload = b"MESSAGE=first\n\nMESSAGE=second\n\n";
    let entries = parse_native(payload);
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].message, "first");
    assert_eq!(entries[1].message, "second");
}

#[test]
fn native_binary_length_prefixed_value() {
    // 二進制形式: FIELD\n + 8 字節 LE 長度 + 數據 + \n
    let msg = b"line1\nline2"; // 含換行的消息必須走二進制形式
    let mut payload = Vec::new();
    payload.extend_from_slice(b"MESSAGE\n");
    payload.extend_from_slice(&(msg.len() as u64).to_le_bytes());
    payload.extend_from_slice(msg);
    payload.extend_from_slice(b"\n\n");
    let entries = parse_native(&payload);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].message, "line1\nline2");
}

#[test]
fn native_missing_message_dropped() {
    let payload = b"PRIORITY=3\n\n";
    assert!(parse_native(payload).is_empty());
}

#[test]
fn native_priority_defaults_and_bad_tolerated() {
    // 無 PRIORITY → 默認 6 (info)
    let e = &parse_native(b"MESSAGE=x\n\n")[0];
    assert_eq!(e.priority, 6);
    // 壞優先級 → 保持默認 6
    let e = &parse_native(b"MESSAGE=x\nPRIORITY=banana\n\n")[0];
    assert_eq!(e.priority, 6);
    // 超範圍 (>7) → 容忍為 6
    let e = &parse_native(b"MESSAGE=x\nPRIORITY=99\n\n")[0];
    assert_eq!(e.priority, 6);
}

#[test]
fn native_realtime_timestamp_micros() {
    let e = &parse_native(b"MESSAGE=x\n__REALTIME_TIMESTAMP=1697000000123456\n\n")[0];
    assert_eq!(e.timestamp_us, 1697000000123456);
}

#[test]
fn native_syslog_identifier() {
    let e = &parse_native(b"MESSAGE=x\nSYSLOG_IDENTIFIER=myscript\n\n")[0];
    assert_eq!(e.identifier.as_deref(), Some("myscript"));
}

// ---------- syslog (RFC3164) ----------

#[test]
fn syslog_standard_line() {
    let e = parse_syslog("<30>Oct 11 10:00:01 myhost myapp[4321]: something happened", 2026)
        .expect("parse ok");
    assert_eq!(e.message, "something happened");
    assert_eq!(e.priority, 30 % 8); // severity = PRI % 8 = 6
    assert_eq!(e.identifier.as_deref(), Some("myapp"));
    assert_eq!(e.pid, Some(4321));
    // 時間戳 (補 2026 年) → 微秒
    assert_eq!(e.timestamp_us, 1_791_712_801_000_000); // 2026-10-11 10:00:01 UTC
}

#[test]
fn syslog_no_pid_form() {
    let e = parse_syslog("<13>Oct 11 09:00:00 myhost cron: job done", 2026).expect("parse ok");
    assert_eq!(e.identifier.as_deref(), Some("cron"));
    assert_eq!(e.pid, None);
    assert_eq!(e.message, "job done");
    assert_eq!(e.priority, 13 % 8);
}

#[test]
fn syslog_malformed_returns_none() {
    assert!(parse_syslog("no pri prefix", 2026).is_none());
    assert!(parse_syslog("<abc>Oct 11 10:00:00 h t: m", 2026).is_none());
}

// ---------- priority 名稱 ----------

#[test]
fn priority_names() {
    let names = [
        "emerg", "alert", "crit", "err", "warning", "notice", "info", "debug",
    ];
    for (i, expect) in names.iter().enumerate() {
        assert_eq!(priority_name(i as u8), *expect);
    }
}

// ---------- rate limit ----------

#[test]
fn rate_limiter_burst_exhaustion_and_reset() {
    let mut rl = RateLimiter::new(3, 10_000);
    assert!(rl.allow(0));
    assert!(rl.allow(1));
    assert!(rl.allow(2));
    assert!(!rl.allow(3), "4th within window rejected");
    assert!(!rl.allow(9_999));
    // 窗口過後重置
    assert!(rl.allow(10_000));
}

#[test]
fn rate_limiter_partial_usage_carries_within_window() {
    let mut rl = RateLimiter::new(2, 10_000);
    assert!(rl.allow(0));
    assert!(rl.allow(5_000));
    assert!(!rl.allow(9_000), "still same window (0..10000)");
    assert!(rl.allow(10_001), "new window at t=10000");
}

// ---------- store ----------

#[test]
fn store_submits_to_sink() {
    let sink = MemorySink::default();
    let mut store = JournalStore::new(Box::new(sink.clone()), 100, 30_000);
    assert!(store.submit(&entry(Some("a.service"), "m1"), 1));
    assert!(store.submit(&entry(Some("b.service"), "m2"), 2));
    assert_eq!(sink.entries.borrow().len(), 2);
    assert_eq!(sink.entries.borrow()[1].message, "m2");
}

#[test]
fn store_rate_limits_per_source() {
    let sink = MemorySink::default();
    let mut store = JournalStore::new(Box::new(sink.clone()), 2, 10_000);
    assert!(store.submit(&entry(Some("a.service"), "1"), 0));
    assert!(store.submit(&entry(Some("a.service"), "1"), 1));
    assert!(!store.submit(&entry(Some("a.service"), "1"), 2), "3rd a rejected");
    assert!(store.submit(&entry(Some("b.service"), "1"), 3), "b unaffected");
    assert_eq!(store.dropped(), 1);
    assert_eq!(sink.entries.borrow().len(), 3);
    // 窗口過後 a 恢復
    assert!(store.submit(&entry(Some("a.service"), "1"), 10_000));
}

#[test]
fn store_source_key_falls_back_to_identifier_then_unknown() {
    let sink = MemorySink::default();
    let mut store = JournalStore::new(Box::new(sink), 1, 10_000);
    assert!(store.submit(&entry(None, "x"), 0));
    // unit=None identifier=None → "_unknown" — 第二條被限
    assert!(!store.submit(&entry(None, "x"), 1));
}

// ---------- JSONL ----------

#[test]
fn jsonl_roundtrip_and_bad_lines_skipped() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.journal");
    let sink = JsonlSink::create(&path).unwrap();
    let mut store = JournalStore::new(Box::new(sink), 100, 30_000);
    store.submit(&entry(Some("u.service"), "hello"), 5);
    store.submit(&entry(None, "world"), 6);
    drop(store);

    // 追加一條壞行 (模擬部分寫入損壞)
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new().append(true).open(&path).unwrap();
    writeln!(f, "{{not json").unwrap();
    drop(f);

    let entries = read_jsonl(&path);
    assert_eq!(entries.len(), 2, "bad line skipped");
    assert_eq!(entries[0].message, "hello");
    assert_eq!(entries[0].unit.as_deref(), Some("u.service"));
    assert_eq!(entries[1].message, "world");
}

#[test]
fn jsonl_empty_or_missing_file() {
    assert!(read_jsonl(std::path::Path::new("/nonexistent/x.journal")).is_empty());
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("empty.journal");
    std::fs::write(&path, "").unwrap();
    assert!(read_jsonl(&path).is_empty());
}
