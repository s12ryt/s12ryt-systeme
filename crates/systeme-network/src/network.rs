//! .network 文件解析 ([Match]/[Network])、接口匹配與應用計劃生成。

use systeme_core::path::glob_match;
use systeme_units::parser::ParsedUnit;

/// .network/.link 共用的 [Match] 段。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct NetworkMatch {
    /// Name= 空格分詞後的 glob 模式列表; 空 = 匹配所有接口。
    pub names: Vec<String>,
    /// Driver= 模式列表。
    pub drivers: Vec<String>,
    /// MACAddress= 列表。
    pub macs: Vec<String>,
}

impl NetworkMatch {
    pub fn from_parsed(p: &ParsedUnit) -> Self {
        let mut m = NetworkMatch::default();
        if let Some(s) = p.section("Match") {
            for v in s.get_list("Name") {
                m.names.extend(v.split_whitespace().map(str::to_string));
            }
            for v in s.get_list("Driver") {
                m.drivers.extend(v.split_whitespace().map(str::to_string));
            }
            for v in s.get_list("MACAddress") {
                m.macs.extend(v.split_whitespace().map(str::to_string));
            }
        }
        m
    }

    /// 是否存在任何匹配條件。
    pub fn is_empty(&self) -> bool {
        self.names.is_empty() && self.drivers.is_empty() && self.macs.is_empty()
    }
}

/// DHCP= 值語義。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum DhcpMode {
    #[default]
    No,
    Yes,
    Ipv4,
    Ipv6,
}

impl DhcpMode {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "" | "no" | "false" | "off" => Some(DhcpMode::No),
            "yes" | "true" | "on" => Some(DhcpMode::Yes),
            "ipv4" => Some(DhcpMode::Ipv4),
            "ipv6" => Some(DhcpMode::Ipv6),
            _ => None,
        }
    }
}

/// .network 文件配置。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct NetworkConfig {
    pub match_: NetworkMatch,
    pub dhcp: DhcpMode,
    /// Address= 多行 CIDR, 保序。
    pub addresses: Vec<String>,
    /// Gateway= 多行, 保序。
    pub gateways: Vec<String>,
    /// DNS= 多行, 保序。
    pub dns: Vec<String>,
    pub warnings: Vec<String>,
}

impl NetworkConfig {
    pub fn from_parsed(p: &ParsedUnit) -> Self {
        let match_ = NetworkMatch::from_parsed(p);
        let mut dhcp = DhcpMode::No;
        let mut addresses = Vec::new();
        let mut gateways = Vec::new();
        let mut dns = Vec::new();
        let mut warnings = Vec::new();

        if let Some(s) = p.section("Network") {
            if let Some(v) = s.get("DHCP") {
                match DhcpMode::parse(v) {
                    Some(d) => dhcp = d,
                    None => warnings.push(format!("Ignoring invalid DHCP value: {v}")),
                }
            }
            for v in s.get_list("Address") {
                addresses.push(v.to_string());
            }
            for v in s.get_list("Gateway") {
                gateways.push(v.to_string());
            }
            for v in s.get_list("DNS") {
                dns.push(v.to_string());
            }
        }

        NetworkConfig {
            match_,
            dhcp,
            addresses,
            gateways,
            dns,
            warnings,
        }
    }
}

/// 判斷接口是否匹配 [Match] 條件。
///
/// 條件組之間是 AND; 組內任一命中即可 (OR)。條件組存在但信息未知
/// (driver/mac 為 None) 時保守判定為不匹配。
pub fn match_iface(m: &NetworkMatch, ifname: &str, driver: Option<&str>, mac: Option<&str>) -> bool {
    if !m.names.is_empty() && !m.names.iter().any(|p| glob_match(p, ifname)) {
        return false;
    }
    if !m.drivers.is_empty() {
        let Some(d) = driver else {
            return false;
        };
        if !m.drivers.iter().any(|p| glob_match(p, d)) {
            return false;
        }
    }
    if !m.macs.is_empty() {
        let Some(x) = mac else {
            return false;
        };
        if !m.macs.iter().any(|p| p.eq_ignore_ascii_case(x)) {
            return false;
        }
    }
    true
}

/// 便捷包裝: 對 NetworkConfig 匹配接口。
pub fn match_network(cfg: &NetworkConfig, ifname: &str, driver: Option<&str>, mac: Option<&str>) -> bool {
    match_iface(&cfg.match_, ifname, driver, mac)
}

/// 生成純文本應用計劃 (bin 層 cfg(unix) 消費, 本層跨平台可測)。
///
/// 順序: DHCP → 地址 → 網關 → DNS。
pub fn network_apply_plan(cfg: &NetworkConfig, ifname: &str) -> Vec<String> {
    let mut plan = Vec::new();
    match cfg.dhcp {
        DhcpMode::Yes => plan.push(format!("dhcp enable on {ifname}")),
        DhcpMode::Ipv4 => plan.push(format!("dhcp enable ipv4 on {ifname}")),
        DhcpMode::Ipv6 => plan.push(format!("dhcp enable ipv6 on {ifname}")),
        DhcpMode::No => {}
    }
    for a in &cfg.addresses {
        plan.push(format!("address add {a} dev {ifname}"));
    }
    for g in &cfg.gateways {
        plan.push(format!("route add default via {g} dev {ifname}"));
    }
    for d in &cfg.dns {
        plan.push(format!("dns add {d}"));
    }
    plan
}
