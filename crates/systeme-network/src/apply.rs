//! 應用層: 把 network_apply_plan 的計劃行翻譯為 ip 命令 argv、
//! 枚舉接口、匯總應用步驟。執行命令本身由 bin 層 cfg(unix) 完成。

use crate::link::LinkConfig;
use crate::netdev::{NetDevConfig, NetDevKind};
use crate::network::{match_network, network_apply_plan, NetworkConfig};
use std::path::Path;

/// 把一行應用計劃翻譯為可執行命令 argv; dhcp/dns/未知行返回 None
/// (bin 層需特殊處理或跳過)。
pub fn plan_to_argv(line: &str) -> Option<Vec<String>> {
    let parts: Vec<&str> = line.split_whitespace().collect();
    match parts.as_slice() {
        ["address", "add", cidr, "dev", dev] => Some(vec![
            "ip".into(),
            "address".into(),
            "add".into(),
            (*cidr).into(),
            "dev".into(),
            (*dev).into(),
        ]),
        ["route", "add", "default", "via", gw, "dev", dev] => Some(vec![
            "ip".into(),
            "route".into(),
            "add".into(),
            "default".into(),
            "via".into(),
            (*gw).into(),
            "dev".into(),
            (*dev).into(),
        ]),
        _ => None,
    }
}

/// 枚舉接口名 (讀 /sys/class/net 風格的目錄): 只取子目錄, 排除 "." 開頭, 字典序。
pub fn list_interface_names(dir: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .flatten()
        .filter(|e| e.path().is_dir())
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| !n.starts_with('.'))
        .collect();
    names.sort();
    names
}

/// 單個應用步驟。
#[derive(Debug, Clone, PartialEq)]
pub struct ApplyStep {
    pub iface: String,
    /// 原始計劃行 (診斷用)。
    pub line: String,
    /// 可直接執行的命令; None = dhcp/dns 等需 bin 層特殊處理。
    pub argv: Option<Vec<String>>,
}

/// 對每個接口選第一個匹配的 .network 配置, 生成應用步驟序列。
///
/// systemd-networkd 真實語義是全部匹配配置疊加; 這裡簡化為首個匹配生效
/// (容器場景的主流用法)。
pub fn apply_steps(configs: &[NetworkConfig], ifaces: &[String]) -> Vec<ApplyStep> {
    let mut steps = Vec::new();
    for iface in ifaces {
        let Some(cfg) = configs.iter().find(|c| match_network(c, iface, None, None)) else {
            continue;
        };
        for line in network_apply_plan(cfg, iface) {
            let argv = plan_to_argv(&line);
            steps.push(ApplyStep {
                iface: iface.clone(),
                argv,
                line,
            });
        }
    }
    steps
}

/// 生成 `ip link add` 創建命令; 缺 Name/Kind 或 vlan 缺 Id 返回 None。
pub fn netdev_create_argv(n: &NetDevConfig) -> Option<Vec<String>> {
    let name = n.name.as_deref()?;
    let kind = n.kind.as_ref()?;
    let mut argv = vec![
        "ip".to_string(),
        "link".to_string(),
        "add".to_string(),
        name.to_string(),
        "type".to_string(),
    ];
    match kind {
        NetDevKind::Vlan => {
            let id = n.vlan_id?;
            argv.push("vlan".into());
            argv.push("id".into());
            argv.push(id.to_string());
        }
        NetDevKind::Dummy => argv.push("dummy".into()),
        NetDevKind::Bridge => argv.push("bridge".into()),
        NetDevKind::Tun => argv.push("tun".into()),
        NetDevKind::Tap => argv.push("tap".into()),
        NetDevKind::Veth => argv.push("veth".into()),
        NetDevKind::MacVlan => argv.push("macvlan".into()),
        NetDevKind::MacVtap => argv.push("macvtap".into()),
        NetDevKind::Ipvlan => argv.push("ipvlan".into()),
        NetDevKind::Wireguard => argv.push("wireguard".into()),
        NetDevKind::Other(k) => argv.push(k.clone()),
    }
    Some(argv)
}

/// 生成 .link 設置命令序列 (MTU → MAC)。
pub fn link_apply_argv(l: &LinkConfig, ifname: &str) -> Vec<Vec<String>> {
    let mut cmds = Vec::new();
    if let Some(mtu) = l.mtu_bytes {
        cmds.push(vec![
            "ip".into(),
            "link".into(),
            "set".into(),
            "dev".into(),
            ifname.into(),
            "mtu".into(),
            mtu.to_string(),
        ]);
    }
    if let Some(mac) = &l.mac {
        cmds.push(vec![
            "ip".into(),
            "link".into(),
            "set".into(),
            "dev".into(),
            ifname.into(),
            "address".into(),
            mac.clone(),
        ]);
    }
    cmds
}
