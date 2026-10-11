//! path 單元 — 配置解析、glob 匹配、PathScheduler 輪詢觸發調度。
//!
//! 觸發語義 (systemd):
//! - 電平觸發 (Exists / ExistsGlob / DirectoryNotEmpty): 條件滿足即觸發一次,
//!   條件復位後再次滿足 → 再次觸發。激活時已滿足 → 立即觸發。
//! - 邊沿觸發 (Changed / Modified): mtime 變化觸發, 首輪只建 baseline 不觸發。

use systeme_units::parser::{parse_bool, ParsedUnit};

use crate::manager::{ActiveState, ManagerCore};

/// 觸發條件種類。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathTriggerKind {
    /// PathExists= 文件存在 (電平)
    Exists,
    /// PathExistsGlob= glob 匹配 (電平)
    ExistsGlob,
    /// PathChanged= 任何變化 (mtime 邊沿)
    Changed,
    /// PathModified= 內容修改 (輪詢模型下與 Changed 等價)
    Modified,
    /// DirectoryNotEmpty= 目錄非空 (電平)
    DirectoryNotEmpty,
}

/// 單條觸發聲明。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathTrigger {
    pub kind: PathTriggerKind,
    pub path: String,
}

/// [Path] 段解析結果。
#[derive(Debug, Clone, PartialEq)]
#[derive(Default)]
pub struct PathConfig {
    /// 按文件順序保序的觸發條件
    pub triggers: Vec<PathTrigger>,
    /// 顯式 Unit= 覆蓋 (None → 同名 .service)
    pub unit: Option<String>,
    pub make_directory: bool,
    pub directory_mode: Option<String>,
    pub warnings: Vec<String>,
}


impl PathConfig {
    /// 直接遍歷原始 entries: 跨 Path* key 保序, 空值清空同 key 累積。
    pub fn from_parsed(unit: &ParsedUnit) -> Self {
        let mut cfg = Self::default();
        let Some(section) = unit.section("Path") else {
            return cfg;
        };
        for entry in &section.entries {
            let v = entry.value.trim();
            let kind = match entry.key.as_str() {
                "PathExists" => PathTriggerKind::Exists,
                "PathExistsGlob" => PathTriggerKind::ExistsGlob,
                "PathChanged" => PathTriggerKind::Changed,
                "PathModified" => PathTriggerKind::Modified,
                "DirectoryNotEmpty" => PathTriggerKind::DirectoryNotEmpty,
                "Unit" => {
                    if !v.is_empty() {
                        cfg.unit = Some(v.to_string());
                    }
                    continue;
                }
                "MakeDirectory" => {
                    match parse_bool(v) {
                        Some(b) => cfg.make_directory = b,
                        None => cfg
                            .warnings
                            .push(format!("MakeDirectory value not boolean, ignoring: {v:?}")),
                    }
                    continue;
                }
                "DirectoryMode" => {
                    cfg.directory_mode = Some(v.to_string());
                    continue;
                }
                _ => continue,
            };
            if v.is_empty() {
                cfg.triggers.retain(|t| t.kind != kind);
            } else {
                cfg.triggers.push(PathTrigger {
                    kind,
                    path: v.to_string(),
                });
            }
        }
        cfg
    }
}

/// path 單元的觸發目標: Unit= 優先, 否則剝 .path 換 .service。
pub fn path_target(cfg: &PathConfig, path_unit_name: &str) -> String {
    if let Some(u) = &cfg.unit {
        return u.clone();
    }
    path_unit_name
        .strip_suffix(".path")
        .map(|base| format!("{base}.service"))
        .unwrap_or_else(|| path_unit_name.to_string())
}

/// 簡單 glob 匹配: `*` 任意序列 (含空)、`?` 單字符、其餘字面。
pub fn glob_match(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    let (mut pi, mut ti) = (0usize, 0usize);
    let (mut star, mut mark) = (None::<usize>, 0usize);
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = ti;
            pi += 1;
        } else if let Some(s) = star {
            pi = s + 1;
            mark += 1;
            ti = mark;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

/// 路徑探測快照 (輪詢籤名)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PathSnapshot {
    pub exists: bool,
    /// mtime (unix 秒); 不存在或不可得為 None
    pub mtime: Option<u64>,
    pub dir_nonempty: bool,
}

