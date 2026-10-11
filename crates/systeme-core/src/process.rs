//! 進程管理: 信號表、waitpid 狀態解碼、KillMode 殺進程計劃 (純邏輯層)。
//!
//! 純邏輯部分跨平台可測; 真實系統調用 (subreaper/kill/waitpid) 位於
//! `sys` 子模組, 僅 `#[cfg(unix)]` 下編譯, 由 CI 容器集成驗證。

// re-export 供本模組使用者統一 import (同時引入本模組作用域)
pub use crate::service::{ExitEvent, KillMode};

// ─── 信號表 (Linux x86-64 通用信號) ─────────────────────────────────

/// Linux 標準信號 (1-31) 名稱表
const SIGNAL_NAMES: [&str; 32] = [
    "", "SIGHUP", "SIGINT", "SIGQUIT", "SIGILL", "SIGTRAP", "SIGABRT", "SIGBUS", "SIGFPE",
    "SIGKILL", "SIGUSR1", "SIGSEGV", "SIGUSR2", "SIGPIPE", "SIGALRM", "SIGTERM", "SIGSTKFLT",
    "SIGCHLD", "SIGCONT", "SIGSTOP", "SIGTSTP", "SIGTTIN", "SIGTTOU", "SIGURG", "SIGXCPU",
    "SIGXFSZ", "SIGVTALRM", "SIGPROF", "SIGWINCH", "SIGIO", "SIGPWR", "SIGSYS",
];

/// 信號編號 → 標準名 ("SIGTERM"); 無效編號返回 None
pub fn signal_name(sig: i32) -> Option<&'static str> {
    if (1..=31).contains(&sig) {
        Some(SIGNAL_NAMES[sig as usize])
    } else {
        None
    }
}

/// 信號名 → 編號。接受 "TERM"/"SIGTERM"/"term"/"sigterm" 與數字字串 "15"。
pub fn signal_from_name(name: &str) -> Option<i32> {
    let name = name.trim();
    if name.is_empty() {
        return None;
    }
    // 純數字
    if let Ok(n) = name.parse::<i32>() {
        return if (1..=31).contains(&n) { Some(n) } else { None };
    }
    let upper = name.to_ascii_uppercase();
    let stripped = upper.strip_prefix("SIG").unwrap_or(&upper);
    SIGNAL_NAMES
        .iter()
        .position(|s| *s == format!("SIG{stripped}"))
        .map(|i| i as i32)
}

// ─── waitpid 狀態解碼 ────────────────────────────────────────────────

/// waitpid 返回的原始 status 整數的分類結果 (libc 標準位布局)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaitStatus {
    /// WIFEXITED: 正常 exit(code)
    Exited { code: i32 },
    /// WIFSIGNALED: 被信號終止 (core = WCOREDUMP)
    Signaled { sig: i32, core: bool },
    /// WIFSTOPPED: 被信號停止 (ptrace/調試)
    Stopped { sig: i32 },
    /// WIFCONTINUED: SIGCONT 恢復 (Linux: status == 0xffff)
    Continued,
}

/// 解碼 waitpid 的 status 值 (Linux/x86 布局):
/// - 低 7 位 = 終止/停止信號; 0x80 = core dump 標誌; 8-15 位 = exit code 或停止信號
pub fn decode_wait_status(status: i32) -> WaitStatus {
    if status == 0xffff {
        return WaitStatus::Continued;
    }
    if status & 0xff == 0x7f {
        return WaitStatus::Stopped {
            sig: (status >> 8) & 0xff,
        };
    }
    let sig = status & 0x7f;
    if sig == 0 {
        WaitStatus::Exited {
            code: (status >> 8) & 0xff,
        }
    } else {
        WaitStatus::Signaled {
            sig,
            core: status & 0x80 != 0,
        }
    }
}

/// WaitStatus → service 層 ExitEvent 分類; Stopped/Continued 非終態返回 None
pub fn waitstatus_to_exit_event(ws: &WaitStatus) -> Option<ExitEvent> {
    match ws {
        WaitStatus::Exited { code: 0 } => Some(ExitEvent::CleanExit),
        WaitStatus::Exited { .. } => Some(ExitEvent::UncleanExit),
        WaitStatus::Signaled { .. } => Some(ExitEvent::Killed),
        WaitStatus::Stopped { .. } | WaitStatus::Continued => None,
    }
}

// ─── KillMode 殺進程計劃 ─────────────────────────────────────────────

