//! Service 執行器 (配置提取 + 命令行解析 + 退出後決策) 的 TDD 測試。
//!
//! 覆蓋:
//! - ExecCommand 命令行解析 (systemd quoting/轉義/前綴旗標)
//! - TimeSpan 時長語法解析 (90 / 90s / 100ms / 1min 30s / infinity)
//! - ServiceConfig 從 [Service] 段提取 (含未知值降級警告)
//! - decide_after_exit: RestartPolicy × ExitEvent 重啟決策矩陣與最終狀態

use systeme_core::service::{
    decide_after_exit, parse_time_span, ExecCommand, ExecParseError, ExitEvent, KillMode,
    RestartPolicy, AfterExitAction, ServiceConfig, ServiceFinalState, ServiceType, TimeSpan,
};
use systeme_units::parse_unit;

// ─── ExecCommand 解析 ───────────────────────────────────────────────

#[test]
fn exec_simple_command() {
    let c = ExecCommand::parse("/usr/bin/sleep 10").unwrap();
    assert_eq!(c.argv, vec!["/usr/bin/sleep", "10"]);
    assert!(!c.ignore_failure);
    assert!(!c.full_privileges);
    assert!(!c.no_env_expand);
}

#[test]
fn exec_double_quotes_keep_spaces() {
    let c = ExecCommand::parse(r#"/bin/echo "hello world""#).unwrap();
    assert_eq!(c.argv, vec!["/bin/echo", "hello world"]);
}

#[test]
fn exec_single_quotes_keep_spaces() {
    let c = ExecCommand::parse("/bin/echo 'a b c'").unwrap();
    assert_eq!(c.argv, vec!["/bin/echo", "a b c"]);
}

#[test]
fn exec_double_quote_escapes() {
    // \s -> 空格, \" -> 引號, \\ -> 反斜杠, \t -> tab (雙引號內 systemd 轉義)
    let c = ExecCommand::parse(r#"/bin/prog "a\sb\"c\\d\tf""#).unwrap();
    assert_eq!(c.argv, vec!["/bin/prog", "a b\"c\\d\tf"]);
}

#[test]
fn exec_unquoted_backslash_escapes_next_char() {
    // 引號外反斜杠 + 任意字符 = 該字符字面 (含空格: 不分割 token)
    let c = ExecCommand::parse("/bin/echo a\\ b").unwrap();
    assert_eq!(c.argv, vec!["/bin/echo", "a b"]);
}

#[test]
fn exec_single_quotes_no_escapes() {
    // 單引號內無任何轉義與展開
    let c = ExecCommand::parse("/bin/echo 'a\\nb\\tc'").unwrap();
    assert_eq!(c.argv, vec!["/bin/echo", "a\\nb\\tc"]);
}

#[test]
fn exec_dash_prefix_ignores_failure() {
    let c = ExecCommand::parse("-/bin/false arg").unwrap();
    assert!(c.ignore_failure);
    assert_eq!(c.argv, vec!["/bin/false", "arg"]);
}

#[test]
fn exec_plus_prefix_full_privileges() {
    let c = ExecCommand::parse("+/bin/task").unwrap();
    assert!(c.full_privileges);
    assert_eq!(c.argv[0], "/bin/task");
}

#[test]
fn exec_combined_prefixes() {
    let c = ExecCommand::parse(":-+/bin/x y").unwrap();
    assert!(c.ignore_failure);
    assert!(c.full_privileges);
    assert!(c.no_env_expand);
    assert_eq!(c.argv, vec!["/bin/x", "y"]);
}

#[test]
fn exec_empty_quotes_make_empty_argument() {
    let c = ExecCommand::parse(r#"/bin/echo """#).unwrap();
    assert_eq!(c.argv, vec!["/bin/echo", ""]);
}

#[test]
fn exec_relative_path_rejected() {
    let err = ExecCommand::parse("echo hi").unwrap_err();
    assert!(matches!(err, ExecParseError::NonAbsolutePath(ref p) if p == "echo"));
}

#[test]
fn exec_env_vars_preserved_unexpanded() {
    // 變量展開發生在執行時;解析層保留字面
    let c = ExecCommand::parse(r#"/bin/echo $FOO "${BAR}""#).unwrap();
    assert_eq!(c.argv, vec!["/bin/echo", "$FOO", "${BAR}"]);
}

#[test]
fn exec_plus_prefix_allows_relative_filename() {
    // systemd: '+' 前綴時可執行文件允許簡單文件名 (執行層走 PATH 搜索)
    let c = ExecCommand::parse("+systemd-socket-proxyd 123").unwrap();
    assert!(c.full_privileges);
    assert_eq!(c.argv[0], "systemd-socket-proxyd");
}

#[test]
fn exec_empty_line_rejected() {
    assert!(matches!(
        ExecCommand::parse("   "),
        Err(ExecParseError::EmptyCommand)
    ));
}

#[test]
fn exec_unterminated_quote_rejected() {
    let err = ExecCommand::parse("/bin/echo \"abc").unwrap_err();
    assert!(matches!(err, ExecParseError::UnterminatedQuote('"')));
    let err2 = ExecCommand::parse("/bin/echo 'x").unwrap_err();
    assert!(matches!(err2, ExecParseError::UnterminatedQuote('\'')));
}

#[test]
fn exec_adjacent_quotes_concatenate() {
    // "ab"'cd' 相鄰引號段拼接為同一 token
    let c = ExecCommand::parse("/bin/echo \"ab\"'cd'ef").unwrap();
    assert_eq!(c.argv, vec!["/bin/echo", "abcdef"]);
}

// ─── TimeSpan 解析 ──────────────────────────────────────────────────

#[test]
fn span_bare_number_is_seconds() {
    assert_eq!(parse_time_span("90").unwrap(), TimeSpan::Finite(90.0));
}

#[test]
fn span_suffixes() {
    assert_eq!(parse_time_span("90s").unwrap(), TimeSpan::Finite(90.0));
    assert_eq!(parse_time_span("100ms").unwrap(), TimeSpan::Finite(0.1));
    assert_eq!(parse_time_span("5min").unwrap(), TimeSpan::Finite(300.0));
    assert_eq!(parse_time_span("2h").unwrap(), TimeSpan::Finite(7200.0));
    assert_eq!(parse_time_span("1d").unwrap(), TimeSpan::Finite(86400.0));
    assert_eq!(parse_time_span("1w").unwrap(), TimeSpan::Finite(604800.0));
    // us 量級存在浮點乘法誤差, 用近似比較
    let us = parse_time_span("10us").unwrap().as_secs().unwrap();
    assert!((us - 1e-5).abs() < 1e-12, "got {us}");
}

#[test]
fn span_combined_parts_add_up() {
    assert_eq!(parse_time_span("1min 30s").unwrap(), TimeSpan::Finite(90.0));
    assert_eq!(parse_time_span("1h 30min").unwrap(), TimeSpan::Finite(5400.0));
}

#[test]
fn span_infinity() {
    assert_eq!(parse_time_span("infinity").unwrap(), TimeSpan::Infinity);
}

#[test]
fn span_garbage_rejected() {
    assert!(parse_time_span("abc").is_err());
    assert!(parse_time_span("").is_err());
    assert!(parse_time_span("5x").is_err());
}

// ─── ServiceConfig 提取 ─────────────────────────────────────────────

#[test]
fn config_defaults() {
    let pu = parse_unit("[Service]\nExecStart=/bin/true\n");
    let cfg = ServiceConfig::from_parsed(&pu);
    assert!(matches!(cfg.service_type, ServiceType::Simple));
    assert!(matches!(cfg.restart, RestartPolicy::No));
    assert_eq!(cfg.restart_sec, TimeSpan::Finite(0.1)); // systemd 默認 100ms
    assert_eq!(cfg.timeout_start_sec, TimeSpan::Finite(90.0));
    assert_eq!(cfg.timeout_stop_sec, TimeSpan::Finite(90.0));
    assert!(matches!(cfg.kill_mode, KillMode::ControlGroup));
    assert!(!cfg.remain_after_exit);
    assert!(cfg.warnings.is_empty());
}

#[test]
fn config_type_and_remain_after_exit() {
    let pu = parse_unit("[Service]\nType=oneshot\nRemainAfterExit=yes\nExecStart=/bin/true\n");
    let cfg = ServiceConfig::from_parsed(&pu);
    assert!(matches!(cfg.service_type, ServiceType::Oneshot));
    assert!(cfg.remain_after_exit);
}

#[test]
fn config_unknown_type_degrades_with_warning() {
    let pu = parse_unit("[Service]\nType=weird\nExecStart=/bin/true\n");
    let cfg = ServiceConfig::from_parsed(&pu);
    assert!(matches!(cfg.service_type, ServiceType::Simple));
    assert!(cfg.warnings.iter().any(|w| w.contains("Type")));
}

#[test]
fn config_unknown_restart_degrades_with_warning() {
    let pu = parse_unit("[Service]\nExecStart=/bin/true\nRestart=weird\n");
    let cfg = ServiceConfig::from_parsed(&pu);
    assert!(matches!(cfg.restart, RestartPolicy::No));
    assert!(cfg.warnings.iter().any(|w| w.contains("Restart")));
}

#[test]
fn config_all_service_types_parse() {
    for (t, expect) in [
        ("simple", ServiceType::Simple),
        ("forking", ServiceType::Forking),
        ("exec", ServiceType::Exec),
        ("notify", ServiceType::Notify),
        ("oneshot", ServiceType::Oneshot),
        ("dbus", ServiceType::DBus),
    ] {
        let pu = parse_unit(&format!("[Service]\nType={t}\nExecStart=/bin/true\n"));
        let cfg = ServiceConfig::from_parsed(&pu);
        assert_eq!(cfg.service_type, expect, "Type={t}");
    }
}

#[test]
fn config_exec_lines_in_order_with_prefixes() {
    let pu = parse_unit(
        "[Service]\n\
         ExecStartPre=/bin/pre1\n\
         ExecStartPre=-/bin/pre2\n\
         ExecStart=/bin/main arg\n\
         ExecStop=-/bin/cleanup\n\
         ExecStartPost=/bin/post\n",
    );
    let cfg = ServiceConfig::from_parsed(&pu);
    assert_eq!(cfg.exec_start_pre.len(), 2);
    assert_eq!(cfg.exec_start_pre[0].argv[0], "/bin/pre1");
    assert!(cfg.exec_start_pre[1].ignore_failure);
    assert_eq!(cfg.exec_start.len(), 1);
    assert_eq!(cfg.exec_start[0].argv, vec!["/bin/main", "arg"]);
    assert!(cfg.exec_stop[0].ignore_failure);
    assert_eq!(cfg.exec_start_post.len(), 1);
}

#[test]
fn config_bad_exec_line_becomes_warning_and_skipped() {
    let pu = parse_unit("[Service]\nExecStart=relative-cmd\nExecStartPost=/bin/ok\n");
    let cfg = ServiceConfig::from_parsed(&pu);
    assert!(cfg.exec_start.is_empty());
    assert!(cfg.warnings.iter().any(|w| w.contains("ExecStart")));
    assert_eq!(cfg.exec_start_post.len(), 1);
}

#[test]
fn config_environment_and_files() {
    let pu = parse_unit(
        "[Service]\n\
         Environment=LANG=C\n\
         Environment=\"A=1\" \"B=with space\"\n\
         EnvironmentFile=/etc/default/foo\n\
         EnvironmentFile=-/etc/optional/bar\n",
    );
    let cfg = ServiceConfig::from_parsed(&pu);
    assert!(cfg.environment.contains(&"LANG=C".to_string()));
    assert!(cfg.environment.contains(&"A=1".to_string()));
    assert!(cfg.environment.contains(&"B=with space".to_string()));
    assert_eq!(cfg.environment.len(), 3);
    assert_eq!(cfg.environment_files.len(), 2);
    assert_eq!(cfg.environment_files[1], "-/etc/optional/bar"); // '-' 前綴保留原樣 (執行層語義)
}

#[test]
fn config_user_group_working_directory() {
    let pu = parse_unit(
        "[Service]\n\
         User=nobody\n\
         Group=nogroup\n\
         WorkingDirectory=-/opt/app\n",
    );
    let cfg = ServiceConfig::from_parsed(&pu);
    assert_eq!(cfg.user.as_deref(), Some("nobody"));
    assert_eq!(cfg.group.as_deref(), Some("nogroup"));
    let wd = cfg.working_directory.expect("wd");
    assert_eq!(wd.path, "/opt/app");
    assert!(wd.ignore_missing);
}

#[test]
fn config_killmode() {
    let pu = parse_unit("[Service]\nKillMode=mixed\nExecStart=/bin/true\n");
    let cfg = ServiceConfig::from_parsed(&pu);
    assert!(matches!(cfg.kill_mode, KillMode::Mixed));

    let pu2 = parse_unit("[Service]\nKillMode=control-group\nExecStart=/bin/true\n");
    let cfg2 = ServiceConfig::from_parsed(&pu2);
    assert!(matches!(cfg2.kill_mode, KillMode::ControlGroup));

    let pu3 = parse_unit("[Service]\nKillMode=bogus\nExecStart=/bin/true\n");
    let cfg3 = ServiceConfig::from_parsed(&pu3);
    assert!(matches!(cfg3.kill_mode, KillMode::ControlGroup));
    assert!(cfg3.warnings.iter().any(|w| w.contains("KillMode")));
}

#[test]
fn config_timeouts() {
    let pu = parse_unit(
        "[Service]\n\
         TimeoutStartSec=infinity\n\
         TimeoutStopSec=30\n\
         RestartSec=5s\n",
    );
    let cfg = ServiceConfig::from_parsed(&pu);
    assert_eq!(cfg.timeout_start_sec, TimeSpan::Infinity);
    assert_eq!(cfg.timeout_stop_sec, TimeSpan::Finite(30.0));
    assert_eq!(cfg.restart_sec, TimeSpan::Finite(5.0));
}

#[test]
fn config_missing_service_section_gives_defaults() {
    let pu = parse_unit("[Unit]\nDescription=x\n");
    let cfg = ServiceConfig::from_parsed(&pu);
    assert!(cfg.exec_start.is_empty());
    assert!(matches!(cfg.service_type, ServiceType::Simple));
}

// ─── 退出後決策: 重啟矩陣與最終狀態 ─────────────────────────────────

fn cfg_with(restart: RestartPolicy, remain: bool) -> ServiceConfig {
    let pu = parse_unit(&format!(
        "[Service]\nExecStart=/bin/true\nRestart={}\nRemainAfterExit={}\n",
        match restart {
            RestartPolicy::No => "no",
            RestartPolicy::OnSuccess => "on-success",
            RestartPolicy::OnFailure => "on-failure",
            RestartPolicy::OnAbnormal => "on-abnormal",
            RestartPolicy::OnAbort => "on-abort",
            RestartPolicy::OnWatchdog => "on-watchdog",
            RestartPolicy::Always => "always",
        },
        if remain { "yes" } else { "no" },
    ));
    ServiceConfig::from_parsed(&pu)
}

/// systemd.service(5) RestartPolicy × 事件 矩陣
#[test]
fn restart_decision_matrix() {
    let events = [
        ExitEvent::CleanExit,
        ExitEvent::UncleanExit,
        ExitEvent::Killed,
        ExitEvent::StartTimeout,
        ExitEvent::Watchdog,
    ];
    //        policy              [clean, unclean, killed, timeout, watchdog]
    let table: Vec<(RestartPolicy, [bool; 5])> = vec![
        (RestartPolicy::No, [false, false, false, false, false]),
        (RestartPolicy::OnSuccess, [true, false, false, false, false]),
        (RestartPolicy::OnFailure, [false, true, true, true, true]),
        (RestartPolicy::OnAbnormal, [false, false, true, true, true]),
        (RestartPolicy::OnAbort, [false, false, true, false, true]),
        (RestartPolicy::OnWatchdog, [false, false, false, false, true]),
        (RestartPolicy::Always, [true, true, true, true, true]),
    ];
    for (policy, expected) in &table {
        for (i, event) in events.iter().enumerate() {
            let cfg = cfg_with(*policy, false);
            let action = decide_after_exit(&cfg, *event);
            let actually_restarts = matches!(action, AfterExitAction::Restart { .. });
            assert_eq!(
                actually_restarts,
                expected[i],
                "policy={policy:?} event={event:?}"
            );
        }
    }
}

#[test]
fn restart_uses_configured_delay() {
    let pu = parse_unit("[Service]\nExecStart=/bin/true\nRestart=always\nRestartSec=5s\n");
    let cfg = ServiceConfig::from_parsed(&pu);
    let action = decide_after_exit(&cfg, ExitEvent::CleanExit);
    match action {
        AfterExitAction::Restart { after } => assert_eq!(after, TimeSpan::Finite(5.0)),
        other => panic!("expected restart, got {other:?}"),
    }
}

#[test]
fn final_states_without_restart() {
    // 乾淨退出 + remain_after_exit -> Active
    let cfg = cfg_with(RestartPolicy::No, true);
    assert_eq!(
        decide_after_exit(&cfg, ExitEvent::CleanExit),
        AfterExitAction::Final(ServiceFinalState::Active)
    );
    // 乾淨退出 + 非 remain -> Dead (simple 型乾淨退出是 inactive/dead 而非 failed)
    let cfg2 = cfg_with(RestartPolicy::No, false);
    assert_eq!(
        decide_after_exit(&cfg2, ExitEvent::CleanExit),
        AfterExitAction::Final(ServiceFinalState::Dead)
    );
    // 不潔退出 -> Failed
    assert_eq!(
        decide_after_exit(&cfg2, ExitEvent::UncleanExit),
        AfterExitAction::Final(ServiceFinalState::Failed)
    );
    // 被信號殺 -> Failed
    assert_eq!(
        decide_after_exit(&cfg2, ExitEvent::Killed),
        AfterExitAction::Final(ServiceFinalState::Failed)
    );
}

#[test]
fn remain_after_exit_only_promotes_clean_exit() {
    // remain_after_exit 不掩蓋失敗: 不潔退出仍 Failed
    let cfg = cfg_with(RestartPolicy::No, true);
    assert_eq!(
        decide_after_exit(&cfg, ExitEvent::UncleanExit),
        AfterExitAction::Final(ServiceFinalState::Failed)
    );
}
