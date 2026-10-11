//! mount 單元真實掛載 — cfg(unix) mount(2)/umount(2), mount_data 跨平台。

use systeme_core::mount::MountConfig;

/// 掛載 data 參數: 選項以逗號連接 (空列表 → 空串)。
pub fn mount_data(options: &[String]) -> String {
    options.join(",")
}

/// 真實掛載: What → Where, Type, Options。Where 目錄自動創建。
#[cfg(unix)]
pub fn real_mount(cfg: &MountConfig) -> Result<(), String> {
    let what = cfg.what.as_deref().ok_or("Mount unit has no What=")?;
    let where_ = cfg.where_.as_deref().ok_or("Mount unit has no Where=")?;
    std::fs::create_dir_all(where_)
        .map_err(|e| format!("create mountpoint {where_}: {e}"))?;
    let fs_type: Option<&str> = cfg.fs_type.as_deref();
    let data = mount_data(&cfg.options);
    let rc = unsafe {
        libc::mount(
            what.as_ptr().cast(),
            where_.as_ptr().cast(),
            fs_type
                .map(|s| s.as_ptr())
                .unwrap_or(std::ptr::null())
                .cast(),
            0,
            if data.is_empty() {
                std::ptr::null()
            } else {
                data.as_ptr().cast()
            },
        )
    };
    if rc == 0 {
        Ok(())
    } else {
        Err(format!(
            "mount {what} on {where_}: {}",
            std::io::Error::last_os_error()
        ))
    }
}

/// 真實卸載; LazyUnmount → MNT_DETACH。
#[cfg(unix)]
pub fn real_umount(cfg: &MountConfig) -> Result<(), String> {
    let where_ = cfg.where_.as_deref().ok_or("Mount unit has no Where=")?;
    let flags = if cfg.lazy_unmount { libc::MNT_DETACH } else { 0 };
    let rc = unsafe { libc::umount2(where_.as_ptr().cast(), flags) };
    if rc == 0 {
        Ok(())
    } else {
        Err(format!(
            "umount {where_}: {}",
            std::io::Error::last_os_error()
        ))
    }
}

#[cfg(not(unix))]
pub fn real_mount(_cfg: &MountConfig) -> Result<(), String> {
    Err("mount(2) is unsupported on this platform".to_string())
}

#[cfg(not(unix))]
pub fn real_umount(_cfg: &MountConfig) -> Result<(), String> {
    Err("umount(2) is unsupported on this platform".to_string())
}
