//! socket 單元配置層測試 — [Socket] 段解析、Listen 分類、fd 傳遞環境構造。
//!
//! 純邏輯層: 不创建真實 socket, 只解析與分類。

use systeme_core::socket::{
    classify_listen, listen_fds_env, socket_service_target, ListenKind, ListenProto, SocketConfig,
};
use systeme_units::parser::parse_unit;

fn cfg(text: &str) -> SocketConfig {
    SocketConfig::from_parsed(&parse_unit(text))
}

#[test]
fn missing_socket_section_defaults() {
    let c = cfg("[Unit]\nDescription=x\n");
    assert!(c.listen.is_empty());
    assert!(!c.accept);
    assert_eq!(c.service, None);
    assert!(c.warnings.is_empty(), "no warnings expected: {:?}", c.warnings);
}

#[test]
fn listen_stream_port() {
    let c = cfg("[Socket]\nListenStream=8080\n");
    assert_eq!(c.listen.len(), 1);
    assert_eq!(c.listen[0].proto, ListenProto::Stream);
    assert_eq!(c.listen[0].kind, ListenKind::Port(8080));
}

#[test]
fn listen_stream_unix_path() {
    let c = cfg("[Socket]\nListenStream=/run/foo.sock\n");
    assert_eq!(c.listen[0].kind, ListenKind::Path("/run/foo.sock".to_string()));
}

#[test]
fn listen_stream_abstract() {
    let c = cfg("[Socket]\nListenStream=@foo\n");
    assert_eq!(c.listen[0].kind, ListenKind::Abstract("foo".to_string()));
}

#[test]
fn listen_stream_addr_port() {
    let c = cfg("[Socket]\nListenStream=192.168.1.10:8080\n");
    assert_eq!(
        c.listen[0].kind,
        ListenKind::AddrPort {
            addr: "192.168.1.10".to_string(),
            port: 8080
        }
    );
}

#[test]
fn listen_stream_ipv6_bracketed() {
    let c = cfg("[Socket]\nListenStream=[::1]:8080\n");
    assert_eq!(
        c.listen[0].kind,
        ListenKind::AddrPort {
            addr: "::1".to_string(),
            port: 8080
        }
    );
}

#[test]
fn multiple_listen_entries_order_preserved() {
    let c = cfg("[Socket]\nListenStream=8080\nListenStream=9090\nListenDatagram=53\n");
    assert_eq!(c.listen.len(), 3);
    assert_eq!(c.listen[0].kind, ListenKind::Port(8080));
    assert_eq!(c.listen[1].kind, ListenKind::Port(9090));
    assert_eq!(c.listen[2].proto, ListenProto::Datagram);
    assert_eq!(c.listen[2].kind, ListenKind::Port(53));
}

#[test]
fn listen_fifo_and_datagram() {
    let c = cfg("[Socket]\nListenFIFO=/run/f\nListenDatagram=/run/d.sock\n");
    assert_eq!(c.listen[0].proto, ListenProto::Fifo);
    assert_eq!(c.listen[0].kind, ListenKind::Path("/run/f".to_string()));
    assert_eq!(c.listen[1].proto, ListenProto::Datagram);
    assert_eq!(c.listen[1].kind, ListenKind::Path("/run/d.sock".to_string()));
}

#[test]
fn accept_flag() {
    assert!(cfg("[Socket]\nListenStream=1\nAccept=yes\n").accept);
    assert!(!cfg("[Socket]\nListenStream=1\nAccept=no\n").accept);
    // 壞值: 警告 + 保持默認 false
    let c = cfg("[Socket]\nListenStream=1\nAccept=maybe\n");
    assert!(!c.accept);
    assert_eq!(c.warnings.len(), 1);
}

#[test]
fn service_directive() {
    let c = cfg("[Socket]\nListenStream=1\nService=web.service\n");
    assert_eq!(c.service, Some("web.service".to_string()));
}

#[test]
fn empty_listen_value_clears_list() {
    // get_list 語義: 空值清空此前累積
    let c = cfg("[Socket]\nListenStream=8080\nListenStream=\n");
    assert!(c.listen.is_empty());
}

#[test]
fn backlog_default_and_parse() {
    assert_eq!(cfg("[Socket]\nListenStream=1\n").backlog, 128);
    assert_eq!(cfg("[Socket]\nListenStream=1\nBacklog=64\n").backlog, 64);
    // 壞值: 警告 + 保持默認
    let c = cfg("[Socket]\nListenStream=1\nBacklog=abc\n");
    assert_eq!(c.backlog, 128);
    assert_eq!(c.warnings.len(), 1);
}

#[test]
fn target_defaults_to_same_name_service() {
    let c = cfg("[Socket]\nListenStream=1\n");
    assert_eq!(socket_service_target(&c, "web.socket"), "web.service");
    let c2 = cfg("[Socket]\nListenStream=1\nService=other.service\n");
    assert_eq!(socket_service_target(&c2, "web.socket"), "other.service");
}

#[test]
fn listen_fds_env_construction() {
    let env = listen_fds_env(3);
    assert_eq!(env, vec![("LISTEN_FDS".to_string(), "3".to_string())]);
    assert_eq!(listen_fds_env(0).len(), 1);
}

#[test]
fn classify_listen_edge_cases() {
    // 純數字但超出 u16 → 保留原值給 bin 層處理
    assert_eq!(classify_listen("99999"), ListenKind::Network("99999".to_string()));
    // 主機名:端口 → AddrPort (systemd 允許解析主機名)
    assert_eq!(
        classify_listen("example.com:80"),
        ListenKind::AddrPort {
            addr: "example.com".to_string(),
            port: 80
        }
    );
    // 右段非端口 → 原值
    assert_eq!(
        classify_listen("a:notaport"),
        ListenKind::Network("a:notaport".to_string())
    );
    // 空 → Network 空
    assert_eq!(classify_listen(""), ListenKind::Network(String::new()));
}
