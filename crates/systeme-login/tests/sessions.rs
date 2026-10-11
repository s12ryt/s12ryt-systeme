//! systeme-login: logind 會話/席位基礎 — 測試。

use systeme_login::{find_seat_of_session, SeatKind, SessionState, Sessions};

#[test]
fn create_session_assigns_id_and_active() {
    let mut s = Sessions::new();
    let id = s.create("alice", 1000, Some("seat0"), "tty1");
    assert!(!id.is_empty());
    let sess = s.get(&id).expect("session exists");
    assert_eq!(sess.user, "alice");
    assert_eq!(sess.uid, 1000);
    assert_eq!(sess.seat.as_deref(), Some("seat0"));
    assert_eq!(sess.tty.as_deref(), Some("tty1"));
    assert_eq!(sess.state, SessionState::Active);
}

#[test]
fn second_session_on_same_seat_marks_first_closing() {
    let mut s = Sessions::new();
    let a = s.create("alice", 1000, Some("seat0"), "tty1");
    let b = s.create("bob", 1001, Some("seat0"), "tty2");
    assert_ne!(a, b);
    // 新會話成為 active, 舊的轉 online (仍登入)
    assert_eq!(s.get(&a).unwrap().state, SessionState::Online);
    assert_eq!(s.get(&b).unwrap().state, SessionState::Active);
    assert_eq!(s.active_on_seat("seat0").as_deref(), Some(b.as_str()));
}

#[test]
fn session_without_seat_has_no_active_seat() {
    let mut s = Sessions::new();
    let id = s.create("carol", 1002, None, "");
    assert_eq!(s.get(&id).unwrap().seat, None);
    assert_eq!(s.active_on_seat("seat0"), None);
}

#[test]
fn close_session_removes_and_reactivates_next() {
    let mut s = Sessions::new();
    let a = s.create("alice", 1000, Some("seat0"), "tty1");
    let b = s.create("bob", 1001, Some("seat0"), "tty2");
    s.close(&b).expect("close ok");
    assert!(s.get(&b).is_none());
    // 剩餘會話被提升為 active
    assert_eq!(s.get(&a).unwrap().state, SessionState::Active);
    assert_eq!(s.active_on_seat("seat0").as_deref(), Some(a.as_str()));
}

#[test]
fn close_unknown_session_errs() {
    let mut s = Sessions::new();
    assert!(s.close("nope").is_err());
}

#[test]
fn close_last_session_marks_seat_dead_but_no_crash() {
    let mut s = Sessions::new();
    let a = s.create("alice", 1000, Some("seat0"), "tty1");
    s.close(&a).unwrap();
    assert_eq!(s.active_on_seat("seat0"), None);
}

#[test]
fn list_sessions_sorted_by_id() {
    let mut s = Sessions::new();
    s.create("bob", 1001, None, "");
    s.create("alice", 1000, None, "");
    let ids: Vec<String> = s.list().iter().map(|x| x.id.clone()).collect();
    let mut sorted = ids.clone();
    sorted.sort();
    assert_eq!(ids, sorted);
}

#[test]
fn sessions_of_user_filter() {
    let mut s = Sessions::new();
    let a = s.create("alice", 1000, None, "");
    s.create("bob", 1001, None, "");
    let mine = s.sessions_of_user(1000);
    assert_eq!(mine.len(), 1);
    assert_eq!(mine[0].id, a);
}

#[test]
fn lock_unlock_session() {
    let mut s = Sessions::new();
    let id = s.create("alice", 1000, None, "");
    assert!(!s.get(&id).unwrap().locked);
    s.set_locked(&id, true).expect("lock");
    assert!(s.get(&id).unwrap().locked);
    s.set_locked(&id, false).expect("unlock");
    assert!(!s.get(&id).unwrap().locked);
    assert!(s.set_locked("ghost", true).is_err());
}

#[test]
fn seat_kind_detection() {
    assert_eq!(SeatKind::classify("seat0"), SeatKind::Physical);
    assert_eq!(SeatKind::classify("seat17"), SeatKind::Physical);
    assert_eq!(SeatKind::classify("seat-1"), SeatKind::Virtual); // 非 seat<數字> 命名
    assert_eq!(SeatKind::classify("autovt"), SeatKind::Virtual); // 非 seat 開頭視為虛擬
    assert_eq!(SeatKind::classify("nosession"), SeatKind::Virtual);
    assert_eq!(SeatKind::classify("seat"), SeatKind::Virtual); // 無數字後綴
}

#[test]
fn find_seat_of_session_lookup() {
    let mut s = Sessions::new();
    let a = s.create("alice", 1000, Some("seat0"), "tty1");
    assert_eq!(find_seat_of_session(&s, &a).as_deref(), Some("seat0"));
    let b = s.create("bob", 1001, None, "");
    assert_eq!(find_seat_of_session(&s, &b), None);
    assert_eq!(find_seat_of_session(&s, "ghost"), None);
}

#[test]
fn session_state_display() {
    assert_eq!(SessionState::Online.as_str(), "online");
    assert_eq!(SessionState::Active.as_str(), "active");
    assert_eq!(SessionState::Closing.as_str(), "closing");
}

#[test]
fn idle_hint_toggle() {
    let mut s = Sessions::new();
    let id = s.create("alice", 1000, None, "");
    assert!(!s.get(&id).unwrap().idle);
    s.set_idle(&id, true).expect("set idle");
    assert!(s.get(&id).unwrap().idle);
    assert!(s.set_idle("ghost", true).is_err());
}

#[test]
fn next_session_id_monotonic_unique() {
    let mut s = Sessions::new();
    let a = s.create("a", 1, None, "");
    let b = s.create("b", 2, None, "");
    let c = s.create("c", 3, None, "");
    let mut set = std::collections::BTreeSet::new();
    for id in [a, b, c] {
        assert!(set.insert(id), "ids must be unique");
    }
}
