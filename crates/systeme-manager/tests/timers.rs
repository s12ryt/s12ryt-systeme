//! TimerScheduler 集成測試 — timer 單元掃描、邊沿錨點、下次觸發快取。
//!
//! 純邏輯層: ManagerCore 注入 MemFs 單元, 不需要真實進程。

use std::path::PathBuf;

use chrono::NaiveDateTime;

use systeme_core::manager::{ActiveState, ManagerCore, RuntimeMode};
use systeme_manager::timers::TimerScheduler;
use systeme_units::search::{MemFs, SearchPaths};

fn dt(s: &str) -> NaiveDateTime {
    NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S").unwrap()
}

fn mk_core(files: &[(&str, &str)]) -> ManagerCore {
    let mut fs = MemFs::new();
    for (path, content) in files {
        fs.file(path, content);
    }
    let sp = SearchPaths {
        dirs: vec![PathBuf::from("/x")],
    };
    let mut core = ManagerCore::new(RuntimeMode::User);
    core.load_all(&fs, &sp);
    core
}

fn sched(boot: &str, startup: &str) -> TimerScheduler {
    TimerScheduler::new(dt(boot), dt(startup))
}

const CAL_TIMER: &str = "[Unit]\nDescription=T\n\n[Timer]\nOnCalendar=*-*-* 10:00:00\n";

#[test]
fn no_timers_no_due() {
    let core = mk_core(&[("/x/foo.service", "[Service]\nExecStart=/bin/sleep 1\n")]);
    let mut s = sched("2026-10-11 09:00:00", "2026-10-11 10:00:00");
    assert!(s.due_timers(&core, dt("2026-10-11 10:00:00")).is_empty());
    assert!(s.tracked().is_empty());
}

#[test]
fn calendar_fires_at_exact_time() {
    let core = mk_core(&[
        ("/x/foo.timer", CAL_TIMER),
        ("/x/foo.service", "[Service]\nExecStart=/bin/sleep 1\n"),
    ]);
    let mut s = sched("2026-10-11 09:00:00", "2026-10-11 09:00:00");
    // 未到觸發時刻
    assert!(s.due_timers(&core, dt("2026-10-11 09:59:59")).is_empty());
    // 到點觸發, 目標默認同名 .service
    let due = s.due_timers(&core, dt("2026-10-11 10:00:00"));
    assert_eq!(due, vec![("foo.timer".to_string(), "foo.service".to_string())]);
    // 觸發後不重複 (下一次是明天 10:00)
    assert!(s.due_timers(&core, dt("2026-10-11 10:00:01")).is_empty());
}

#[test]
fn target_from_unit_directive() {
    let core = mk_core(&[
        (
            "/x/bar.timer",
            "[Timer]\nUnit=web.service\nOnCalendar=*-*-* 10:00:00\n",
        ),
        ("/x/web.service", "[Service]\nExecStart=/bin/sleep 1\n"),
    ]);
    let mut s = sched("2026-10-11 09:00:00", "2026-10-11 09:00:00");
    let due = s.due_timers(&core, dt("2026-10-11 10:00:00"));
    assert_eq!(
        due,
        vec![("bar.timer".to_string(), "web.service".to_string())]
    );
}

#[test]
fn boot_sec_elapses_immediately_when_anchor_passed() {
    // boot=09:00, OnBootSec=5min → 09:05 已過, timer 首次被追蹤時 (10:00) 立即觸發 (systemd 語義)
    let core = mk_core(&[
        ("/x/foo.timer", "[Timer]\nOnBootSec=5min\n"),
        ("/x/foo.service", "[Service]\nExecStart=/bin/sleep 1\n"),
    ]);
    let mut s = sched("2026-10-11 09:00:00", "2026-10-11 09:00:00");
    let due = s.due_timers(&core, dt("2026-10-11 10:00:00"));
    assert_eq!(due.len(), 1);
    // boot 錨點固定, 已觸發過不再重複
    assert!(s.due_timers(&core, dt("2026-10-11 10:00:01")).is_empty());
    assert!(s.due_timers(&core, dt("2026-10-11 11:00:00")).is_empty());
}

#[test]
fn boot_sec_future_fires_at_offset() {
    let core = mk_core(&[
        ("/x/foo.timer", "[Timer]\nOnBootSec=10min\n"),
        ("/x/foo.service", "[Service]\nExecStart=/bin/sleep 1\n"),
    ]);
    let mut s = sched("2026-10-11 09:55:00", "2026-10-11 09:55:00");
    assert!(s.due_timers(&core, dt("2026-10-11 10:00:00")).is_empty());
    assert!(s.due_timers(&core, dt("2026-10-11 10:04:59")).is_empty());
    let due = s.due_timers(&core, dt("2026-10-11 10:05:00"));
    assert_eq!(due.len(), 1);
}

