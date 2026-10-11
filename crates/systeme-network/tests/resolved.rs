//! resolved 測試: resolv.conf 解析/渲染、stub 配置、上游 DNS 聚合

use systeme_network::network::NetworkConfig;
use systeme_network::resolved::{
    collect_dns, parse_resolv_conf, render_resolv_conf, stub_resolv_conf, ResolvConf,
};

fn cfg_with_dns(dns: &[&str]) -> NetworkConfig {
    NetworkConfig {
        dns: dns.iter().map(|s| s.to_string()).collect(),
        ..Default::default()
    }
}

#[test]
fn parse_nameservers_multiple() {
    let rc = parse_resolv_conf("nameserver 8.8.8.8\nnameserver 1.1.1.1\n");
    assert_eq!(rc.nameservers, vec!["8.8.8.8", "1.1.1.1"]);
}

#[test]
fn parse_search_and_options() {
    let rc = parse_resolv_conf("search example.com local\noptions timeout:2 attempts:3\n");
    assert_eq!(rc.search, vec!["example.com", "local"]);
    assert_eq!(rc.options, vec!["timeout:2", "attempts:3"]);
}

#[test]
fn parse_ignores_comments_blank_and_legacy() {
    let rc = parse_resolv_conf("# comment\n\n; legacy\nnameserver 9.9.9.9\n");
    assert_eq!(rc.nameservers, vec!["9.9.9.9"]);
    assert!(rc.search.is_empty());
}

#[test]
fn parse_empty_text_all_empty() {
    let rc = parse_resolv_conf("");
    assert!(rc.nameservers.is_empty());
    assert!(rc.search.is_empty());
    assert!(rc.options.is_empty());
}

#[test]
fn parse_ipv6_nameserver() {
    let rc = parse_resolv_conf("nameserver 2001:4860:4860::8888\n");
    assert_eq!(rc.nameservers, vec!["2001:4860:4860::8888"]);
}

#[test]
fn parse_domain_alias_search() {
    let rc = parse_resolv_conf("domain example.com\n");
    assert_eq!(rc.search, vec!["example.com"]);
}

#[test]
fn stub_defaults_127_0_0_53() {
    let rc = stub_resolv_conf();
    assert_eq!(rc.nameservers, vec!["127.0.0.53"]);
}

#[test]
fn render_stub_contains_managed_header() {
    let text = render_resolv_conf(&stub_resolv_conf());
    assert!(text.contains("systemd-resolved"));
    assert!(text.contains("nameserver 127.0.0.53"));
}

#[test]
fn render_roundtrip_parse() {
    let rc = ResolvConf {
        nameservers: vec!["10.0.0.1".into(), "10.0.0.2".into()],
        search: vec!["a.example".into(), "b.example".into()],
        options: vec!["timeout:1".into(), "attempts:2".into()],
    };
    assert_eq!(parse_resolv_conf(&render_resolv_conf(&rc)), rc);
}

#[test]
fn collect_dns_dedup_preserves_order() {
    let out = collect_dns(&[
        cfg_with_dns(&["8.8.8.8", "1.1.1.1"]),
        cfg_with_dns(&["1.1.1.1", "9.9.9.9"]),
    ]);
    assert_eq!(out, vec!["8.8.8.8", "1.1.1.1", "9.9.9.9"]);
}

#[test]
fn collect_dns_empty_when_no_networks() {
    assert!(collect_dns(&[]).is_empty());
    assert!(collect_dns(&[NetworkConfig::default()]).is_empty());
}
