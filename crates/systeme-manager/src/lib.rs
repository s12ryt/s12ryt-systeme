//! s12ryt-systeme 守護進程組裝層。
//!
//! - [`daemon::DaemonCore`] — IPC 請求 → ManagerCore 調度接線 (純邏輯, 跨平台可測)
//! - [`cgroup_fs::CgroupFsBackend`] — cgroup v2/v1 文件系統後端 (tempfile 可測)
//! - [`executor::StdCommandExecutor`] — 真實進程執行器 (std::process::Command + setsid)
//! - [`timers::TimerScheduler`] — timer 單元到期調度 (邊沿錨點 + 觸發快取)
//! - [`os_probe::OsProbe`] — path 單元的真實文件系統探測器
//! - [`mounts`] — mount 單元真實掛載 (cfg(unix) mount(2))
//! - [`sockets`] — socket 單元真實監聽 + fd 傳遞 (cfg(unix))
//! - `src/bin/systemd.rs` — 守護進程二進制入口 (tokio 事件循環, cfg(unix))

pub mod cgroup_fs;
pub mod daemon;
pub mod executor;
pub mod mounts;
pub mod os_probe;
pub mod sockets;
pub mod timers;

pub use cgroup_fs::CgroupFsBackend;
pub use daemon::DaemonCore;
pub use executor::StdCommandExecutor;
pub use mounts::{mount_data, real_mount, real_umount};
pub use os_probe::OsProbe;
pub use timers::TimerScheduler;
