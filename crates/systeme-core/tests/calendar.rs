//! Calendar 事件解析器測試 (systemd.time(7) 語法子集)。
//!
//! 覆蓋: 特殊值 (daily/hourly/weekly/...)、字段語法 (`*`/值/`a..b`/`a..b/n`/`*/n`/`a/n`/列表)、
//! 月/星期名稱、大小寫、越界拒絕、缺省補值 (時間補 0、日期補 1)、UTC 後綴容忍、
//! 以及 next_elapse 下一觸發時刻計算。

use chrono::NaiveDateTime;
use systeme_core::calendar::{next_elapse, parse_calendar, CalendarError};

fn dt(s: &str) -> NaiveDateTime {
    NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S").unwrap()
}

// ---------- 特殊值 ----------

#[test]
fn special_daily() {
    let c = parse_calendar("daily").unwrap();
    assert_eq!(c.dow, None);
    assert_eq!(c.year, None);
    assert_eq!(c.month, None);
    assert_eq!(c.day, None);
    assert_eq!(c.hour, Some(vec![0]));
    assert_eq!(c.minute, Some(vec![0]));
    assert_eq!(c.second, Some(vec![0]));
}

#[test]
fn special_hourly() {
    let c = parse_calendar("hourly").unwrap();
    assert_eq!(c.hour, None); // *:00:00 → 小時任意
    assert_eq!(c.minute, Some(vec![0]));
    assert_eq!(c.second, Some(vec![0]));
}

#[test]
fn special_weekly() {
    let c = parse_calendar("weekly").unwrap();
    assert_eq!(c.dow, Some(vec![0])); // 0 = Monday (內部 0=Mon..6=Sun)
    assert_eq!(c.hour, Some(vec![0]));
    assert_eq!(c.minute, Some(vec![0]));
    assert_eq!(c.second, Some(vec![0]));
}

#[test]
fn special_monthly() {
    let c = parse_calendar("monthly").unwrap();
    assert_eq!(c.day, Some(vec![1]));
    assert_eq!(c.hour, Some(vec![0]));
    assert_eq!(c.minute, Some(vec![0]));
    assert_eq!(c.second, Some(vec![0]));
}

#[test]
fn special_yearly_and_annually() {
    for expr in ["yearly", "annually"] {
        let c = parse_calendar(expr).unwrap();
        assert_eq!(c.month, Some(vec![1]));
        assert_eq!(c.day, Some(vec![1]));
        assert_eq!(c.hour, Some(vec![0]));
    }
}

#[test]
fn special_quarterly() {
    let c = parse_calendar("quarterly").unwrap();
    assert_eq!(c.month, Some(vec![1, 4, 7, 10]));
    assert_eq!(c.day, Some(vec![1]));
    assert_eq!(c.hour, Some(vec![0]));
}

#[test]
fn special_semiannually() {
    let c = parse_calendar("semiannually").unwrap();
    assert_eq!(c.month, Some(vec![1, 7]));
    assert_eq!(c.day, Some(vec![1]));
}

// ---------- 一般語法 ----------

#[test]
fn full_expression() {
    let c = parse_calendar("Mon 2026-11-01 12:34:56").unwrap();
    assert_eq!(c.dow, Some(vec![0]));
    assert_eq!(c.year, Some(vec![2026]));
    assert_eq!(c.month, Some(vec![11]));
    assert_eq!(c.day, Some(vec![1]));
    assert_eq!(c.hour, Some(vec![12]));
    assert_eq!(c.minute, Some(vec![34]));
    assert_eq!(c.second, Some(vec![56]));
}

#[test]
fn dow_range() {
    let c = parse_calendar("Mon..Fri *-*-* 00:00:00").unwrap();
    assert_eq!(c.dow, Some(vec![0, 1, 2, 3, 4]));
}

#[test]
fn dow_list() {
    let c = parse_calendar("Sat,Sun *-*-* 00:00:00").unwrap();
    assert_eq!(c.dow, Some(vec![5, 6]));
}

