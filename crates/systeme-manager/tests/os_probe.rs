//! OsProbe — PathProbe 的真實文件系統實現測試。

use systeme_core::path::PathProbe;
use systeme_manager::os_probe::OsProbe;

#[test]
fn snapshot_existing_file() {
    let dir = tempfile::tempdir().unwrap();
    let f = dir.path().join("exists.txt");
    std::fs::write(&f, "data").unwrap();
    let s = OsProbe.snapshot(f.to_str().unwrap());
    assert!(s.exists);
    assert!(s.mtime.is_some());
    assert!(!s.dir_nonempty);
}

#[test]
fn snapshot_missing_file() {
    let dir = tempfile::tempdir().unwrap();
    let s = OsProbe.snapshot(dir.path().join("nope.txt").to_str().unwrap());
    assert!(!s.exists);
    assert_eq!(s.mtime, None);
    assert!(!s.dir_nonempty);
}

#[test]
fn snapshot_empty_directory() {
    let dir = tempfile::tempdir().unwrap();
    let s = OsProbe.snapshot(dir.path().to_str().unwrap());
    assert!(s.exists);
    assert!(!s.dir_nonempty);
}

#[test]
fn snapshot_nonempty_directory() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("child"), "x").unwrap();
    let s = OsProbe.snapshot(dir.path().to_str().unwrap());
    assert!(s.exists);
    assert!(s.dir_nonempty);
}

#[test]
fn mtime_changes_on_write() {
    let dir = tempfile::tempdir().unwrap();
    let f = dir.path().join("watched");
    std::fs::write(&f, "v1").unwrap();
    let s1 = OsProbe.snapshot(f.to_str().unwrap());
    std::thread::sleep(std::time::Duration::from_millis(1100));
    std::fs::write(&f, "v2").unwrap();
    let s2 = OsProbe.snapshot(f.to_str().unwrap());
    assert!(s1.mtime.is_some() && s2.mtime.is_some());
    assert_ne!(s1.mtime, s2.mtime);
}

#[test]
fn glob_matches_file_in_dir() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.log"), "x").unwrap();
    std::fs::write(dir.path().join("b.txt"), "y").unwrap();
    let pat = dir.path().join("*.log").to_str().unwrap().to_string();
    assert!(OsProbe.glob(&pat));
}

#[test]
fn glob_no_match() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.log"), "x").unwrap();
    let pat = dir.path().join("*.missing").to_str().unwrap().to_string();
    assert!(!OsProbe.glob(&pat));
}

#[test]
fn glob_missing_dir_no_match() {
    let dir = tempfile::tempdir().unwrap();
    let pat = dir.path().join("nodir").join("*.log").to_str().unwrap().to_string();
    assert!(!OsProbe.glob(&pat));
}

#[test]
fn glob_question_mark() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("ab"), "x").unwrap();
    let pat = dir.path().join("a?").to_str().unwrap().to_string();
    assert!(OsProbe.glob(&pat));
}
