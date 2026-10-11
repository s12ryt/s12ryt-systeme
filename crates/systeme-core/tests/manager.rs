//! 管理器核心狀態機測試: 模式探測、load/reload、start/stop 編排、退出事件回饋。

use std::path::PathBuf;

use systeme_core::manager::{
    ActiveState, ManagerCore, ManagerError, RuntimeMode, ServiceExecutor,
};
use systeme_core::process::{ProcessInfo, WaitStatus};
use systeme_core::service::ExecCommand;
use systeme_units::search::{MemFs, SearchPaths};

// ─── 測試替身 ────────────────────────────────────────────────────────

/// Mock 執行器: 記錄 spawn / signal; 可注入失敗 (argv 含標記即 Err)。
#[derive(Default)]
struct MockExec {
    spawns: Vec<(String, String)>, // (unit, argv joined)
    signals: Vec<(i32, i32)>,      // (pid, sig)
    fail_marker: Option<String>,   // argv[0] 含此子串 → spawn Err
    next_pid: i32,
}

impl ServiceExecutor for MockExec {
    fn spawn(
        &mut self,
        unit: &str,
        cmd: &ExecCommand,
        _env: &[String],
        _cwd: Option<&str>,
    ) -> Result<i32, String> {
        if let Some(m) = &self.fail_marker {
            if cmd.argv.first().map(|a| a.contains(m.as_str())).unwrap_or(false) {
                return Err(format!("injected failure for {unit}"));
            }
        }
        self.next_pid += 1;
        self.spawns
            .push((unit.to_string(), cmd.argv.join(" ")));
        Ok(self.next_pid)
    }
    fn signal(&mut self, pid: i32, sig: i32) -> Result<(), String> {
        self.signals.push((pid, sig));
        Ok(())
    }
}

// ─── 共用 fixture ────────────────────────────────────────────────────

fn sp() -> SearchPaths {
    SearchPaths {
        dirs: vec![
            PathBuf::from("/usr/lib/systemd/system"),
            PathBuf::from("/etc/systemd/system"),
        ],
    }
}

fn fs_basic() -> MemFs {
    let mut fs = MemFs::new();
    fs.file(
        "/etc/systemd/system/a.service",
        "[Unit]\nDescription=A service\nRequires=b.service\nAfter=b.service\n\n\
         [Service]\nType=simple\nExecStart=/bin/a --run\n",
    );
    fs.file(
        "/etc/systemd/system/b.service",
        "[Unit]\nDescription=B service\n\n[Service]\nType=simple\nExecStart=/bin/b\n",
    );
    fs
}

fn manager_with(fs: &MemFs) -> ManagerCore {
    let mut m = ManagerCore::new(RuntimeMode::detect(false));
    m.load_all(fs, &sp());
    m
}

// ─── 模式探測 ────────────────────────────────────────────────────────

#[test]
fn mode_detect_root_is_system() {
    assert!(matches!(RuntimeMode::detect(true), RuntimeMode::System));
}

#[test]
fn mode_detect_non_root_is_user() {
    assert!(matches!(RuntimeMode::detect(false), RuntimeMode::User));
}

// ─── load_all / reload ───────────────────────────────────────────────

#[test]
fn load_all_builds_graph_configs_and_names() {
    let m = manager_with(&fs_basic());
    // 兩個單元都註冊
    assert_eq!(m.unit_names(), vec!["a.service".to_string(), "b.service".to_string()]);
    // 配置提取
    let a = m.unit("a.service").unwrap();
    assert_eq!(a.config.exec_start[0].argv[0], "/bin/a");
    assert_eq!(a.config.exec_start[0].argv[1], "--run");
    // 依賴圖建立
    let g = m.graph_snapshot();
    assert!(g.contains("a.service"));
    assert!(g.contains("b.service"));
}

#[test]
fn load_all_masked_unit() {
    let mut fs = fs_basic();
    fs.symlink("/etc/systemd/system/b.service", "/dev/null");
    let m = manager_with(&fs);
    let b = m.unit("b.service").unwrap();
    assert!(b.masked);
    let a = m.unit("a.service").unwrap();
    assert!(!a.masked);
}

#[test]
fn load_all_ignores_non_unit_files() {
    let mut fs = fs_basic();
    fs.file("/etc/systemd/system/random.conf", "junk");
    let m = manager_with(&fs);
    assert_eq!(m.unit_names().len(), 2);
}

#[test]
fn reload_preserves_runtime_state() {
    let fs = fs_basic();
    let mut m = manager_with(&fs);
    let mut ex = MockExec::default();
    m.start_unit("a.service", &mut ex).unwrap();
    assert!(matches!(m.active_state("a.service"), Some(ActiveState::Active)));

    m.reload(&fs, &sp());
    // 重載後運行時狀態保留 (b 先 spawn 佔 pid 1, a 主進程是 pid 2)
    assert!(matches!(m.active_state("a.service"), Some(ActiveState::Active)));
    assert_eq!(m.unit("a.service").unwrap().runtime.main_pid, Some(2));
}

