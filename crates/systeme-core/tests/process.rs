//! 進程管理純邏輯層的 TDD 測試: 信號表、waitpid 狀態解碼、KillMode 殺進程計劃。
//!
//! (真實 subreaper/kill/waitpid 系統調用為 #[cfg(unix)] 層, 由 CI 容器驗證)

use systeme_core::process::{
    decode_wait_status, plan_kill, signal_from_name, signal_name, ExitEvent, KillMode, KillPlan,
    ProcessInfo, WaitStatus,
};

// ─── 信號表 ─────────────────────────────────────────────────────────

#[test]
fn signal_names_roundtrip() {
    assert_eq!(signal_name(15), Some("SIGTERM"));
    assert_eq!(signal_name(9), Some("SIGKILL"));
    assert_eq!(signal_name(1), Some("SIGHUP"));
    assert_eq!(signal_name(19), Some("SIGSTOP"));
    assert_eq!(signal_name(18), Some("SIGCONT"));
    assert_eq!(signal_name(31), Some("SIGSYS"));
    assert_eq!(signal_name(0), None);
    assert_eq!(signal_name(32), None);
    assert_eq!(signal_name(-9), None);
}

#[test]
fn signal_from_name_variants() {
    assert_eq!(signal_from_name("TERM"), Some(15));
    assert_eq!(signal_from_name("SIGTERM"), Some(15));
    assert_eq!(signal_from_name("term"), Some(15));
    assert_eq!(signal_from_name("sigkill"), Some(9));
    assert_eq!(signal_from_name("KILL"), Some(9));
    assert_eq!(signal_from_name("HUP"), Some(1));
}

#[test]
fn signal_from_name_accepts_numeric() {
    assert_eq!(signal_from_name("9"), Some(9));
    assert_eq!(signal_from_name("15"), Some(15));
}

#[test]
fn signal_from_name_rejects_garbage() {
    assert_eq!(signal_from_name("SIGBOGUS"), None);
    assert_eq!(signal_from_name("99"), None);
    assert_eq!(signal_from_name("0"), None);
    assert_eq!(signal_from_name(""), None);
}

// ─── waitpid 狀態解碼 (libc 標準位布局) ─────────────────────────────

#[test]
fn decode_exit_zero() {
    assert_eq!(decode_wait_status(0x0000), WaitStatus::Exited { code: 0 });
}

#[test]
fn decode_exit_code() {
    assert_eq!(decode_wait_status(0x0300), WaitStatus::Exited { code: 3 });
    assert_eq!(decode_wait_status(0xff00), WaitStatus::Exited { code: 255 });
}

#[test]
fn decode_signaled() {
    // SIGTERM(15), 無 core dump
    assert_eq!(
        decode_wait_status(0x000f),
        WaitStatus::Signaled { sig: 15, core: false }
    );
    // SIGKILL(9)
    assert_eq!(
        decode_wait_status(0x0009),
        WaitStatus::Signaled { sig: 9, core: false }
    );
}

#[test]
fn decode_signaled_with_coredump() {
    // SIGSEGV(11) + 0x80 core 標誌 = 0x8b
    assert_eq!(
        decode_wait_status(0x008b),
        WaitStatus::Signaled { sig: 11, core: true }
    );
}

#[test]
fn decode_stopped() {
    // WIFSTOPPED: 低字節 0x7f, 高字節為停止信號。SIGSTOP=19 → (19<<8)|0x7f
    assert_eq!(
        decode_wait_status(0x137f),
        WaitStatus::Stopped { sig: 19 }
    );
}

#[test]
fn decode_continued() {
    // Linux WIFCONTINUED: status == 0xffff
    assert_eq!(decode_wait_status(0xffff), WaitStatus::Continued);
}

// ─── WaitStatus → ExitEvent 映射 (連接 service 層) ──────────────────

