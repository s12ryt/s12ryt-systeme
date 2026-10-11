//! networkd 配置解析/匹配/應用計劃的 TDD 測試 (RED 先行)。

use systeme_network::link::LinkConfig;
use systeme_network::netdev::{NetDevConfig, NetDevKind};
use systeme_network::network::{match_iface, match_network, network_apply_plan, DhcpMode, NetworkConfig};
use systeme_units::parser::parse_unit;

fn cfg(text: &str) -> NetworkConfig {
    NetworkConfig::from_parsed(&parse_unit(text))
}

fn lcfg(text: &str) -> LinkConfig {
    LinkConfig::from_parsed(&parse_unit(text))
}

fn ndcfg(text: &str) -> NetDevConfig {
    NetDevConfig::from_parsed(&parse_unit(text))
}

// ---------- .network 解析 ----------

#[test]
fn network_default_when_no_sections() {
    let c = cfg("");
    assert!(c.match_.names.is_empty());
    assert!(c.match_.drivers.is_empty());
    assert!(c.match_.macs.is_empty());
    assert_eq!(c.dhcp, DhcpMode::No);
    assert!(c.addresses.is_empty());
    assert!(c.gateways.is_empty());
    assert!(c.dns.is_empty());
    assert!(c.warnings.is_empty());
}

#[test]
fn network_match_name_parses_multi() {
    let c = cfg("[Match]\nName=eth0 en*\n");
    assert_eq!(c.match_.names, vec!["eth0", "en*"]);
}

#[test]
fn network_match_driver_and_mac() {
    let c = cfg("[Match]\nDriver=virtio_net e1000\nMACAddress=00:11:22:33:44:55\n");
    assert_eq!(c.match_.drivers, vec!["virtio_net", "e1000"]);
    assert_eq!(c.match_.macs, vec!["00:11:22:33:44:55"]);
}

#[test]
fn network_dhcp_modes() {
    assert_eq!(cfg("[Network]\nDHCP=yes\n").dhcp, DhcpMode::Yes);
    assert_eq!(cfg("[Network]\nDHCP=ipv4\n").dhcp, DhcpMode::Ipv4);
    assert_eq!(cfg("[Network]\nDHCP=ipv6\n").dhcp, DhcpMode::Ipv6);
    assert_eq!(cfg("[Network]\nDHCP=no\n").dhcp, DhcpMode::No);
    assert_eq!(cfg("[Network]\nDHCP=YES\n").dhcp, DhcpMode::Yes);
}

#[test]
fn network_dhcp_bad_value_warns_and_defaults_no() {
    let c = cfg("[Network]\nDHCP=maybe\n");
    assert_eq!(c.dhcp, DhcpMode::No);
    assert_eq!(c.warnings.len(), 1);
    assert!(c.warnings[0].contains("DHCP"));
}

#[test]
fn network_addresses_gateways_dns_multi_preserved() {
    let c = cfg("[Network]\nAddress=192.168.1.10/24\nAddress=fd00::10/64\nGateway=192.168.1.1\nGateway=fd00::1\nDNS=8.8.8.8\nDNS=1.1.1.1\n");
    assert_eq!(c.addresses, vec!["192.168.1.10/24", "fd00::10/64"]);
    assert_eq!(c.gateways, vec!["192.168.1.1", "fd00::1"]);
    assert_eq!(c.dns, vec!["8.8.8.8", "1.1.1.1"]);
}

// ---------- 匹配 ----------

#[test]
fn network_match_glob_hits() {
    let c = cfg("[Match]\nName=en*\n");
    assert!(match_network(&c, "enp0s3", None, None));
    assert!(!match_network(&c, "wlan0", None, None));
}

#[test]
fn network_match_all_when_no_match_section() {
    let c = cfg("[Network]\nDHCP=yes\n");
    assert!(match_network(&c, "anything0", None, None));
}

#[test]
fn network_match_driver_gates_on_known() {
    let c = cfg("[Match]\nDriver=virtio_net\n");
    // 條件存在但驅動未知 → 不匹配 (保守)
    assert!(!match_network(&c, "eth0", None, None));
    assert!(match_network(&c, "eth0", Some("virtio_net"), None));
    assert!(!match_network(&c, "eth0", Some("e1000"), None));
}

#[test]
fn network_match_name_and_driver_are_and() {
    let c = cfg("[Match]\nName=eth*\nDriver=virtio_net\n");
    assert!(match_network(&c, "eth0", Some("virtio_net"), None));
    assert!(!match_network(&c, "wlan0", Some("virtio_net"), None));
    assert!(!match_network(&c, "eth0", Some("e1000"), None));
}

#[test]
fn network_match_mac_gate() {
    let c = cfg("[Match]\nMACAddress=aa:bb:cc:dd:ee:ff\n");
    assert!(match_network(&c, "eth0", None, Some("aa:bb:cc:dd:ee:ff")));
    assert!(!match_network(&c, "eth0", None, Some("11:22:33:44:55:66")));
    assert!(!match_network(&c, "eth0", None, None));
}

#[test]
fn match_iface_direct_on_match_struct() {
    let c = cfg("[Match]\nName=eth0\n");
    assert!(match_iface(&c.match_, "eth0", None, None));
}

// ---------- 應用計劃 ----------

