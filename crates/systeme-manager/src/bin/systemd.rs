//! systemd 守護進程入口 (s12ryt-systeme)
//!
//! 與原版差異: 不要求 PID 1 — 以 subreaper + 進程追蹤代替,
//! cgroup 可用時自動掛上, 不可用時降級為純進程追蹤。
//! IPC 走自有 JSON 行協議 (Unix socket), D-Bus 後期。

#[cfg(unix)]
mod imp {
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::UnixListener;
    use tokio::sync::{mpsc, Mutex};

    use systeme_core::cgroup::{detect_backend, CgroupMode};
    use systeme_core::ipc::{Request, Response};
    use systeme_core::manager::RuntimeMode;
    use systeme_core::path::PathScheduler;
    use systeme_core::process::{decode_wait_status, sys};
    use systeme_manager::{DaemonCore, OsProbe, StdCommandExecutor, TimerScheduler};
    use systeme_units::search::{user_search_paths, OsFs, SearchPaths};

    enum Event {
        ChildExit(i32, i32),
    }

    pub fn main() {
        let args: Vec<String> = std::env::args().skip(1).collect();
        if args.iter().any(|a| a == "--version") {
            println!("systemd 257 (s12ryt-systeme)");
            return;
        }
        // --user/--system 顯式指定模式; 其餘參數容忍忽略, 模式自動探測
        let force_user = args.iter().any(|a| a == "--user");
        let force_system = args.iter().any(|a| a == "--system");
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("failed to build tokio runtime");
        if let Err(e) = rt.block_on(run(force_user, force_system)) {
            eprintln!("systemd: fatal: {e}");
            std::process::exit(1);
        }
    }