#[test]
fn reload_picks_up_new_unit_and_drops_vanished() {
    let mut fs = fs_basic();
    let mut m = manager_with(&fs);

    // 新增 c.service, 移除 b.service
    fs.file(
        "/etc/systemd/system/c.service",
        "[Service]\nExecStart=/bin/c\n",
    );
    fs.nodes.remove(&PathBuf::from("/etc/systemd/system/b.service"));

    let report = m.reload(&fs, &sp());
    assert!(m.unit("c.service").is_some());
    assert!(m.unit("b.service").is_none());
    assert_eq!(report.dropped, 1);
}

// ─── start 編排 ──────────────────────────────────────────────────────

#[test]
fn start_simple_service_spawns_and_activates() {
    let fs = fs_basic();
    let mut m = manager_with(&fs);
    let mut ex = MockExec::default();
    let touched = m.start_unit("b.service", &mut ex).unwrap();

    assert_eq!(touched, vec!["b.service".to_string()]);
    assert_eq!(ex.spawns.len(), 1);
    assert_eq!(ex.spawns[0].1, "/bin/b");
    assert!(matches!(m.active_state("b.service"), Some(ActiveState::Active)));
    assert_eq!(m.unit("b.service").unwrap().runtime.main_pid, Some(1));
    assert!(m.is_active("b.service"));
}

#[test]
fn start_runs_exec_start_pre_first() {
    let mut fs = MemFs::new();
    fs.file(
        "/etc/systemd/system/p.service",
        "[Service]\n\
         ExecStartPre=/bin/prep one\n\
         ExecStartPre=/bin/prep two\n\
         ExecStart=/bin/main\n",
    );
    let mut m = manager_with(&fs);
    let mut ex = MockExec::default();
    m.start_unit("p.service", &mut ex).unwrap();

    let argvs: Vec<&str> = ex.spawns.iter().map(|(_, a)| a.as_str()).collect();
    assert_eq!(argvs, vec!["/bin/prep one", "/bin/prep two", "/bin/main"]);
}

#[test]
fn start_pre_failure_aborts_and_fails_unit() {
    let mut fs = MemFs::new();
    fs.file(
        "/etc/systemd/system/p.service",
        "[Service]\nExecStartPre=/bin/prep\nExecStart=/bin/main\n",
    );
    let mut m = manager_with(&fs);
    let mut ex = MockExec {
        fail_marker: Some("prep".into()),
        ..Default::default()
    };
    let err = m.start_unit("p.service", &mut ex).unwrap_err();
    assert!(matches!(err, ManagerError::StartFailed { ref unit, .. } if unit == "p.service"));
    // ExecStart 未被執行
    assert_eq!(ex.spawns.len(), 0);
    assert!(matches!(m.active_state("p.service"), Some(ActiveState::Failed)));
}

#[test]
fn start_pre_ignore_failure_prefix_continues() {
    let mut fs = MemFs::new();
    fs.file(
        "/etc/systemd/system/p.service",
        "[Service]\nExecStartPre=-/bin/prep\nExecStart=/bin/main\n",
    );
    let mut m = manager_with(&fs);
    let mut ex = MockExec {
        fail_marker: Some("prep".into()),
        ..Default::default()
    };
    m.start_unit("p.service", &mut ex).unwrap();
    // prep 失敗被 '-' 前綴忽略, main 仍執行
    assert_eq!(ex.spawns.len(), 1);
    assert_eq!(ex.spawns[0].1, "/bin/main");
    assert!(m.is_active("p.service"));
}

#[test]
fn start_pulls_requires_dependency_in_order() {
    let fs = fs_basic(); // a Requires=b, After=b
    let mut m = manager_with(&fs);
    let mut ex = MockExec::default();
    let touched = m.start_unit("a.service", &mut ex).unwrap();

    // b (被依賴者) 先於 a
    assert_eq!(touched, vec!["b.service".to_string(), "a.service".to_string()]);
    assert!(m.is_active("a.service"));
    assert!(m.is_active("b.service"));
    let b_spawn = ex.spawns.iter().position(|(u, _)| u == "b.service").unwrap();
    let a_spawn = ex.spawns.iter().position(|(u, _)| u == "a.service").unwrap();
    assert!(b_spawn < a_spawn);
}

#[test]
fn start_target_marks_active_without_spawn() {
    let mut fs = fs_basic();
    fs.file("/etc/systemd/system/multi-user.target", "[Unit]\nDescription=Multi\n");
    let mut m = manager_with(&fs);
    let mut ex = MockExec::default();
    m.start_unit("multi-user.target", &mut ex).unwrap();
    assert!(m.is_active("multi-user.target"));
    assert_eq!(ex.spawns.len(), 0);
}