/// 單元管轄的一個進程的描述
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcessInfo {
    pub pid: i32,
    pub pgid: i32,
    /// 是否為服務主進程
    pub is_main: bool,
}

/// 殺進程計劃: 兩階段 (先 SIGTERM, 超時後 SIGKILL)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KillPlan {
    /// 不發信號 (KillMode=none 或無主進程)
    None,
    /// TERM 與 KILL 同一 pid 集合 (control-group/process/process-group)
    Pids(Vec<i32>),
    /// Mixed: SIGTERM 只發主進程, SIGKILL 發全體
    Mixed { term: Vec<i32>, kill: Vec<i32> },
}

/// 依 KillMode 計算要殺的進程集合 (systemd kill 語義)。
///
/// - ControlGroup: 管轄全體 (無 cgroup 時為追蹤到的全部子進程)
/// - Process: 僅主進程
/// - ProcessGroup: 主進程所在進程組
/// - Mixed: TERM→主進程, KILL→全體
/// - None: 不殺 (只等其自行退出)
///
/// 輸出 pid 升序保證確定性; `procs` 中無 is_main 時返回 None (防禦)。
pub fn plan_kill(mode: KillMode, procs: &[ProcessInfo]) -> KillPlan {
    let Some(main) = procs.iter().find(|p| p.is_main) else {
        return KillPlan::None;
    };
    let mut all: Vec<i32> = procs.iter().map(|p| p.pid).collect();
    all.sort_unstable();

    match mode {
        KillMode::None => KillPlan::None,
        KillMode::Process => KillPlan::Pids(vec![main.pid]),
        KillMode::ProcessGroup => {
            let mut pids: Vec<i32> = procs
                .iter()
                .filter(|p| p.pgid == main.pgid)
                .map(|p| p.pid)
                .collect();
            pids.sort_unstable();
            KillPlan::Pids(pids)
        }
        KillMode::ControlGroup => KillPlan::Pids(all),
        KillMode::Mixed => KillPlan::Mixed {
            term: vec![main.pid],
            kill: all,
        },
    }
}

// ─── Unix 系統層 (cfg(unix); CI 容器驗證) ───────────────────────────

#[cfg(unix)]
pub mod sys {
    use super::{signal_from_name, KillPlan};
    use std::collections::HashSet;

    /// 設置本進程為 subreaper (PR_SET_CHILD_SUBREAPER), 使孤兒孫進程被收養。
    /// PID≠1 運行時的孤兒追蹤基礎。
    pub fn set_subreaper() -> std::io::Result<()> {
        // PR_SET_CHILD_SUBREAPER = 36
        let ret = unsafe { libc::prctl(libc::PR_SET_CHILD_SUBREAPER, 1, 0, 0, 0) };
        if ret < 0 {
            Err(std::io::Error::last_os_error())
        } else {
            Ok(())
        }
    }

    /// 是否可設置 subreaper (內核 >= 3.4); 靜默失敗僅供診斷
    pub fn can_subreaper() -> bool {
        set_subreaper().is_ok()
    }

    /// 發送信號到單個 pid
    pub fn kill_pid(pid: i32, sig: i32) -> std::io::Result<()> {
        let ret = unsafe { libc::kill(pid, sig) };
        if ret < 0 {
            Err(std::io::Error::last_os_error())
        } else {
            Ok(())
        }
    }

    /// 依計劃第一波發送 SIGTERM; 返回目標 pid 集合 (供超時後 SIGKILL 升級)
    pub fn send_term(plan: &KillPlan) -> std::io::Result<HashSet<i32>> {
        let (term_pids, all_pids) = match plan {
            KillPlan::None => return Ok(HashSet::new()),
            KillPlan::Pids(pids) => (pids.clone(), pids.clone()),
            KillPlan::Mixed { term, kill } => (term.clone(), kill.clone()),
        };
        for pid in &term_pids {
            kill_pid(*pid, libc::SIGTERM)?;
        }
        Ok(all_pids.into_iter().collect())
    }

    /// 超時升級: 對仍存活的目標集合發送 SIGKILL
    pub fn send_kill(pids: &HashSet<i32>) -> std::io::Result<()> {
        for pid in pids {
            kill_pid(*pid, libc::SIGKILL)?;
        }
        Ok(())
    }

    /// 解析 systemctl kill -s 的信號參數 (名稱或數字)
    pub fn resolve_signal(s: &str) -> std::io::Result<i32> {
        signal_from_name(s)
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, format!("unknown signal {s:?}")))
    }
}