    async fn run(force_user: bool, force_system: bool) -> Result<(), String> {
        let is_root = unsafe { libc::geteuid() } == 0;
        let mode = if force_user {
            RuntimeMode::User
        } else if force_system {
            RuntimeMode::System
        } else {
            RuntimeMode::detect(is_root)
        };
        eprintln!("systemd: running in {} mode (PID {})", mode.as_str(), std::process::id());

        // subreaper: 非 PID 1 也能收養孤兒 (best-effort)
        if let Err(e) = sys::set_subreaper(true) {
            eprintln!("systemd: warning: PR_SET_CHILD_SUBREAPER failed ({e}), orphan reparenting to us unavailable");
        }

        // cgroup 後端探測 (v2 > v1 > none); none 時降級純進程追蹤
        let cgroup_mode = detect_backend(&|p| Path::new(p).exists());
        match cgroup_mode {
            CgroupMode::V2 => eprintln!("systemd: cgroup v2 detected"),
            CgroupMode::V1 => eprintln!("systemd: cgroup v1 detected"),
            CgroupMode::None => eprintln!("systemd: no cgroup filesystem, falling back to process tracking"),
        }

        // 搜索路徑: 按運行模式 (User 模式即使 root 也用用戶路徑)
        let search_paths: SearchPaths = if mode == RuntimeMode::User {
            let home = std::env::var("HOME").ok().map(std::path::PathBuf::from);
            let xdg = std::env::var("XDG_CONFIG_HOME").ok().map(std::path::PathBuf::from);
            user_search_paths(xdg.as_deref(), home.as_deref())
        } else {
            systeme_units::search::system_search_paths()
        };

        let mut core = DaemonCore::new(
            systeme_core::manager::ManagerCore::new(mode),
            search_paths.clone(),
        );
        {
            let os_fs = OsFs;
            let report = core.manager.load_all(&os_fs, &search_paths);
            eprintln!(
                "systemd: loaded {} units ({} masked, {} dropped)",
                report.loaded, report.masked, report.dropped
            );
        }
        let core = Arc::new(Mutex::new(core));

        // IPC socket
        let sock_path = socket_path();
        if let Some(parent) = sock_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("cannot create socket dir {}: {e}", parent.display()))?;
        }
        // 清理殘留 socket
        match std::fs::remove_file(&sock_path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(format!("cannot remove stale socket: {e}")),
        }
        let listener = UnixListener::bind(&sock_path)
            .map_err(|e| format!("cannot bind {}: {e}", sock_path.display()))?;
        eprintln!("systemd: listening on {}", sock_path.display());

        // 子進程收割線程: waitpid(-1, WNOHANG) 輪詢 → mpsc
        let (tx, mut rx) = mpsc::unbounded_channel::<Event>();
        spawn_reaper_thread(tx);

        // timer 調度器: boot 錨點從 /proc/uptime 推導 (失敗用 now 近似)
        let now_local = chrono::Local::now();
        let boot = TimerScheduler::detect_boot(now_local);
        let mut scheduler = TimerScheduler::new(boot, now_local.naive_local());

        // path 單元調度器 (文件系統邊沿觸發)
        let mut path_scheduler = PathScheduler::new();

        // 優雅退出
        let mut sigterm =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .map_err(|e| format!("cannot install SIGTERM handler: {e}"))?;
        let mut sigint =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())
                .map_err(|e| format!("cannot install SIGINT handler: {e}"))?;

        let mut tick = tokio::time::interval(std::time::Duration::from_secs(1));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        loop {
            tokio::select! {
                accepted = listener.accept() => {
                    match accepted {
                        Ok((stream, _)) => {
                            let core = Arc::clone(&core);
                            tokio::spawn(handle_conn(stream, core));
                        }
                        Err(e) => eprintln!("systemd: accept error: {e}"),
                    }
                }
                Some(ev) = rx.recv() => {
                    if let Event::ChildExit(pid, raw_status) = ev {
                        let mut guard = core.lock().await;
                        let unit = unit_of_pid(&guard, pid);
                        if let Some(name) = unit {
                            let status = decode_wait_status(raw_status);
                            if let Some(state) = guard.manager.on_exit(&name, pid, status) {
                                eprintln!("systemd: {name} is now {}", state.as_str());
                            }
                        }
                    }
                }
                _ = tick.tick() => {
                    let mut guard = core.lock().await;
                    fire_pending_restarts(&mut guard);
                    fire_due_timers(&mut guard, &mut scheduler);
                    fire_due_paths(&mut guard, &mut path_scheduler);
                }
                _ = sigterm.recv() => {
                    eprintln!("systemd: SIGTERM received, shutting down");
                    break;
                }
                _ = sigint.recv() => {
                    eprintln!("systemd: SIGINT received, shutting down");
                    break;
                }
            }
        }

        // 清理 socket 文件
        let _ = std::fs::remove_file(&sock_path);
        Ok(())
    }

    async fn handle_conn(stream: tokio::net::UnixStream, core: Arc<Mutex<DaemonCore>>) {
        let (rd, mut wr) = stream.into_split();
        let mut lines = BufReader::new(rd).lines();
        loop {
            let line = match lines.next_line().await {
                Ok(Some(l)) => l,
                _ => return,
            };
            let resp = match serde_json::from_str::<Request>(&line) {
                Ok(req) => {
                    let mut guard = core.lock().await;
                    let mut exec = StdCommandExecutor::new();
                    let os_fs = OsFs;
                    guard.handle(&req, &mut exec, &os_fs)
                }
                Err(e) => Response::Error {
                    message: format!("malformed request: {e}"),
                },
            };
            let mut out = match serde_json::to_string(&resp) {
                Ok(s) => s,
                Err(_) => continue,
            };
            out.push('\n');
            if wr.write_all(out.as_bytes()).await.is_err() {
                return;
            }
        }
    }

    fn unit_of_pid(core: &DaemonCore, pid: i32) -> Option<String> {
        core.manager
            .units
            .iter()
            .find(|(_, e)| e.runtime.main_pid == Some(pid) || e.runtime.procs.iter().any(|p| p.pid == pid))
            .map(|(name, _)| name.clone())
    }

    fn fire_pending_restarts(core: &mut DaemonCore) {
        let due: Vec<String> = {
            let mut due = Vec::new();
            for (name, entry) in core.manager.units.iter_mut() {
                if let Some(t) = entry.runtime.pending_restart_in.as_mut() {
                    *t -= 1.0;
                    if *t <= 0.0 {
                        entry.runtime.pending_restart_in = None;
                        due.push(name.clone());
                    }
                }
            }
            due
        };
        if !due.is_empty() {
            let mut exec = StdCommandExecutor::new();
            for name in due {
                eprintln!("systemd: restarting {name}");
                if let Err(e) = core.manager.start_unit(&name, &mut exec) {
                    eprintln!("systemd: restart {name} failed: {e}");
                }
            }
        }
    }

    fn fire_due_timers(core: &mut DaemonCore, scheduler: &mut TimerScheduler) {
        let now = chrono::Local::now().naive_local();
        let due = scheduler.due_timers(&core.manager, now);
        if due.is_empty() {
            return;
        }
        let mut exec = StdCommandExecutor::new();
        for (timer, target) in due {
            eprintln!("systemd: {timer} fired, starting {target}");
            if let Err(e) = core.manager.start_unit(&target, &mut exec) {
                eprintln!("systemd: start {target} (from {timer}) failed: {e}");
            }
        }
    }

    fn fire_due_paths(core: &mut DaemonCore, scheduler: &mut PathScheduler) {
        let probe = OsProbe;
        let due = scheduler.due_paths(&core.manager, &probe);
        if due.is_empty() {
            return;
        }
        let mut exec = StdCommandExecutor::new();
        for (path_unit, target) in due {
            eprintln!("systemd: {path_unit} triggered, starting {target}");
            if let Err(e) = core.manager.start_unit(&target, &mut exec) {
                eprintln!("systemd: start {target} (from {path_unit}) failed: {e}");
            }
        }
    }

    fn spawn_reaper_thread(tx: mpsc::UnboundedSender<Event>) {
        std::thread::spawn(move || loop {
            let mut status: i32 = 0;
            let pid = unsafe { libc::waitpid(-1, &mut status, libc::WNOHANG) };
            if pid > 0 {
                if tx.send(Event::ChildExit(pid, status)).is_err() {
                    return; // 接收端已關閉, 退出線程
                }
            } else {
                std::thread::sleep(std::time::Duration::from_millis(200));
            }
        });
    }

    fn socket_path() -> PathBuf {
        if let Ok(p) = std::env::var("SYSTEME_SOCKET") {
            return PathBuf::from(p);
        }
        if unsafe { libc::geteuid() } == 0 {
            return PathBuf::from("/run/systemd/private");
        }
        if let Ok(run) = std::env::var("XDG_RUNTIME_DIR") {
            return Path::new(&run).join("systemd").join("private");
        }
        PathBuf::from("/run/systemd/private")
    }
}

#[cfg(unix)]
fn main() {
    imp::main();
}

#[cfg(not(unix))]
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--version") {
        println!("systemd 257 (s12ryt-systeme)");
        return;
    }
    eprintln!("systemd (s12ryt-systeme): this daemon requires Unix (Unix sockets, fork/waitpid).");
    eprintln!("On Windows only the library and pure-logic layers are supported.");
    std::process::exit(1);
}