#[test]
fn waitstatus_to_exit_event_mapping() {
    use systeme_core::process::waitstatus_to_exit_event;
    assert_eq!(
        waitstatus_to_exit_event(&WaitStatus::Exited { code: 0 }),
        Some(ExitEvent::CleanExit)
    );
    assert_eq!(
        waitstatus_to_exit_event(&WaitStatus::Exited { code: 1 }),
        Some(ExitEvent::UncleanExit)
    );
    assert_eq!(
        waitstatus_to_exit_event(&WaitStatus::Signaled { sig: 15, core: false }),
        Some(ExitEvent::Killed)
    );
    // Stopped/Continued 不是終態
    assert_eq!(
        waitstatus_to_exit_event(&WaitStatus::Stopped { sig: 19 }),
        None
    );
    assert_eq!(waitstatus_to_exit_event(&WaitStatus::Continued), None);
}

// ─── KillMode 殺進程計劃 ────────────────────────────────────────────

/// 測試用進程集: main(100, pgid 100) + 同組 worker(101,102) + 異組 child(200, pgid 200)
fn sample_procs() -> Vec<ProcessInfo> {
    vec![
        ProcessInfo { pid: 100, pgid: 100, is_main: true },
        ProcessInfo { pid: 101, pgid: 100, is_main: false },
        ProcessInfo { pid: 102, pgid: 100, is_main: false },
        ProcessInfo { pid: 200, pgid: 200, is_main: false },
    ]
}

#[test]
fn kill_plan_control_group_targets_all() {
    let plan = plan_kill(KillMode::ControlGroup, &sample_procs());
    match plan {
        KillPlan::Pids(pids) => assert_eq!(pids, vec![100, 101, 102, 200]),
        other => panic!("expected Pids, got {other:?}"),
    }
}

#[test]
fn kill_plan_process_targets_main_only() {
    let plan = plan_kill(KillMode::Process, &sample_procs());
    match plan {
        KillPlan::Pids(pids) => assert_eq!(pids, vec![100]),
        other => panic!("expected Pids, got {other:?}"),
    }
}

#[test]
fn kill_plan_process_group_targets_same_pgid() {
    let plan = plan_kill(KillMode::ProcessGroup, &sample_procs());
    match plan {
        KillPlan::Pids(pids) => assert_eq!(pids, vec![100, 101, 102]),
        other => panic!("expected Pids, got {other:?}"),
    }
}

#[test]
fn kill_plan_mixed_term_main_kill_all() {
    let plan = plan_kill(KillMode::Mixed, &sample_procs());
    match plan {
        KillPlan::Mixed { term, kill } => {
            assert_eq!(term, vec![100]);
            assert_eq!(kill, vec![100, 101, 102, 200]);
        }
        other => panic!("expected Mixed, got {other:?}"),
    }
}

#[test]
fn kill_plan_none_sends_nothing() {
    assert_eq!(plan_kill(KillMode::None, &sample_procs()), KillPlan::None);
}

#[test]
fn kill_plan_without_main_is_noop() {
    // 防禦: 進程集中無主進程 (已退出) 時不殺任何進程
    let procs = vec![ProcessInfo { pid: 101, pgid: 100, is_main: false }];
    assert_eq!(plan_kill(KillMode::ControlGroup, &procs), KillPlan::None);
}

#[test]
fn kill_plan_empty_procs_is_noop() {
    assert_eq!(plan_kill(KillMode::ControlGroup, &[]), KillPlan::None);
}

#[test]
fn kill_plan_pid_ordering_is_sorted() {
    // 輸出 pid 按升序, 保證 kill 順序確定性
    let procs = vec![
        ProcessInfo { pid: 305, pgid: 300, is_main: false },
        ProcessInfo { pid: 300, pgid: 300, is_main: true },
        ProcessInfo { pid: 301, pgid: 300, is_main: false },
    ];
    let plan = plan_kill(KillMode::ControlGroup, &procs);
    match plan {
        KillPlan::Pids(pids) => assert_eq!(pids, vec![300, 301, 305]),
        other => panic!("expected Pids, got {other:?}"),
    }
}
