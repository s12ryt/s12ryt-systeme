//! DaemonCore (IPC Request → Response 接線層) 測試

use std::cell::RefCell;

use systeme_core::ipc::{Request, Response};
use systeme_core::manager::{ManagerCore, RuntimeMode, ServiceExecutor};
use systeme_core::service::ExecCommand;
use systeme_manager::daemon::DaemonCore;
use systeme_units::search::{MemFs, SearchPaths};

// ─── 測試替身 ────────────────────────────────────────────────────────

#[derive(Default)]
struct MockExec {
    spawns: RefCell<Vec<(String, Vec<String>)>>,
    signals: RefCell<Vec<(i32, i32)>>,
    next_pid: std::cell::Cell<i32>,
    /// 對這些單元 spawn 失敗
    fail_units: Vec<String>,
}

impl MockExec {
    fn new() -> Self {
        MockExec {
            next_pid: std::cell::Cell::new(100),
            ..Default::default()
        }
    }
}

impl ServiceExecutor for MockExec {
    fn spawn(
        &mut self,
        unit: &str,
        cmd: &ExecCommand,
        _env: &[String],
        _cwd: Option<&str>,
    ) -> Result<i32, String> {
        if self.fail_units.iter().any(|u| u == unit) {
            return Err(format!("mock spawn failure for {unit}"));
        }
        let pid = self.next_pid.get();
        self.next_pid.set(pid + 1);
        self.spawns
            .borrow_mut()
            .push((unit.to_string(), cmd.argv.clone()));
        Ok(pid)
    }

    fn signal(&mut self, pid: i32, sig: i32) -> Result<(), String> {
        self.signals.borrow_mut().push((pid, sig));
        Ok(())
    }
}

// ─── 工具 ────────────────────────────────────────────────────────────

fn paths_of(dirs: &[&str]) -> SearchPaths {
    SearchPaths {
        dirs: dirs.iter().map(std::path::PathBuf::from).collect(),
    }
}

/// 標準測試環境: /etc 下兩個服務 + 一個 mask
fn setup() -> (DaemonCore, MemFs, MockExec) {
    let mut fs = MemFs::new();
    fs.file(
        "/etc/systemd/system/web.service",
        "[Unit]\nDescription=Web\n\n[Service]\nExecStart=/usr/bin/webd\n",
    );
    fs.file(
        "/etc/systemd/system/db.service",
        "[Unit]\nDescription=DB\n\n[Service]\nExecStart=/usr/bin/dbd\n",
    );
    // masked: 指向 /dev/null 的符號鏈接
    fs.symlink("/etc/systemd/system/evil.service", "/dev/null");

    let mut manager = ManagerCore::new(RuntimeMode::System);
    let sp = paths_of(&["/etc/systemd/system"]);
    manager.load_all(&fs, &sp);
    let core = DaemonCore::new(manager, sp);
    (core, fs, MockExec::new())
}

fn req(r: &Request, core: &mut DaemonCore, exec: &mut MockExec, fs: &MemFs) -> Response {
    core.handle(r, exec, fs)
}

// ─── ListUnits / Status ──────────────────────────────────────────────

#[test]
fn list_units_empty() {
    let mut core = DaemonCore::new(
        ManagerCore::new(RuntimeMode::User),
        paths_of(&["/etc/systemd/system"]),
    );
    let mut exec = MockExec::new();
    let fs = MemFs::new();
    let r = req(&Request::ListUnits, &mut core, &mut exec, &fs);
    match r {
        Response::Units(v) => assert!(v.is_empty()),
        other => panic!("expected Units, got {other:?}"),
    }
}

#[test]
fn list_units_returns_sorted_entries() {
    let (mut core, _fs, mut exec) = setup();
    let fs = MemFs::new();
    let r = req(&Request::ListUnits, &mut core, &mut exec, &fs);
    match r {
        Response::Units(v) => {
            let names: Vec<&str> = v.iter().map(|u| u.name.as_str()).collect();
            // BTreeMap 字典序: db < evil < web
            assert_eq!(names, vec!["db.service", "evil.service", "web.service"]);
            // masked 標記
            assert_eq!(v.iter().find(|u| u.name == "evil.service").unwrap().load_state, "masked");
        }
        other => panic!("expected Units, got {other:?}"),
    }
}

