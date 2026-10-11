//! cgroup 抽象層 TDD 測試: 後端探測 (v2/v1/none)、slice 層級計算、
//! cgroup 路徑計算、內存假後端 (MemCgroup)、無 cgroup 降級後端 (NoCgroup)。

use systeme_core::cgroup::{
    default_cgroup_path, detect_backend, slice_parent, CgroupBackend, CgroupError, CgroupMode,
    MemCgroup, NoCgroup,
};

// ─── 後端探測 ────────────────────────────────────────────────────────

#[test]
fn detect_unified_v2() {
    // cgroup.controllers 檔案存在 → v2 unified
    let probe = |p: &str| p == "/sys/fs/cgroup/cgroup.controllers";
    assert_eq!(detect_backend(&probe), CgroupMode::V2);
}

#[test]
fn detect_legacy_v1() {
    // memory controller 目錄 → v1
    let probe = |p: &str| p == "/sys/fs/cgroup/memory";
    assert_eq!(detect_backend(&probe), CgroupMode::V1);
}

#[test]
fn detect_none() {
    let probe = |_: &str| false;
    assert_eq!(detect_backend(&probe), CgroupMode::None);
}

#[test]
fn detect_v2_wins_on_hybrid() {
    // hybrid 系統 (兩者皆在) → 選 v2
    let probe = |p: &str| {
        p == "/sys/fs/cgroup/cgroup.controllers" || p == "/sys/fs/cgroup/memory"
    };
    assert_eq!(detect_backend(&probe), CgroupMode::V2);
}

// ─── slice 層級 (命名語義) ───────────────────────────────────────────

#[test]
fn slice_parent_multi_segment() {
    assert_eq!(slice_parent("a-b-c.slice").as_deref(), Some("a-b.slice"));
    assert_eq!(slice_parent("a-b.slice").as_deref(), Some("a.slice"));
}

#[test]
fn slice_parent_top_level_is_root() {
    assert_eq!(slice_parent("a.slice").as_deref(), Some("-.slice"));
}

#[test]
fn slice_root_has_no_parent() {
    assert_eq!(slice_parent("-.slice"), None);
}

#[test]
fn slice_parent_respects_escapes() {
    // \x2d 是字面 '-', 不算分隔符: "a\x2db.slice" 是頂層 slice
    assert_eq!(slice_parent(r"a\x2db.slice").as_deref(), Some("-.slice"));
    // "a-b\x2dc.slice" 的分隔符是 a 與 b\x2dc 之間那個
    assert_eq!(
        slice_parent(r"a-b\x2dc.slice").as_deref(),
        Some("a.slice")
    );
}

// ─── cgroup 路徑計算 ─────────────────────────────────────────────────

#[test]
fn service_defaults_to_system_slice() {
    assert_eq!(
        default_cgroup_path("nginx.service", None).as_deref(),
        Some("/system.slice/nginx.service")
    );
}

#[test]
fn service_with_custom_slice() {
    assert_eq!(
        default_cgroup_path("web.service", Some("foo-bar.slice")).as_deref(),
        Some("/foo.slice/foo-bar.slice/web.service")
    );
}

#[test]
fn slice_own_path_recursive() {
    assert_eq!(default_cgroup_path("-.slice", None).as_deref(), Some(""));
    assert_eq!(
        default_cgroup_path("a.slice", None).as_deref(),
        Some("/a.slice")
    );
    assert_eq!(
        default_cgroup_path("a-b.slice", None).as_deref(),
        Some("/a.slice/a-b.slice")
    );
}

#[test]
fn scope_like_service() {
    assert_eq!(
        default_cgroup_path("session-1.scope", None).as_deref(),
        Some("/system.slice/session-1.scope")
    );
}

#[test]
fn invalid_unit_names_rejected() {
    assert_eq!(default_cgroup_path("../evil.service", None), None);
    assert_eq!(default_cgroup_path("a/b.service", None), None);
    assert_eq!(default_cgroup_path("", None), None);
}

// ─── MemCgroup: 內存假後端 ───────────────────────────────────────────

#[test]
fn mem_create_and_exists() {
    let mut cg = MemCgroup::new();
    assert!(!cg.group_exists("/system.slice/nginx.service"));
    cg.create_group("/system.slice/nginx.service").unwrap();
    assert!(cg.group_exists("/system.slice/nginx.service"));
}

#[test]
fn mem_create_builds_parents() {
    let mut cg = MemCgroup::new();
    cg.create_group("/a.slice/a-b.slice/x.service").unwrap();
    assert!(cg.group_exists("/a.slice"));
    assert!(cg.group_exists("/a.slice/a-b.slice"));
}

#[test]
fn mem_attach_and_list_pids() {
    let mut cg = MemCgroup::new();
    cg.create_group("/g.service").unwrap();
    cg.attach_pid("/g.service", 100).unwrap();
    cg.attach_pid("/g.service", 101).unwrap();
    assert_eq!(cg.list_pids("/g.service").unwrap(), vec![100, 101]);
}

#[test]
fn mem_attach_missing_group_errors() {
    let mut cg = MemCgroup::new();
    let err = cg.attach_pid("/nope.service", 1).unwrap_err();
    assert!(matches!(err, CgroupError::NotFound(_)));
}

#[test]
fn mem_kill_group_returns_pids() {
    let mut cg = MemCgroup::new();
    cg.create_group("/g.service").unwrap();
    cg.attach_pid("/g.service", 7).unwrap();
    cg.attach_pid("/g.service", 8).unwrap();
    let killed = cg.kill_group("/g.service", 15).unwrap();
    assert_eq!(killed, vec![7, 8]);
    // 信號記錄可供斷言
    assert_eq!(cg.signal_log(), &[(7, 15), (8, 15)]);
}

#[test]
fn mem_kill_missing_group_is_empty_ok() {
    // 殺不存在的組: 無 pid 可殺, Ok(空)
    let mut cg = MemCgroup::new();
    assert_eq!(cg.kill_group("/nope", 15).unwrap(), Vec::<i32>::new());
}

#[test]
fn mem_remove_group() {
    let mut cg = MemCgroup::new();
    cg.create_group("/g").unwrap();
    cg.remove_group("/g").unwrap();
    assert!(!cg.group_exists("/g"));
    assert!(matches!(cg.remove_group("/g"), Err(CgroupError::NotFound(_))));
}

#[test]
fn mem_remove_missing_errors() {
    let mut cg = MemCgroup::new();
    assert!(matches!(
        cg.remove_group("/never"),
        Err(CgroupError::NotFound(_))
    ));
}

// ─── NoCgroup: 無 cgroup 降級後端 (進程追蹤替代) ─────────────────────

#[test]
fn no_cgroup_is_graceful_noop() {
    let mut cg = NoCgroup;
    assert_eq!(cg.mode(), CgroupMode::None);
    // 一切操作成功但無實際效果
    cg.create_group("/system.slice/x.service").unwrap();
    assert!(!cg.group_exists("/system.slice/x.service"));
    cg.attach_pid("/system.slice/x.service", 42).unwrap();
    assert_eq!(cg.list_pids("/system.slice/x.service").unwrap(), vec![]);
    // kill_group 返回空 — 管理器應改用進程追蹤殺進程
    assert_eq!(cg.kill_group("/system.slice/x.service", 15).unwrap(), vec![]);
    cg.remove_group("/system.slice/x.service").unwrap();
}
