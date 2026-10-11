//! IPC 協議類型測試: serde 序列化往返、文字 JSON 兼容。

use systeme_core::ipc::{Request, Response, UnitInfo};

#[test]
fn request_start_roundtrip() {
    let r = Request::Start {
        unit: "nginx.service".into(),
    };
    let j = serde_json::to_string(&r).unwrap();
    let back: Request = serde_json::from_str(&j).unwrap();
    assert!(matches!(back, Request::Start { unit } if unit == "nginx.service"));
}

#[test]
fn request_list_and_daemon_reload_roundtrip() {
    let list = serde_json::to_string(&Request::ListUnits).unwrap();
    assert!(matches!(serde_json::from_str::<Request>(&list).unwrap(), Request::ListUnits));
    let rl = serde_json::to_string(&Request::DaemonReload).unwrap();
    assert!(matches!(serde_json::from_str::<Request>(&rl).unwrap(), Request::DaemonReload));
}

#[test]
fn request_parses_from_literal_json() {
    // 字面 JSON (協議文檔示例形式) 必須可直接反序列化
    let r: Request = serde_json::from_str(r#"{"Start":{"unit":"a.service"}}"#).unwrap();
    assert!(matches!(r, Request::Start { unit } if unit == "a.service"));
    // 純 unit variant (無字段) 以裸字串形式表示
    let r: Request = serde_json::from_str::<Request>(r#""DaemonReload""#).unwrap();
    assert!(matches!(r, Request::DaemonReload));
}

#[test]
fn response_units_roundtrip() {
    let info = UnitInfo {
        name: "a.service".into(),
        description: "A service".into(),
        load_state: "loaded".into(),
        active_state: "active".into(),
        main_pid: Some(1234),
    };
    let resp = Response::Units(vec![info]);
    let j = serde_json::to_string(&resp).unwrap();
    let back: Response = serde_json::from_str(&j).unwrap();
    match back {
        Response::Units(v) => {
            assert_eq!(v.len(), 1);
            assert_eq!(v[0].name, "a.service");
            assert_eq!(v[0].active_state, "active");
            assert_eq!(v[0].main_pid, Some(1234));
        }
        _ => panic!("expected Units"),
    }
}

#[test]
fn response_error_roundtrip() {
    let resp = Response::Error {
        message: "unit not found".into(),
    };
    let j = serde_json::to_string(&resp).unwrap();
    let back: Response = serde_json::from_str(&j).unwrap();
    assert!(matches!(back, Response::Error { message } if message == "unit not found"));
}

#[test]
fn response_active_and_ok_roundtrip() {
    let a = serde_json::to_string(&Response::Active(true)).unwrap();
    assert!(matches!(serde_json::from_str::<Response>(&a).unwrap(), Response::Active(true)));
    let o = serde_json::to_string(&Response::Ok).unwrap();
    assert!(matches!(serde_json::from_str::<Response>(&o).unwrap(), Response::Ok));
}

#[test]
fn response_cat_roundtrip() {
    let resp = Response::Cat {
        name: "a.service".into(),
        text: "[Service]\nExecStart=/bin/a\n".into(),
    };
    let j = serde_json::to_string(&resp).unwrap();
    let back: Response = serde_json::from_str(&j).unwrap();
    assert!(matches!(back, Response::Cat { name, .. } if name == "a.service"));
}
