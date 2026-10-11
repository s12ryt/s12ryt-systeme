//! journald bin 的數據報處理輔助函數測試 (跨平台)。

use systeme_journal::store::{JsonlSink, MemorySink};
use systeme_journal::{process_datagram, JournalStore};

fn store_with(burst: usize, interval_ms: u64) -> (JournalStore, MemorySink) {
    let sink = MemorySink::default();
    let store = JournalStore::new(Box::new(sink.clone()), burst, interval_ms);
    (store, sink)
}

#[test]
fn syslog_line_is_accepted() {
    let (mut store, sink) = store_with(10, 10_000);
    let ok = process_datagram(
        b"<30>Oct 11 10:00:01 myhost myapp[4321]: hello",
        2026,
        &mut store,
        1_000,
    );
    assert!(ok);
    assert_eq!(sink.entries.borrow().len(), 1);
    let e = &sink.entries.borrow()[0];
    assert_eq!(e.message, "hello");
    assert_eq!(e.identifier.as_deref(), Some("myapp"));
}

#[test]
fn native_payload_is_accepted() {
    let (mut store, sink) = store_with(10, 10_000);
    let payload = b"MESSAGE=from native\nPRIORITY=5\n\n";
    let ok = process_datagram(payload, 2026, &mut store, 1_000);
    assert!(ok);
    assert_eq!(sink.entries.borrow().len(), 1);
    assert_eq!(sink.entries.borrow()[0].message, "from native");
}

#[test]
fn garbage_is_rejected() {
    let (mut store, sink) = store_with(10, 10_000);
    let ok = process_datagram(b"not a syslog line, not native", 2026, &mut store, 1_000);
    assert!(!ok);
    assert!(sink.entries.borrow().is_empty());
}

#[test]
fn rate_limit_rejects_second() {
    let (mut store, sink) = store_with(1, 10_000);
    assert!(process_datagram(b"MESSAGE=one\n", 2026, &mut store, 1_000));
    // burst=1: 同源第二條被拒
    assert!(!process_datagram(b"MESSAGE=two\n", 2026, &mut store, 2_000));
    assert_eq!(sink.entries.borrow().len(), 1);
    assert_eq!(store.dropped(), 1);
}

#[test]
fn multi_entry_native_all_submitted() {
    let (mut store, sink) = store_with(10, 10_000);
    let payload = b"MESSAGE=first\n\nMESSAGE=second\n\n";
    let ok = process_datagram(payload, 2026, &mut store, 1_000);
    assert!(ok);
    assert_eq!(sink.entries.borrow().len(), 2);
}

#[test]
fn jsonl_sink_used_by_process_datagram() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("j.jsonl");
    let mut store = JournalStore::new(
        Box::new(JsonlSink::create(&path).unwrap()),
        10,
        10_000,
    );
    assert!(process_datagram(b"MESSAGE=persisted\n", 2026, &mut store, 1_000));
    drop(store);
    let entries = systeme_journal::store::read_jsonl(&path);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].message, "persisted");
}
