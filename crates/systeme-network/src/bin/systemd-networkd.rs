//! systemd-networkd — 網絡配置守護進程 (二進制 drop-in 兼容名)
//!
//! 讀取 network 配置目錄 ($SYSTEMD_NETWORK_DIR 或 /etc/systemd/network):
//! - *.netdev: 先創建虛擬設備 (ip link add)
//! - *.link:   對匹配接口應用 link 層配置 (MTU/MAC)
//! - *.network: 對匹配接口應用網絡配置 (address/route)
//!
//! DHCP/DNS 行由外部工具或後續守護進程處理, 此處打印 skip。
//! --dry-run 僅打印將執行的命令。

const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(unix)]
use std::path::PathBuf;

#[cfg(unix)]
fn config_dir() -> PathBuf {
    if let Ok(d) = std::env::var("SYSTEMD_NETWORK_DIR") {
        return PathBuf::from(d);
    }
    PathBuf::from("/etc/systemd/network")
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("systemd-networkd (systeme) {VERSION}");
        return;
    }
    let dry_run = args.iter().any(|a| a == "--dry-run");

    #[cfg(unix)]
    {
        match run(&config_dir(), dry_run) {
            Ok(count) => {
                if count == 0 {
                    println!("systemd-networkd: nothing to apply");
                }
            }
            Err(e) => {
                eprintln!("systemd-networkd: {e}");
                std::process::exit(1);
            }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = dry_run;
        eprintln!("systemd-networkd: this daemon requires a Linux system (unix).");
        std::process::exit(1);
    }
}

#[cfg(unix)]
fn run(dir: &std::path::Path, dry_run: bool) -> Result<usize, String> {
    use systeme_network::{apply_steps, link_apply_argv, list_interface_names, netdev_create_argv};
    use systeme_network::{LinkConfig, NetDevConfig, NetworkConfig};
    use systeme_units::parser::parse_unit;

    let mut applied = 0usize;

    // 收集配置文件 (排序保證確定性)
    let mut networks: Vec<(String, NetworkConfig)> = Vec::new();
    let mut netdevs: Vec<(String, NetDevConfig)> = Vec::new();
    let mut links: Vec<(String, LinkConfig)> = Vec::new();

    let entries = std::fs::read_dir(dir).map_err(|e| format!("read {}: {e}", dir.display()))?;
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let Some(text) = std::fs::read_to_string(&path).ok() else {
            continue;
        };
        let parsed = parse_unit(&text);
        if name.ends_with(".network") {
            networks.push((name.to_string(), NetworkConfig::from_parsed(&parsed)));
        } else if name.ends_with(".netdev") {
            netdevs.push((name.to_string(), NetDevConfig::from_parsed(&parsed)));
        } else if name.ends_with(".link") {
            links.push((name.to_string(), LinkConfig::from_parsed(&parsed)));
        }
    }
    networks.sort_by(|a, b| a.0.cmp(&b.0));
    netdevs.sort_by(|a, b| a.0.cmp(&b.0));
    links.sort_by(|a, b| a.0.cmp(&b.0));

    // 1. netdev: 創建虛擬設備
    for (name, nd) in &netdevs {
        match netdev_create_argv(nd) {
            Some(argv) => {
                exec_argv(&argv, dry_run, &mut applied)?;
            }
            None => eprintln!(
                "networkd: {name}: netdev has no usable Name=/Kind= (or VLAN Id missing), skipped"
            ),
        }
    }

    let ifaces = list_interface_names(std::path::Path::new("/sys/class/net"));

    // 2. link: 應用鏈路層配置 (每接口取第一個匹配)
    for iface in &ifaces {
        for (_, lc) in &links {
            if systeme_network::match_network(
                &systeme_network::NetworkConfig {
                    match_: lc.match_.clone(),
                    ..Default::default()
                },
                iface.as_str(),
                None,
                None,
            ) {
                for argv in link_apply_argv(lc, iface) {
                    exec_argv(&argv, dry_run, &mut applied)?;
                }
                break;
            }
        }
    }

    // 3. network: 地址/路由
    let configs: Vec<NetworkConfig> = networks.iter().map(|(_, c)| c.clone()).collect();
    for step in apply_steps(&configs, &ifaces) {
        match &step.argv {
            Some(argv) => exec_argv(argv, dry_run, &mut applied)?,
            None => println!(
                "networkd: {}: {} (skipped: needs external dhcp/dns handler)",
                step.iface, step.line
            ),
        }
    }

    Ok(applied)
}

#[cfg(unix)]
fn exec_argv(argv: &[String], dry_run: bool, applied: &mut usize) -> Result<(), String> {
    use std::process::Command;
    println!("networkd: $ {}", argv.join(" "));
    if dry_run {
        *applied += 1;
        return Ok(());
    }
    let status = Command::new(&argv[0])
        .args(&argv[1..])
        .status()
        .map_err(|e| format!("spawn {}: {e}", argv[0]))?;
    if !status.success() {
        return Err(format!("command failed ({}): {}", status, argv.join(" ")));
    }
    *applied += 1;
    Ok(())
}
