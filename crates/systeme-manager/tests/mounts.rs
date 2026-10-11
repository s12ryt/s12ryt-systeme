//! mount 單元真實掛載測試 — mount_data 跨平台 + cfg(unix) 真掛載。

#[cfg(unix)]
use systeme_manager::mounts::{real_mount, real_umount};
use systeme_manager::mounts::mount_data;
use systeme_units::parser::parse_unit;

#[test]
fn mount_data_empty() {
    assert_eq!(mount_data(&[]), "");
}

#[test]
fn mount_data_single() {
    assert_eq!(mount_data(&["rw".to_string()]), "rw");
}

#[test]
fn mount_data_multiple() {
    assert_eq!(
        mount_data(&["rw".to_string(), "size=10M".to_string(), "mode=700".to_string()]),
        "rw,size=10M,mode=700"
    );
}

#[test]
fn mount_config_options_parse() {
    let unit = parse_unit(
        "[Mount]\nWhat=none\nWhere=/mnt/tmp\nType=tmpfs\nOptions=rw,size=8M\nOptions=nodev,nosuid\n",
    );
    let cfg = systeme_core::mount::MountConfig::from_parsed(&unit);
    assert_eq!(cfg.what.as_deref(), Some("none"));
    assert_eq!(cfg.where_.as_deref(), Some("/mnt/tmp"));
    assert_eq!(cfg.fs_type.as_deref(), Some("tmpfs"));
    assert_eq!(cfg.options, vec!["rw", "size=8M", "nodev", "nosuid"]);
}

#[cfg(unix)]
#[test]
fn real_mount_and_umount_tmpfs() {
    let dir = tempfile::tempdir().unwrap();
    let unit = parse_unit(&format!(
        "[Mount]\nWhat=none\nWhere={}\nType=tmpfs\nOptions=size=1M\n",
        dir.path().display()
    ));
    let cfg = systeme_core::mount::MountConfig::from_parsed(&unit);

    real_mount(&cfg).expect("tmpfs mount should succeed (root)");
    // 掛載點內寫入落於 tmpfs
    std::fs::write(dir.path().join("inside"), "hello").unwrap();
    assert!(real_umount(&cfg).is_ok());
    // umount 後文件不再可見 (tmpfs 內容隨卸載消失)
    assert!(!dir.path().join("inside").exists());
}

#[cfg(unix)]
#[test]
fn real_mount_missing_where_errors() {
    let unit = parse_unit("[Mount]\nWhat=none\nType=tmpfs\n");
    let cfg = systeme_core::mount::MountConfig::from_parsed(&unit);
    assert!(real_mount(&cfg).is_err());
}

#[cfg(unix)]
#[test]
fn real_umount_not_mounted_is_err() {
    let dir = tempfile::tempdir().unwrap();
    let unit = parse_unit(&format!("[Mount]\nWhere={}\n", dir.path().display()));
    let cfg = systeme_core::mount::MountConfig::from_parsed(&unit);
    assert!(real_umount(&cfg).is_err());
}
