//! DaemonCore 對 .mount / .automount / .socket 單元的攔截接線測試。
//!
//! - .socket: bind_socket_unit → 標 Active → prepare_socket_fds → 拉起目標 service
//! - .mount: real_mount → 標 Active (cfg(unix) 真掛載; 非 unix 恆 Err)
//! - .automount: 簡化 — 自身直達 Active; 對應 .mount 存在則一併啟動

use systeme_core::ipc::{Request, Response};
use systeme_core::manager::{ActiveState, ManagerCore, RuntimeMode};
#[cfg(unix)]
use systeme_core::manager::ServiceExecutor;
use systeme_manager::daemon::DaemonCore;
use systeme_manager::executor::StdCommandExecutor;
use systeme_units::search::{MemFs, SearchPaths};

const DIR: &str = "/etc/systemd/system";

fn core_with(fs: &MemFs) -> DaemonCore {
    let sp = SearchPaths {
        dirs: vec![DIR.into()],
    };
    let mut m = ManagerCore::new(RuntimeMode::User);
    m.load_all(fs, &sp);
    DaemonCore::new(m, sp)
}

fn err_msg(r: &Response) -> String {
    match r {
        Response::Error { message } => message.clone(),
        other => panic!("expected Error response, got {other:?}"),
    }
}

fn start(core: &mut DaemonCore, exec: &mut StdCommandExecutor, fs: &MemFs, unit: &str) -> Response {
    core.handle(&Request::Start { unit: unit.into() }, exec, fs)
}

// ─── socket 單元 ─────────────────────────────────────────────────────

#[test]
fn socket_not_found_errors() {
    let fs = MemFs::new();
    let mut core = core_with(&fs);
    let mut exec = StdCommandExecutor::new();
    let resp = start(&mut core, &mut exec, &fs, "nope.socket");
    assert!(err_msg(&resp).contains("could not be found"));
}

#[test]
fn masked_socket_start_errors() {
    let mut fs = MemFs::new();
    fs.symlink(&format!("{DIR}/evil.socket"), "/dev/null");
    let mut core = core_with(&fs);
    let mut exec = StdCommandExecutor::new();
    let resp = start(&mut core, &mut exec, &fs, "evil.socket");
    assert!(err_msg(&resp).to_lowercase().contains("masked"));
}

#[test]
#[cfg(not(unix))]
fn socket_start_unsupported_platform_errors() {
    let mut fs = MemFs::new();
    fs.file(
        &format!("{DIR}/app.socket"),
        "[Socket]\nListenStream=127.0.0.1:9123\n",
    );
    let mut core = core_with(&fs);
    let mut exec = StdCommandExecutor::new();
    let resp = start(&mut core, &mut exec, &fs, "app.socket");
    assert!(!err_msg(&resp).is_empty());
    // bind 失敗時單元不得標 Active
    assert_eq!(core.manager.active_state("app.socket"), Some(ActiveState::Inactive));
}

#[test]
#[cfg(unix)]
fn socket_start_binds_and_activates_service() {
    let tmp = tempfile::tempdir().unwrap();
    let sock_path = tmp.path().join("app.sock");
    let mut fs = MemFs::new();
    fs.file(
        &format!("{DIR}/app.socket"),
        &format!("[Socket]\nListenStream={}\n", sock_path.display()),
    );
    fs.file(
        &format!("{DIR}/app.service"),
        "[Service]\nType=simple\nExecStart=/bin/sh -c 'sleep 5'\n",
    );
    let mut core = core_with(&fs);
    let mut exec = StdCommandExecutor::new();

    let resp = start(&mut core, &mut exec, &fs, "app.socket");
    assert!(matches!(resp, Response::Ok), "got {resp:?}");
    assert_eq!(core.manager.active_state("app.socket"), Some(ActiveState::Active));
    assert_eq!(core.manager.active_state("app.service"), Some(ActiveState::Active));
    assert_eq!(core.sockets.len(), 1);
    assert_eq!(core.sockets["app.socket"].len(), 1);
    // socket 文件已存在 (監聽中)
    assert!(sock_path.exists());

    // 清理 service 進程, 再 stop socket
    if let Some(pid) = core.manager.unit("app.service").unwrap().runtime.main_pid {
        let _ = exec.signal(pid, 9);
    }
    let resp = core.handle(&Request::Stop { unit: "app.socket".into() }, &mut exec, &fs);
    assert!(matches!(resp, Response::Ok), "got {resp:?}");
    assert!(core.sockets.is_empty());
    assert_eq!(core.manager.active_state("app.socket"), Some(ActiveState::Inactive));
}

