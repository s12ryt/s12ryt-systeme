//! networkd 應用層: 計劃翻譯 / 接口枚舉 / 應用步驟 / netdev/link 命令生成。

use systeme_network::apply::{
    apply_steps, link_apply_argv, list_interface_names, netdev_create_argv, plan_to_argv,
};
use systeme_network::netdev::NetDevKind;
use systeme_network::link::LinkConfig;
use systeme_network::network::{NetworkConfig, NetworkMatch};
use systeme_units::parser::parse_unit;

// ---------- plan_to_argv ----------

#[test]
fn plan_to_argv_address() {
    assert_eq!(
        plan_to_argv("address add 192.168.1.10/24 dev eth0"),
        Some(vec![
            "ip".to_string(),
            "address".to_string(),
            "add".to_string(),
            "192.168.1.10/24".to_string(),
            "dev".to_string(),
            "eth0".to_string(),
        ])
    );
}

#[test]
fn plan_to_argv_route() {
    assert_eq!(
        plan_to_argv("route add default via 192.168.1.1 dev eth0"),
        Some(vec![
            "ip".to_string(),
            "route".to_string(),
            "add".to_string(),
            "default".to_string(),
            "via".to_string(),
            "192.168.1.1".to_string(),
            "dev".to_string(),
            "eth0".to_string(),
        ])
    );
}

#[test]
fn plan_to_argv_dhcp_is_none() {
    assert_eq!(plan_to_argv("dhcp enable on eth0"), None);
    assert_eq!(plan_to_argv("dhcp enable ipv4 on eth0"), None);
}

#[test]
fn plan_to_argv_dns_is_none() {
    assert_eq!(plan_to_argv("dns add 8.8.8.8"), None);
}

#[test]
fn plan_to_argv_unknown_is_none() {
    assert_eq!(plan_to_argv("something weird"), None);
    assert_eq!(plan_to_argv(""), None);
}

// ---------- list_interface_names ----------

#[test]
fn list_interface_names_sorted_dirs_only() {
    let dir = tempfile::tempdir().unwrap();
    for name in ["wlan0", "lo", "eth0"] {
        std::fs::create_dir(dir.path().join(name)).unwrap();
    }
    std::fs::write(dir.path().join("notdir"), b"x").unwrap();
    let names = list_interface_names(dir.path());
    assert_eq!(names, vec!["eth0", "lo", "wlan0"]);
}

// ---------- apply_steps ----------

#[test]
fn apply_steps_static_config() {
    let cfg = NetworkConfig::from_parsed(&parse_unit(
        "[Match]\nName=eth0\n\n[Network]\nAddress=192.168.1.10/24\nGateway=192.168.1.1\nDNS=8.8.8.8\n",
    ));
    let steps = apply_steps(&[cfg], &["eth0".to_string(), "wlan0".to_string()]);
    assert_eq!(steps.len(), 3);
    assert_eq!(steps[0].iface, "eth0");
    assert_eq!(steps[0].line, "address add 192.168.1.10/24 dev eth0");
    assert!(steps[0].argv.is_some());
    assert_eq!(steps[2].line, "dns add 8.8.8.8");
    assert!(steps[2].argv.is_none());
}

#[test]
fn apply_steps_no_match_is_empty() {
    let cfg = NetworkConfig::from_parsed(&parse_unit("[Match]\nName=eth9\n"));
    assert!(apply_steps(&[cfg], &["eth0".to_string()]).is_empty());
}

#[test]
fn apply_steps_first_match_wins() {
    let a = NetworkConfig::from_parsed(&parse_unit("[Match]\nName=eth*\n\n[Network]\nAddress=10.0.0.1/8\n"));
    let b = NetworkConfig::from_parsed(&parse_unit("[Match]\nName=eth0\n\n[Network]\nAddress=10.0.0.2/8\n"));
    let steps = apply_steps(&[a, b], &["eth0".to_string()]);
    assert_eq!(steps.len(), 1);
    assert_eq!(steps[0].line, "address add 10.0.0.1/8 dev eth0");
}

