//! 真實進程執行器: std::process::Command 實現 ServiceExecutor。
//!
//! - argv[0] 為可執行文件, 其餘為參數
//! - env 為 "KEY=VAL" 字串列表 (ManagerCore 組裝的最終環境)
//! - stdin 接 /dev/null; stdout/stderr 暫時繼承守護進程 (journald 輪接管, 後續)
//! - unix 下 pre_exec setsid: 服務進程自建會話/進程組 (KillMode=ProcessGroup 基礎)
//! - spawn 後不持有 Child: 進程收割由守護進程 subreaper + waitpid 事件循環負責

use std::process::{Command, Stdio};

use systeme_core::manager::ServiceExecutor;
use systeme_core::service::ExecCommand;

/// 真實系統執行器
#[derive(Debug, Default, Clone)]
pub struct StdCommandExecutor {
    /// socket 激活 fd 預載: 下一個 spawn 的進程將帶上這些 fd (之後清空)。
    pub pending_fds: Vec<i32>,
}

impl StdCommandExecutor {
    pub fn new() -> Self {
        StdCommandExecutor {
            pending_fds: Vec::new(),
        }
    }
}

impl ServiceExecutor for StdCommandExecutor {
    fn spawn(
        &mut self,
        _unit: &str,
        cmd: &ExecCommand,
        env: &[String],
        cwd: Option<&str>,
    ) -> Result<i32, String> {
        let (argv0, args) = cmd
            .argv
            .split_first()
            .ok_or_else(|| "empty command".to_string())?;

        let mut c = Command::new(argv0);
        c.args(args);
        for pair in env {
            if let Some((k, v)) = pair.split_once('=') {
                c.env(k, v);
            }
        }
        if let Some(dir) = cwd {
            c.current_dir(dir);
        }
        c.stdin(Stdio::null());

        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            // setsid: 脫離守護進程會話, 自成進程組組長
            unsafe {
                c.pre_exec(|| {
                    libc::setsid();
                    Ok(())
                });
            }
            // socket 激活 fd: dup2 到 3,4,5... + LISTEN_FDS/LISTEN_PID
            if !self.pending_fds.is_empty() {
                crate::sockets::inject_fds(&mut c, &self.pending_fds);
            }
        }

        let child = c.spawn().map_err(|e| format!("spawn {argv0}: {e}"))?;
        // fd 已注入子進程: 消費後清空, 不洩漏到後續 spawn
        self.pending_fds.clear();
        Ok(child.id() as i32)
    }

    fn signal(&mut self, pid: i32, sig: i32) -> Result<(), String> {
        #[cfg(unix)]
        {
            systeme_core::process::sys::kill_pid(pid, sig).map_err(|e| e.to_string())
        }
        #[cfg(not(unix))]
        {
            let _ = (pid, sig);
            Err("signals are unsupported on this platform".to_string())
        }
    }

    fn prepare_socket_fds(&mut self, fds: &[i32]) {
        self.pending_fds = fds.to_vec();
    }
}