#[test]
#[cfg(unix)]
fn socket_restart_rebinds() {
    let tmp = tempfile::tempdir().unwrap();
    let sock_path = tmp.path().join("app.sock");
    let mut fs = MemFs::new();
    fs.file(
        &format!("{DIR}/app.socket"),
        &format!("[Socket]\nListenStream={}\n", sock_path.display()),
    );
    fs.file(
        &format!("{DIR}/app.service"),
        "[Service]\nType=simple\nExecStart=/bin/sh -c 'sleep 5'\n",
    );
    let mut core = core_with(&fs);
    let mut exec = StdCommandExecutor::new();

    let resp = start(&mut core, &mut exec, &fs, "app.socket");
    assert!(matches!(resp, Response::Ok));
    assert_eq!(core.sockets.len(), 1);

    let resp = core.handle(
        &Request::Restart { unit: "app.socket".into() },
        &mut exec,
        &fs,
    );
    assert!(matches!(resp, Response::Ok), "got {resp:?}");
    assert_eq!(core.sockets.len(), 1);
    assert_eq!(core.manager.active_state("app.socket"), Some(ActiveState::Active));

    if let Some(pid) = core.manager.unit("app.service").unwrap().runtime.main_pid {
        let _ = exec.signal(pid, 9);
    }
}

// ─── mount 單元 ──────────────────────────────────────────────────────

#[test]
fn mount_not_found_errors() {
    let fs = MemFs::new();
    let mut core = core_with(&fs);
    let mut exec = StdCommandExecutor::new();
    let resp = start(&mut core, &mut exec, &fs, "nope.mount");
    assert!(err_msg(&resp).contains("could not be found"));
}

#[test]
fn start_mount_missing_what_errors() {
    let mut fs = MemFs::new();
    fs.file(&format!("{DIR}/bad.mount"), "[Mount]\nWhere=/tmp/x\n");
    let mut core = core_with(&fs);
    let mut exec = StdCommandExecutor::new();
    let resp = start(&mut core, &mut exec, &fs, "bad.mount");
    let msg = err_msg(&resp);
    // unix: "Mount unit has no What="; 非 unix: unsupported platform — 兩者皆為錯誤
    assert!(!msg.is_empty());
    assert_eq!(core.manager.active_state("bad.mount"), Some(ActiveState::Inactive));
}

#[test]
fn stop_inactive_mount_is_ok() {
    let mut fs = MemFs::new();
    fs.file(
        &format!("{DIR}/m.mount"),
        "[Mount]\nWhat=tmpfs\nWhere=/tmp/systeme-mtest\nType=tmpfs\n",
    );
    let mut core = core_with(&fs);
    let mut exec = StdCommandExecutor::new();
    // 未啟動 → Stop 冪等 Ok, 不觸發 umount
    let resp = core.handle(&Request::Stop { unit: "m.mount".into() }, &mut exec, &fs);
    assert!(matches!(resp, Response::Ok), "got {resp:?}");
}

#[test]
#[cfg(unix)]
fn mount_roundtrip_unix() {
    let tmp = tempfile::tempdir().unwrap();
    let mnt = tmp.path().join("mnt");
    std::fs::create_dir_all(&mnt).unwrap();
    let mut fs = MemFs::new();
    fs.file(
        &format!("{DIR}/mymount.mount"),
        &format!("[Mount]\nWhat=tmpfs\nWhere={}\nType=tmpfs\n", mnt.display()),
    );
    let mut core = core_with(&fs);
    let mut exec = StdCommandExecutor::new();

    let resp = start(&mut core, &mut exec, &fs, "mymount.mount");
    assert!(matches!(resp, Response::Ok), "got {resp:?}");
    assert_eq!(core.manager.active_state("mymount.mount"), Some(ActiveState::Active));

    // tmpfs 內寫文件
    std::fs::write(mnt.join("inside.txt"), "x").unwrap();

    let resp = core.handle(
        &Request::Stop { unit: "mymount.mount".into() },
        &mut exec,
        &fs,
    );
    assert!(matches!(resp, Response::Ok), "got {resp:?}");
    assert_eq!(core.manager.active_state("mymount.mount"), Some(ActiveState::Inactive));
    // umount 後 tmpfs 內容不可見
    assert!(!mnt.join("inside.txt").exists());
}

// ─── automount 單元 ──────────────────────────────────────────────────

#[test]
fn automount_start_marks_active() {
    let mut fs = MemFs::new();
    fs.file(&format!("{DIR}/a.automount"), "[Automount]\nWhere=/tmp/systeme-amtest\n");
    let mut core = core_with(&fs);
    let mut exec = StdCommandExecutor::new();
    let resp = start(&mut core, &mut exec, &fs, "a.automount");
    assert!(matches!(resp, Response::Ok), "got {resp:?}");
    assert_eq!(core.manager.active_state("a.automount"), Some(ActiveState::Active));
}

#[test]
fn automount_stop_deactivates() {
    let mut fs = MemFs::new();
    fs.file(&format!("{DIR}/a.automount"), "[Automount]\nWhere=/tmp/systeme-amtest\n");
    let mut core = core_with(&fs);
    let mut exec = StdCommandExecutor::new();
    start(&mut core, &mut exec, &fs, "a.automount");
    let resp = core.handle(&Request::Stop { unit: "a.automount".into() }, &mut exec, &fs);
    assert!(matches!(resp, Response::Ok), "got {resp:?}");
    assert_eq!(core.manager.active_state("a.automount"), Some(ActiveState::Inactive));
}