/// 文件系統探測抽象 (daemon 層用 std::fs 實現, 測試用 Mock)。
pub trait PathProbe {
    fn snapshot(&self, path: &str) -> PathSnapshot;
    /// glob 模式是否有匹配 (簡單 `*`/`?`)
    fn glob(&self, pattern: &str) -> bool;
}

/// 單條觸發條件的監視狀態。
#[derive(Debug, Clone, PartialEq, Eq)]
enum Watch {
    /// 電平: 上次條件是否滿足 (false 起步 → 激活時已滿足即觸發)
    Level(bool),
    /// 邊沿: 上次 mtime (None = baseline 未建立)
    Stamp(Option<u64>),
}

#[derive(Debug, Clone)]
struct PathTracking {
    cfg: PathConfig,
    target: String,
    watches: Vec<Watch>,
}

/// path 單元觸發調度器 — 輪詢邊沿檢測, 語義對齊 TimerScheduler。
#[derive(Debug, Default)]
pub struct PathScheduler {
    tracked: std::collections::BTreeMap<String, PathTracking>,
}

fn watch_init(kind: PathTriggerKind) -> Watch {
    match kind {
        PathTriggerKind::Exists
        | PathTriggerKind::ExistsGlob
        | PathTriggerKind::DirectoryNotEmpty => Watch::Level(false),
        PathTriggerKind::Changed | PathTriggerKind::Modified => Watch::Stamp(None),
    }
}

fn level_edge(watch: &mut Watch, now: bool) -> bool {
    if let Watch::Level(prev) = watch {
        let fire = now && !*prev;
        *prev = now;
        fire
    } else {
        false
    }
}

fn stamp_edge(watch: &mut Watch, now: Option<u64>) -> bool {
    if let Watch::Stamp(prev) = watch {
        let fire = match (*prev, now) {
            (Some(p), Some(n)) => n != p,
            _ => false,
        };
        *prev = now;
        fire
    } else {
        false
    }
}

impl PathScheduler {
    pub fn new() -> Self {
        Self::default()
    }

    /// 掃描所有 active 的 .path 單元, 返回本輪到期的 (path_unit, target) 列表 (字典序)。
    pub fn due_paths(&mut self, manager: &ManagerCore, probe: &dyn PathProbe) -> Vec<(String, String)> {
        let mut due: Vec<(String, String)> = Vec::new();

        // 清理已消失的單元
        self.tracked.retain(|name, _| manager.units.contains_key(name));

        for (name, entry) in manager.units.iter() {
            if !name.ends_with(".path") || entry.masked {
                continue;
            }
            if entry.runtime.state != ActiveState::Active {
                continue;
            }
            let cfg = PathConfig::from_parsed(&entry.parsed);
            let target = path_target(&cfg, name);

            let tracking = self.tracked.entry(name.clone()).or_insert_with(|| {
                PathTracking {
                    watches: cfg.triggers.iter().map(|t| watch_init(t.kind)).collect(),
                    cfg: cfg.clone(),
                    target: target.clone(),
                }
            });
            // cfg/target 變化 → 重置監視
            if tracking.cfg != cfg || tracking.target != target {
                tracking.cfg = cfg.clone();
                tracking.target = target.clone();
                tracking.watches = cfg.triggers.iter().map(|t| watch_init(t.kind)).collect();
            }

            for (i, trig) in cfg.triggers.iter().enumerate() {
                let fire = match trig.kind {
                    PathTriggerKind::Exists => {
                        level_edge(&mut tracking.watches[i], probe.snapshot(&trig.path).exists)
                    }
                    PathTriggerKind::ExistsGlob => {
                        level_edge(&mut tracking.watches[i], probe.glob(&trig.path))
                    }
                    PathTriggerKind::DirectoryNotEmpty => level_edge(
                        &mut tracking.watches[i],
                        probe.snapshot(&trig.path).dir_nonempty,
                    ),
                    PathTriggerKind::Changed | PathTriggerKind::Modified => {
                        stamp_edge(&mut tracking.watches[i], probe.snapshot(&trig.path).mtime)
                    }
                };
                if fire {
                    due.push((name.clone(), target.clone()));
                }
            }
        }

        due
    }

    /// 觀察: 當前追蹤的單元列表。
    pub fn tracked(&self) -> Vec<String> {
        self.tracked.keys().cloned().collect()
    }
}
