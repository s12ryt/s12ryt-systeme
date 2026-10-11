//! Timer 集成調度器 — 把 systeme-core 的 timer 純邏輯接到 ManagerCore。
//!
//! 職責:
//! - 掃描 ManagerCore 中的 `.timer` 單元 (masked 跳過)
//! - 邊沿檢測單元 active/inactive 轉換, 維護 OnUnitActiveSec/OnUnitInactiveSec 錨點
//! - 快取每個 timer 的下次觸發時刻; config 變化 (daemon-reload) 自動重置
//! - systemd 語義: timer 首次被追蹤時若 monotonic 錨點候選已過期 → 立即觸發一次
//!
//! 觸發本身 (start_unit) 由調用方 (bin/systemd) 執行, 本模組只計算到期列表。

use std::collections::BTreeMap;

use chrono::{DateTime, Duration, Local, NaiveDateTime};

use systeme_core::manager::{ActiveState, ManagerCore};
use systeme_core::timer::{next_timer_fire, TimerConfig};
use systeme_core::service::TimeSpan;
use systeme_units::types::UnitType;

/// 單個 timer 單元的追蹤狀態
struct TimerTracking {
    cfg: TimerConfig,
    /// 觸發目標 (Unit= 或同名 .service)
    target: String,
    next: NextFire,
}

/// 下次觸發狀態
enum NextFire {
    /// 尚未計算 — 初始化時做「過期錨點立即觸發」檢查
    NotComputed,
    /// 具體觸發時刻
    At(NaiveDateTime),
    /// 已計算但無未來觸發點 (config 變化或錨點失效才會重算)
    Never,
}

/// 目標單元的激活/去激活錨點
struct UnitAnchors {
    active_at: Option<NaiveDateTime>,
    inactive_at: Option<NaiveDateTime>,
}

/// Timer 調度器: 每 tick 調用 [`TimerScheduler::due_timers`]。
pub struct TimerScheduler {
    boot: NaiveDateTime,
    daemon_startup: NaiveDateTime,
    per_timer: BTreeMap<String, TimerTracking>,
    unit_anchors: BTreeMap<String, UnitAnchors>,
    last_states: BTreeMap<String, bool>,
}

impl TimerScheduler {
    /// `boot`: 系統啟動時刻 (OnBootSec 錨點, 可用 now - /proc/uptime 近似);
    /// `daemon_startup`: 守護進程啟動時刻 (OnStartupSec 錨點)。
    pub fn new(boot: NaiveDateTime, daemon_startup: NaiveDateTime) -> Self {
        Self {
            boot,
            daemon_startup,
            per_timer: BTreeMap::new(),
            unit_anchors: BTreeMap::new(),
            last_states: BTreeMap::new(),
        }
    }

    /// 從系統讀取 boot 時刻: /proc/uptime 第一字段 (秒), 失敗時用 now 近似。
    pub fn detect_boot(now: DateTime<Local>) -> NaiveDateTime {
        let uptime_secs = std::fs::read_to_string("/proc/uptime")
            .ok()
            .and_then(|s| {
                s.split_whitespace()
                    .next()
                    .and_then(|f| f.parse::<f64>().ok())
            });
        match uptime_secs {
            Some(secs) => now
                .naive_local()
                - Duration::milliseconds((secs * 1000.0).round() as i64),
            None => now.naive_local(),
        }
    }

    /// 記錄目標單元激活時刻 (OnUnitActiveSec 錨點); 依賴該單元的 timer 下次觸發重算。
    pub fn record_activation(&mut self, unit: &str, at: NaiveDateTime) {
        let anchors = self
            .unit_anchors
            .entry(unit.to_string())
            .or_insert(UnitAnchors {
                active_at: None,
                inactive_at: None,
            });
        let changed = anchors.active_at != Some(at);
        anchors.active_at = Some(at);
        if changed {
            self.invalidate_timer_for(unit);
        }
    }

    /// 記錄目標單元去激活時刻 (OnUnitInactiveSec 錨點)。
    pub fn record_deactivation(&mut self, unit: &str, at: NaiveDateTime) {
        let anchors = self
            .unit_anchors
            .entry(unit.to_string())
            .or_insert(UnitAnchors {
                active_at: None,
                inactive_at: None,
            });
        let changed = anchors.inactive_at != Some(at);
        anchors.inactive_at = Some(at);
        if changed {
            self.invalidate_timer_for(unit);
        }
    }

    /// 當前追蹤中的 timer 單元名 (字典序)。
    pub fn tracked(&self) -> Vec<String> {
        self.per_timer.keys().cloned().collect()
    }

