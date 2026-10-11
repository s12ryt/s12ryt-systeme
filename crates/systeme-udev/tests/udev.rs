//! systeme-udev: udevd 基礎 — uevent 解析/資料庫/trigger 計劃測試。

use systeme_udev::{parse_uevent, UdevDb};

#[test]
fn parse_uevent_fields() {
    let text = "ACTION=add\0DEVPATH=/devices/virtual/net/lo\0SUBSYSTEM=net\0DEVNAME=lo\0\0";
    let ev = parse_uevent(text).expect("parses");
    assert_eq!(ev.action.as_deref(), Some("add"));
    assert_eq!(ev.devpath.as_deref(), Some("/devices/virtual/net/lo"));
    assert_eq!(ev.subsystem.as_deref(), Some("net"));
    assert_eq!(ev.devname.as_deref(), Some("lo"));
}

#[test]
fn parse_uevent_minimal() {
    let ev = parse_uevent("ACTION=remove\0DEVPATH=/devices/x\0\0").expect("parses");
    assert_eq!(ev.action.as_deref(), Some("remove"));
    assert_eq!(ev.devname, None);
    assert_eq!(ev.subsystem, None);
}

#[test]
fn parse_uevent_rejects_no_devpath() {
    assert!(parse_uevent("ACTION=add\0\0").is_none());
    assert!(parse_uevent("").is_none());
    assert!(parse_uevent("garbage no nulls").is_none());
}

#[test]
fn db_apply_add_and_remove() {
    let mut db = UdevDb::new();
    let ev = parse_uevent("ACTION=add\0DEVPATH=/devices/net/eth0\0SUBSYSTEM=net\0DEVNAME=eth0\0\0").unwrap();
    db.apply(&ev);
    assert_eq!(db.get("/devices/net/eth0").unwrap().devname.as_deref(), Some("eth0"));
    assert_eq!(db.len(), 1);

    let rm = parse_uevent("ACTION=remove\0DEVPATH=/devices/net/eth0\0\0").unwrap();
    db.apply(&rm);
    assert!(db.get("/devices/net/eth0").is_none());
    assert_eq!(db.len(), 0);
}

#[test]
fn db_change_updates() {
    let mut db = UdevDb::new();
    let add = parse_uevent("ACTION=add\0DEVPATH=/d/a\0SUBSYSTEM=block\0DEVNAME=sda\0\0").unwrap();
    db.apply(&add);
    let chg = parse_uevent("ACTION=change\0DEVPATH=/d/a\0SUBSYSTEM=block\0DEVNAME=sda1\0\0").unwrap();
    db.apply(&chg);
    assert_eq!(db.get("/d/a").unwrap().devname.as_deref(), Some("sda1"));
    assert_eq!(db.len(), 1);
}

#[test]
fn db_remove_unknown_ignored() {
    let mut db = UdevDb::new();
    let rm = parse_uevent("ACTION=remove\0DEVPATH=/nope\0\0").unwrap();
    db.apply(&rm); // 不 panic
    assert_eq!(db.len(), 0);
}

#[test]
fn db_by_subsystem() {
    let mut db = UdevDb::new();
    for (devpath, devname, subsys) in [
        ("/d/eth0", "eth0", "net"),
        ("/d/lo", "lo", "net"),
        ("/d/sda", "sda", "block"),
    ] {
        let text = format!("ACTION=add\0DEVPATH={devpath}\0SUBSYSTEM={subsys}\0DEVNAME={devname}\0\0");
        db.apply(&parse_uevent(&text).unwrap());
    }
    let net = db.by_subsystem("net");
    assert_eq!(net.len(), 2);
    assert!(net.iter().any(|d| d.devname.as_deref() == Some("eth0")));
}

#[test]
fn db_unknown_action_stored_as_is() {
    let mut db = UdevDb::new();
    let ev = parse_uevent("ACTION=bind\0DEVPATH=/d/x\0DEVNAME=xx\0\0").unwrap();
    db.apply(&ev); // 非標準 action 仍入庫 (add/change 語義)
    assert_eq!(db.len(), 1);
}

#[test]
fn scan_sysfs_collects_uevent_texts() {
    // 用 tempdir 模擬 /sys: 兩個設備目錄各含 uevent 文件, 一個無 uevent
    let tmp = tempfile::tempdir().unwrap();
    let d1 = tmp.path().join("devices").join("a");
    let d2 = tmp.path().join("devices").join("b");
    std::fs::create_dir_all(&d1).unwrap();
    std::fs::create_dir_all(&d2).unwrap();
    std::fs::create_dir_all(tmp.path().join("devices").join("empty")).unwrap();
    std::fs::write(d1.join("uevent"), "ACTION=add\0DEVPATH=/devices/a\0DEVNAME=aa\0\0").unwrap();
    std::fs::write(d2.join("uevent"), "ACTION=add\0DEVPATH=/devices/b\0DEVNAME=bb\0\0").unwrap();

    let texts = systeme_udev::scan_sysfs(tmp.path());
    assert_eq!(texts.len(), 2);
    // 全部可解析
    for t in &texts {
        assert!(parse_uevent(t).is_some());
    }
}

#[test]
fn trigger_plan_replays_db() {
    let mut db = UdevDb::new();
    for name in ["eth0", "eth1"] {
        let text = format!("ACTION=add\0DEVPATH=/devices/{name}\0SUBSYSTEM=net\0DEVNAME={name}\0\0");
        db.apply(&parse_uevent(&text).unwrap());
    }
    let plan = db.trigger_plan("add");
    assert_eq!(plan.len(), 2);
    assert!(plan.iter().all(|t| t.contains("ACTION=add")));
}
