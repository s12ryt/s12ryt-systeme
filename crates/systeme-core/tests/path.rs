//! path 單元測試 — 配置解析、glob 匹配、PathScheduler 邊沿觸發調度。

use std::collections::BTreeMap;

use systeme_core::path::{
    glob_match, path_target, PathConfig, PathProbe, PathScheduler, PathSnapshot, PathTriggerKind,
};
use systeme_core::manager::{ActiveState, ManagerCore, RuntimeMode};
use systeme_units::parser::parse_unit;

fn cfg(text: &str) -> PathConfig {
    PathConfig::from_parsed(&parse_unit(text))
}

// ---------- 配置層 ----------

#[test]
fn path_missing_section_defaults() {
    let c = cfg("[Unit]\nDescription=x\n");
    assert!(c.triggers.is_empty());
    assert_eq!(c.unit, None);
    assert!(!c.make_directory);
    assert!(c.warnings.is_empty(), "no warnings: {:?}", c.warnings);
}

#[test]
fn path_triggers_parse_ordered() {
    let c = cfg("[Path]\nPathExists=/run/foo\nPathChanged=/etc/conf\nPathExistsGlob=/tmp/*.log\nDirectoryNotEmpty=/var/spool\nPathModified=/etc/conf2\n");
    assert_eq!(c.triggers.len(), 5);
    assert_eq!(c.triggers[0].kind, PathTriggerKind::Exists);
    assert_eq!(c.triggers[0].path, "/run/foo");
    assert_eq!(c.triggers[1].kind, PathTriggerKind::Changed);
    assert_eq!(c.triggers[2].kind, PathTriggerKind::ExistsGlob);
    assert_eq!(c.triggers[3].kind, PathTriggerKind::DirectoryNotEmpty);
    assert_eq!(c.triggers[4].kind, PathTriggerKind::Modified);
}

#[test]
fn path_trigger_multiple_values_one_line() {
    let c = cfg("[Path]\nPathExists=/run/a /run/b\n");
    // systemd: 每個指令值是單一路徑 (可含空格), 不分詞 — 但 systemd 250 前分行;
    // 我們按 systemd 文檔: 一行一值, 值整體為路徑。多值靠多行。
    assert_eq!(c.triggers.len(), 1);
    assert_eq!(c.triggers[0].path, "/run/a /run/b");
}

#[test]
fn path_empty_value_clears_kind() {
    let c = cfg("[Path]\nPathExists=/run/a\nPathExists=\n");
    assert!(c.triggers.is_empty());
}

#[test]
fn path_unit_target_default_and_override() {
    let c = cfg("[Path]\nPathExists=/run/a\n");
    assert_eq!(path_target(&c, "foo.path"), "foo.service");
    let c2 = cfg("[Path]\nPathExists=/run/a\nUnit=bar.service\n");
    assert_eq!(path_target(&c2, "foo.path"), "bar.service");
}

#[test]
fn path_make_directory_and_mode() {
    assert!(cfg("[Path]\nPathExists=/run/a\nMakeDirectory=yes\n").make_directory);
    assert!(!cfg("[Path]\nPathExists=/run/a\nMakeDirectory=maybe\n").make_directory);
    let c = cfg("[Path]\nPathExists=/run/a\nMakeDirectory=yes\nDirectoryMode=0700\n");
    assert_eq!(c.directory_mode.as_deref(), Some("0700"));
}

// ---------- glob ----------

#[test]
fn glob_match_basics() {
    assert!(glob_match("*.log", "a.log"));
    assert!(glob_match("*.log", "sys.log"));
    assert!(!glob_match("*.log", "a.txt"));
    assert!(glob_match("a?c", "abc"));
    assert!(glob_match("a?c", "axc"));
    assert!(!glob_match("a?c", "abbc"));
    assert!(glob_match("*", "anything"));
    assert!(glob_match("foo", "foo"));
    assert!(!glob_match("foo", "foobar"));
    assert!(glob_match("f*r", "foobar"));
    assert!(glob_match("**", "x"));
}

// ---------- PathScheduler ----------

/// 測試用探測器: 路徑 → 快照, glob → 集合。
#[derive(Default)]
struct MockProbe {
    snaps: BTreeMap<String, PathSnapshot>,
    globs: Vec<String>, // 已匹配的 glob 模式
}

impl PathProbe for MockProbe {
    fn snapshot(&self, path: &str) -> PathSnapshot {
        self.snaps.get(path).copied().unwrap_or(PathSnapshot {
            exists: false,
            mtime: None,
            dir_nonempty: false,
        })
    }
    fn glob(&self, pattern: &str) -> bool {
        self.globs.iter().any(|g| g == pattern)
    }
}

fn snap(exists: bool, mtime: Option<u64>, dir_nonempty: bool) -> PathSnapshot {
    PathSnapshot { exists, mtime, dir_nonempty }
}

