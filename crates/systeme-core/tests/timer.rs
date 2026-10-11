//! Timer 單元配置解析與下次觸發計算測試。
//!
//! 覆蓋: [Timer] 段全字段解析、默認值、壞值降級警告、
//! 錨點式觸發計算 (OnCalendar/OnBootSec/OnStartupSec/OnUnitActiveSec/OnUnitInactiveSec)。

use chrono::NaiveDateTime;
use systeme_units::parser::parse_unit;
use systeme_core::service::TimeSpan;
use systeme_core::timer::{next_timer_fire, TimerAnchors, TimerConfig};

fn dt(s: &str) -> NaiveDateTime {
    NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S").unwrap()
}

fn cfg(text: &str) -> TimerConfig {
    TimerConfig::from_parsed(&parse_unit(text))
}

// ---------- 解析 ----------

#[test]
fn empty_timer_section_defaults() {
    let c = cfg("[Timer]\n");
    assert!(c.on_calendar.is_empty());
    assert_eq!(c.on_boot_sec, None);
    assert_eq!(c.on_startup_sec, None);
    assert_eq!(c.on_unit_active_sec, None);
    assert_eq!(c.on_unit_inactive_sec, None);
    assert_eq!(c.unit, None);
    assert_eq!(c.accuracy_sec, TimeSpan::Finite(60.0)); // systemd 默認 1min
    assert!(!c.persistent);
    assert_eq!(c.randomized_delay_sec, TimeSpan::Finite(0.0));
    assert!(c.warnings.is_empty());
}

#[test]
fn missing_timer_section_defaults() {
    let c = cfg("[Unit]\nDescription=No timer here\n");
    assert!(c.on_calendar.is_empty());
    assert_eq!(c.accuracy_sec, TimeSpan::Finite(60.0));
    assert!(c.warnings.is_empty());
}

#[test]
fn on_calendar_single() {
    let c = cfg("[Timer]\nOnCalendar=daily\n");
    assert_eq!(c.on_calendar.len(), 1);
    assert!(c.warnings.is_empty());
}

#[test]
fn on_calendar_multiple_values() {
    let c = cfg("[Timer]\nOnCalendar=Mon..Fri 09:00:00\nOnCalendar=Sat 10:00:00\n");
    assert_eq!(c.on_calendar.len(), 2);
}

#[test]
fn on_calendar_bad_value_warns_and_skips() {
    let c = cfg("[Timer]\nOnCalendar=bogus!!\nOnCalendar=daily\n");
    assert_eq!(c.on_calendar.len(), 1); // 壞行跳過,好行保留
    assert_eq!(c.warnings.len(), 1);
    assert!(c.warnings[0].contains("OnCalendar"));
}

#[test]
fn time_spans_parsed() {
    let c = cfg("[Timer]\nOnBootSec=5min\nOnStartupSec=30s\nOnUnitActiveSec=1h 30s\nOnUnitInactiveSec=2d\n");
    assert_eq!(c.on_boot_sec, Some(TimeSpan::Finite(300.0)));
    assert_eq!(c.on_startup_sec, Some(TimeSpan::Finite(30.0)));
    assert_eq!(c.on_unit_active_sec, Some(TimeSpan::Finite(3630.0)));
    assert_eq!(c.on_unit_inactive_sec, Some(TimeSpan::Finite(172800.0)));
}

#[test]
fn infinite_span_warns_and_skips() {
    let c = cfg("[Timer]\nOnBootSec=infinity\nOnUnitActiveSec=1min\n");
    assert_eq!(c.on_boot_sec, None);
    assert_eq!(c.on_unit_active_sec, Some(TimeSpan::Finite(60.0)));
    assert!(c.warnings.iter().any(|w| w.contains("OnBootSec")));
}

#[test]
fn bad_span_warns_and_skips() {
    let c = cfg("[Timer]\nOnBootSec=not-a-time\n");
    assert_eq!(c.on_boot_sec, None);
    assert_eq!(c.warnings.len(), 1);
}

#[test]
fn unit_override() {
    let c = cfg("[Timer]\nUnit=backup.service\n");
    assert_eq!(c.unit, Some("backup.service".to_string()));
}

#[test]
fn accuracy_and_randomized_delay() {
    let c = cfg("[Timer]\nAccuracySec=30s\nRandomizedDelaySec=5min\n");
    assert_eq!(c.accuracy_sec, TimeSpan::Finite(30.0));
    assert_eq!(c.randomized_delay_sec, TimeSpan::Finite(300.0));
}

#[test]
fn persistent_flag() {
    let c = cfg("[Timer]\nPersistent=true\n");
    assert!(c.persistent);
}

// ---------- 下次觸發計算 ----------

