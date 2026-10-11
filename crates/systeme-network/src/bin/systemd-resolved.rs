//! systemd-resolved: DNS stub 解析器 (127.0.0.53) — 二進制兼容名。
//!
//! 純轉發 DNS stub: 收 UDP 查詢 → 逐上游轉發 → 首個成功響應回客戶端。
//! 同時 best-effort 生成 /run/systemd/resolve/stub-resolv.conf。

use std::process::ExitCode;

const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(unix)]
mod imp {
    use std::fs;
    use std::net::UdpSocket;
    use std::path::Path;
    use std::time::Duration;

    use systeme_network::{collect_dns, render_resolv_conf, stub_resolv_conf, NetworkConfig};

    /// 上游 DNS 列表: $SYSTEME_DNS (逗號分隔) > 配置目錄 .network DNS= 聚合 > fallback。
    fn upstream_dns(dir: &Path) -> Vec<String> {
        if let Ok(v) = std::env::var("SYSTEME_DNS") {
            let list: Vec<String> = v
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            if !list.is_empty() {
                return list;
            }
        }
        let mut configs = Vec::new();
        if let Ok(entries) = fs::read_dir(dir) {
            let mut names: Vec<_> = entries
                .filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|x| x == "network"))
                .collect();
            names.sort();
            for p in names {
                if let Ok(text) = fs::read_to_string(&p) {
                    let parsed = systeme_units::parser::parse_unit(&text);
                    configs.push(NetworkConfig::from_parsed(&parsed));
                }
            }
        }
        let collected = collect_dns(&configs);
        if collected.is_empty() {
            eprintln!("no upstream DNS configured, falling back to 8.8.8.8 / 1.1.1.1");
            vec!["8.8.8.8".into(), "1.1.1.1".into()]
        } else {
            collected
        }
    }

    /// best-effort 寫 stub-resolv.conf 到指定路徑。
    fn write_stub_conf(path: &Path) {
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let conf = render_resolv_conf(&stub_resolv_conf());
        if let Err(e) = fs::write(path, conf) {
            eprintln!("warning: cannot write {}: {}", path.display(), e);
        }
    }

    /// 轉發一個 DNS 查詢到上游, 返回響應字節。
    fn forward(query: &[u8], upstreams: &[String]) -> Option<Vec<u8>> {
        for up in upstreams {
            let sock = match UdpSocket::bind("0.0.0.0:0") {
                Ok(s) => s,
                Err(_) => continue,
            };
            let addr = format!("{}:53", up);
            if sock.connect(&addr).is_err() {
                continue;
            }
            if sock.send(query).is_err() {
                continue;
            }
            let _ = sock.set_read_timeout(Some(Duration::from_secs(2)));
            let mut buf = vec![0u8; 4096];
            match sock.recv(&mut buf) {
                Ok(n) => return Some(buf[..n].to_vec()),
                Err(_) => continue,
            }
        }
        None
    }

    pub fn run() -> i32 {
        let dir = std::env::var("SYSTEMD_NETWORK_DIR")
            .unwrap_or_else(|_| "/etc/systemd/network".into());
        let dir = Path::new(&dir);

        let upstreams = upstream_dns(dir);
        eprintln!("systemd-resolved: upstream DNS: {}", upstreams.join(", "));

        // stub-resolv.conf best-effort
        let stub_path = std::env::var("SYSTEME_RESOLV_CONF")
            .unwrap_or_else(|_| "/run/systemd/resolve/stub-resolv.conf".into());
        write_stub_conf(Path::new(&stub_path));

        // 綁定: 127.0.0.53:53 → 失敗退 127.0.0.1:5353
        let sock = match UdpSocket::bind("127.0.0.53:53") {
            Ok(s) => {
                eprintln!("listening on 127.0.0.53:53");
                s
            }
            Err(e) => {
                eprintln!("warning: cannot bind 127.0.0.53:53 ({e}), falling back to 127.0.0.1:5353");
                match UdpSocket::bind("127.0.0.1:5353") {
                    Ok(s) => {
                        eprintln!("listening on 127.0.0.1:5353");
                        s
                    }
                    Err(e2) => {
                        eprintln!("error: cannot bind fallback socket: {e2}");
                        return 1;
                    }
                }
            }
        };

        let mut buf = vec![0u8; 512];
        loop {
            match sock.recv_from(&mut buf) {
                Ok((n, client)) => {
                    if let Some(resp) = forward(&buf[..n], &upstreams) {
                        let _ = sock.send_to(&resp, client);
                    }
                }
                Err(e) => {
                    eprintln!("recv error: {e}");
                }
            }
        }
    }
}

#[cfg(unix)]
fn real_main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("systemd-resolved (systeme) {VERSION}");
        return ExitCode::SUCCESS;
    }
    ExitCode::from(imp::run().clamp(0, 255) as u8)
}

#[cfg(not(unix))]
fn real_main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("systemd-resolved (systeme) {VERSION}");
        return ExitCode::SUCCESS;
    }
    eprintln!("systemd-resolved: only supported on unix (PID!=1 container target).");
    eprintln!("Use --version for version info.");
    ExitCode::FAILURE
}

fn main() -> ExitCode {
    real_main()
}
