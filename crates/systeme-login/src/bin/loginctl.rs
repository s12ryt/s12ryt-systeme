//! loginctl — logind 查詢 CLI 二進制。
//!
//! 本工具為獨立子集: 進程內 Sessions 狀態演示 + 查詢格式化。
//! 與 systemed 守護進程的 IPC 集成留待後續輪 (logind D-Bus/IPC 協議)。

use std::process::ExitCode;

use systeme_login::cli::{parse_args, Command};
use systeme_login::render::{render_session_detail, render_sessions_table};
use systeme_login::Sessions;

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = match parse_args(&args) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(2);
        }
    };

    // 無守護進程 IPC: 對查詢命令顯示本地空表 (真實會話由 logind 守護進程提供)。
    let sessions = Sessions::new();
    match cmd {
        Command::Version => {
            println!("loginctl (systeme) {VERSION}");
            ExitCode::SUCCESS
        }
        Command::ListSessions => {
            print!("{}", render_sessions_table(&sessions));
            ExitCode::SUCCESS
        }
        Command::SessionStatus { id } | Command::Lock { id } | Command::Unlock { id } | Command::Terminate { id } => {
            // 未連接守護進程時, 查詢未知會話
            print!("{}", render_session_detail(&sessions, &id));
            ExitCode::from(1)
        }
        Command::ShowUser { user } => {
            eprintln!("Failed to get user: {user} (no logind daemon connected)");
            ExitCode::from(1)
        }
    }
}