#[test]
fn hour_star_step() {
    let c = parse_calendar("*-*-* */2:00:00").unwrap();
    assert_eq!(c.hour, Some(vec![0, 2, 4, 6, 8, 10, 12, 14, 16, 18, 20, 22]));
    assert_eq!(c.minute, Some(vec![0]));
}

#[test]
fn hour_range_step() {
    let c = parse_calendar("*-*-* 9..17/2:00:00").unwrap();
    assert_eq!(c.hour, Some(vec![9, 11, 13, 15, 17]));
}

#[test]
fn month_names() {
    let c = parse_calendar("*-Jan,Mar-* 00:00:00").unwrap();
    assert_eq!(c.month, Some(vec![1, 3]));
}

#[test]
fn names_case_insensitive() {
    let c = parse_calendar("MON *-jan-* 00:00:00").unwrap();
    assert_eq!(c.dow, Some(vec![0]));
    assert_eq!(c.month, Some(vec![1]));
}

#[test]
fn repetition_syntax() {
    // 5/15 = 從 5 開始每 15 分鐘,直到上限 59
    let c = parse_calendar("*-*-* 00:5/15:00").unwrap();
    assert_eq!(c.minute, Some(vec![5, 20, 35, 50]));
}

#[test]
fn dow_names_in_range() {
    let c = parse_calendar("Mon..Wed *-*-* 00:00:00").unwrap();
    assert_eq!(c.dow, Some(vec![0, 1, 2]));
}

// ---------- 錯誤拒絕 ----------

#[test]
fn empty_is_error() {
    assert!(matches!(parse_calendar(""), Err(CalendarError::Empty)));
    assert!(matches!(parse_calendar("   "), Err(CalendarError::Empty)));
}

#[test]
fn garbage_is_error() {
    assert!(parse_calendar("not a calendar").is_err());
    assert!(parse_calendar("%%$#").is_err());
}

#[test]
fn out_of_range_values_rejected() {
    assert!(parse_calendar("*-*-* 25:00:00").is_err());
    assert!(parse_calendar("*-0-* 00:00:00").is_err());
    assert!(parse_calendar("*-13-* 00:00:00").is_err());
    assert!(parse_calendar("*-*-32 00:00:00").is_err());
    assert!(parse_calendar("*-*-0 00:00:00").is_err());
    assert!(parse_calendar("8 *-*-* 00:00:00").is_err()); // dow 1..=7
    assert!(parse_calendar("*-*-* 00:60:00").is_err());
    assert!(parse_calendar("*-*-* 00:00:60").is_err());
}

#[test]
fn bad_syntax_rejected() {
    assert!(parse_calendar("*-*-* a..b:00:00").is_err());
    assert!(parse_calendar("*-*-* 1..2..3:00:00").is_err());
    assert!(parse_calendar("*-*-* *:1/0:00").is_err()); // 步進 0
    assert!(parse_calendar("Mon Tue *-*-* 00:00:00").is_err()); // 重複 dow
    assert!(parse_calendar("*-*-* 12:00:00 13:00:00").is_err()); // 重複 time
}

// ---------- 缺省補值 ----------

#[test]
fn time_missing_parts_default_zero() {
    let c = parse_calendar("12:30").unwrap();
    assert_eq!(c.hour, Some(vec![12]));
    assert_eq!(c.minute, Some(vec![30]));
    assert_eq!(c.second, Some(vec![0])); // 缺秒補 0,不是 *
}

#[test]
fn time_hour_only() {
    let c = parse_calendar("12").unwrap();
    assert_eq!(c.hour, Some(vec![12]));
    assert_eq!(c.minute, Some(vec![0]));
    assert_eq!(c.second, Some(vec![0]));
}

#[test]
fn date_only_day_given() {
    let c = parse_calendar("*-*-15").unwrap();
    assert_eq!(c.year, None);
    assert_eq!(c.month, None);
    assert_eq!(c.day, Some(vec![15]));
    // 日期給了但時間整體省略 → 00:00:00
    assert_eq!(c.hour, Some(vec![0]));
    assert_eq!(c.minute, Some(vec![0]));
    assert_eq!(c.second, Some(vec![0]));
}