    /// 掃描 manager 單元表, 返回本 tick 到期的 `(timer 單元名, 觸發目標)` 列表。
    ///
    /// 內部: 先做邊沿檢測更新錨點, 再逐 timer 檢查快取的下次觸發時刻。
    pub fn due_timers(&mut self, core: &ManagerCore, now: NaiveDateTime) -> Vec<(String, String)> {
        self.update_edges(core, now);
        self.prune_tracked(core);

        let mut due = Vec::new();
        let timer_names: Vec<String> = core
            .units
            .iter()
            .filter(|(name, entry)| !entry.masked && is_timer_unit(name))
            .map(|(name, _)| name.clone())
            .collect();

        for name in timer_names {
            let entry = &core.units[&name];
            let cfg = TimerConfig::from_parsed(&entry.parsed);
            let target = timer_target(&cfg, &name);

            // config 變化 (daemon-reload) → 重置追蹤
            let needs_reset = self
                .per_timer
                .get(&name)
                .map(|t| t.cfg != cfg || t.target != target)
                .unwrap_or(false);
            if needs_reset {
                self.per_timer.remove(&name);
            }
            self.per_timer
                .entry(name.clone())
                .or_insert_with(|| TimerTracking {
                    cfg: cfg.clone(),
                    target: target.clone(),
                    next: NextFire::NotComputed,
                });

            // 錨點先算好 (update_edges 已跑完, 此處不再修改 unit_anchors,
            // 避免 self.anchors_for 與 per_timer 可變借用重疊)
            let anchors = self.anchors_for(&target);
            let tracking = self.per_timer.get_mut(&name).expect("just inserted");

            if matches!(tracking.next, NextFire::NotComputed) {
                if expired_span_candidate(&tracking.cfg, &anchors, now)
                    || calendar_due_now(&tracking.cfg, now)
                {
                    // systemd 語義: timer 激活時錨點/calendar 候選已到期 (含恰好等於) → 立即觸發
                    tracking.next = NextFire::At(now);
                } else {
                    tracking.next = match next_timer_fire(&tracking.cfg, now, &anchors) {
                        Some(t) => NextFire::At(t),
                        None => NextFire::Never,
                    };
                }
            }

            // 檢查是否到點 (含剛初始化為 At(now) 的)
            let fire_now = match &tracking.next {
                NextFire::At(t) => *t <= now,
                _ => false,
            };
            if fire_now {
                due.push((name.clone(), tracking.target.clone()));
                // 觸發後重算 (嚴格 > now, 不會同 tick 重複); 過期候選被忽略
                tracking.next = match next_timer_fire(&tracking.cfg, now, &anchors) {
                    Some(t) => NextFire::At(t),
                    None => NextFire::Never,
                };
            }
        }
        due
    }

    /// 邊沿檢測: 對比上次觀察的 active 狀態, 更新錨點。
    fn update_edges(&mut self, core: &ManagerCore, now: NaiveDateTime) {
        let observed: BTreeMap<String, bool> = core
            .units
            .iter()
            .map(|(name, entry)| (name.clone(), entry.runtime.state == ActiveState::Active))
            .collect();

        for (unit, active) in &observed {
            match self.last_states.get(unit) {
                Some(prev) if *prev != *active => {
                    if *active {
                        self.record_activation(unit, now);
                    } else {
                        self.record_deactivation(unit, now);
                    }
                }
                // 首次觀察已 active: 記錄近似時刻 (不覆蓋外部已記錄的精確值)
                None if *active
                    && self
                        .unit_anchors
                        .get(unit)
                        .and_then(|a| a.active_at)
                        .is_none()
                    => {
                        self.record_activation(unit, now);
                    }
                _ => {}
            }
        }
        self.last_states = observed;
    }

    /// 清理已不存在的 timer 追蹤 (daemon-reload 後單元被移除)。
    fn prune_tracked(&mut self, core: &ManagerCore) {
        self.per_timer
            .retain(|name, _| core.units.contains_key(name));
    }

    /// 錨點失效: 依賴指定目標單元的 timer 下次觸發需要重算。
    fn invalidate_timer_for(&mut self, unit: &str) {
        for tracking in self.per_timer.values_mut() {
            if tracking.target == unit {
                tracking.next = NextFire::NotComputed;
            }
        }
    }

    fn anchors_for(&self, target: &str) -> systeme_core::timer::TimerAnchors {
        let a = self.unit_anchors.get(target);
        systeme_core::timer::TimerAnchors {
            boot: Some(self.boot),
            unit_active: a.and_then(|x| x.active_at),
            unit_inactive: a.and_then(|x| x.inactive_at),
            daemon_startup: Some(self.daemon_startup),
        }
    }
}

fn is_timer_unit(name: &str) -> bool {
    matches!(
        UnitType::split_name(name),
        Some((_, UnitType::Timer))
    )
}

/// 觸發目標: Unit= 指定, 否則同名 .service。
fn timer_target(cfg: &TimerConfig, timer_name: &str) -> String {
    cfg.unit.clone().unwrap_or_else(|| {
        timer_name
            .strip_suffix(".timer")
            .map(|s| format!("{s}.service"))
            .unwrap_or_else(|| timer_name.to_string())
    })
}

/// 是否存在「已過期」的 monotonic 錨點候選 (錨點 + 時長 <= now)。
fn expired_span_candidate(
    cfg: &TimerConfig,
    anchors: &systeme_core::timer::TimerAnchors,
    now: NaiveDateTime,
) -> bool {
    let pairs = [
        (cfg.on_boot_sec, anchors.boot),
        (cfg.on_startup_sec, anchors.daemon_startup),
        (cfg.on_unit_active_sec, anchors.unit_active),
        (cfg.on_unit_inactive_sec, anchors.unit_inactive),
    ];
    pairs.iter().any(|(span, anchor)| match (span, anchor) {
        (Some(TimeSpan::Finite(secs)), Some(a)) => a
            .checked_add_signed(Duration::milliseconds((*secs * 1000.0).round() as i64))
            .map(|t| t <= now)
            .unwrap_or(false),
        _ => false,
    })
}

/// 首次追蹤時: 是否存在已到期的 calendar 時刻 (含恰好等於 now)。
///
/// next_timer_fire 是嚴格大於語義, 故用 now-1s 計算 — 若得出 <= now 的候選
/// 表示該時刻在 now 這一秒或之前, timer 激活時應立即觸發 (systemd 語義)。
fn calendar_due_now(cfg: &TimerConfig, now: NaiveDateTime) -> bool {
    if cfg.on_calendar.is_empty() {
        return false;
    }
    match now.checked_sub_signed(Duration::seconds(1)) {
        Some(before) => {
            // 錨點全 None → 只算 calendar 候選
            next_timer_fire(cfg, before, &systeme_core::timer::TimerAnchors::default())
                .map(|t| t <= now)
                .unwrap_or(false)
        }
        None => false,
    }
}