#[test]
fn network_apply_plan_static() {
    let c = cfg("[Network]\nAddress=192.168.1.10/24\nGateway=192.168.1.1\nDNS=8.8.8.8\n");
    assert_eq!(
        network_apply_plan(&c, "eth0"),
        vec![
            "address add 192.168.1.10/24 dev eth0",
            "route add default via 192.168.1.1 dev eth0",
            "dns add 8.8.8.8",
        ]
    );
}

#[test]
fn network_apply_plan_dhcp_first() {
    let c = cfg("[Network]\nDHCP=yes\nAddress=10.0.0.5/24\n");
    assert_eq!(
        network_apply_plan(&c, "eth0"),
        vec![
            "dhcp enable on eth0",
            "address add 10.0.0.5/24 dev eth0",
        ]
    );
}

#[test]
fn network_apply_plan_dhcp_ipv4_variant() {
    let c = cfg("[Network]\nDHCP=ipv4\n");
    assert_eq!(network_apply_plan(&c, "eth1"), vec!["dhcp enable ipv4 on eth1"]);
}

#[test]
fn network_apply_plan_dhcp_ipv6_variant() {
    let c = cfg("[Network]\nDHCP=ipv6\n");
    assert_eq!(network_apply_plan(&c, "eth1"), vec!["dhcp enable ipv6 on eth1"]);
}

#[test]
fn network_apply_plan_empty_when_nothing() {
    let c = cfg("[Network]\nDHCP=no\n");
    assert!(network_apply_plan(&c, "eth0").is_empty());
}

// ---------- .link ----------

#[test]
fn link_default_when_no_sections() {
    let l = lcfg("");
    assert!(l.match_.names.is_empty());
    assert_eq!(l.mac, None);
    assert_eq!(l.description, None);
    assert_eq!(l.mtu_bytes, None);
    assert!(l.warnings.is_empty());
}

#[test]
fn link_match_and_settings() {
    let l = lcfg("[Match]\nName=eth*\n\n[Link]\nMACAddress=aa:bb:cc:dd:ee:ff\nDescription=External NIC\nMTUBytes=1500\n");
    assert_eq!(l.match_.names, vec!["eth*"]);
    assert_eq!(l.mac.as_deref(), Some("aa:bb:cc:dd:ee:ff"));
    assert_eq!(l.description.as_deref(), Some("External NIC"));
    assert_eq!(l.mtu_bytes, Some(1500));
}

#[test]
fn link_mtu_bytes_suffixes() {
    let l = lcfg("[Link]\nMTUBytes=4K\n");
    assert_eq!(l.mtu_bytes, Some(4096));
}

#[test]
fn link_mtu_bad_value_warns() {
    let l = lcfg("[Link]\nMTUBytes=abc\n");
    assert_eq!(l.mtu_bytes, None);
    assert_eq!(l.warnings.len(), 1);
    assert!(l.warnings[0].contains("MTUBytes"));
}

#[test]
fn link_match_conditions_and() {
    let l = lcfg("[Match]\nName=eth*\nMACAddress=aa:bb:cc:dd:ee:ff\n");
    assert!(match_iface(&l.match_, "eth0", None, Some("aa:bb:cc:dd:ee:ff")));
    assert!(!match_iface(&l.match_, "eth0", None, None));
    assert!(!match_iface(&l.match_, "wlan0", None, Some("aa:bb:cc:dd:ee:ff")));
}

// ---------- .netdev ----------

#[test]
fn netdev_default_when_no_sections() {
    let n = ndcfg("");
    assert_eq!(n.name, None);
    assert_eq!(n.kind, None);
    assert_eq!(n.vlan_id, None);
    assert!(n.warnings.is_empty());
}

#[test]
fn netdev_dummy() {
    let n = ndcfg("[NetDev]\nName=dmy0\nKind=dummy\n");
    assert_eq!(n.name.as_deref(), Some("dmy0"));
    assert_eq!(n.kind, Some(NetDevKind::Dummy));
}

#[test]
fn netdev_bridge_case_insensitive_kind() {
    let n = ndcfg("[NetDev]\nName=br0\nKind=Bridge\n");
    assert_eq!(n.kind, Some(NetDevKind::Bridge));
}

#[test]
fn netdev_vlan_with_id() {
    let n = ndcfg("[NetDev]\nName=vlan42\nKind=vlan\n\n[VLAN]\nId=42\n");
    assert_eq!(n.kind, Some(NetDevKind::Vlan));
    assert_eq!(n.vlan_id, Some(42));
}

#[test]
fn netdev_vlan_id_bad_warns() {
    let n = ndcfg("[NetDev]\nName=vlan1\nKind=vlan\n\n[VLAN]\nId=abc\n");
    assert_eq!(n.vlan_id, None);
    assert_eq!(n.warnings.len(), 1);
    assert!(n.warnings[0].contains("Id"));
}

#[test]
fn netdev_unknown_kind_maps_other() {
    let n = ndcfg("[NetDev]\nName=bnd0\nKind=bond\n");
    assert_eq!(n.kind, Some(NetDevKind::Other("bond".to_string())));
}

#[test]
fn netdev_mtu_and_mac() {
    let n = ndcfg("[NetDev]\nName=dmy0\nKind=dummy\nMACAddress=aa:bb:cc:dd:ee:ff\nMTUBytes=9000\n");
    assert_eq!(n.mac.as_deref(), Some("aa:bb:cc:dd:ee:ff"));
    assert_eq!(n.mtu_bytes, Some(9000));
}
