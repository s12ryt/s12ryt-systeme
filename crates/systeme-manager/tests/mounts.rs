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

/// 環境能力探測: 無 CAP_SYS_ADMIN (如 CI 容器) 時 mount(2) EPERM → 測試跳過。
#[cfg(unix)]
fn mount_permitted() -> bool {
    let probe = tempfile::tempdir().unwrap();
    let dir = probe.path().join("probe");
    std::fs::create_dir_all(&dir).unwrap();
    let what_c = std::ffi::CString::new("tmpfs").unwrap();
    let mnt_c = std::ffi::CString::new(dir.as_os_str().as_encoded_bytes()).unwrap();
    let type_c = std::ffi::CString::new("tmpfs").unwrap();
    let rc = unsafe { libc::mount(what_c.as_ptr(), mnt_c.as_ptr(), type_c.as_ptr(), 0, std::ptr::null()) };
    if rc == 0 {
        unsafe { libc::umount(mnt_c.as_ptr()) };
        true
    } else {
        eprintln!(
            "skip: mount(2) not permitted ({}) — CI container lacks CAP_SYS_ADMIN",
            std::io::Error::last_os_error()
        );
        false
    }
}

#[cfg(unix)]
#[test]
fn real_mount_and_umount_tmpfs() {
    if !mount_permitted() {
        return;
    }
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