#[test]
fn unit_active_edge_sets_anchor() {
    let core = mk_core(&[
        ("/x/foo.timer", "[Timer]\nOnUnitActiveSec=30s\n"),
        ("/x/foo.service", "[Service]\nExecStart=/bin/sleep 1\n"),
    ]);
    let mut s = sched("2026-10-11 09:00:00", "2026-10-11 09:00:00");
    // foo 尚未 active: 無錨點, 永不觸發
    assert!(s.due_timers(&core, dt("2026-10-11 10:00:00")).is_empty());
    assert!(s.due_timers(&core, dt("2026-10-11 12:00:00")).is_empty());

    // 注入 foo 變 active (模擬 start_unit 效果), 邊沿檢測記錄 active_at=10:00
    let core = {
        let mut core = core;
        core.units.get_mut("foo.service").unwrap().runtime.state = ActiveState::Active;
        core
    };
    // 同一 tick: 邊沿先記錄, 10:00+30s=10:00:30 > now → 不觸發
    assert!(s.due_timers(&core, dt("2026-10-11 10:00:00")).is_empty());
    assert!(s.due_timers(&core, dt("2026-10-11 10:00:29")).is_empty());
    let due = s.due_timers(&core, dt("2026-10-11 10:00:30"));
    assert_eq!(due.len(), 1);
}

#[test]
fn unit_inactive_edge_sets_anchor() {
    let core = mk_core(&[
        ("/x/foo.timer", "[Timer]\nOnUnitInactiveSec=30s\n"),
        ("/x/foo.service", "[Service]\nExecStart=/bin/sleep 1\n"),
    ]);
    let mut s = sched("2026-10-11 09:00:00", "2026-10-11 09:00:00");

    let mut core = core;
    // active (10:00) → inactive (10:00:10) 兩次邊沿
    core.units.get_mut("foo.service").unwrap().runtime.state = ActiveState::Active;
    s.due_timers(&core, dt("2026-10-11 10:00:00"));
    core.units.get_mut("foo.service").unwrap().runtime.state = ActiveState::Inactive;
    s.due_timers(&core, dt("2026-10-11 10:00:10"));
    // inactive_at=10:00:10 + 30s = 10:00:40
    assert!(s.due_timers(&core, dt("2026-10-11 10:00:39")).is_empty());
    let due = s.due_timers(&core, dt("2026-10-11 10:00:40"));
    assert_eq!(due.len(), 1);
}

#[test]
fn target_restart_rearms_unit_active_timer() {
    let core = mk_core(&[
        ("/x/foo.timer", "[Timer]\nOnUnitActiveSec=1min\n"),
        ("/x/foo.service", "[Service]\nExecStart=/bin/sleep 1\n"),
    ]);
    let mut s = sched("2026-10-11 09:00:00", "2026-10-11 09:00:00");
    let mut core = core;

    // foo 09:00 已激活 (record_activation 記錄精確時刻), timer 首次追蹤 10:00 → 錨點已過 → 立即觸發
    s.record_activation("foo.service", dt("2026-10-11 09:00:00"));
    core.units.get_mut("foo.service").unwrap().runtime.state = ActiveState::Active;
    assert_eq!(s.due_timers(&core, dt("2026-10-11 10:00:00")).len(), 1);
    // 舊錨點已過 → 不再觸發
    assert!(s.due_timers(&core, dt("2026-10-11 10:00:01")).is_empty());
    assert!(s.due_timers(&core, dt("2026-10-11 10:30:00")).is_empty());

    // foo 重啟: inactive (10:00:20) → active (10:00:30) 邊沿更新錨點 → 重新武裝
    core.units.get_mut("foo.service").unwrap().runtime.state = ActiveState::Inactive;
    s.due_timers(&core, dt("2026-10-11 10:00:20"));
    core.units.get_mut("foo.service").unwrap().runtime.state = ActiveState::Active;
    s.due_timers(&core, dt("2026-10-11 10:00:30"));
    // active_at=10:00:30 + 1min = 10:01:30
    assert!(s.due_timers(&core, dt("2026-10-11 10:01:29")).is_empty());
    assert_eq!(s.due_timers(&core, dt("2026-10-11 10:01:30")).len(), 1);
}

#[test]
fn config_change_resets_tracking() {
    let mut core = mk_core(&[
        ("/x/foo.timer", CAL_TIMER),
        ("/x/foo.service", "[Service]\nExecStart=/bin/sleep 1\n"),
    ]);
    let mut s = sched("2026-10-11 09:00:00", "2026-10-11 09:00:00");
    assert_eq!(s.due_timers(&core, dt("2026-10-11 10:00:00")).len(), 1);

    // daemon-reload 換成 11:00 的 calendar
    let mut fs = MemFs::new();
    fs.file("/x/foo.timer", "[Timer]\nOnCalendar=*-*-* 11:00:00\n");
    fs.file(
        "/x/foo.service",
        "[Service]\nExecStart=/bin/sleep 1\n",
    );
    let sp = SearchPaths {
        dirs: vec![PathBuf::from("/x")],
    };
    core.load_all(&fs, &sp);
    assert!(s.due_timers(&core, dt("2026-10-11 10:00:01")).is_empty());
    assert!(s.due_timers(&core, dt("2026-10-11 10:59:59")).is_empty());
    assert_eq!(s.due_timers(&core, dt("2026-10-11 11:00:00")).len(), 1);
}

