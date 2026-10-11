//! socket 單元真實監聽 + fd 傳遞。
//!
//! 語義 (systemd 簡化版):
//! - 啟動 .socket → 依 Listen* 聲明 bind 所有 listener, fd 由 SocketActivation 持有
//! - 隨後啟動目標 service 時, fd 經 dup2 放到 3,4,5... 並注入 LISTEN_FDS/LISTEN_PID
//! - 惰性激活 (第一連接才啟服務) 留 bin 層 watcher; 本模組負責 bind 與 fd 注入
//!
//! 非 unix 平台: bind 恆 Err (類型仍存在, 供 DaemonCore 跨平台編譯)。

use systeme_core::socket::SocketConfig;

/// 一個已綁定的監聽 socket (持有 listener 對象以保住 fd)。
#[cfg(unix)]
pub struct BoundSocket {
    pub label: String,
    listener: Listener,
}

#[cfg(unix)]
enum Listener {
    Unix(std::os::unix::net::UnixListener),
    Tcp(std::net::TcpListener),
    /// abstract namespace socket (libc 直建, fd 手持)
    Raw(i32),
}

#[cfg(unix)]
impl Listener {
    fn raw_fd(&self) -> i32 {
        use std::os::unix::io::AsRawFd;
        match self {
            Listener::Unix(l) => l.as_raw_fd(),
            Listener::Tcp(l) => l.as_raw_fd(),
            Listener::Raw(fd) => *fd,
        }
    }
}

/// 一次 socket 單元激活的全部 listener。
pub struct SocketActivation {
    pub target_hint: Option<String>,
    #[cfg(unix)]
    sockets: Vec<BoundSocket>,
}

impl SocketActivation {
    /// 依序的 fd 列表 (對應 LISTEN_FDS 傳遞順序)。
    pub fn fds(&self) -> Vec<i32> {
        #[cfg(unix)]
        {
            self.sockets.iter().map(|s| s.listener.raw_fd()).collect()
        }
        #[cfg(not(unix))]
        {
            Vec::new()
        }
    }

    /// listener 數量。
    pub fn len(&self) -> usize {
        #[cfg(unix)]
        {
            self.sockets.len()
        }
        #[cfg(not(unix))]
        {
            0
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// 依 [Socket] 配置綁定全部監聽 socket。任何一個失敗 → 整體 Err (已建的會被 drop)。
#[cfg(unix)]
pub fn bind_socket_unit(cfg: &SocketConfig) -> Result<SocketActivation, String> {
    use systeme_core::socket::ListenKind;
    use std::net::TcpListener;
    use std::os::unix::net::UnixListener;

    let mut sockets = Vec::new();
    for spec in &cfg.listen {
        match &spec.kind {
            ListenKind::Path(p) => {
                // 殘留 socket 文件先刪 (bind 對既有文件會 EADDRINUSE)
                let _ = std::fs::remove_file(p);
                let l = UnixListener::bind(p)
                    .map_err(|e| format!("bind unix socket {p}: {e}"))?;
                sockets.push(BoundSocket {
                    label: p.clone(),
                    listener: Listener::Unix(l),
                });
            }
            ListenKind::Abstract(name) => {
                let fd = bind_abstract(name)?;
                sockets.push(BoundSocket {
                    label: format!("@{name}"),
                    listener: Listener::Raw(fd),
                });
            }
            ListenKind::Port(port) => {
                let l = TcpListener::bind(("0.0.0.0", *port))
                    .map_err(|e| format!("bind tcp 0.0.0.0:{port}: {e}"))?;
                sockets.push(BoundSocket {
                    label: port.to_string(),
                    listener: Listener::Tcp(l),
                });
            }
            ListenKind::AddrPort { addr, port } => {
                let l = TcpListener::bind((addr.as_str(), *port))
                    .map_err(|e| format!("bind tcp {addr}:{port}: {e}"))?;
                sockets.push(BoundSocket {
                    label: format!("{addr}:{port}"),
                    listener: Listener::Tcp(l),
                });
            }
            ListenKind::Network(s) => {
                // 兜底: 嘗試當 "host:port" 解析
                let (host, port) = s
                    .rsplit_once(':')
                    .ok_or_else(|| format!("cannot parse listen spec: {s}"))?;
                let port: u16 = port
                    .parse()
                    .map_err(|_| format!("cannot parse listen spec: {s}"))?;
                let l = TcpListener::bind((host, port))
                    .map_err(|e| format!("bind tcp {host}:{port}: {e}"))?;
                sockets.push(BoundSocket {
                    label: s.clone(),
                    listener: Listener::Tcp(l),
                });
            }
        }
    }
    Ok(SocketActivation {
        target_hint: cfg.service.clone(),
        sockets,
    })
}

/// 非 unix: 不支持真實 socket 綁定。
#[cfg(not(unix))]
pub fn bind_socket_unit(_cfg: &SocketConfig) -> Result<SocketActivation, String> {
    Err("socket activation is unsupported on this platform".to_string())
}

/// abstract namespace unix socket (libc: socket + bind with sun_path[0]=\0 + listen)。
#[cfg(unix)]
fn bind_abstract(name: &str) -> Result<i32, String> {
    let fd = unsafe {
        libc::socket(
            libc::AF_UNIX,
            libc::SOCK_STREAM | libc::SOCK_CLOEXEC,
            0,
        )
    };
    if fd < 0 {
        return Err(format!("socket(): {}", std::io::Error::last_os_error()));
    }
    // sockaddr_un: family(2 bytes) + abstract name ("\0" + name)
    let mut addr = Vec::with_capacity(2 + 1 + name.len());
    addr.extend_from_slice(&libc::AF_UNIX.to_ne_bytes()[..2]);
    addr.push(0); // abstract 前綴 \0
    addr.extend_from_slice(name.as_bytes());

    let rc = unsafe { libc::bind(fd, addr.as_ptr().cast(), addr.len() as libc::socklen_t) };
    if rc != 0 {
        let e = std::io::Error::last_os_error();
        unsafe { libc::close(fd) };
        return Err(format!("bind abstract @{name}: {e}"));
    }
    if unsafe { libc::listen(fd, 128) } != 0 {
        let e = std::io::Error::last_os_error();
        unsafe { libc::close(fd) };
        return Err(format!("listen abstract @{name}: {e}"));
    }
    Ok(fd)
}

/// 把 fd 列表注入子進程: dup2 到 3,4,5... 並設 LISTEN_FDS/LISTEN_PID。
/// 在 Command::pre_exec (fork 後、exec 前) 中執行。
#[cfg(unix)]
pub fn inject_fds(cmd: &mut std::process::Command, fds: &[i32]) {
    use std::os::unix::process::CommandExt;

    let fds: Vec<i32> = fds.to_vec();
    let n = fds.len();
    unsafe {
        cmd.pre_exec(move || {
            for (i, fd) in fds.iter().enumerate() {
                if libc::dup2(*fd, 3 + i as i32) < 0 {
                    return Err(std::io::Error::last_os_error());
                }
            }
            if n > 0 {
                let pid = libc::getpid().to_string();
                libc::setenv(
                    b"LISTEN_FDS\0".as_ptr().cast(),
                    format!("{n}\0").as_ptr().cast(),
                    1,
                );
                libc::setenv(
                    b"LISTEN_PID\0".as_ptr().cast(),
                    format!("{pid}\0").as_ptr().cast(),
                    1,
                );
            }
            Ok(())
        });
    }
}
