//! StdCommandExecutor (真實進程執行器) 測試 — 跨平台基本功能

use systeme_core::manager::ServiceExecutor;
use systeme_core::service::ExecCommand;
use systeme_manager::executor::StdCommandExecutor;

fn cmd(argv: &[&str]) -> ExecCommand {
    ExecCommand {
        argv: argv.iter().map(|s| s.to_string()).collect(),
        ignore_failure: false,
        full_privileges: false,
        no_env_expand: false,
    }
}

/// 跨平台「立即退出 0」命令
fn exit0_cmd() -> ExecCommand {
    if cfg!(unix) {
        cmd(&["/bin/sh", "-c", "exit 0"])
    } else {
        cmd(&["cmd", "/c", "exit 0"])
    }
}

#[test]
fn spawn_returns_positive_pid() {
    let mut ex = StdCommandExecutor::new();
    let pid = ex.spawn("t.service", &exit0_cmd(), &[], None).unwrap();
    assert!(pid > 0, "pid should be positive, got {pid}");
}

#[test]
fn spawn_missing_binary_returns_error() {
    let mut ex = StdCommandExecutor::new();
    let e = ex
        .spawn("t.service", &cmd(&["/nonexistent/binary/xyz", "--flag"]), &[], None)
        .unwrap_err();
    assert!(!e.is_empty());
}

#[test]
fn spawn_accepts_env_and_cwd() {
    let mut ex = StdCommandExecutor::new();
    let tmp = tempfile::tempdir().unwrap();
    let env = vec!["MY_VAR=hello".to_string(), "BAD_NO_EQ".to_string()];
    let pid = ex
        .spawn("t.service", &exit0_cmd(), &env, Some(tmp.path().to_str().unwrap()))
        .unwrap();
    assert!(pid > 0);
}

#[test]
#[cfg(unix)]
fn signal_zero_probes_alive_process() {
    let mut ex = StdCommandExecutor::new();
    // sleep 30 秒的進程, sig=0 探測存活
    let pid = ex
        .spawn("t.service", &cmd(&["/bin/sh", "-c", "sleep 30"]), &[], None)
        .unwrap();
    ex.signal(pid, 0).unwrap();
    // 真殺掉避免洩漏
    ex.signal(pid, 9).unwrap();
}

#[test]
#[cfg(unix)]
fn signal_nonexistent_pid_errors() {
    let mut ex = StdCommandExecutor::new();
    // 對確定不存在的 pid (i32::MAX 遠超 pid_max 上限) 發信號必須 Err
    assert!(ex.signal(i32::MAX, 0).is_err());
}

#[test]
fn prepare_socket_fds_roundtrip() {
    let mut ex = StdCommandExecutor::new();
    assert!(ex.pending_fds.is_empty(), "fresh executor has no pending fds");
    ex.prepare_socket_fds(&[3, 4]);
    assert_eq!(ex.pending_fds, vec![3, 4]);
    // spawn 一次後清空 (fd 已被消費, 不洩漏到後續 spawn)
    ex.spawn("t.service", &exit0_cmd(), &[], None).unwrap();
    assert!(ex.pending_fds.is_empty(), "fds must be consumed by spawn");
}
