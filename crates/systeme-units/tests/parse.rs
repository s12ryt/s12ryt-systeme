//! 單元文件解析器整合測試 (TDD)
//! 覆蓋 systemd conf-parser 的關鍵行為 + 高兼容要求 (未知指令僅警告)

use systeme_units::parse_unit;

#[test]
fn basic_sections_and_keys() {
    let src = "[Unit]\nDescription=Hello World\n\n[Service]\nExecStart=/bin/true\n";
    let u = parse_unit(src);
    assert_eq!(
        u.section("Unit").unwrap().get("Description"),
        Some("Hello World")
    );
    assert_eq!(u.section("Service").unwrap().get("ExecStart"), Some("/bin/true"));
    assert!(u.warnings.is_empty(), "no warnings expected: {:?}", u.warnings);
}

#[test]
fn repeated_sections_merge() {
    let src = "[Service]\nExecStart=/bin/a\n[Install]\nWantedBy=multi-user.target\n[Service]\nExecReload=/bin/b\n";
    let u = parse_unit(src);
    let s = u.section("Service").unwrap();
    assert_eq!(s.get_all("ExecStart"), vec!["/bin/a"]);
    assert_eq!(s.get("ExecReload"), Some("/bin/b"));
    // 同名段合併為單一段
    assert_eq!(u.sections.iter().filter(|s| s.name == "Service").count(), 1);
}

#[test]
fn flag_without_equals_is_true() {
    let src = "[Service]\nRemainAfterExit\n";
    let u = parse_unit(src);
    let s = u.section("Service").unwrap();
    assert!(s.get_flag("RemainAfterExit"), "bare key must act as true");
}

#[test]
fn explicit_empty_value_is_false() {
    let src = "[Service]\nPrivateTmp=\n";
    let u = parse_unit(src);
    assert!(!u.section("Service").unwrap().get_flag("PrivateTmp"));
}

#[test]
fn boolean_vocabulary() {
    let cases: &[(&str, Option<bool>)] = &[
        ("yes", Some(true)), ("YES", Some(true)), ("1", Some(true)), ("true", Some(true)), ("On", Some(true)),
        ("no", Some(false)), ("0", Some(false)), ("false", Some(false)), ("OFF", Some(false)),
        ("maybe", None), ("", None),
    ];
    for (raw, expect) in cases {
        assert_eq!(systeme_units::parse_bool(raw), *expect, "parse_bool({raw:?})");
    }
}

#[test]
fn comments_hash_and_semicolon() {
    let src = "# full line\n; also comment\n[Unit]\nDescription=x\n";
    let u = parse_unit(src);
    assert_eq!(u.section("Unit").unwrap().get("Description"), Some("x"));
    assert!(u.warnings.is_empty());
}

#[test]
fn inline_hash_is_part_of_value() {
    // systemd 行內 # 不是註釋 —— 高兼容關鍵點
    let src = "[Unit]\nDescription=value # not a comment\n";
    let u = parse_unit(src);
    assert_eq!(
        u.section("Unit").unwrap().get("Description"),
        Some("value # not a comment")
    );
}

#[test]
fn continuation_lines() {
    let src = "[Unit]\nDescription=foo \\\n bar\n";
    let u = parse_unit(src);
    assert_eq!(u.section("Unit").unwrap().get("Description"), Some("foo bar"));
}

#[test]
fn continuation_preserves_inner_spacing() {
    let src = "[Service]\nExecStart=/bin/echo a\\\nb\n";
    let u = parse_unit(src);
    assert_eq!(
        u.section("Service").unwrap().get("ExecStart"),
        Some("/bin/echo ab")
    );
}

#[test]
fn escape_sequences() {
    let src = "[Unit]\nDescription=hello\\sworld\\ttab\\nnewline\n";
    let u = parse_unit(src);
    assert_eq!(
        u.section("Unit").unwrap().get("Description"),
        Some("hello world\ttab\nnewline")
    );
}

#[test]
fn escape_backslash_quote_and_hex() {
    let src = "[Unit]\nDescription=a\\\\b\\\"c\\x41B\n";
    let u = parse_unit(src);
    assert_eq!(
        u.section("Unit").unwrap().get("Description"),
        Some("a\\b\"cAB")
    );
}

#[test]
fn escape_unknown_backslash_kept_literal() {
    // 未知轉義 (如 \\q) 保留原樣 —— systemd cunescape 對未知序列保留反斜杠
    let src = "[Unit]\nDescription=keep\\qme\n";
    let u = parse_unit(src);
    assert_eq!(u.section("Unit").unwrap().get("Description"), Some("keep\\qme"));
}

#[test]
fn whitespace_trimmed_around_key_and_value() {
    let src = "[Unit]\n  Description  =  padded value  \n";
    let u = parse_unit(src);
    assert_eq!(u.section("Unit").unwrap().get("Description"), Some("padded value"));
}