#[test]
fn apply_steps_dhcp_line_kept_without_argv() {
    let cfg = NetworkConfig::from_parsed(&parse_unit("[Network]\nDHCP=yes\n"));
    let steps = apply_steps(&[cfg], &["eth0".to_string()]);
    assert_eq!(steps.len(), 1);
    assert_eq!(steps[0].line, "dhcp enable on eth0");
    assert!(steps[0].argv.is_none());
}

// ---------- netdev_create_argv ----------

#[test]
fn netdev_create_argv_dummy() {
    let n = systeme_network::netdev::NetDevConfig::from_parsed(&parse_unit(
        "[NetDev]\nName=dmy0\nKind=dummy\n",
    ));
    assert_eq!(
        netdev_create_argv(&n),
        Some(vec![
            "ip".to_string(),
            "link".to_string(),
            "add".to_string(),
            "dmy0".to_string(),
            "type".to_string(),
            "dummy".to_string(),
        ])
    );
}

#[test]
fn netdev_create_argv_vlan_with_id() {
    let n = systeme_network::netdev::NetDevConfig::from_parsed(&parse_unit(
        "[NetDev]\nName=vlan42\nKind=vlan\n\n[VLAN]\nId=42\n",
    ));
    assert_eq!(
        netdev_create_argv(&n),
        Some(vec![
            "ip".to_string(),
            "link".to_string(),
            "add".to_string(),
            "vlan42".to_string(),
            "type".to_string(),
            "vlan".to_string(),
            "id".to_string(),
            "42".to_string(),
        ])
    );
}

#[test]
fn netdev_create_argv_missing_kind_or_name_none() {
    let no_kind = systeme_network::netdev::NetDevConfig::from_parsed(&parse_unit("[NetDev]\nName=x\n"));
    assert_eq!(netdev_create_argv(&no_kind), None);
    let no_name = systeme_network::netdev::NetDevConfig::from_parsed(&parse_unit("[NetDev]\nKind=dummy\n"));
    assert_eq!(netdev_create_argv(&no_name), None);
    let vlan_no_id =
        systeme_network::netdev::NetDevConfig::from_parsed(&parse_unit("[NetDev]\nName=v\nKind=vlan\n"));
    assert_eq!(netdev_create_argv(&vlan_no_id), None);
}

#[test]
fn netdev_create_argv_other_kind_preserved() {
    let n = systeme_network::netdev::NetDevConfig::from_parsed(&parse_unit(
        "[NetDev]\nName=bnd0\nKind=bond\n",
    ));
    let argv = netdev_create_argv(&n).unwrap();
    assert_eq!(argv.last().unwrap(), "bond");
}

// ---------- link_apply_argv ----------

#[test]
fn link_apply_argv_mtu_and_mac() {
    let l = LinkConfig::from_parsed(&parse_unit(
        "[Link]\nMTUBytes=1500\nMACAddress=aa:bb:cc:dd:ee:ff\n",
    ));
    let cmds = link_apply_argv(&l, "eth0");
    assert_eq!(cmds.len(), 2);
    assert_eq!(cmds[0], vec!["ip", "link", "set", "dev", "eth0", "mtu", "1500"]);
    assert_eq!(
        cmds[1],
        vec!["ip", "link", "set", "dev", "eth0", "address", "aa:bb:cc:dd:ee:ff"]
    );
}

#[test]
fn link_apply_argv_empty_when_no_settings() {
    let l = LinkConfig::from_parsed(&parse_unit("[Match]\nName=eth0\n"));
    assert!(link_apply_argv(&l, "eth0").is_empty());
}

// ---------- NetworkMatch 補充 ----------

#[test]
fn network_match_is_empty_helper() {
    let m = NetworkMatch::default();
    assert!(m.is_empty());
    let n = NetworkConfig::from_parsed(&parse_unit(""));
    assert!(n.match_.is_empty());
    assert_eq!(n.match_.names, Vec::<String>::new());
    // NetDevKind 其他類型相等性
    assert_eq!(NetDevKind::parse("wireguard"), NetDevKind::Wireguard);
}
