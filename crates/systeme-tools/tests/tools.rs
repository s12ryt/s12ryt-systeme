//! systemd-escape / systemd-analyze CLI 子集測試 (systeme-tools crate)。

use systeme_tools::analyze;
use systeme_tools::escape;

// ---------- systemd-escape ----------

#[test]
fn escape_path() {
    assert_eq!(escape::run(&["/srv/web".into()], &[]), "srv-web");
}

#[test]
fn escape_path_flag_is_default() {
    // 無參數原樣轉義 (systemd-escape 將參數當模板名處理)
    assert_eq!(escape::run(&["foo bar".into()], &[]), "foo\\x20bar");
}

#[test]
fn escape_with_suffix() {
    assert_eq!(
        escape::run(&["/srv/web".into()], &["--suffix=service".into()]),
        "srv-web.service"
    );
}

#[test]
fn escape_template_instance() {
    assert_eq!(
        escape::run(&["/dev/sda1".into()], &["--template=fsck@.service".into()]),
        "fsck@dev-sda1.service"
    );
}

#[test]
fn escape_instance_flag() {
    // --instance: 參數視為已轉義實例名, 取實例部分
    assert_eq!(
        escape::run(&["dev-sda1".into()], &["--instance".into(), "--unescape".into()]),
        "/dev/sda1"
    );
}

#[test]
fn unescape() {
    assert_eq!(escape::run(&["srv-web".into()], &["--unescape".into()]), "/srv/web");
}

#[test]
fn unescape_mount_suffix() {
    // systemd-escape --unescape --suffix=mount 補回後綴語義: 剝後綴再反轉義
    assert_eq!(
        escape::run(&["srv-web.mount".into()], &["--unescape".into(), "--suffix=mount".into()]),
        "/srv/web"
    );
}

#[test]
fn escape_dashes_and_dots() {
    // 路徑模式: '-' → \x2d, '.' 保留
    assert_eq!(escape::run(&["/a-b.c".into()], &[]), "a\\x2db.c");
}

#[test]
fn escape_empty_args_prints_nothing() {
    assert_eq!(escape::run(&[], &[]), "");
}

// ---------- systemd-analyze ----------

#[test]
fn analyze_critical_chain_empty() {
    let out = analyze::run(&["critical-chain".into()]);
    assert!(out.contains("The time when unit became active"),
        "header expected: {out}");
}

#[test]
fn analyze_blame_with_samples() {
    let out = analyze::blame(&[
        ("a.service".to_string(), 1.5),
        ("b.service".to_string(), 3.2),
    ]);
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines.len(), 2);
    assert!(lines[0].starts_with("3.2s"), "desc order: {out}"); // 大的在前
    assert!(lines[0].contains("b.service"));
    assert!(lines[1].contains("a.service"));
}

#[test]
fn analyze_blame_formats() {
    let out = analyze::blame(&[("x.service".to_string(), 0.05)]);
    assert!(out.starts_with("50ms"), "ms format: {out}");
    let out2 = analyze::blame(&[("y.service".to_string(), 61.0)]);
    assert!(out2.starts_with("1min 1s"), "min format: {out2}");
}

#[test]
fn analyze_time_basic() {
    // 無啟動數據時輸出提示而非 panic
    let out = analyze::run(&["time".into()]);
    assert!(!out.is_empty());
}

#[test]
fn analyze_dot_rejects_unknown() {
    let out = analyze::run(&["frobnicate".into()]);
    assert!(out.contains("Unknown command") || out.contains("unknown"), "{out}");
}