/// 建 manager: 一個 active 的 watch.path 單元 + 目標 svc 占位。
fn manager_with_path_unit(unit_text: &str) -> ManagerCore {
    let mut m = ManagerCore::new(RuntimeMode::User);
    let dir = systeme_units::search::SearchPaths { dirs: vec![] };
    let mut fs = systeme_units::search::MemFs::new();
    fs.file("/lib/watch.path", unit_text);
    let sp = systeme_units::search::SearchPaths { dirs: vec!["/lib".into()] };
    let _ = dir;
    m.load_all(&fs, &sp);
    // 直接把 watch.path 標 Active (模擬已啟動監視)
    if let Some(e) = m.units.get_mut("watch.path") {
        e.runtime.state = ActiveState::Active;
    }
    m
}

#[test]
fn scheduler_initial_exists_fires_immediately() {
    let m = manager_with_path_unit("[Path]\nPathExists=/run/foo\nUnit=t.service\n");
    let mut sched = PathScheduler::new();
    let probe = MockProbe {
        snaps: BTreeMap::from([("/run/foo".into(), snap(true, None, false))]),
        globs: vec![],
    };
    let due = sched.due_paths(&m, &probe);
    assert_eq!(due, vec![("watch.path".to_string(), "t.service".to_string())]);
    // 持續滿足不重複
    assert!(sched.due_paths(&m, &probe).is_empty());
}

#[test]
fn scheduler_edge_fires_on_appear() {
    let m = manager_with_path_unit("[Path]\nPathExists=/run/foo\nUnit=t.service\n");
    let mut sched = PathScheduler::new();
    let absent = MockProbe::default();
    assert!(sched.due_paths(&m, &absent).is_empty());
    let present = MockProbe {
        snaps: BTreeMap::from([("/run/foo".into(), snap(true, Some(1), false))]),
        globs: vec![],
    };
    let due = sched.due_paths(&m, &present);
    assert_eq!(due.len(), 1);
    // 消失再出現 → 再觸發
    assert!(sched.due_paths(&m, &absent).is_empty());
    let again = MockProbe {
        snaps: BTreeMap::from([("/run/foo".into(), snap(true, Some(2), false))]),
        globs: vec![],
    };
    assert_eq!(sched.due_paths(&m, &again).len(), 1);
}

#[test]
fn scheduler_changed_mtime_baseline_then_fire() {
    let m = manager_with_path_unit("[Path]\nPathChanged=/etc/conf\nUnit=t.service\n");
    let mut sched = PathScheduler::new();
    let v1 = MockProbe {
        snaps: BTreeMap::from([("/etc/conf".into(), snap(true, Some(100), false))]),
        globs: vec![],
    };
    // 首輪只建 baseline, 不觸發 (systemd: inotify 從激活起算)
    assert!(sched.due_paths(&m, &v1).is_empty());
    let v2 = MockProbe {
        snaps: BTreeMap::from([("/etc/conf".into(), snap(true, Some(200), false))]),
        globs: vec![],
    };
    assert_eq!(sched.due_paths(&m, &v2).len(), 1);
    // mtime 不變 → 不觸發
    assert!(sched.due_paths(&m, &v2).is_empty());
}

#[test]
fn scheduler_glob_trigger() {
    let m = manager_with_path_unit("[Path]\nPathExistsGlob=/tmp/*.log\nUnit=t.service\n");
    let mut sched = PathScheduler::new();
    let none = MockProbe::default();
    assert!(sched.due_paths(&m, &none).is_empty());
    let hit = MockProbe { snaps: BTreeMap::new(), globs: vec!["/tmp/*.log".into()] };
    assert_eq!(sched.due_paths(&m, &hit).len(), 1);
}

#[test]
fn scheduler_directory_not_empty() {
    let m = manager_with_path_unit("[Path]\nDirectoryNotEmpty=/var/spool\nUnit=t.service\n");
    let mut sched = PathScheduler::new();
    let empty = MockProbe {
        snaps: BTreeMap::from([("/var/spool".into(), snap(true, None, false))]),
        globs: vec![],
    };
    assert!(sched.due_paths(&m, &empty).is_empty());
    let full = MockProbe {
        snaps: BTreeMap::from([("/var/spool".into(), snap(true, None, true))]),
        globs: vec![],
    };
    assert_eq!(sched.due_paths(&m, &full).len(), 1);
}

#[test]
fn scheduler_only_watches_active_units() {
    let mut m = manager_with_path_unit("[Path]\nPathExists=/run/foo\nUnit=t.service\n");
    if let Some(e) = m.units.get_mut("watch.path") {
        e.runtime.state = ActiveState::Inactive;
    }
    let mut sched = PathScheduler::new();
    let present = MockProbe {
        snaps: BTreeMap::from([("/run/foo".into(), snap(true, None, false))]),
        globs: vec![],
    };
    assert!(sched.due_paths(&m, &present).is_empty());
}

#[test]
fn scheduler_default_target_same_name_service() {
    let m = manager_with_path_unit("[Path]\nPathExists=/run/foo\n");
    let mut sched = PathScheduler::new();
    let present = MockProbe {
        snaps: BTreeMap::from([("/run/foo".into(), snap(true, None, false))]),
        globs: vec![],
    };
    let due = sched.due_paths(&m, &present);
    assert_eq!(due, vec![("watch.path".to_string(), "watch.service".to_string())]);
}