#[test]
fn status_found() {
    let (mut core, _fs, mut exec) = setup();
    let fs = MemFs::new();
    let r = req(&Request::Status { unit: "web.service".into() }, &mut core, &mut exec, &fs);
    match r {
        Response::UnitStatus(info) => {
            assert_eq!(info.name, "web.service");
            assert_eq!(info.description, "Web");
            assert_eq!(info.load_state, "loaded");
            assert_eq!(info.active_state, "inactive");
            assert_eq!(info.main_pid, None);
        }
        other => panic!("expected UnitStatus, got {other:?}"),
    }
}

#[test]
fn status_not_found() {
    let (mut core, _fs, mut exec) = setup();
    let fs = MemFs::new();
    let r = req(&Request::Status { unit: "ghost.service".into() }, &mut core, &mut exec, &fs);
    match r {
        Response::Error { message } => assert!(message.contains("ghost.service"), "{message}"),
        other => panic!("expected Error, got {other:?}"),
    }
}

// ─── Start / Stop / Restart ──────────────────────────────────────────

#[test]
fn start_ok_response() {
    let (mut core, fs, mut exec) = setup();
    let r = req(&Request::Start { unit: "web.service".into() }, &mut core, &mut exec, &fs);
    assert_eq!(r, Response::Ok);
    assert_eq!(exec.spawns.borrow().len(), 1);
    assert_eq!(exec.spawns.borrow()[0].0, "web.service");
    assert_eq!(core.manager.active_state("web.service"), Some(systeme_core::manager::ActiveState::Active));
}

#[test]
fn start_not_found_error() {
    let (mut core, fs, mut exec) = setup();
    let r = req(&Request::Start { unit: "ghost.service".into() }, &mut core, &mut exec, &fs);
    match r {
        Response::Error { message } => assert!(message.contains("not found"), "{message}"),
        other => panic!("expected Error, got {other:?}"),
    }
}

#[test]
fn start_masked_error() {
    let (mut core, fs, mut exec) = setup();
    let r = req(&Request::Start { unit: "evil.service".into() }, &mut core, &mut exec, &fs);
    match r {
        Response::Error { message } => assert!(message.contains("masked"), "{message}"),
        other => panic!("expected Error, got {other:?}"),
    }
}

#[test]
fn start_spawn_failure_error() {
    let (mut core, fs, mut _exec) = setup();
    let mut exec = MockExec::new();
    exec.fail_units.push("web.service".into());
    let r = req(&Request::Start { unit: "web.service".into() }, &mut core, &mut exec, &fs);
    match r {
        Response::Error { message } => assert!(message.contains("web.service"), "{message}"),
        other => panic!("expected Error, got {other:?}"),
    }
}

#[test]
fn stop_ok_response() {
    let (mut core, fs, mut exec) = setup();
    req(&Request::Start { unit: "web.service".into() }, &mut core, &mut exec, &fs);
    let r = req(&Request::Stop { unit: "web.service".into() }, &mut core, &mut exec, &fs);
    assert_eq!(r, Response::Ok);
    assert_eq!(core.manager.active_state("web.service"), Some(systeme_core::manager::ActiveState::Inactive));
    // TERM 已發給主進程
    assert!(exec.signals.borrow().iter().any(|&(pid, sig)| pid == 100 && sig == 15));
}

#[test]
fn stop_not_found_error() {
    let (mut core, fs, mut exec) = setup();
    let r = req(&Request::Stop { unit: "ghost.service".into() }, &mut core, &mut exec, &fs);
    match r {
        Response::Error { message } => assert!(message.contains("not found"), "{message}"),
        other => panic!("expected Error, got {other:?}"),
    }
}

#[test]
fn restart_stops_then_starts() {
    let (mut core, fs, mut exec) = setup();
    req(&Request::Start { unit: "web.service".into() }, &mut core, &mut exec, &fs);
    let spawns_before = exec.spawns.borrow().len();
    let signals_before = exec.signals.borrow().len();
    let r = req(&Request::Restart { unit: "web.service".into() }, &mut core, &mut exec, &fs);
    assert_eq!(r, Response::Ok);
    assert_eq!(core.manager.active_state("web.service"), Some(systeme_core::manager::ActiveState::Active));
    // stop 產生 TERM 信號, start 產生新 spawn
    assert_eq!(exec.signals.borrow().len(), signals_before + 1);
    assert_eq!(exec.spawns.borrow().len(), spawns_before + 1);
}

#[test]
fn restart_inactive_unit_still_starts() {
    let (mut core, fs, mut exec) = setup();
    let r = req(&Request::Restart { unit: "web.service".into() }, &mut core, &mut exec, &fs);
    assert_eq!(r, Response::Ok);
    assert_eq!(core.manager.active_state("web.service"), Some(systeme_core::manager::ActiveState::Active));
}

