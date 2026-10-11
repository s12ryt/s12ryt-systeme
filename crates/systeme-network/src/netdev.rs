//! .netdev 文件解析 ([NetDev]/[VLAN] 等)。

use crate::link::parse_size_bytes;
use systeme_units::parser::ParsedUnit;

/// NetDev Kind= 值; 未知類型歸入 Other 保留原值 (高兼容不報錯)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetDevKind {
    Dummy,
    Bridge,
    Vlan,
    Tun,
    Tap,
    Veth,
    MacVlan,
    MacVtap,
    Ipvlan,
    Wireguard,
    Other(String),
}

impl NetDevKind {
    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "dummy" => NetDevKind::Dummy,
            "bridge" => NetDevKind::Bridge,
            "vlan" => NetDevKind::Vlan,
            "tun" => NetDevKind::Tun,
            "tap" => NetDevKind::Tap,
            "veth" => NetDevKind::Veth,
            "macvlan" => NetDevKind::MacVlan,
            "macvtap" => NetDevKind::MacVtap,
            "ipvlan" => NetDevKind::Ipvlan,
            "wireguard" => NetDevKind::Wireguard,
            other => NetDevKind::Other(other.to_string()),
        }
    }
}

/// .netdev 文件配置。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct NetDevConfig {
    pub name: Option<String>,
    pub kind: Option<NetDevKind>,
    /// [VLAN] Id=。
    pub vlan_id: Option<u16>,
    pub mac: Option<String>,
    pub mtu_bytes: Option<u64>,
    pub warnings: Vec<String>,
}

impl NetDevConfig {
    pub fn from_parsed(p: &ParsedUnit) -> Self {
        let mut name = None;
        let mut kind = None;
        let mut vlan_id = None;
        let mut mac = None;
        let mut mtu_bytes = None;
        let mut warnings = Vec::new();

        if let Some(s) = p.section("NetDev") {
            if let Some(v) = s.get("Name") {
                if !v.trim().is_empty() {
                    name = Some(v.trim().to_string());
                }
            }
            if let Some(v) = s.get("Kind") {
                if !v.trim().is_empty() {
                    kind = Some(NetDevKind::parse(v));
                }
            }
            if let Some(v) = s.get("MACAddress") {
                if !v.trim().is_empty() {
                    mac = Some(v.trim().to_string());
                }
            }
            if let Some(v) = s.get("MTUBytes") {
                match parse_size_bytes(v) {
                    Some(n) => mtu_bytes = Some(n),
                    None => warnings.push(format!("Ignoring invalid MTUBytes value: {v}")),
                }
            }
        }

        if let Some(s) = p.section("VLAN") {
            if let Some(v) = s.get("Id") {
                match v.trim().parse::<u16>() {
                    Ok(id) => vlan_id = Some(id),
                    Err(_) => warnings.push(format!("Ignoring invalid VLAN Id value: {v}")),
                }
            }
        }

        NetDevConfig {
            name,
            kind,
            vlan_id,
            mac,
            mtu_bytes,
            warnings,
        }
    }
}
