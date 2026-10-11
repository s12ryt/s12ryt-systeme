//! Timer 單元配置解析與下次觸發計算。
//!
//! 對應 systemd 的 `.timer` 單元: [Timer] 段的 OnCalendar /
//! OnBootSec / OnStartupSec / OnUnitActiveSec / OnUnitInactiveSec
//! 等指令。`next_timer_fire` 以「錨點 + 時長」與日曆表達式兩類
//! 候選取最早未來時刻,供 daemon 層排定 tick。

use chrono::{Duration, NaiveDateTime};
use systeme_units::parser::ParsedUnit;

use crate::calendar::{next_elapse, parse_calendar, CalendarSpec};
use crate::service::{parse_time_span, TimeSpan};

/// systemd 默認 `AccuracySec=1min`。
pub const DEFAULT_ACCURACY_SECS: f64 = 60.0;

/// [Timer] 段配置 (未知指令由解析器層警告, 這裡只關心已知鍵)。
#[derive(Debug, Clone, PartialEq)]
pub struct TimerConfig {
    /// `OnCalendar=` 可能多行/一行多值, 全部收集。
    pub on_calendar: Vec<CalendarSpec>,
    /// `OnBootSec=` — 開機錨點 (boot) 起算。
    pub on_boot_sec: Option<TimeSpan>,
    /// `OnStartupSec=` — daemon 啟動錨點起算 (systemd 語義為 manager 啟動)。
    pub on_startup_sec: Option<TimeSpan>,
    /// `OnUnitActiveSec=` — 目標單元上次激活錨點起算。
    pub on_unit_active_sec: Option<TimeSpan>,
    /// `OnUnitInactiveSec=` — 目標單元上次停止錨點起算。
    pub on_unit_inactive_sec: Option<TimeSpan>,
    /// `Unit=` 顯式指定觸發目標 (默認為同名 .service)。
    pub unit: Option<String>,
    /// `AccuracySec=` 默認 1min (timer 層允許的延遲鬆弛量)。
    pub accuracy_sec: TimeSpan,
    /// `RandomizedDelaySec=` 默認 0 (額外隨機延遲, 計算層僅存儲)。
    pub randomized_delay_sec: TimeSpan,
    /// `Persistent=` 丟失觸發補跑 (跨重啟持久化, 計算層僅存儲旗標)。
    pub persistent: bool,
    /// 解析期警告 (壞值跳過但保留事件)。
    pub warnings: Vec<String>,
}

impl Default for TimerConfig {
    fn default() -> Self {
        Self {
            on_calendar: Vec::new(),
            on_boot_sec: None,
            on_startup_sec: None,
            on_unit_active_sec: None,
            on_unit_inactive_sec: None,
            unit: None,
            accuracy_sec: TimeSpan::Finite(DEFAULT_ACCURACY_SECS),
            randomized_delay_sec: TimeSpan::Finite(0.0),
            persistent: false,
            warnings: Vec::new(),
        }
    }
}

