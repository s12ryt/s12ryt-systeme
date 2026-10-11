//! systemctl 執行層: 命令 → IPC 請求 → 輸出格式化 + exit code
//!
//! `IpcTransport` 抽象使客戶端邏輯可脫離真實 Unix socket 測試
//! (Windows 開發機 / CI 容器共用同一套測試)。

use crate::cli::Command;
#[cfg(unix)]
use std::io::{BufRead, BufReader, Write};
use systeme_core::ipc::{Request, Response, UnitInfo};

/// IPC 傳輸抽象: 發送一條請求, 返回一條響應
pub trait IpcTransport {
    fn request(&mut self, req: &Request) -> Result<Response, String>;
}

/// 執行命令, 返回 (stdout 文本, exit code)。
/// Help/Version 不觸發 IPC。
pub fn run_command(cmd: &Command, transport: &mut dyn IpcTransport) -> (String, i32) {
    match cmd {
        Command::Help => (help_text().to_string(), 0),
        Command::Version => (
            format!("systemctl (s12ryt-systeme {})", env!("CARGO_PKG_VERSION")),
            0,
        ),
        _ => match to_request(cmd) {
            None => ("internal error: command has no request mapping".to_string(), 1),
            Some(req) => match transport.request(&req) {
                Err(e) => (format!("Failed to connect to daemon: {e}"), 1),
                Ok(resp) => render_response(resp),
            },
        },
    }
}

/// 命令 → IPC 請求映射。`Status{None}` 退化為 ListUnits (顯示全部單元表格)。
pub fn to_request(cmd: &Command) -> Option<Request> {
    Some(match cmd {
        Command::Start { unit } => Request::Start { unit: unit.clone() },
        Command::Stop { unit } => Request::Stop { unit: unit.clone() },
        Command::Restart { unit } => Request::Restart { unit: unit.clone() },
        Command::Reload { unit } => Request::Reload { unit: unit.clone() },
        Command::Status { unit } => match unit {
            Some(u) => Request::Status { unit: u.clone() },
            None => Request::ListUnits,
        },
        Command::ListUnits => Request::ListUnits,
        Command::IsActive { unit } => Request::IsActive { unit: unit.clone() },
        Command::Cat { unit } => Request::Cat { unit: unit.clone() },
        Command::Enable { unit } => Request::Enable { unit: unit.clone() },
        Command::Disable { unit } => Request::Disable { unit: unit.clone() },
        Command::Mask { unit } => Request::Mask { unit: unit.clone() },
        Command::Unmask { unit } => Request::Unmask { unit: unit.clone() },
        Command::DaemonReload => Request::DaemonReload,
        Command::Help | Command::Version => return None,
    })
}

/// 響應 → (stdout, exit code)
fn render_response(resp: Response) -> (String, i32) {
    match resp {
        Response::Ok => (String::new(), 0),
        Response::Units(units) => (format_units_table(&units), 0),
        Response::UnitStatus(info) => (format_status(&info), 0),
        Response::Active(b) => (
            if b { "active" } else { "inactive" }.to_string(),
            if b { 0 } else { 3 },
        ),
        Response::Cat { text, .. } => (text, 0),
        Response::Error { message } => (message, 1),
        Response::NotImplemented { request } => {
            (format!("Operation {request} is not implemented by the daemon."), 1)
        }
    }
}

/// list-units 表格: UNIT / LOAD / ACTIVE / SUB / DESCRIPTION (對齊)
pub fn format_units_table(units: &[UnitInfo]) -> String {
    let mut out = String::new();
    if units.is_empty() {
        out.push_str("0 loaded units listed.\n");
        return out;
    }
    let w_unit = units.iter().map(|u| u.name.len()).max().unwrap_or(4).max(4);
    let w_load = units.iter().map(|u| u.load_state.len()).max().unwrap_or(4).max(4);
    let w_act = units
        .iter()
        .map(|u| u.active_state.len())
        .max()
        .unwrap_or(6)
        .max(6);
    out.push_str(&format!(
        "{:<w_unit$} {:<w_load$} {:<w_act$} {:<8} {}\n",
        "UNIT", "LOAD", "ACTIVE", "SUB", "DESCRIPTION"
    ));
    for u in units {
        // 無獨立 sub-state 字段, 與 active_state 同值 (簡化)
        out.push_str(&format!(
            "{:<w_unit$} {:<w_load$} {:<w_act$} {:<8} {}\n",
            u.name, u.load_state, u.active_state, u.active_state, u.description
        ));
    }
    out
}

