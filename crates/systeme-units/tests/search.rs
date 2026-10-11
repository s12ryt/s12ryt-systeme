//! 目錄搜索 / mask / alias / override 疊加測試

use systeme_units::search::*;

fn sp(layers: &[&str]) -> SearchPaths {
    SearchPaths {
        dirs: layers.iter().map(PathBuf::from).collect(),
    }
}

#[test]
fn system_paths_low_to_high_order() {
    let p = system_search_paths();
    assert_eq!(p.dirs.first().unwrap(), &PathBuf::from("/usr/lib/systemd/system"));
    assert_eq!(p.dirs.last().unwrap(), &PathBuf::from("/etc/systemd/system"));
    // /run 必須高於 /usr/lib
    let run = p.dirs.iter().position(|d| d.ends_with("run/systemd/system")).unwrap();
    let usr = p.dirs.iter().position(|d| d.ends_with("usr/lib/systemd/system")).unwrap();
    assert!(run > usr, "/run must outrank /usr/lib: {:?}", p.dirs);
}

#[test]
fn user_paths_include_config() {
    let p = user_search_paths(Some(Path::new("/home/u/.config")), Some(Path::new("/home/u")));
    assert_eq!(p.dirs.last().unwrap(), &PathBuf::from("/home/u/.config/systemd/user"));
    // 未提供 config 時 fallback 到 home/.config
    let p2 = user_search_paths(None, Some(Path::new("/home/u")));
    assert!(p2.dirs.contains(&PathBuf::from("/home/u/.config/systemd/user")));
}

#[test]
fn find_prefers_higher_priority_layer() {
    let mut fs = MemFs::new();
    fs.file("/usr/lib/systemd/system/demo.service", "[Unit]\nDescription=from usr-lib\n");
    fs.file("/etc/systemd/system/demo.service", "[Unit]\nDescription=from etc\n");
    let paths = sp(&["/usr/lib/systemd/system", "/etc/systemd/system"]);
    let f = find_unit("demo.service", &paths, &fs).unwrap();
    assert_eq!(f.path, PathBuf::from("/etc/systemd/system/demo.service"));
    assert!(!f.masked);
}

#[test]
fn find_missing_returns_none() {
    let fs = MemFs::new();
    let paths = sp(&["/etc/systemd/system"]);
    assert!(find_unit("nope.service", &paths, &fs).is_none());
}

#[test]
fn symlink_to_devnull_is_masked() {
    let mut fs = MemFs::new();
    fs.symlink("/etc/systemd/system/demo.service", "/dev/null");
    let paths = sp(&["/etc/systemd/system"]);
    let f = find_unit("demo.service", &paths, &fs).unwrap();
    assert!(f.masked, "link to /dev/null must be masked");
}

#[test]
fn symlink_alias_resolves_to_target() {
    let mut fs = MemFs::new();
    fs.file("/usr/lib/systemd/system/real.service", "[Unit]\nDescription=real\n");
    fs.symlink("/etc/systemd/system/alias.service", "real.service");
    let paths = sp(&["/usr/lib/systemd/system", "/etc/systemd/system"]);
    let f = find_unit("alias.service", &paths, &fs).unwrap();
    assert_eq!(f.real_name, "real.service");
    assert!(!f.masked);
}

#[test]
fn alias_loop_terminates() {
    let mut fs = MemFs::new();
    fs.symlink("/etc/systemd/system/a.service", "b.service");
    fs.symlink("/etc/systemd/system/b.service", "a.service");
    let paths = sp(&["/etc/systemd/system"]);
    // 不允許無限遞迴: 返回某個實體結果
    let f = find_unit("a.service", &paths, &fs);
    assert!(f.is_some(), "loop must terminate");
}

#[test]
fn override_dir_layers_apply() {
    let mut fs = MemFs::new();
    fs.file("/usr/lib/systemd/system/demo.service", "[Unit]\nDescription=base\n\n[Service]\nEnvironment=A=1\nExecStart=/bin/true\n");
    // /run 層 override: 追加環境
    fs.file("/run/systemd/system/demo.service.d/10-run.conf", "[Service]\nEnvironment=B=2\n");
    // /etc 層 override: 覆蓋描述 + 清空環境再設
    fs.file("/etc/systemd/system/demo.service.d/20-etc.conf", "[Unit]\nDescription=override\n[Service]\nEnvironment=\nEnvironment=C=3\n");
    let paths = sp(&["/usr/lib/systemd/system", "/run/systemd/system", "/etc/systemd/system"]);
    match load_unit("demo.service", &paths, &fs).unwrap() {
        Loaded::Unit(_f, p) => {
            assert_eq!(p.section("Unit").unwrap().get("Description"), Some("override"));
            let svc = p.section("Service").unwrap();
            assert_eq!(svc.get_list("Environment"), vec!["C=3"]);
            assert_eq!(svc.get("ExecStart"), Some("/bin/true"));
        }
        other => panic!("expected Unit, got {other:?}"),
    }
}

#[test]
fn load_masked_returns_masked() {
    let mut fs = MemFs::new();
    fs.symlink("/etc/systemd/system/demo.service", "/dev/null");
    let paths = sp(&["/etc/systemd/system"]);
    assert!(matches!(
        load_unit("demo.service", &paths, &fs),
        Some(Loaded::Masked(_))
    ));
}

#[test]
fn load_via_alias_finds_override_of_real_name() {
    let mut fs = MemFs::new();
    fs.file("/usr/lib/systemd/system/real.service", "[Unit]\nDescription=base\n");
    fs.symlink("/etc/systemd/system/alias.service", "real.service");
    fs.file("/etc/systemd/system/real.service.d/x.conf", "[Unit]\nDescription=patched\n");
    let paths = sp(&["/usr/lib/systemd/system", "/etc/systemd/system"]);
    match load_unit("alias.service", &paths, &fs).unwrap() {
        Loaded::Unit(f, p) => {
            assert_eq!(f.real_name, "real.service");
            assert_eq!(p.section("Unit").unwrap().get("Description"), Some("patched"));
        }
        other => panic!("expected Unit, got {other:?}"),
    }
}

#[test]
fn osfs_real_directory_roundtrip() {
    // 真實 FS 冒煙 (不依賴 symlink 權限, Windows 可跑)
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("systemd/system");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("hello.service"), "[Unit]\nDescription=hi\n").unwrap();
    let paths = SearchPaths { dirs: vec![dir] };
    match load_unit("hello.service", &paths, &OsFs).unwrap() {
        Loaded::Unit(f, p) => {
            assert_eq!(f.real_name, "hello.service");
            assert_eq!(p.section("Unit").unwrap().get("Description"), Some("hi"));
        }
        other => panic!("expected Unit, got {other:?}"),
    }
}

use std::fs;
use std::path::{Path, PathBuf};