#[test]
fn start_missing_unit_is_not_found() {
    let fs = fs_basic();
    let mut m = manager_with(&fs);
    let mut ex = MockExec::default();
    assert!(matches!(
        m.start_unit("nope.service", &mut ex),
        Err(ManagerError::NotFound(_))
    ));
}

#[test]
fn start_masked_unit_errors() {
    let mut fs = fs_basic();
    fs.symlink("/etc/systemd/system/b.service", "/dev/null");
    let mut m = manager_with(&fs);
    let mut ex = MockExec::default();
    assert!(matches!(
        m.start_unit("b.service", &mut ex),
        Err(ManagerError::Masked(_))
    ));
}

// ─── stop 編排 ───────────────────────────────────────────────────────

#[test]
fn stop_signals_main_pid_term_and_deactivates() {
    let fs = fs_basic();
    let mut m = manager_with(&fs);
    let mut ex = MockExec::default();
    m.start_unit("b.service", &mut ex).unwrap();

    let touched = m.stop_unit("b.service", &mut ex).unwrap();
    assert_eq!(touched, vec!["b.service".to_string()]);
    assert_eq!(ex.signals, vec![(1, 15)]); // SIGTERM=15
    assert!(matches!(m.active_state("b.service"), Some(ActiveState::Inactive)));
    assert_eq!(m.unit("b.service").unwrap().runtime.main_pid, None);
}

#[test]
fn stop_kill_mode_process_only_signals_main() {
    let mut fs = MemFs::new();
    fs.file(
        "/etc/systemd/system/k.service",
        "[Service]\nKillMode=process\nExecStart=/bin/k\n",
    );
    let mut m = manager_with(&fs);
    let mut ex = MockExec::default();
    m.start_unit("k.service", &mut ex).unwrap();

    // 模擬服務 fork 出子進程 (main pid=1)
    m.units.get_mut("k.service").unwrap().runtime.procs.push(ProcessInfo {
        pid: 99,
        pgid: 1,
        is_main: false,
    });
    m.stop_unit("k.service", &mut ex).unwrap();
    // 只殺主進程
    assert_eq!(ex.signals, vec![(1, 15)]);
}

#[test]
fn stop_kill_mode_mixed_terms_main_kills_rest() {
    let mut fs = MemFs::new();
    fs.file(
        "/etc/systemd/system/k.service",
        "[Service]\nKillMode=mixed\nExecStart=/bin/k\n",
    );
    let mut m = manager_with(&fs);
    let mut ex = MockExec::default();
    m.start_unit("k.service", &mut ex).unwrap();
    m.units.get_mut("k.service").unwrap().runtime.procs.push(ProcessInfo {
        pid: 99,
        pgid: 1,
        is_main: false,
    });
    m.stop_unit("k.service", &mut ex).unwrap();
    // TERM 主進程 (15), KILL 其餘 (9)
    assert!(ex.signals.contains(&(1, 15)));
    assert!(ex.signals.contains(&(99, 9)));
    assert_eq!(ex.signals.len(), 2);
}

#[test]
fn stop_runs_exec_stop_then_signals() {
    let mut fs = MemFs::new();
    fs.file(
        "/etc/systemd/system/k.service",
        "[Service]\nExecStop=/bin/kstop\nExecStart=/bin/k\n",
    );
    let mut m = manager_with(&fs);
    let mut ex = MockExec::default();
    m.start_unit("k.service", &mut ex).unwrap();
    m.stop_unit("k.service", &mut ex).unwrap();

    let argvs: Vec<&str> = ex.spawns.iter().map(|(_, a)| a.as_str()).collect();
    assert_eq!(argvs, vec!["/bin/k", "/bin/kstop"]);
    assert_eq!(ex.signals, vec![(1, 15)]);
}

#[test]
fn stop_propagates_to_required_by() {
    let fs = fs_basic(); // a Requires=b ⇒ b.rdeps.required_by = {a}
    let mut m = manager_with(&fs);
    let mut ex = MockExec::default();
    m.start_unit("a.service", &mut ex).unwrap();

    let touched = m.stop_unit("b.service", &mut ex).unwrap();
    // 停 b 傳播停 a (a Requires b)
    assert!(touched.contains(&"a.service".to_string()));
    assert!(touched.contains(&"b.service".to_string()));
    assert!(matches!(m.active_state("a.service"), Some(ActiveState::Inactive)));
    assert!(matches!(m.active_state("b.service"), Some(ActiveState::Inactive)));
}

// ─── oneshot 與退出事件 ───────────────────────────────────────────────