impl TimerConfig {
    /// 從已解析單元提取 [Timer] 段。缺段 → 全默認無警告;
    /// 壞值 → 警告 + 跳過該行 (高兼容不報錯)。
    pub fn from_parsed(unit: &ParsedUnit) -> Self {
        let mut cfg = Self::default();
        let Some(section) = unit.section("Timer") else {
            return cfg;
        };

        for value in section.get_list("OnCalendar") {
            match parse_calendar(value) {
                Ok(spec) => cfg.on_calendar.push(spec),
                Err(e) => cfg
                    .warnings
                    .push(format!("OnCalendar=\"{value}\": {e}, skipped")),
            }
        }

        cfg.on_boot_sec = parse_span_field(section.get("OnBootSec"), "OnBootSec", &mut cfg.warnings);
        cfg.on_startup_sec = parse_span_field(
            section.get("OnStartupSec"),
            "OnStartupSec",
            &mut cfg.warnings,
        );
        cfg.on_unit_active_sec = parse_span_field(
            section.get("OnUnitActiveSec"),
            "OnUnitActiveSec",
            &mut cfg.warnings,
        );
        cfg.on_unit_inactive_sec = parse_span_field(
            section.get("OnUnitInactiveSec"),
            "OnUnitInactiveSec",
            &mut cfg.warnings,
        );

        if let Some(name) = section.get("Unit") {
            if !name.is_empty() {
                cfg.unit = Some(name.to_string());
            }
        }

        if let Some(v) = section.get("AccuracySec") {
            match parse_time_span(v) {
                Ok(s) => cfg.accuracy_sec = s,
                Err(()) => cfg
                    .warnings
                    .push(format!("AccuracySec=\"{v}\": invalid time span, using default")),
            }
        }
        if let Some(v) = section.get("RandomizedDelaySec") {
            match parse_time_span(v) {
                Ok(s) => cfg.randomized_delay_sec = s,
                Err(()) => cfg
                    .warnings
                    .push(format!("RandomizedDelaySec=\"{v}\": invalid time span, using default")),
            }
        }

        cfg.persistent = section.get_flag("Persistent");

        cfg
    }
}

/// 解析單值時長字段: `infinity` 與壞值 → 警告 + None (timer 語義下
/// 無限時長沒有意義, systemd 同樣報錯)。
fn parse_span_field(
    raw: Option<&str>,
    field: &'static str,
    warnings: &mut Vec<String>,
) -> Option<TimeSpan> {
    match raw {
        None => None,
        Some(v) => match parse_time_span(v) {
            Ok(TimeSpan::Infinity) => {
                warnings.push(format!("{field}=\"{v}\": infinite time span is invalid for timers, skipped"));
                None
            }
            Ok(span) => Some(span),
            Err(()) => {
                warnings.push(format!("{field}=\"{v}\": invalid time span, skipped"));
                None
            }
        },
    }
}

/// 觸發計算的錨點集合。缺失錨點對應的候選直接忽略。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TimerAnchors {
    /// 開機時刻 (OnBootSec 基準)。
    pub boot: Option<NaiveDateTime>,
    /// 目標單元上次激活時刻 (OnUnitActiveSec 基準)。
    pub unit_active: Option<NaiveDateTime>,
    /// 目標單元上次停止時刻 (OnUnitInactiveSec 基準)。
    pub unit_inactive: Option<NaiveDateTime>,
    /// daemon 啟動時刻 (OnStartupSec 基準)。
    pub daemon_startup: Option<NaiveDateTime>,
}

/// 計算下次觸發時刻: 所有候選 (日曆 + 錨點+時長) 中嚴格晚於 `now`
/// 的最早者。無候選 → None。
///
/// RandomizedDelaySec 屬於執行層鬆弛量 (真實 systemd 在觸發時刻上
/// 疊加隨機量), 這裡返回理論時刻, 由調用方決定延遲。
pub fn next_timer_fire(
    cfg: &TimerConfig,
    now: NaiveDateTime,
    anchors: &TimerAnchors,
) -> Option<NaiveDateTime> {
    let mut best: Option<NaiveDateTime> = None;
    let mut consider = |candidate: Option<NaiveDateTime>| {
        if let Some(t) = candidate {
            if t > now && best.is_none_or(|b| t < b) {
                best = Some(t);
            }
        }
    };

    for spec in &cfg.on_calendar {
        consider(next_elapse(spec, now));
    }

    for (span, anchor) in [
        (cfg.on_boot_sec, anchors.boot),
        (cfg.on_startup_sec, anchors.daemon_startup),
        (cfg.on_unit_active_sec, anchors.unit_active),
        (cfg.on_unit_inactive_sec, anchors.unit_inactive),
    ] {
        if let (Some(TimeSpan::Finite(secs)), Some(anchor)) = (span, anchor) {
            consider(anchor.checked_add_signed(Duration::milliseconds(
                (secs * 1000.0).round() as i64,
            )));
        }
    }

    best
}