#[test]
fn list_semantics_reset_on_empty() {
    let src = "[Service]\nEnvironment=A=1\nEnvironment=B=2\nEnvironment=\nEnvironment=C=3\n";
    let u = parse_unit(src);
    let s = u.section("Service").unwrap();
    assert_eq!(s.get_all("Environment"), vec!["A=1", "B=2", "", "C=3"]);
    assert_eq!(s.get_list("Environment"), vec!["C=3"]);
}

#[test]
fn list_semantics_without_reset() {
    let src = "[Service]\nExecStartPre=/bin/a\nExecStartPre=/bin/b\n";
    let u = parse_unit(src);
    assert_eq!(
        u.section("Service").unwrap().get_list("ExecStartPre"),
        vec!["/bin/a", "/bin/b"]
    );
}

#[test]
fn garbage_before_any_section_warns_not_panics() {
    let src = "stray line without section\n[Unit]\nDescription=ok\n";
    let u = parse_unit(src);
    assert_eq!(u.section("Unit").unwrap().get("Description"), Some("ok"));
    assert!(!u.warnings.is_empty(), "must warn about stray line");
}

#[test]
fn garbage_line_inside_section_warns() {
    let src = "[Unit]\nnot a valid directive\nDescription=ok\n";
    let u = parse_unit(src);
    assert_eq!(u.section("Unit").unwrap().get("Description"), Some("ok"));
    assert!(!u.warnings.is_empty());
}

#[test]
fn empty_key_warns() {
    let src = "[Unit]\n=novalue\n";
    let u = parse_unit(src);
    assert!(!u.warnings.is_empty());
}

#[test]
fn malformed_section_header_warns() {
    let src = "[Unit]\nDescription=x\n[Bad Section]\nFoo=1\n";
    let u = parse_unit(src);
    assert!(!u.warnings.is_empty(), "section with space must warn");
    // 但內容仍被收集 (高兼容)
    assert_eq!(u.section("Bad Section").unwrap().get("Foo"), Some("1"));
}

#[test]
fn crlf_line_endings_tolerated() {
    let src = "[Unit]\r\nDescription=crlf ok\r\n";
    let u = parse_unit(src);
    assert_eq!(u.section("Unit").unwrap().get("Description"), Some("crlf ok"));
}

#[test]
fn utf8_bom_skipped() {
    let src = "\u{feff}[Unit]\nDescription=bom\n";
    let u = parse_unit(src);
    assert_eq!(u.section("Unit").unwrap().get("Description"), Some("bom"));
}

#[test]
fn utf8_values() {
    let src = "[Unit]\nDescription=服務描述 中文 OK\n";
    let u = parse_unit(src);
    assert_eq!(
        u.section("Unit").unwrap().get("Description"),
        Some("服務描述 中文 OK")
    );
}

#[test]
fn real_world_nginx_style_unit() {
    // 模擬真實發行版單元的高級特性混合
    let src = r#"
[Unit]
Description=A high performance web server and a reverse proxy server
Documentation=man:nginx(8)
After=network-online.target
Wants=network-online.target
Conflicts=shutdown.target

[Service]
Type=forking
PIDFile=/run/nginx.pid
ExecStartPre=/usr/sbin/nginx -t -q -g daemon on; master_process on;
ExecStart=/usr/sbin/nginx -g daemon on; master_process on;
ExecReload=/usr/sbin/nginx -g daemon off; master_process on; -s reload
ExecStop=-/sbin/start-stop-daemon --quiet --stop --retry QUIT/5
TimeoutStopSec=5
KillMode=mixed
Restart=on-failure

[Install]
WantedBy=multi-user.target
"#;
    let u = parse_unit(src);
    let unit = u.section("Unit").unwrap();
    assert!(unit.get("Description").unwrap().starts_with("A high performance"));
    assert_eq!(unit.get_list("After"), vec!["network-online.target"]);
    assert_eq!(unit.get_list("Wants"), vec!["network-online.target"]);
    assert_eq!(unit.get_list("Conflicts"), vec!["shutdown.target"]);
    let svc = u.section("Service").unwrap();
    assert_eq!(svc.get("Type"), Some("forking"));
    // `ExecStop=-/sbin/...` 前綴 `-` 表示允許失敗, 解析層保留原樣
    assert!(svc.get("ExecStop").unwrap().starts_with("-/sbin/"));
    assert_eq!(svc.get("KillMode"), Some("mixed"));
    let inst = u.section("Install").unwrap();
    assert_eq!(inst.get_list("WantedBy"), vec!["multi-user.target"]);
    assert!(u.warnings.is_empty(), "real-world unit must parse clean: {:?}", u.warnings);
}
