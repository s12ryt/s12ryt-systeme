//! cgroup v2/v1 文件系統後端測試 (tempdir 真實文件操作, 跨平台)

use systeme_core::cgroup::{CgroupBackend, CgroupError, CgroupMode};
use systeme_manager::cgroup_fs::CgroupFsBackend;

fn backend(root: &std::path::Path) -> CgroupFsBackend {
    CgroupFsBackend::new_v2(root)
}

#[test]
fn v2_backend_reports_v2_mode() {
    let dir = tempfile::tempdir().unwrap();
    let b = backend(dir.path());
    assert_eq!(b.mode(), CgroupMode::V2);
}

#[test]
fn v1_backend_reports_v1_mode() {
    let dir = tempfile::tempdir().unwrap();
    let b = CgroupFsBackend::new_v1(dir.path());
    assert_eq!(b.mode(), CgroupMode::V1);
}

#[test]
fn create_group_builds_dir_chain() {
    let dir = tempfile::tempdir().unwrap();
    let mut b = backend(dir.path());
    b.create_group("/system.slice/web.service").unwrap();
    assert!(b.group_exists("/system.slice/web.service"));
    // 中間層也建立了
    assert!(b.group_exists("/system.slice"));
}

#[test]
fn create_group_idempotent() {
    let dir = tempfile::tempdir().unwrap();
    let mut b = backend(dir.path());
    b.create_group("/system.slice").unwrap();
    b.create_group("/system.slice").unwrap(); // 冪等
    assert!(b.group_exists("/system.slice"));
}

#[test]
fn create_group_with_leading_slash_stays_in_root() {
    // 帶前導斜杠的路徑不能逃離 root (PathBuf::join 陷阱)
    let dir = tempfile::tempdir().unwrap();
    let mut b = backend(dir.path());
    b.create_group("/system.slice").unwrap();
    assert!(dir.path().join("system.slice").is_dir());
    assert!(dir.path().join("system.slice").exists());
}

#[test]
fn attach_and_list_pids_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let mut b = backend(dir.path());
    b.create_group("/app.slice").unwrap();
    b.attach_pid("/app.slice", 42).unwrap();
    b.attach_pid("/app.slice", 7).unwrap();
    b.attach_pid("/app.slice", 42).unwrap(); // 重複 attach 無害
    let pids = b.list_pids("/app.slice").unwrap();
    // 升序且去重
    assert_eq!(pids, vec![7, 42]);
}

#[test]
fn attach_to_missing_group_errors_not_found() {
    let dir = tempfile::tempdir().unwrap();
    let mut b = backend(dir.path());
    let e = b.attach_pid("/nope.slice", 1).unwrap_err();
    assert!(matches!(e, CgroupError::NotFound(_)), "{e:?}");
}

#[test]
fn list_missing_group_errors_not_found() {
    let dir = tempfile::tempdir().unwrap();
    let b = backend(dir.path());
    let e = b.list_pids("/nope.slice").unwrap_err();
    assert!(matches!(e, CgroupError::NotFound(_)), "{e:?}");
}

#[test]
fn list_empty_group_returns_empty() {
    // 組存在但無 procs 文件 (空組) → Ok(空)
    let dir = tempfile::tempdir().unwrap();
    let mut b = backend(dir.path());
    b.create_group("/empty.slice").unwrap();
    assert_eq!(b.list_pids("/empty.slice").unwrap(), Vec::<i32>::new());
}

#[test]
fn kill_group_with_injectable_signal_fn() {
    let dir = tempfile::tempdir().unwrap();
    let mut b = backend(dir.path());
    b.create_group("/app.slice").unwrap();
    b.attach_pid("/app.slice", 11).unwrap();
    b.attach_pid("/app.slice", 5).unwrap();
    let mut sent: Vec<(i32, i32)> = Vec::new();
    let pids = b
        .kill_group_with("/app.slice", 15, &mut |pid, sig| {
            sent.push((pid, sig));
            Ok(())
        })
        .unwrap();
    assert_eq!(pids, vec![5, 11]);
    assert_eq!(sent, vec![(5, 15), (11, 15)]);
}

#[test]
fn kill_group_missing_group_returns_empty_ok() {
    // 與 MemCgroup 語義一致: 殺不存在的組 = 無進程可殺
    let dir = tempfile::tempdir().unwrap();
    let mut b = backend(dir.path());
    let mut sent: Vec<(i32, i32)> = Vec::new();
    let pids = b
        .kill_group_with("/ghost.slice", 15, &mut |pid, sig| {
            sent.push((pid, sig));
            Ok(())
        })
        .unwrap();
    assert!(pids.is_empty());
    assert!(sent.is_empty());
}

#[test]
fn kill_group_signal_failure_propagates() {
    let dir = tempfile::tempdir().unwrap();
    let mut b = backend(dir.path());
    b.create_group("/app.slice").unwrap();
    b.attach_pid("/app.slice", 9).unwrap();
    let e = b
        .kill_group_with("/app.slice", 15, &mut |_pid, _sig| Err("boom".to_string()))
        .unwrap_err();
    assert!(matches!(e, CgroupError::Backend(ref m) if m.contains("boom")), "{e:?}");
}

#[test]
fn remove_group_deletes_directory() {
    let dir = tempfile::tempdir().unwrap();
    let mut b = backend(dir.path());
    b.create_group("/gone.slice").unwrap();
    b.remove_group("/gone.slice").unwrap();
    assert!(!b.group_exists("/gone.slice"));
}

#[test]
fn remove_missing_group_errors_not_found() {
    let dir = tempfile::tempdir().unwrap();
    let mut b = backend(dir.path());
    let e = b.remove_group("/nope.slice").unwrap_err();
    assert!(matches!(e, CgroupError::NotFound(_)), "{e:?}");
}

#[test]
fn v1_backend_uses_tasks_file() {
    let dir = tempfile::tempdir().unwrap();
    let mut b = CgroupFsBackend::new_v1(dir.path());
    b.create_group("/sysinit.slice").unwrap();
    b.attach_pid("/sysinit.slice", 3).unwrap();
    // v1 層級寫 tasks 文件而非 cgroup.procs
    assert!(dir.path().join("sysinit.slice").join("tasks").exists());
    assert!(!dir.path().join("sysinit.slice").join("cgroup.procs").exists());
    assert_eq!(b.list_pids("/sysinit.slice").unwrap(), vec![3]);
}
