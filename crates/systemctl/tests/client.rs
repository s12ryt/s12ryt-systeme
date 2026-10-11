//! systemctl client 執行層測試: 請求映射 + MockTransport + 輸出格式 + exit code (TDD RED → GREEN)
use std::cell::RefCell;

use systemctl::cli::{parse_args, Command};
use systemctl::client::{run_command, IpcTransport};
use systeme_core::ipc::{Request, Response, UnitInfo};

/// 記錄請求並回放預置響應的 mock 傳輸層
struct MockTransport {
    responses: RefCell<Vec<Response>>,
    seen: RefCell<Vec<Request>>,
}

impl MockTransport {
    fn new(responses: Vec<Response>) -> Self {
        Self {
            responses: RefCell::new(responses),
            seen: RefCell::new(Vec::new()),
        }
    }
}

impl IpcTransport for MockTransport {
    fn request(&mut self, req: &Request) -> Result<Response, String> {
        self.seen.borrow_mut().push(req.clone());
        self.responses
            .borrow_mut()
            .pop()
            .ok_or_else(|| "no more canned responses".to_string())
    }
}

fn run(argv: &[&str], responses: Vec<Response>) -> (String, i32) {
    let cmd = parse_args(&argv.iter().map(|s| s.to_string()).collect::<Vec<_>>()).unwrap();
    let mut t = MockTransport::new(responses);
    
    run_command(&cmd, &mut t)
}

fn info(name: &str, desc: &str, load: &str, active: &str, pid: Option<i32>) -> UnitInfo {
    UnitInfo {
        name: name.to_string(),
        description: desc.to_string(),
        load_state: load.to_string(),
        active_state: active.to_string(),
        main_pid: pid,
    }
}

#[test]
fn start_ok_is_silent_zero() {
    let (out, code) = run(&["start", "a.service"], vec![Response::Ok]);
    assert_eq!(out, "");
    assert_eq!(code, 0);
}

#[test]
fn status_unit_prints_block() {
    let resp = Response::UnitStatus(info(
        "nginx.service",
        "A high performance web server",
        "loaded",
        "active",
        Some(4242),
    ));
    let (out, code) = run(&["status", "nginx.service"], vec![resp]);
    assert_eq!(code, 0);
    assert!(out.contains("nginx.service"), "output: {out}");
    assert!(out.contains("A high performance web server"));
    assert!(out.contains("Loaded: loaded"));
    assert!(out.contains("Active: active"));
    assert!(out.contains("Main PID: 4242"));
}

#[test]
fn status_masked_shows_masked_load() {
    let resp = Response::UnitStatus(info("evil.service", "n/a", "masked", "inactive", None));
    let (out, _) = run(&["status", "evil.service"], vec![resp]);
    assert!(out.contains("Loaded: masked"), "output: {out}");
    assert!(!out.contains("Main PID"), "no Main PID line when None");
}

#[test]
fn status_without_unit_lists_table() {
    let resp = Response::Units(vec![
        info("a.service", "First", "loaded", "active", None),
        info("b.timer", "Second", "loaded", "inactive", None),
    ]);
    let (out, code) = run(&["status"], vec![resp]);
    assert_eq!(code, 0);
    assert!(out.contains("UNIT"), "header row missing: {out}");
    assert!(out.contains("a.service"));
    assert!(out.contains("b.timer"));
    assert!(out.contains("First"));
    assert!(out.contains("Second"));
}

#[test]
fn list_units_prints_table() {
    let resp = Response::Units(vec![
        info("a.service", "First", "loaded", "active", None),
        info("b.timer", "Second", "loaded", "inactive", None),
    ]);
    let (out, code) = run(&["list-units"], vec![resp]);
    assert_eq!(code, 0);
    assert!(out.contains("UNIT"));
    assert!(out.contains("a.service"));
    assert!(out.contains("loaded"));
    assert!(out.contains("active"));
    assert!(out.contains("inactive"));
}

