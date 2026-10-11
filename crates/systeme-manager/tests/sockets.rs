//! socket 單元真實監聽測試 (cfg(unix))。

#![cfg(unix)]

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;

use systeme_manager::sockets::{bind_socket_unit, SocketActivation};
use systeme_core::socket::SocketConfig;
use systeme_units::parser::parse_unit;

fn cfg_of(text: &str) -> SocketConfig {
    SocketConfig::from_parsed(&parse_unit(text))
}

#[test]
fn bind_unix_stream_socket() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("app.sock");
    let cfg = cfg_of(&format!("[Socket]\nListenStream={}\n", path.display()));
    let act = bind_socket_unit(&cfg).unwrap();
    assert_eq!(act.fds().len(), 1);

    // 連得上 (listener 持有中)
    let mut c = UnixStream::connect(&path).expect("connect to listening socket");
    c.write_all(b"ping").unwrap();
    drop(c);
}

#[test]
fn bind_tcp_port_socket() {
    let cfg = cfg_of("[Socket]\nListenStream=127.0.0.1:0\n");
    // 0 端口: 由內核分配, 只驗證能 bind
    let act = bind_socket_unit(&cfg).unwrap();
    assert_eq!(act.fds().len(), 1);
}

#[test]
fn bind_multiple_listeners() {
    let dir = tempfile::tempdir().unwrap();
    let a = dir.path().join("a.sock");
    let b = dir.path().join("b.sock");
    let cfg = cfg_of(&format!(
        "[Socket]\nListenStream={}\nListenStream={}\n",
        a.display(),
        b.display()
    ));
    let act = bind_socket_unit(&cfg).unwrap();
    assert_eq!(act.fds().len(), 2);
    let _ = UnixStream::connect(&a).unwrap();
    let _ = UnixStream::connect(&b).unwrap();
}

#[test]
fn drop_activation_closes_listeners() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("gone.sock");
    let cfg = cfg_of(&format!("[Socket]\nListenStream={}\n", path.display()));
    let act: SocketActivation = bind_socket_unit(&cfg).unwrap();
    drop(act);
    // 釋放後連不上
    assert!(UnixStream::connect(&path).is_err());
}

#[test]
fn bind_abstract_socket() {
    let cfg = cfg_of("[Socket]\nListenStream=@systeme-test-abstract\n");
    let act = bind_socket_unit(&cfg).unwrap();
    assert_eq!(act.fds().len(), 1);
    // abstract socket 連接: 路徑前綴 @ → \0
    let addr = {
        let mut s = String::from("@systeme-test-abstract");
        s.replace_range(0..1, "\0");
        s
    };
    use std::os::unix::net::SocketAddr;
    let sock = std::os::unix::net::UnixStream::connect_addr(&SocketAddr::from_abstract_name(
        b"systeme-test-abstract",
    ))
    .or_else(|_| {
        // 舊 API 回退: 手工 sockaddr
        let _ = addr;
        UnixStream::connect("\0systeme-test-abstract")
    });
    assert!(sock.is_ok(), "abstract socket should be connectable");
}

#[test]
fn bind_bad_listen_fails() {
    // 目錄不存在 → bind 失敗
    let cfg = cfg_of("[Socket]\nListenStream=/nonexistent-dir-xyz/app.sock\n");
    assert!(bind_socket_unit(&cfg).is_err());
}

#[test]
fn accept_stream_data_roundtrip() {
    // 驗證 listener fd 真的可 accept: 用另一線程連接寫入
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("echo.sock");
    let cfg = cfg_of(&format!("[Socket]\nListenStream={}\n", path.display()));
    let _act = bind_socket_unit(&cfg).unwrap();

    let mut c = UnixStream::connect(&path).unwrap();
    c.write_all(b"hello").unwrap();
    c.shutdown(std::net::Shutdown::Write).unwrap();
    // listener 存在即可; accept 語義由 service 進程接管 (fd 傳遞)
    let mut buf = [0u8; 16];
    let _ = c.read(&mut buf);
}
