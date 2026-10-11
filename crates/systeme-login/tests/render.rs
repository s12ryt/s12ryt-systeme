//! loginctl 輸出渲染測試。

use systeme_login::render::{render_session_detail, render_sessions_table};
use systeme_login::Sessions;

#[test]
fn table_lists_sessions_sorted() {
    let mut s = Sessions::new();
    let _a = s.create("alice", 1000, Some("seat0"), "tty1");
    let b = s.create("bob", 1001, None, "");
    let out = render_sessions_table(&s);
    assert!(out.contains("SESSION"), "header: {out}");
    assert!(out.contains("alice"));
    assert!(out.contains("bob"));
    // alice 行在 bob 行之前 (id 1 < 2)
    let pa = out.find("alice").expect("alice");
    let pb = out.find("bob").expect("bob");
    assert!(pa < pb);
    let _ = b;
}

#[test]
fn empty_table_has_header_only() {
    let s = Sessions::new();
    let out = render_sessions_table(&s);
    assert!(out.contains("SESSION"));
    assert!(!out.contains("alice"));
}

#[test]
fn detail_shows_fields() {
    let mut s = Sessions::new();
    let id = s.create("alice", 1000, Some("seat0"), "tty1");
    s.set_locked(&id, true).unwrap();
    let out = render_session_detail(&s, &id);
    assert!(out.contains(&id), "id: {out}");
    assert!(out.contains("alice"));
    assert!(out.contains("1000"));
    assert!(out.contains("seat0"));
    assert!(out.contains("tty1"));
    assert!(out.contains("active"));
    assert!(out.contains("LockedHint=yes"), "locked: {out}");
}