#[test]
fn is_active_true_exits_zero() {
    let (out, code) = run(&["is-active", "a.service"], vec![Response::Active(true)]);
    assert_eq!(out.trim(), "active");
    assert_eq!(code, 0);
}

#[test]
fn is_active_false_exits_three() {
    // systemd 語義: is-active 對非 active 返回 exit 3
    let (out, code) = run(&["is-active", "a.service"], vec![Response::Active(false)]);
    assert_eq!(out.trim(), "inactive");
    assert_eq!(code, 3);
}

#[test]
fn cat_prints_text() {
    let resp = Response::Cat {
        name: "a.service".into(),
        text: "# /etc/systemd/system/a.service\n[Unit]\nDescription=Foo\n".into(),
    };
    let (out, code) = run(&["cat", "a.service"], vec![resp]);
    assert_eq!(code, 0);
    assert!(out.contains("[Unit]"));
    assert!(out.contains("Description=Foo"));
}

#[test]
fn error_response_exits_one() {
    let (out, code) = run(
        &["start", "missing.service"],
        vec![Response::Error { message: "Unit missing.service not found.".into() }],
    );
    assert_eq!(code, 1);
    assert!(out.contains("not found"), "output: {out}");
}

#[test]
fn not_implemented_exits_one() {
    let (out, code) = run(
        &["enable", "a.service"],
        vec![Response::NotImplemented { request: "Enable".into() }],
    );
    assert_eq!(code, 1);
    assert!(out.contains("not implemented"), "output: {out}");
}

#[test]
fn transport_failure_exits_one() {
    let cmd = Command::Start { unit: "a.service".into() };
    let mut t = FailingTransport;
    let (out, code) = run_command(&cmd, &mut t);
    assert_eq!(code, 1);
    assert!(out.contains("connect") || out.contains("Failed"), "output: {out}");
}

struct FailingTransport;
impl IpcTransport for FailingTransport {
    fn request(&mut self, _req: &Request) -> Result<Response, String> {
        Err("connection refused".into())
    }
}

#[test]
fn help_and_version_do_not_touch_transport() {
    let mut t = MockTransport::new(vec![]);
    let (out, code) = run_command(&Command::Help, &mut t);
    assert_eq!(code, 0);
    assert!(out.contains("start"), "help text lists commands: {out}");

    let (out, code) = run_command(&Command::Version, &mut t);
    assert_eq!(code, 0);
    assert!(out.contains("s12ryt-systeme"), "version banner: {out}");
    assert!(t.seen.borrow().is_empty(), "no IPC request should be sent");
}

#[test]
fn sends_expected_requests() {
    let mut t = MockTransport::new(vec![
        Response::Ok, // restart
        Response::Ok, // daemon-reload
        Response::Ok, // enable
        Response::Ok, // mask
        Response::Ok, // unmask
        Response::Ok, // disable
    ]);
    // 注意 MockTransport 是 pop (LIFO),按發送順序倒序填充
    for argv in [
        vec!["disable", "a.service"],
        vec!["unmask", "a.service"],
        vec!["mask", "a.service"],
        vec!["enable", "a.service"],
        vec!["daemon-reload"],
        vec!["restart", "a.service"],
    ] {
        let cmd = parse_args(&argv.iter().map(|s| s.to_string()).collect::<Vec<_>>()).unwrap();
        let _ = run_command(&cmd, &mut t);
    }
    let seen = t.seen.borrow();
    assert!(matches!(&seen[0], Request::Disable { unit } if unit == "a.service"));
    assert!(matches!(&seen[1], Request::Unmask { unit } if unit == "a.service"));
    assert!(matches!(&seen[2], Request::Mask { unit } if unit == "a.service"));
    assert!(matches!(&seen[3], Request::Enable { unit } if unit == "a.service"));
    assert!(matches!(&seen[4], Request::DaemonReload));
    assert!(matches!(&seen[5], Request::Restart { unit } if unit == "a.service"));
}