#[test]
fn date_two_segments_day_defaults_one() {
    let c = parse_calendar("2026-11").unwrap();
    assert_eq!(c.year, Some(vec![2026]));
    assert_eq!(c.month, Some(vec![11]));
    assert_eq!(c.day, Some(vec![1])); // 缺日補 1
}

#[test]
fn time_only_leaves_date_star() {
    let c = parse_calendar("12:30:45").unwrap();
    assert_eq!(c.year, None);
    assert_eq!(c.month, None);
    assert_eq!(c.day, None);
    assert_eq!(c.dow, None);
}

#[test]
fn utc_suffix_tolerated() {
    let c = parse_calendar("*-*-* 12:00:00 UTC").unwrap();
    assert_eq!(c.hour, Some(vec![12]));
}

// ---------- next_elapse ----------

#[test]
fn next_daily() {
    let c = parse_calendar("daily").unwrap();
    let next = next_elapse(&c, dt("2026-10-11 10:00:00")).unwrap();
    assert_eq!(next, dt("2026-10-12 00:00:00"));
}

#[test]
fn next_strictly_after_input() {
    // 輸入正好是觸發時刻 → 下一次 (嚴格大於)
    let c = parse_calendar("daily").unwrap();
    let next = next_elapse(&c, dt("2026-10-11 00:00:00")).unwrap();
    assert_eq!(next, dt("2026-10-12 00:00:00"));
}

#[test]
fn next_hourly_same_day() {
    let c = parse_calendar("hourly").unwrap();
    let next = next_elapse(&c, dt("2026-10-11 10:30:00")).unwrap();
    assert_eq!(next, dt("2026-10-11 11:00:00"));
}

#[test]
fn next_half_hour() {
    let c = parse_calendar("*:30:00").unwrap();
    let next = next_elapse(&c, dt("2026-10-11 12:10:00")).unwrap();
    assert_eq!(next, dt("2026-10-11 12:30:00"));
}

#[test]
fn next_weekly_from_sunday() {
    // 2026-10-11 是星期日;下一個星期一 = 10-12
    let c = parse_calendar("Mon 00:00:00").unwrap();
    let next = next_elapse(&c, dt("2026-10-11 08:00:00")).unwrap();
    assert_eq!(next, dt("2026-10-12 00:00:00"));
}

#[test]
fn next_past_fixed_date_never() {
    // 過去的固定日期永不觸發
    let c = parse_calendar("2020-01-01 00:00:00").unwrap();
    assert_eq!(next_elapse(&c, dt("2026-10-11 00:00:00")), None);
}

#[test]
fn next_feb_29_skips_non_leap() {
    // 2027 平年,下一個 2/29 是 2028
    let c = parse_calendar("*-02-29 12:00:00").unwrap();
    let next = next_elapse(&c, dt("2026-10-11 00:00:00")).unwrap();
    assert_eq!(next, dt("2028-02-29 12:00:00"));
}

#[test]
fn next_specific_datetime_future() {
    let c = parse_calendar("2027-06-15 08:00:00").unwrap();
    let next = next_elapse(&c, dt("2026-10-11 00:00:00")).unwrap();
    assert_eq!(next, dt("2027-06-15 08:00:00"));
}

#[test]
fn next_dow_with_time_today_later() {
    // 2026-10-11 週日,當天 20:00 的 Sat,Sun → 今天 20:00
    let c = parse_calendar("Sat,Sun *-*-* 20:00:00").unwrap();
    let next = next_elapse(&c, dt("2026-10-11 10:00:00")).unwrap();
    assert_eq!(next, dt("2026-10-11 20:00:00"));
}

#[test]
fn next_minute_step() {
    // 每 15 分鐘 (從 0): 10:07 → 10:15
    let c = parse_calendar("*-*-* *:0/15:00").unwrap();
    let next = next_elapse(&c, dt("2026-10-11 10:07:30")).unwrap();
    assert_eq!(next, dt("2026-10-11 10:15:00"));
}
