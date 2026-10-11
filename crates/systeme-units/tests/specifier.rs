//! 說明符 (specifier) 與變量展開測試

use std::collections::HashMap;

use systeme_units::specifier::*;

fn ctx() -> SpecContext {
    SpecContext {
        runtime_dir: "/run".into(),
        state_dir: "/var/lib".into(),
        cache_dir: "/var/cache".into(),
        log_dir: "/var/log".into(),
        home: "/root".into(),
        user: "root".into(),
        uid: 0,
        group: "root".into(),
        gid: 0,
        machine_id: "deadbeefcafe".into(),
        boot_id: "aabb-ccdd".into(),
        hostname: "container1".into(),
        kernel_release: "6.8.0-40".into(),
    }
}

fn expand(input: &str, unit: &str) -> String {
    expand_specifiers(input, unit, &ctx()).text
}

#[test]
fn literal_percent() {
    assert_eq!(expand("100%%", "a.service"), "100%");
}

#[test]
fn full_unit_name() {
    assert_eq!(expand("%n", "nginx.service"), "nginx.service");
}

#[test]
fn template_prefix_instance() {
    assert_eq!(expand("%p", "foo@bar.service"), "foo");
    assert_eq!(expand("%i", "foo@bar.service"), "bar");
    assert_eq!(expand("%I", "foo@bar.service"), "bar");
    assert_eq!(expand("%n", "foo@bar.service"), "foo@bar.service");
    // 非模板單元: %i 為空
    assert_eq!(expand("[%i]", "nginx.service"), "[]");
}

#[test]
fn escaped_instance_unescape() {
    // 裸 '-' 在路徑語義下還原為 '/' (systemd %I 行為)
    assert_eq!(expand("%I", "fsck@dev-sda1.service"), "dev/sda1");
    // \x2d 明確轉義的字面 '-' 被保留
    let e = expand_specifiers("%i|%I", "foo@dev\\x2dsda1.service", &ctx());
    assert_eq!(e.text, "dev\\x2dsda1|dev-sda1");
}

#[test]
fn mount_final_component() {
    // dev-sda1.mount: %P = "dev/sda1", %J = "sda1", %j = "sda1"
    let e = expand_specifiers("%P|%J|%j", "dev-sda1.mount", &ctx());
    assert_eq!(e.text, "dev/sda1|sda1|sda1");
}

#[test]
fn instance_file_prefix_f() {
    assert_eq!(expand("%f", "fsck@dev-sda1.service"), "/dev/sda1");
    assert_eq!(expand("[%f]", "nginx.service"), "[]");
}

#[test]
fn context_dirs_and_ids() {
    assert_eq!(expand("%t/x.sock", "a.service"), "/run/x.sock");
    assert_eq!(expand("%S/%n", "a.service"), "/var/lib/a.service");
    assert_eq!(expand("%C", "a.service"), "/var/cache");
    assert_eq!(expand("%L", "a.service"), "/var/log");
    assert_eq!(expand("%h", "a.service"), "/root");
    assert_eq!(expand("%u-%U", "a.service"), "root-0");
    assert_eq!(expand("%g-%G", "a.service"), "root-0");
    assert_eq!(expand("%m", "a.service"), "deadbeefcafe");
    assert_eq!(expand("%b", "a.service"), "aabb-ccdd");
    assert_eq!(expand("%H", "a.service"), "container1");
    assert_eq!(expand("%v", "a.service"), "6.8.0-40");
}

#[test]
fn unknown_specifier_kept_with_warning() {
    let e = expand_specifiers("keep %z ok", "a.service", &ctx());
    assert_eq!(e.text, "keep %z ok");
    assert_eq!(e.warnings, vec!["unknown specifier %z"]);
}

#[test]
fn trailing_percent_kept() {
    let e = expand_specifiers("100%", "a.service", &ctx());
    assert_eq!(e.text, "100%");
    assert!(e.warnings.is_empty());
}

#[test]
fn split_instance_variants() {
    assert_eq!(split_instance("nginx.service"), ("nginx".into(), None));
    assert_eq!(split_instance("foo@bar.service"), ("foo".into(), Some("bar".into())));
    assert_eq!(split_instance("a@b@c.service"), ("a".into(), Some("b@c".into())));
}

#[test]
fn escape_roundtrip() {
    assert_eq!(escape_unit_str("dev/sda1", true), "dev-sda1");
    assert_eq!(unescape_unit_str("dev-sda1"), "dev-sda1");
    assert_eq!(escape_unit_str("foo bar", false), "foo\\x20bar");
    assert_eq!(unescape_unit_str("foo\\x20bar"), "foo bar");
    // path 模式 '-' 轉義避免歧義
    assert_eq!(escape_unit_str("foo-bar", true), "foo\\x2dbar");
    assert_eq!(escape_unit_str("foo-bar", false), "foo-bar");
    assert_eq!(unescape_unit_str("foo\\x2dbar"), "foo-bar");
}

#[test]
fn env_var_expansion() {
    let mut env = HashMap::new();
    env.insert("FOO".to_string(), "val".to_string());
    env.insert("EMPTY".to_string(), String::new());
    let lk = |k: &str| env.get(k).cloned();
    assert_eq!(expand_env_vars("$FOO x", lk), "val x");
    assert_eq!(expand_env_vars("${FOO}x", lk), "valx");
    assert_eq!(expand_env_vars("$$FOO", lk), "$FOO");
    // 未定義變量展開為空
    assert_eq!(expand_env_vars("[${NOPE}]", lk), "[]");
    assert_eq!(expand_env_vars("[$NOPE]", lk), "[]");
    // 空變量名/畸形保留
    assert_eq!(expand_env_vars("${}", lk), "${}");
    assert_eq!(expand_env_vars("${unclosed", lk), "${unclosed");
    // $ 後非變量字符保留
    assert_eq!(expand_env_vars("$ $1 $,", lk), "$ $1 $,");
    assert_eq!(expand_env_vars("$EMPTY.", lk), ".");
}
