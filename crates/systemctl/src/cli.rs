//! systemctl 命令行參數解析
//!
//! 語義對齊 systemd systemctl 子集:
//! - 已知/未知旗標一律容忍 (高兼容, 不因未知參數報錯)
//! - 裸單元名自動補 `.service` 後綴 (systemd default suffix)
//! - 需要單元的命令缺單元 → `MissingUnit` 錯誤

use systeme_units::UnitType;

/// 解析後的 systemctl 命令
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    Start { unit: String },
    Stop { unit: String },
    Restart { unit: String },
    Reload { unit: String },
    Status { unit: Option<String> },
    ListUnits,
    IsActive { unit: String },
    Cat { unit: String },
    Enable { unit: String },
    Disable { unit: String },
    Mask { unit: String },
    Unmask { unit: String },
    DaemonReload,
    Help,
    Version,
}

/// 參數解析錯誤
#[derive(Debug, Clone, PartialEq)]
pub enum ParseError {
    UnknownCommand(String),
    MissingUnit(String),
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParseError::UnknownCommand(c) => {
                write!(f, "Unknown operation {c}.")
            }
            ParseError::MissingUnit(c) => {
                write!(f, "Operation {c} requires a unit name.")
            }
        }
    }
}

/// 解析 argv (不含程序名)。空參數 → Help。
pub fn parse_args(args: &[String]) -> Result<Command, ParseError> {
    // --version 是常見的全局旗標調用形式
    if args.iter().any(|a| a == "--version") {
        return Ok(Command::Version);
    }

    // 高兼容: 旗標 (以 '-' 開頭) 一律容忍跳過, 不論已知未知
    let positional: Vec<&str> = args
        .iter()
        .map(|s| s.as_str())
        .filter(|s| !s.starts_with('-'))
        .collect();

    if positional.is_empty() {
        return Ok(Command::Help);
    }

    let word = positional[0];
    let unit = |name: &str| -> Result<String, ParseError> {
        positional
            .get(1)
            .map(|s| default_suffix(s))
            .ok_or_else(|| ParseError::MissingUnit(name.to_string()))
    };

    match word {
        "start" => Ok(Command::Start { unit: unit(word)? }),
        "stop" => Ok(Command::Stop { unit: unit(word)? }),
        "restart" => Ok(Command::Restart { unit: unit(word)? }),
        "reload" => Ok(Command::Reload { unit: unit(word)? }),
        "status" => Ok(Command::Status {
            unit: positional.get(1).map(|s| default_suffix(s)),
        }),
        "list-units" => Ok(Command::ListUnits),
        "is-active" => Ok(Command::IsActive { unit: unit(word)? }),
        "cat" => Ok(Command::Cat { unit: unit(word)? }),
        "enable" => Ok(Command::Enable { unit: unit(word)? }),
        "disable" => Ok(Command::Disable { unit: unit(word)? }),
        "mask" => Ok(Command::Mask { unit: unit(word)? }),
        "unmask" => Ok(Command::Unmask { unit: unit(word)? }),
        "daemon-reload" => Ok(Command::DaemonReload),
        "help" => Ok(Command::Help),
        other => Err(ParseError::UnknownCommand(other.to_string())),
    }
}

/// systemd default-suffix 語義: 名字無已知單元類型後綴 → 追加 `.service`。
/// `foo` → `foo.service`;`foo.timer` 不變;`foo.bar` (未知後綴) → `foo.bar.service`。
pub fn default_suffix(name: &str) -> String {
    if UnitType::split_name(name).is_some() {
        name.to_string()
    } else {
        format!("{name}.service")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_suffix_rules() {
        assert_eq!(default_suffix("foo"), "foo.service");
        assert_eq!(default_suffix("foo.service"), "foo.service");
        assert_eq!(default_suffix("foo.socket"), "foo.socket");
        assert_eq!(default_suffix("foo.bar"), "foo.bar.service");
    }
}
