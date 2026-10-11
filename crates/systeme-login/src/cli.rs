//! loginctl CLI 參數解析。

/// 命令。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    ListSessions,
    SessionStatus { id: String },
    ShowUser { user: String },
    Lock { id: String },
    Unlock { id: String },
    Terminate { id: String },
    Version,
}

/// 解析參數; 未知旗標容忍跳過 (高兼容), 未知命令 Err。
pub fn parse_args(args: &[String]) -> Result<Command, String> {
    let positional: Vec<&String> = args.iter().filter(|a| !a.starts_with('-')).collect();

    if args.iter().any(|a| a == "--version") {
        return Ok(Command::Version);
    }

    match positional.as_slice() {
        [] => Ok(Command::ListSessions),
        [cmd] => match cmd.as_str() {
            "list-sessions" => Ok(Command::ListSessions),
            other => Err(format!("Unknown command '{other}'.")),
        },
        [cmd, arg] => match cmd.as_str() {
            "session-status" => Ok(Command::SessionStatus { id: arg.to_string() }),
            "show-user" => Ok(Command::ShowUser { user: arg.to_string() }),
            "lock-session" => Ok(Command::Lock { id: arg.to_string() }),
            "unlock-session" => Ok(Command::Unlock { id: arg.to_string() }),
            "terminate-session" => Ok(Command::Terminate { id: arg.to_string() }),
            other => Err(format!("Unknown command '{other}'.")),
        },
        [cmd, ..] => Err(format!("Command '{cmd}' takes at most one argument.")),
    }
}