#[test]
fn oneshot_stays_activating_until_exit_then_active_with_remain() {
    let mut fs = MemFs::new();
    fs.file(
        "/etc/systemd/system/one.service",
        "[Service]\nType=oneshot\nRemainAfterExit=yes\nExecStart=/bin/one\n",
    );
    let mut m = manager_with(&fs);
    let mut ex = MockExec::default();
    m.start_unit("one.service", &mut ex).unwrap();
    // oneshot 啟動中, 尚未 active
    assert!(matches!(m.active_state("one.service"), Some(ActiveState::Activating)));
    assert!(!m.is_active("one.service"));

    // 主進程乾淨退出 → RemainAfterExit=yes → Active
    let st = m.on_exit("one.service", 1, WaitStatus::Exited { code: 0 });
    assert!(matches!(st, Some(ActiveState::Active)));
    assert!(m.is_active("one.service"));
    assert_eq!(m.unit("one.service").unwrap().runtime.main_pid, None);
}

#[test]
fn oneshot_unclean_exit_becomes_failed() {
    let mut fs = MemFs::new();
    fs.file(
        "/etc/systemd/system/one.service",
        "[Service]\nType=oneshot\nExecStart=/bin/one\n",
    );
    let mut m = manager_with(&fs);
    let mut ex = MockExec::default();
    m.start_unit("one.service", &mut ex).unwrap();
    let st = m.on_exit("one.service", 1, WaitStatus::Exited { code: 3 });
    assert!(matches!(st, Some(ActiveState::Failed)));
}

#[test]
fn restart_always_schedules_pending_restart() {
    let mut fs = MemFs::new();
    fs.file(
        "/etc/systemd/system/r.service",
        "[Service]\nRestart=always\nExecStart=/bin/r\n",
    );
    let mut m = manager_with(&fs);
    let mut ex = MockExec::default();
    m.start_unit("r.service", &mut ex).unwrap();
    let st = m.on_exit("r.service", 1, WaitStatus::Signaled { sig: 15, core: false });
    assert!(matches!(st, Some(ActiveState::AutoRestart)));
    let rt = &m.unit("r.service").unwrap().runtime;
    let after = rt.pending_restart_in.unwrap();
    assert!((after - 0.1).abs() < 1e-9); // 默認 RestartSec=100ms
}

#[test]
fn restart_no_policy_no_pending() {
    let fs = fs_basic();
    let mut m = manager_with(&fs);
    let mut ex = MockExec::default();
    m.start_unit("b.service", &mut ex).unwrap();
    let st = m.on_exit("b.service", 1, WaitStatus::Exited { code: 0 });
    assert!(matches!(st, Some(ActiveState::Inactive)));
    assert!(m.unit("b.service").unwrap().runtime.pending_restart_in.is_none());
}

#[test]
fn child_exit_does_not_finalize_unit() {
    let fs = fs_basic();
    let mut m = manager_with(&fs);
    let mut ex = MockExec::default();
    m.start_unit("b.service", &mut ex).unwrap();
    m.units.get_mut("b.service").unwrap().runtime.procs.push(ProcessInfo {
        pid: 42,
        pgid: 1,
        is_main: false,
    });
    // 子進程退出: 單元維持 Active, procs 縮減
    let st = m.on_exit("b.service", 42, WaitStatus::Exited { code: 0 });
    assert!(matches!(st, Some(ActiveState::Active)));
    assert_eq!(m.unit("b.service").unwrap().runtime.procs.len(), 1);
    assert!(m.unit("b.service").unwrap().runtime.main_pid.is_some());
}

#[test]
fn on_exit_stopped_status_is_ignored() {
    let fs = fs_basic();
    let mut m = manager_with(&fs);
    let mut ex = MockExec::default();
    m.start_unit("b.service", &mut ex).unwrap();
    let st = m.on_exit("b.service", 1, WaitStatus::Stopped { sig: 18 });
    assert!(st.is_none());
    assert!(m.is_active("b.service"));
}

#[test]
fn on_exit_unknown_unit_is_none() {
    let fs = fs_basic();
    let mut m = manager_with(&fs);
    assert!(m.on_exit("ghost.service", 1, WaitStatus::Exited { code: 0 }).is_none());
}

// ─── 狀態字串 ────────────────────────────────────────────────────────

#[test]
fn active_state_str_roundtrip() {
    assert_eq!(ActiveState::Inactive.as_str(), "inactive");
    assert_eq!(ActiveState::Activating.as_str(), "activating");
    assert_eq!(ActiveState::Active.as_str(), "active");
    assert_eq!(ActiveState::Deactivating.as_str(), "deactivating");
    assert_eq!(ActiveState::AutoRestart.as_str(), "auto-restart");
    assert_eq!(ActiveState::Failed.as_str(), "failed");
}
