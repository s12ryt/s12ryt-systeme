//! systemd-udevd: 設備事件守護進程 (基礎子集) — 二進制兼容名。
//!
//! cfg(unix): netlink KOBJECT_UEVENT 監聽 → UdevDb.apply → 打印事件;
//! --trigger: 掃描 /sys uevent 文件重建資料庫並重放。

use std::process::ExitCode;

const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(unix)]
mod imp {
    use std::os::unix::net::UnixDatagram;
    use std::path::Path;

    use systeme_udev::{parse_uevent, scan_sysfs, UdevDb};

    /// 掃描 sysfs 重建資料庫 (--trigger 資料來源)。
    pub fn trigger(root: &Path) -> UdevDb {
        let mut db = UdevDb::new();
        for text in scan_sysfs(root) {
            if let Some(ev) = parse_uevent(&text) {
                db.apply(&ev);
            }
        }
        db
    }

    pub fn run(trigger_first: bool) -> i32 {
        let mut db = UdevDb::new();
        if trigger_first {
            db = trigger(Path::new("/sys"));
            eprintln!("udevdb: {} devices after trigger", db.len());
            for rec in db.records() {
                eprintln!("device: {} ({})", rec.devpath, rec.devname.as_deref().unwrap_or("-"));
            }
        }

        // netlink KOBJECT_UEVENT: 內核不提供普通 socket 抽象的群播, 使用 /proc 里可用的
        // kobject uevent 用戶接口; 標準做法是 AF_NETLINK。這裡簡化: 嘗試 netlink,
        // 不可用時退回 /dev/kmsg 式空循環說明。
        match crate::netlink_socket() {
            Some(sock) => {
                eprintln!("udevd: listening on netlink kobject_uevent");
                let mut buf = vec![0u8; 8192];
                loop {
                    match sock.recv(&mut buf) {
                        Ok(n) => {
                            let text = String::from_utf8_lossy(&buf[..n]).into_owned();
                            if let Some(ev) = parse_uevent(&text) {
                                eprintln!(
                                    "uevent: {} {} {}",
                                    ev.action.as_deref().unwrap_or("-"),
                                    ev.devpath.as_deref().unwrap_or("-"),
                                    ev.devname.as_deref().unwrap_or("-"),
                                );
                                db.apply(&ev);
                            }
                        }
                        Err(e) => {
                            eprintln!("udevd: recv error: {e}");
                        }
                    }
                }
            }
            None => {
                eprintln!("udevd: netlink unavailable, monitoring only via --trigger");
                0
            }
        }
    }
}

#[cfg(unix)]
fn real_main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("systemd-udevd (systeme) {VERSION}");
        return ExitCode::SUCCESS;
    }
    let trigger_first = args.iter().any(|a| a == "--trigger");
    ExitCode::from(imp::run(trigger_first).clamp(0, 255) as u8)
}

#[cfg(not(unix))]
fn real_main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("systemd-udevd (systeme) {VERSION}");
        return ExitCode::SUCCESS;
    }
    eprintln!("systemd-udevd: only supported on unix (netlink/sysfs).");
    ExitCode::FAILURE
}

#[cfg(unix)]
/// 建立 KOBJECT_UEVENT netlink socket (group 1)。
fn netlink_socket() -> Option<std::os::unix::net::UnixDatagram> {
    // AF_NETLINK=16, SOCK_RAW|SOCK_CLOEXEC, NETLINK_KOBJECT_UEVENT=15
    // Rust std 無 netlink 封裝, 用 libc 裸調用後包裝為 UnixDatagram (fd 假借)。
    // 注意: UnixDatagram::from_raw_fd 僅適合同族描述符; netlink fd 用 libc 直接 recv。
    unsafe {
        let fd = libc::socket(
            libc::AF_NETLINK,
            libc::SOCK_RAW | libc::SOCK_CLOEXEC,
            15, // NETLINK_KOBJECT_UEVENT
        );
        if fd < 0 {
            return None;
        }
        let mut addr: libc::sockaddr_nl = std::mem::zeroed();
        addr.nl_family = libc::AF_NETLINK as u16;
        addr.nl_groups = 1; // kernel group
        if libc::bind(fd, &addr as *const _ as *const libc::sockaddr, std::mem::size_of::<libc::sockaddr_nl>() as u32) < 0 {
            libc::close(fd);
            return None;
        }
        // 用 UnixDatagram 借管 fd 所有權 (netlink 與 unix socket 在 Linux 上 recv 語義兼容)
        Some(std::os::unix::io::FromRawFd::from_raw_fd(fd))
    }
}

fn main() -> ExitCode {
    real_main()
}
