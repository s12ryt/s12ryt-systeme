//! journald 二進制入口: 監聽 /dev/log (unix datagram), 解析並寫入 JSONL。
//!
//! - /dev/log 綁定失敗 (無權限/容器限制) 時退回 $XDG_RUNTIME_DIR/systeme-dev-log
//! - $SYSTEME_DEV_LOG 可覆蓋監聽路徑 (測試用)
//! - SIGTERM/SIGINT 走默認終止 (不留殘留 socket 文件; /dev/log 由系統管理)
//! - 非 unix 平台: 僅 --version, 否則退出 1

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("journald (systeme) 0.1.0");
        return ExitCode::SUCCESS;
    }

    #[cfg(unix)]
    {
        run()
    }
    #[cfg(not(unix))]
    {
        eprintln!("journald: /dev/log is only available on unix platforms");
        ExitCode::FAILURE
    }
}

#[cfg(unix)]
fn run() -> ExitCode {
    use std::os::unix::net::UnixDatagram;

    use systeme_journal::{journal_path, process_datagram, JournalStore, JsonlSink};

    let path = journal_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let sink = match JsonlSink::create(&path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("journald: cannot open {}: {e}", path.display());
            return ExitCode::FAILURE;
        }
    };
    // 100 條 / 30s per-source (systemd 風格的收緊值)
    let mut store = JournalStore::new(Box::new(sink), 100, 30_000);

    let sock = match bind_socket() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("journald: {e}");
            return ExitCode::FAILURE;
        }
    };

    eprintln!("journald: writing to {}", path.display());
    let mut buf = [0u8; 8192];
    loop {
        match sock.recv_from(&mut buf) {
            Ok((n, _addr)) => {
                let now_ms = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis() as u64)
                    .unwrap_or(0);
                let year = chrono::Local::now().format("%Y").to_string();
                let year: i32 = year.parse().unwrap_or(1970);
                process_datagram(&buf[..n], year, &mut store, now_ms);
            }
            Err(e) => {
                eprintln!("journald: recv error: {e}");
            }
        }
    }
}

#[cfg(unix)]
fn bind_socket() -> Result<std::os::unix::net::UnixDatagram, String> {
    use std::os::unix::net::UnixDatagram;
    use std::path::PathBuf;

    // 測試覆蓋路徑
    if let Ok(p) = std::env::var("SYSTEME_DEV_LOG") {
        let p = PathBuf::from(p);
        if let Some(parent) = p.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::remove_file(&p);
        return UnixDatagram::bind(&p).map_err(|e| format!("bind {p:?}: {e}"));
    }

    match UnixDatagram::bind("/dev/log") {
        Ok(s) => Ok(s),
        Err(e) => {
            let fallback = std::env::var("XDG_RUNTIME_DIR")
                .map(|d| PathBuf::from(d).join("systeme-dev-log"))
                .unwrap_or_else(|_| PathBuf::from("/tmp/systeme-dev-log"));
            eprintln!(
                "journald: /dev/log bind failed ({e}); falling back to {}",
                fallback.display()
            );
            let _ = std::fs::remove_file(&fallback);
            UnixDatagram::bind(&fallback).map_err(|e| format!("bind {}: {e}", fallback.display()))
        }
    }
}