#[test]
fn restart_not_found_error() {
    let (mut core, fs, mut exec) = setup();
    let r = req(&Request::Restart { unit: "ghost.service".into() }, &mut core, &mut exec, &fs);
    match r {
        Response::Error { message } => assert!(message.contains("not found"), "{message}"),
        other => panic!("expected Error, got {other:?}"),
    }
}

// ─── IsActive ────────────────────────────────────────────────────────

#[test]
fn is_active_loaded_inactive() {
    let (mut core, fs, mut exec) = setup();
    let r = req(&Request::IsActive { unit: "web.service".into() }, &mut core, &mut exec, &fs);
    assert_eq!(r, Response::Active(false));
}

#[test]
fn is_active_after_start() {
    let (mut core, fs, mut exec) = setup();
    req(&Request::Start { unit: "web.service".into() }, &mut core, &mut exec, &fs);
    let r = req(&Request::IsActive { unit: "web.service".into() }, &mut core, &mut exec, &fs);
    assert_eq!(r, Response::Active(true));
}

#[test]
fn is_active_not_loaded_error() {
    let (mut core, fs, mut exec) = setup();
    let r = req(&Request::IsActive { unit: "ghost.service".into() }, &mut core, &mut exec, &fs);
    match r {
        Response::Error { message } => assert!(message.contains("ghost.service"), "{message}"),
        other => panic!("expected Error, got {other:?}"),
    }
}

// ─── Cat ─────────────────────────────────────────────────────────────

#[test]
fn cat_returns_file_text() {
    let (mut core, fs, mut exec) = setup();
    let r = req(&Request::Cat { unit: "web.service".into() }, &mut core, &mut exec, &fs);
    match r {
        Response::Cat { name, text } => {
            assert_eq!(name, "web.service");
            assert!(text.contains("ExecStart=/usr/bin/webd"), "{text}");
            assert!(text.contains("[Unit]"));
        }
        other => panic!("expected Cat, got {other:?}"),
    }
}

#[test]
fn cat_masked_error() {
    let (mut core, fs, mut exec) = setup();
    let r = req(&Request::Cat { unit: "evil.service".into() }, &mut core, &mut exec, &fs);
    match r {
        Response::Error { message } => assert!(message.contains("masked"), "{message}"),
        other => panic!("expected Error, got {other:?}"),
    }
}

#[test]
fn cat_not_found_error() {
    let (mut core, fs, mut exec) = setup();
    let r = req(&Request::Cat { unit: "ghost.service".into() }, &mut core, &mut exec, &fs);
    match r {
        Response::Error { message } => assert!(message.contains("ghost.service"), "{message}"),
        other => panic!("expected Error, got {other:?}"),
    }
}

// ─── DaemonReload ────────────────────────────────────────────────────

#[test]
fn daemon_reload_picks_up_new_units() {
    let (mut core, mut fs, mut exec) = setup();
    // 新增一個單元文件
    fs.file(
        "/etc/systemd/system/new.service",
        "[Unit]\nDescription=New\n\n[Service]\nExecStart=/usr/bin/newd\n",
    );
    let r = req(&Request::DaemonReload, &mut core, &mut exec, &fs);
    assert_eq!(r, Response::Ok);
    assert!(core.manager.unit("new.service").is_some());
    // 舊單元仍在
    assert!(core.manager.unit("web.service").is_some());
}

#[test]
fn daemon_reload_preserves_running_state() {
    let (mut core, fs, mut exec) = setup();
    req(&Request::Start { unit: "web.service".into() }, &mut core, &mut exec, &fs);
    let r = req(&Request::DaemonReload, &mut core, &mut exec, &fs);
    assert_eq!(r, Response::Ok);
    assert_eq!(core.manager.active_state("web.service"), Some(systeme_core::manager::ActiveState::Active));
}

// ─── NotImplemented 族 ───────────────────────────────────────────────

#[test]
fn enable_disable_mask_unmask_reload_not_implemented() {
    let (mut core, fs, mut exec) = setup();
    for r in [
        Request::Enable { unit: "web.service".into() },
        Request::Disable { unit: "web.service".into() },
        Request::Mask { unit: "web.service".into() },
        Request::Unmask { unit: "web.service".into() },
        Request::Reload { unit: "web.service".into() },
    ] {
        let resp = req(&r, &mut core, &mut exec, &fs);
        match resp {
            Response::NotImplemented { request } => {
                assert!(!request.is_empty());
            }
            other => panic!("{r:?} should be NotImplemented, got {other:?}"),
        }
    }
}