/// 單單元 status 塊 (模仿 systemctl status 版式)
pub fn format_status(info: &UnitInfo) -> String {
    let mut out = String::new();
    let dot = if info.load_state == "masked" { "×" } else { "●" };
    out.push_str(&format!("{dot} {} - {}\n", info.name, info.description));
    out.push_str(&format!("     Loaded: {}\n", info.load_state));
    out.push_str(&format!("     Active: {}\n", info.active_state));
    if let Some(pid) = info.main_pid {
        out.push_str(&format!("   Main PID: {pid}\n"));
    }
    out
}

pub fn help_text() -> &'static str {
    "systemctl [OPTIONS...] COMMAND [UNIT...]\n\
     \n\
     Commands:\n\
       start UNIT...        Start one or more units\n\
       stop UNIT...         Stop one or more units\n\
       restart UNIT...      Restart one or more units\n\
       reload UNIT...       Reload one or more units\n\
       status [UNIT...]     Show runtime status of units\n\
       list-units           List known units\n\
       is-active UNIT...    Check whether units are active\n\
       cat UNIT...          Show unit files\n\
       enable UNIT...       Enable units to start at boot\n\
       disable UNIT...      Disable units\n\
       mask UNIT...         Mask units\n\
       unmask UNIT...       Unmask units\n\
       daemon-reload        Reload the manager configuration\n\
       help                 Show this help\n\
       --version            Show version\n"
}

// ---------------------------------------------------------------------------
// 真實 Unix socket 傳輸 (JSON 行協議)
// ---------------------------------------------------------------------------

/// 連接管理器私有 socket 的同步傳輸實現
#[cfg(unix)]
pub struct UnixSocketTransport {
    stream: std::os::unix::net::UnixStream,
    reader: BufReader<std::os::unix::net::UnixStream>,
}

#[cfg(unix)]
impl UnixSocketTransport {
    pub fn connect(path: &std::path::Path) -> Result<Self, String> {
        let stream = std::os::unix::net::UnixStream::connect(path)
            .map_err(|e| format!("connect {}: {e}", path.display()))?;
        let reader = BufReader::new(stream.try_clone().map_err(|e| e.to_string())?);
        Ok(Self { stream, reader })
    }
}

#[cfg(unix)]
impl IpcTransport for UnixSocketTransport {
    fn request(&mut self, req: &Request) -> Result<Response, String> {
        let mut line = serde_json::to_string(req).map_err(|e| e.to_string())?;
        line.push('\n');
        self.stream
            .write_all(line.as_bytes())
            .and_then(|_| self.stream.flush())
            .map_err(|e| e.to_string())?;

        let mut buf = String::new();
        self.reader
            .read_line(&mut buf)
            .map_err(|e| e.to_string())?;
        serde_json::from_str(buf.trim()).map_err(|e| format!("bad response: {e}"))
    }
}

/// 選擇管理器 socket 路徑:
/// 1. `$SYSTEME_SOCKET` 覆蓋 (測試/非標準部署)
/// 2. root → `/run/systemd/private`
/// 3. 其他 → `$XDG_RUNTIME_DIR/systemd/private`
#[cfg(unix)]
pub fn socket_path() -> std::path::PathBuf {
    if let Ok(p) = std::env::var("SYSTEME_SOCKET") {
        return std::path::PathBuf::from(p);
    }
    if unsafe { libc::geteuid() } == 0 {
        return std::path::PathBuf::from("/run/systemd/private");
    }
    if let Ok(run) = std::env::var("XDG_RUNTIME_DIR") {
        return std::path::PathBuf::from(run).join("systemd/private");
    }
    std::path::PathBuf::from("/run/systemd/private")
}
