//! systemctl CLI 參數解析測試 (TDD RED → GREEN)
use systemctl::cli::{parse_args, Command, ParseError};

fn args(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

#[test]
fn no_args_yields_help() {
    let cmd = parse_args(&args(&[])).unwrap();
    assert!(matches!(cmd, Command::Help));
}

#[test]
fn parses_start() {
    let cmd = parse_args(&args(&["start", "foo.service"])).unwrap();
    match cmd {
        Command::Start { unit } => assert_eq!(unit, "foo.service"),
        other => panic!("expected Start, got {other:?}"),
    }
}

#[test]
fn parses_stop_restart_reload() {
    assert!(matches!(
        parse_args(&args(&["stop", "a.service"])).unwrap(),
        Command::Stop { unit } if unit == "a.service"
    ));
    assert!(matches!(
        parse_args(&args(&["restart", "a.service"])).unwrap(),
        Command::Restart { unit } if unit == "a.service"
    ));
    assert!(matches!(
        parse_args(&args(&["reload", "a.service"])).unwrap(),
        Command::Reload { unit } if unit == "a.service"
    ));
}

#[test]
fn status_without_unit_is_none() {
    let cmd = parse_args(&args(&["status"])).unwrap();
    match cmd {
        Command::Status { unit } => assert!(unit.is_none()),
        other => panic!("expected Status, got {other:?}"),
    }
}

#[test]
fn status_with_unit() {
    let cmd = parse_args(&args(&["status", "foo.service"])).unwrap();
    match cmd {
        Command::Status { unit } => assert_eq!(unit.as_deref(), Some("foo.service")),
        other => panic!("expected Status, got {other:?}"),
    }
}

#[test]
fn parses_list_units() {
    assert!(matches!(
        parse_args(&args(&["list-units"])).unwrap(),
        Command::ListUnits
    ));
}

#[test]
fn parses_is_active_cat_enable_disable_mask_unmask() {
    assert!(matches!(
        parse_args(&args(&["is-active", "a.service"])).unwrap(),
        Command::IsActive { unit } if unit == "a.service"
    ));
    assert!(matches!(
        parse_args(&args(&["cat", "a.service"])).unwrap(),
        Command::Cat { unit } if unit == "a.service"
    ));
    assert!(matches!(
        parse_args(&args(&["enable", "a.service"])).unwrap(),
        Command::Enable { unit } if unit == "a.service"
    ));
    assert!(matches!(
        parse_args(&args(&["disable", "a.service"])).unwrap(),
        Command::Disable { unit } if unit == "a.service"
    ));
    assert!(matches!(
        parse_args(&args(&["mask", "a.service"])).unwrap(),
        Command::Mask { unit } if unit == "a.service"
    ));
    assert!(matches!(
        parse_args(&args(&["unmask", "a.service"])).unwrap(),
        Command::Unmask { unit } if unit == "a.service"
    ));
}

#[test]
fn parses_daemon_reload() {
    assert!(matches!(
        parse_args(&args(&["daemon-reload"])).unwrap(),
        Command::DaemonReload
    ));
}

#[test]
fn unknown_command_errors() {
    let err = parse_args(&args(&["frobnicate", "x"])).unwrap_err();
    match err {
        ParseError::UnknownCommand(c) => assert_eq!(c, "frobnicate"),
        other => panic!("expected UnknownCommand, got {other:?}"),
    }
}

#[test]
fn start_without_unit_errors() {
    let err = parse_args(&args(&["start"])).unwrap_err();
    match err {
        ParseError::MissingUnit(c) => assert_eq!(c, "start"),
        other => panic!("expected MissingUnit, got {other:?}"),
    }
}

#[test]
fn known_flags_are_tolerated() {
    // 高兼容: 常用旗標不報錯、不影響命令語義
    let cmd = parse_args(&args(&["--user", "--no-pager", "--no-legend", "status", "foo.service"]))
        .unwrap();
    match cmd {
        Command::Status { unit } => assert_eq!(unit.as_deref(), Some("foo.service")),
        other => panic!("expected Status, got {other:?}"),
    }
}

#[test]
fn unknown_flags_are_tolerated_with_warning() {
    // 未知旗標降級為警告,不報錯 (用戶需求: 高兼容、未知不報錯)
    let cmd = parse_args(&args(&["--frobnicate", "list-units"])).unwrap();
    assert!(matches!(cmd, Command::ListUnits));
}

#[test]
fn bare_unit_gets_service_suffix() {
    // systemctl status foo ≡ status foo.service (systemd default suffix)
    let cmd = parse_args(&args(&["status", "foo"])).unwrap();
    match cmd {
        Command::Status { unit } => assert_eq!(unit.as_deref(), Some("foo.service")),
        other => panic!("expected Status, got {other:?}"),
    }
}

#[test]
fn explicit_suffix_is_kept() {
    for name in ["foo.service", "bar.timer", "baz.target", "weird.name.socket"] {
        let cmd = parse_args(&args(&["status", name])).unwrap();
        match cmd {
            Command::Status { unit } => assert_eq!(unit.as_deref(), Some(name)),
            other => panic!("expected Status, got {other:?}"),
        }
    }
}

#[test]
fn dotted_unknown_suffix_still_appends_service() {
    // foo.bar 後綴未知 → foo.bar.service (systemd 行為)
    let cmd = parse_args(&args(&["is-active", "foo.bar"])).unwrap();
    match cmd {
        Command::IsActive { unit } => assert_eq!(unit, "foo.bar.service"),
        other => panic!("expected IsActive, got {other:?}"),
    }
}
