//! journalctl CLI 參數解析測試 (RED 先行)。

use journalctl::cli::{parse_args, Query};
use systeme_journal::format::OutputFormat;

fn args(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

#[test]
fn default_query() {
    let q = parse_args(&args(&[])).unwrap();
    assert!(q.units.is_empty());
    assert_eq!(q.priority_max, None);
    assert_eq!(q.since, None);
    assert_eq!(q.until, None);
    assert_eq!(q.format, OutputFormat::Short);
    assert_eq!(q.lines, None);
    assert!(!q.reverse);
    assert!(!q.follow);
    assert!(q.patterns.is_empty());
}

#[test]
fn unit_flag_space_form() {
    let q = parse_args(&args(&["-u", "nginx.service"])).unwrap();
    assert_eq!(q.units, vec!["nginx.service"]);
}

#[test]
fn unit_flag_equals_form() {
    let q = parse_args(&args(&["--unit=nginx.service"])).unwrap();
    assert_eq!(q.units, vec!["nginx.service"]);
}

#[test]
fn unit_flag_repeated() {
    let q = parse_args(&args(&["-u", "a.service", "-u", "b.service", "--unit=c.service"]))
        .unwrap();
    assert_eq!(q.units, vec!["a.service", "b.service", "c.service"]);
}

#[test]
fn priority_numeric() {
    let q = parse_args(&args(&["-p", "3"])).unwrap();
    assert_eq!(q.priority_max, Some(3));
}

#[test]
fn priority_by_name() {
    let q = parse_args(&args(&["--priority", "err"])).unwrap();
    assert_eq!(q.priority_max, Some(3));
}

#[test]
fn priority_bad_value_errors() {
    assert!(parse_args(&args(&["-p", "banana"])).is_err());
    assert!(parse_args(&args(&["-p", "99"])).is_err());
}

#[test]
fn priority_missing_value_errors() {
    assert!(parse_args(&args(&["-p"])).is_err());
}

#[test]
fn since_parses_datetime() {
    // 2026-10-11 00:00:00 UTC
    let q = parse_args(&args(&["--since", "2026-10-11 00:00:00"])).unwrap();
    assert_eq!(q.since, Some(1_791_676_800_000_000));
}

#[test]
fn until_parses_iso_datetime() {
    // 2026-10-11 10:00:00 UTC
    let q = parse_args(&args(&["--until", "2026-10-11T10:00:00"])).unwrap();
    assert_eq!(q.until, Some(1_791_712_800_000_000));
}

#[test]
fn since_bad_value_errors() {
    assert!(parse_args(&args(&["--since", "not-a-date"])).is_err());
}

#[test]
fn output_format_flag() {
    let q = parse_args(&args(&["-o", "json"])).unwrap();
    assert_eq!(q.format, OutputFormat::Json);
    let q = parse_args(&args(&["--output=cat"])).unwrap();
    assert_eq!(q.format, OutputFormat::Cat);
}

#[test]
fn output_format_bad_value_errors() {
    assert!(parse_args(&args(&["-o", "xml"])).is_err());
}

#[test]
fn lines_flag() {
    let q = parse_args(&args(&["-n", "20"])).unwrap();
    assert_eq!(q.lines, Some(20));
    let q = parse_args(&args(&["--lines=5"])).unwrap();
    assert_eq!(q.lines, Some(5));
}

#[test]
fn lines_bad_value_errors() {
    assert!(parse_args(&args(&["-n", "abc"])).is_err());
}

#[test]
fn reverse_flag() {
    let q = parse_args(&args(&["-r"])).unwrap();
    assert!(q.reverse);
}

#[test]
fn follow_flag() {
    let q = parse_args(&args(&["-f"])).unwrap();
    assert!(q.follow);
}

#[test]
fn no_pager_ignored() {
    let q = parse_args(&args(&["--no-pager", "-u", "x.service"])).unwrap();
    assert_eq!(q.units, vec!["x.service"]);
}

#[test]
fn positional_args_become_patterns() {
    let q = parse_args(&args(&["disk", "failing"])).unwrap();
    assert_eq!(q.patterns, vec!["disk", "failing"]);
}

#[test]
fn combined_flags() {
    let q = parse_args(&args(&[
        "-u",
        "nginx.service",
        "-p",
        "warning",
        "-n",
        "10",
        "-r",
        "-o",
        "json",
    ]))
    .unwrap();
    assert_eq!(q.units, vec!["nginx.service"]);
    assert_eq!(q.priority_max, Some(4));
    assert_eq!(q.lines, Some(10));
    assert!(q.reverse);
    assert_eq!(q.format, OutputFormat::Json);
}

#[test]
fn query_defaults_match_struct_default() {
    // Query 應可由 Default 建構並與空參數解析一致
    let q = parse_args(&args(&[])).unwrap();
    let d = Query::default();
    assert_eq!(q.units, d.units);
    assert_eq!(q.format, d.format);
    assert_eq!(q.reverse, d.reverse);
}
