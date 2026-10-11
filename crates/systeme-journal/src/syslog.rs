//! RFC3164 syslog 行解析 (/dev/log datagram)。
//!
//! `<PRI>Mmm dd hh:mm:ss host tag[pid]: message`

use chrono::{Datelike, NaiveDate};

use crate::entry::{JournalEntry, DEFAULT_PRIORITY};

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// 解析一行 syslog, 失敗返回 None (容忍丟棄)。
/// `default_year`: RFC3164 時間戳無年份, 由調用方補。
pub fn parse_syslog(line: &str, default_year: i32) -> Option<JournalEntry> {
    let line = line.trim_end_matches('\n').trim_end_matches('\r');
    let rest = line.strip_prefix('<')?;
    let (pri, rest) = rest.split_once('>')?;
    let pri: u8 = pri.parse().ok()?;
    let severity = pri % 8;

    // "Mmm dd hh:mm:ss host tag[pid]: msg"
    let (mon_str, after) = rest.split_once(' ')?;
    
    
    let month = MONTHS.iter().position(|m| *m == mon_str)? as u32 + 1;

    let mut toks = after.splitn(3, ' ');
    let day: u32 = toks.next()?.parse().ok()?;
    let time = toks.next()?;
    let remainder = toks.next()?;

    let (h, m, s) = parse_hms(time)?;
    // RFC3164 慣例: 無年份; 12 月的 "未來日期" 回退一年等邊界情況不做猜測, 直接補年
    let dt = NaiveDate::from_ymd_opt(default_year, month, day)?.and_hms_opt(h, m, s)?;
    let ts_us = dt.and_utc().timestamp_micros().max(0) as u64;

    // host 後是 "tag[pid]: msg" / "tag: msg" / 其他
    let (_host, tail) = remainder.split_once(' ')?;
    let (ident, pid, message) = split_tag(tail);

    Some(JournalEntry {
        message,
        priority: if severity > 7 { DEFAULT_PRIORITY } else { severity },
        timestamp_us: ts_us,
        unit: None,
        pid,
        identifier: ident,
    })
}

fn parse_hms(time: &str) -> Option<(u32, u32, u32)> {
    let mut it = time.split(':');
    let h: u32 = it.next()?.parse().ok()?;
    let m: u32 = it.next()?.parse().ok()?;
    let s: u32 = it.next()?.parse().ok()?;
    if h < 24 && m < 60 && s < 60 {
        Some((h, m, s))
    } else {
        None
    }
}

fn split_tag(tail: &str) -> (Option<String>, Option<u32>, String) {
    if let Some(colon) = tail.find(": ") {
        let tag = &tail[..colon];
        let msg = tail[colon + 2..].to_string();
        if let Some(br) = tag.strip_suffix(']') {
            if let Some(bo) = br.find('[') {
                let name = &br[..bo];
                let pid = br[bo + 1..].parse::<u32>().ok();
                return (Some(name.to_string()), pid, msg);
            }
        }
        (Some(tag.to_string()), None, msg)
    } else {
        (None, None, tail.to_string())
    }
}

// 避免 Datelike 未使用警告 (NaiveDate::from_ymd_opt 不需要它, 留給時間邏輯擴展)
#[allow(dead_code)]
fn _year(d: NaiveDate) -> i32 {
    d.year()
}
