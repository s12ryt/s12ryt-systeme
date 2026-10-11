//! mount / automount / swap 單元配置層測試 — 純邏輯解析與單元名推導。

use systeme_core::mount::{
    automount_target, automount_unit_name, mount_unit_name, swap_unit_name, AutoMountConfig,
    MountConfig, SwapConfig,
};
use systeme_units::parser::parse_unit;

fn mc(text: &str) -> MountConfig {
    MountConfig::from_parsed(&parse_unit(text))
}

#[test]
fn mount_missing_section_defaults() {
    let c = mc("[Unit]\nDescription=x\n");
    assert_eq!(c.what, None);
    assert_eq!(c.where_, None);
    assert_eq!(c.fs_type, None);
    assert!(c.options.is_empty());
    assert!(!c.lazy_unmount);
    assert!(c.warnings.is_empty(), "no warnings: {:?}", c.warnings);
}

#[test]
fn mount_full_parse() {
    let c = mc("[Mount]\nWhat=/dev/sda1\nWhere=/mnt/data\nType=ext4\nOptions=ro,noatime\n");
    assert_eq!(c.what.as_deref(), Some("/dev/sda1"));
    assert_eq!(c.where_.as_deref(), Some("/mnt/data"));
    assert_eq!(c.fs_type.as_deref(), Some("ext4"));
    assert_eq!(c.options, vec!["ro".to_string(), "noatime".to_string()]);
}

#[test]
fn mount_options_multiline_accumulate() {
    let c = mc("[Mount]\nOptions=ro\nOptions=noatime,user\n");
    assert_eq!(c.options, vec!["ro".to_string(), "noatime".to_string(), "user".to_string()]);
}

#[test]
fn mount_lazy_unmount_flag() {
    assert!(mc("[Mount]\nLazyUnmount=yes\n").lazy_unmount);
    assert!(!mc("[Mount]\n").lazy_unmount);
    let bad = mc("[Mount]\nLazyUnmount=maybe\n");
    assert!(!bad.lazy_unmount);
    assert_eq!(bad.warnings.len(), 1);
}

#[test]
fn mount_unit_name_basic() {
    assert_eq!(mount_unit_name("/var/www"), "var-www.mount");
    assert_eq!(mount_unit_name("/usr/local/share"), "usr-local-share.mount");
}

#[test]
fn mount_unit_name_root() {
    assert_eq!(mount_unit_name("/"), "-.mount");
}

#[test]
fn mount_unit_name_trailing_slash_normalized() {
    assert_eq!(mount_unit_name("/var/www/"), "var-www.mount");
}

#[test]
fn mount_unit_name_special_chars_escaped() {
    // 空格 → \x20, 連字符 path_mode → \x2d
    assert_eq!(mount_unit_name("/mnt/a b"), "mnt-a\\x20b.mount");
    assert_eq!(mount_unit_name("/mnt/d-data"), "mnt-d\\x2ddata.mount");
}

#[test]
fn swap_config_parse() {
    let c = SwapConfig::from_parsed(&parse_unit(
        "[Swap]\nWhat=/swapfile\nPriority=100\nOptions=discard\n",
    ));
    assert_eq!(c.what.as_deref(), Some("/swapfile"));
    assert_eq!(c.priority, Some(100));
    assert_eq!(c.options, vec!["discard".to_string()]);
    assert_eq!(swap_unit_name("/swapfile"), "swapfile.swap");
}

#[test]
fn swap_bad_priority_warns() {
    let c = SwapConfig::from_parsed(&parse_unit("[Swap]\nWhat=/swapfile\nPriority=abc\n"));
    assert_eq!(c.priority, None);
    assert_eq!(c.warnings.len(), 1);
}

#[test]
fn automount_config_parse() {
    let c = AutoMountConfig::from_parsed(&parse_unit(
        "[Automount]\nWhere=/mnt\nDirectoryMode=0755\nTimeoutIdleSec=120\n",
    ));
    assert_eq!(c.where_.as_deref(), Some("/mnt"));
    assert_eq!(c.directory_mode.as_deref(), Some("0755"));
    assert_eq!(automount_unit_name("/mnt"), "mnt.automount");
    // automount 觸發同名 .mount
    assert_eq!(automount_target("mnt.automount"), "mnt.mount");
}