#[test]
fn masked_timer_skipped() {
    let mut fs = MemFs::new();
    fs.file(
        "/x/foo.service",
        "[Service]\nExecStart=/bin/sleep 1\n",
    );
    fs.symlink("/x/evil.timer", "/dev/null");
    let sp = SearchPaths {
        dirs: vec![PathBuf::from("/x")],
    };
    let mut core = ManagerCore::new(RuntimeMode::User);
    core.load_all(&fs, &sp);

    let mut s = sched("2026-10-11 09:00:00", "2026-10-11 09:00:00");
    assert!(s.due_timers(&core, dt("2026-10-11 10:00:00")).is_empty());
    assert!(!s.tracked().iter().any(|t| t == "evil.timer"));
}

#[test]
fn due_list_sorted_deterministically() {
    let core = mk_core(&[
        ("/x/z.timer", "[Timer]\nOnCalendar=*-*-* 10:00:00\n"),
        ("/x/a.timer", "[Timer]\nOnCalendar=*-*-* 10:00:00\n"),
        ("/x/m.timer", "[Timer]\nOnCalendar=*-*-* 10:00:00\n"),
    ]);
    let mut s = sched("2026-10-11 09:00:00", "2026-10-11 09:00:00");
    let due = s.due_timers(&core, dt("2026-10-11 10:00:00"));
    let names: Vec<&str> = due.iter().map(|(t, _)| t.as_str()).collect();
    assert_eq!(names, vec!["a.timer", "m.timer", "z.timer"]);
}

#[test]
fn tracked_pruned_when_unit_dropped() {
    let mut core = mk_core(&[
        ("/x/foo.timer", CAL_TIMER),
        ("/x/foo.service", "[Service]\nExecStart=/bin/sleep 1\n"),
    ]);
    let mut s = sched("2026-10-11 09:00:00", "2026-10-11 09:00:00");
    s.due_timers(&core, dt("2026-10-11 09:30:00"));
    assert_eq!(s.tracked(), vec!["foo.timer".to_string()]);

    // reload 後 foo.timer 消失
    let fs = MemFs::new();
    let sp = SearchPaths {
        dirs: vec![PathBuf::from("/x")],
    };
    core.load_all(&fs, &sp);
    s.due_timers(&core, dt("2026-10-11 09:31:00"));
    assert!(s.tracked().is_empty());
}

#[test]
fn startup_sec_uses_daemon_startup_anchor() {
    let core = mk_core(&[
        ("/x/foo.timer", "[Timer]\nOnStartupSec=2min\n"),
        ("/x/foo.service", "[Service]\nExecStart=/bin/sleep 1\n"),
    ]);
    // daemon 09:58 啟動 + 2min = 10:00
    let mut s = sched("2026-10-11 09:00:00", "2026-10-11 09:58:00");
    assert!(s.due_timers(&core, dt("2026-10-11 09:59:59")).is_empty());
    assert_eq!(s.due_timers(&core, dt("2026-10-11 10:00:00")).len(), 1);
    // 已觸發不再重複
    assert!(s.due_timers(&core, dt("2026-10-11 10:00:01")).is_empty());
}

#[test]
fn multiple_specs_fires_each_elapse() {
    // 同一 timer 多個 OnCalendar 值
    let core = mk_core(&[
        (
            "/x/foo.timer",
            "[Timer]\nOnCalendar=*-*-* 10:00:00\nOnCalendar=*-*-* 10:30:00\n",
        ),
        ("/x/foo.service", "[Service]\nExecStart=/bin/sleep 1\n"),
    ]);
    let mut s = sched("2026-10-11 09:00:00", "2026-10-11 09:00:00");
    assert_eq!(s.due_timers(&core, dt("2026-10-11 10:00:00")).len(), 1);
    assert!(s.due_timers(&core, dt("2026-10-11 10:29:59")).is_empty());
    assert_eq!(s.due_timers(&core, dt("2026-10-11 10:30:00")).len(), 1);
}

#[test]
fn record_activation_deactivation_public_api() {
    // 公開記錄 API: 不經邊沿檢測直接記錄 (bin 層或其他集成可用)
    let core = mk_core(&[
        ("/x/foo.timer", "[Timer]\nOnUnitActiveSec=10s\n"),
        ("/x/foo.service", "[Service]\nExecStart=/bin/sleep 1\n"),
    ]);
    let mut s = sched("2026-10-11 09:00:00", "2026-10-11 09:00:00");
    s.record_activation("foo.service", dt("2026-10-11 10:00:00"));
    // 錨點記錄後 timer 追蹤仍正常計算: 10:00+10s
    assert!(s.due_timers(&core, dt("2026-10-11 10:00:05")).is_empty());
    assert_eq!(s.due_timers(&core, dt("2026-10-11 10:00:10")).len(), 1);
}