#[test]
fn fire_from_calendar_only() {
    let c = cfg("[Timer]\nOnCalendar=daily\n");
    let now = dt("2026-10-11 10:00:00");
    let next = next_timer_fire(&c, now, &TimerAnchors::default()).unwrap();
    assert_eq!(next, dt("2026-10-12 00:00:00"));
}

#[test]
fn fire_picks_earliest_calendar() {
    let c = cfg("[Timer]\nOnCalendar=*-*-* 12:00:00\nOnCalendar=*:30:00\n");
    let now = dt("2026-10-11 10:00:00");
    let next = next_timer_fire(&c, now, &TimerAnchors::default()).unwrap();
    assert_eq!(next, dt("2026-10-11 10:30:00")); // 10:30 早於 12:00
}

#[test]
fn fire_on_boot_sec_future() {
    let c = cfg("[Timer]\nOnBootSec=5min\n");
    let now = dt("2026-10-11 10:00:00");
    let anchors = TimerAnchors {
        boot: Some(dt("2026-10-11 09:58:00")),
        ..TimerAnchors::default()
    };
    // boot+5min = 10:03 > now
    let next = next_timer_fire(&c, now, &anchors).unwrap();
    assert_eq!(next, dt("2026-10-11 10:03:00"));
}

#[test]
fn fire_on_boot_sec_past_ignored() {
    let c = cfg("[Timer]\nOnBootSec=5min\n");
    let now = dt("2026-10-11 10:00:00");
    let anchors = TimerAnchors {
        boot: Some(dt("2026-10-11 09:00:00")), // +5min = 09:05 已過
        ..TimerAnchors::default()
    };
    assert_eq!(next_timer_fire(&c, now, &anchors), None);
}

#[test]
fn fire_on_boot_sec_without_anchor_ignored() {
    let c = cfg("[Timer]\nOnBootSec=5min\n");
    let now = dt("2026-10-11 10:00:00");
    assert_eq!(next_timer_fire(&c, now, &TimerAnchors::default()), None);
}

#[test]
fn fire_on_unit_active_sec() {
    let c = cfg("[Timer]\nOnUnitActiveSec=15min\n");
    let now = dt("2026-10-11 10:00:00");
    let anchors = TimerAnchors {
        unit_active: Some(dt("2026-10-11 09:50:00")),
        ..TimerAnchors::default()
    };
    let next = next_timer_fire(&c, now, &anchors).unwrap();
    assert_eq!(next, dt("2026-10-11 10:05:00"));
}

#[test]
fn fire_on_unit_inactive_sec() {
    let c = cfg("[Timer]\nOnUnitInactiveSec=10min\n");
    let now = dt("2026-10-11 10:00:00");
    let anchors = TimerAnchors {
        unit_inactive: Some(dt("2026-10-11 09:55:00")),
        ..TimerAnchors::default()
    };
    let next = next_timer_fire(&c, now, &anchors).unwrap();
    assert_eq!(next, dt("2026-10-11 10:05:00"));
}

#[test]
fn fire_on_startup_sec() {
    let c = cfg("[Timer]\nOnStartupSec=1min\n");
    let now = dt("2026-10-11 10:00:00");
    let anchors = TimerAnchors {
        daemon_startup: Some(dt("2026-10-11 09:59:30")),
        ..TimerAnchors::default()
    };
    let next = next_timer_fire(&c, now, &anchors).unwrap();
    assert_eq!(next, dt("2026-10-11 10:00:30"));
}

#[test]
fn fire_mixed_picks_earliest() {
    let c = cfg("[Timer]\nOnCalendar=*-*-* 23:00:00\nOnUnitActiveSec=30min\n");
    let now = dt("2026-10-11 10:00:00");
    let anchors = TimerAnchors {
        unit_active: Some(dt("2026-10-11 09:45:00")),
        ..TimerAnchors::default()
    };
    let next = next_timer_fire(&c, now, &anchors).unwrap();
    assert_eq!(next, dt("2026-10-11 10:15:00")); // active+30m 早於 23:00
}

#[test]
fn fire_empty_config_none() {
    let c = cfg("[Timer]\n");
    let now = dt("2026-10-11 10:00:00");
    assert_eq!(next_timer_fire(&c, now, &TimerAnchors::default()), None);
}

#[test]
fn fire_strictly_after_now() {
    // 錨點恰好等於 now → 不觸發 (嚴格大於)
    let c = cfg("[Timer]\nOnUnitActiveSec=10min\n");
    let now = dt("2026-10-11 10:00:00");
    let anchors = TimerAnchors {
        unit_active: Some(dt("2026-10-11 09:50:00")),
        ..TimerAnchors::default()
    };
    assert_eq!(next_timer_fire(&c, now, &anchors), None);
}
