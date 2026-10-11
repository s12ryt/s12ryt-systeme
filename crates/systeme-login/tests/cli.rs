//! loginctl 查詢 CLI 測試。

use systeme_login::cli::{parse_args, Command};

#[test]
fn no_args_lists_sessions() {
    assert_eq!(parse_args(&[]).unwrap(), Command::ListSessions);
}

#[test]
fn list_sessions_command() {
    assert_eq!(
        parse_args(&["list-sessions".into()]).unwrap(),
        Command::ListSessions
    );
}

#[test]
fn session_status() {
    assert_eq!(
        parse_args(&["session-status".into(), "3".into()]).unwrap(),
        Command::SessionStatus { id: "3".into() }
    );
    assert!(parse_args(&["session-status".into()]).is_err());
}

#[test]
fn show_user() {
    assert_eq!(
        parse_args(&["show-user".into(), "alice".into()]).unwrap(),
        Command::ShowUser { user: "alice".into() }
    );
    assert!(parse_args(&["show-user".into()]).is_err());
}

#[test]
fn lock_and_unlock() {
    assert_eq!(
        parse_args(&["lock-session".into(), "5".into()]).unwrap(),
        Command::Lock { id: "5".into() }
    );
    assert_eq!(
        parse_args(&["unlock-session".into(), "5".into()]).unwrap(),
        Command::Unlock { id: "5".into() }
    );
}

#[test]
fn terminate_session() {
    assert_eq!(
        parse_args(&["terminate-session".into(), "7".into()]).unwrap(),
        Command::Terminate { id: "7".into() }
    );
}

#[test]
fn unknown_flag_tolerated_no_positional() {
    // 高兼容: 未知旗標跳過, 仍無位置參數→列表
    assert_eq!(
        parse_args(&["--no-legend".into(), "--no-pager".into()]).unwrap(),
        Command::ListSessions
    );
}

#[test]
fn unknown_command_errs() {
    assert!(parse_args(&["frobnicate".into()]).is_err());
}

#[test]
fn version_flag() {
    assert_eq!(parse_args(&["--version".into()]).unwrap(), Command::Version);
}
