//! systemd.time(7) Calendar 事件語法子集。
//!
//! 支持:
//! - 特殊值: `daily` / `hourly` / `weekly` / `monthly` / `yearly` (`annually`) /
//!   `quarterly` / `semiannually`
//! - 一般格式: `[DOW] [DATE] [TIME]`,各部分可省略 (位置無關,按形式識別)
//! - 字段語法: `*`、單值、`a..b` 範圍、`a..b/n`、`*/n`、`a/n` (重複)、逗號列表
//! - 名稱: 月份 `Jan..Dec`、星期 `Mon..Sun` (大小寫不敏感,接受全名前綴)
//! - DATE 為 `Y-M-D` 1..3 段 (缺月/日補 1);TIME 為 `H:M:S` 1..3 戵 (缺分/秒補 0)
//! - `UTC` 時區標記容忍忽略 (時區支持後期)
//!
//! 內部星期編號: 0=Monday..6=Sunday (與 chrono `days_from_monday` 一致)。

use chrono::{Datelike, Duration, NaiveDate, NaiveDateTime, NaiveTime};

/// Calendar 解析錯誤。
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum CalendarError {
    /// 空表達式。
    #[error("empty calendar expression")]
    Empty,
    /// 整體語法錯誤 (重複部分、無法識別的 token 等)。
    #[error("bad calendar expression: {0}")]
    BadExpression(String),
    /// 字段值域或語法錯誤。
    #[error("bad calendar field {field}: {value}")]
    BadField { field: &'static str, value: String },
}

fn bad_field(field: &'static str, value: &str) -> CalendarError {
    CalendarError::BadField {
        field,
        value: value.to_string(),
    }
}

/// 解析後的 Calendar 規格。`None` 表示該字段為 `*` (任意值)。
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CalendarSpec {
    /// 星期幾 (0=Mon..6=Sun)。
    pub dow: Option<Vec<u32>>,
    /// 年 (1970..=9999)。
    pub year: Option<Vec<i32>>,
    /// 月 (1..=12)。
    pub month: Option<Vec<u32>>,
    /// 日 (1..=31)。
    pub day: Option<Vec<u32>>,
    /// 時 (0..=23)。
    pub hour: Option<Vec<u32>>,
    /// 分 (0..=59)。
    pub minute: Option<Vec<u32>>,
    /// 秒 (0..=59)。
    pub second: Option<Vec<u32>>,
}

const MONTH_NAMES: [&str; 12] = [
    "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
];
const DOW_NAMES: [&str; 7] = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"];

/// 解析 systemd calendar 表達式。
pub fn parse_calendar(expr: &str) -> Result<CalendarSpec, CalendarError> {
    let expr = expr.trim();
    if expr.is_empty() {
        return Err(CalendarError::Empty);
    }

    // 特殊值直接展開。
    let lower = expr.to_ascii_lowercase();
    let midnight = (Some(vec![0]), Some(vec![0]), Some(vec![0]));
    let spec = match lower.as_str() {
        "daily" => CalendarSpec {
            hour: Some(vec![0]),
            minute: Some(vec![0]),
            second: Some(vec![0]),
            ..CalendarSpec::default()
        },
        "hourly" => CalendarSpec {
            minute: Some(vec![0]),
            second: Some(vec![0]),
            ..CalendarSpec::default()
        },
        "weekly" => CalendarSpec {
            dow: Some(vec![0]),
            hour: midnight.0,
            minute: midnight.1,
            second: midnight.2,
            ..CalendarSpec::default()
        },
        "monthly" => CalendarSpec {
            day: Some(vec![1]),
            hour: midnight.0,
            minute: midnight.1,
            second: midnight.2,
            ..CalendarSpec::default()
        },
        "yearly" | "annually" => CalendarSpec {
            month: Some(vec![1]),
            day: Some(vec![1]),
            hour: midnight.0,
            minute: midnight.1,
            second: midnight.2,
            ..CalendarSpec::default()
        },
        "quarterly" => CalendarSpec {
            month: Some(vec![1, 4, 7, 10]),
            day: Some(vec![1]),
            hour: midnight.0,
            minute: midnight.1,
            second: midnight.2,
            ..CalendarSpec::default()
        },
        "semiannually" => CalendarSpec {
            month: Some(vec![1, 7]),
            day: Some(vec![1]),
            hour: midnight.0,
            minute: midnight.1,
            second: midnight.2,
            ..CalendarSpec::default()
        },
        _ => CalendarSpec::default(),
    };
    if !matches!(
        lower.as_str(),
        "" | "daily" | "hourly" | "weekly" | "monthly" | "yearly" | "annually" | "quarterly"
            | "semiannually"
    ) {
        return parse_general(expr);
    }
    Ok(spec)
}

/// 一般 `[DOW] [DATE] [TIME]` 表達式解析。
fn parse_general(expr: &str) -> Result<CalendarSpec, CalendarError> {
    let mut dow_tok: Option<&str> = None;
    let mut date_tok: Option<&str> = None;
    let mut time_tok: Option<&str> = None;

    for tok in expr.split_whitespace() {
        if tok.eq_ignore_ascii_case("UTC") {
            continue; // 時區標記: 暫以本地/naive 時間處理
        }
        if tok.contains(':') || tok.parse::<u32>().is_ok() {
            if time_tok.replace(tok).is_some() {
                return Err(CalendarError::BadExpression(expr.to_string()));
            }
        } else if tok.contains('-') || tok == "*" {
            if date_tok.replace(tok).is_some() {
                return Err(CalendarError::BadExpression(expr.to_string()));
            }
        } else if dow_tok.replace(tok).is_some() {
            return Err(CalendarError::BadExpression(expr.to_string()));
        }
    }

    if dow_tok.is_none() && date_tok.is_none() && time_tok.is_none() {
        return Err(CalendarError::BadExpression(expr.to_string()));
    }

    let mut spec = CalendarSpec::default();

    if let Some(t) = dow_tok {
        let v = parse_field(t, 1, 7, "day-of-week", Some(&DOW_NAMES))?;
        spec.dow = v.map(|xs| xs.into_iter().map(|x| x - 1).collect());
    }

    if let Some(t) = date_tok {
        let segs: Vec<&str> = t.split('-').collect();
        if segs.len() > 3 || segs.is_empty() {
            return Err(bad_field("date", t));
        }
        let y = parse_field(segs[0], 1970, 9999, "year", None)?;
        spec.year = y.map(|xs| xs.into_iter().map(|x| x as i32).collect());
        spec.month = match segs.get(1) {
            Some(s) => parse_field(s, 1, 12, "month", Some(&MONTH_NAMES))?,
            None => Some(vec![1]), // 缺月補 1
        };
        spec.day = match segs.get(2) {
            Some(s) => parse_field(s, 1, 31, "day", None)?,
            None => Some(vec![1]), // 缺日補 1
        };
    }

    match time_tok {
        Some(t) => {
            let segs: Vec<&str> = t.split(':').collect();
            if segs.len() > 3 || segs.is_empty() {
                return Err(bad_field("time", t));
            }
            spec.hour = parse_field(segs[0], 0, 23, "hour", None)?;
            spec.minute = match segs.get(1) {
                Some(s) => parse_field(s, 0, 59, "minute", None)?,
                None => Some(vec![0]), // 缺分補 0
            };
            spec.second = match segs.get(2) {
                Some(s) => parse_field(s, 0, 59, "second", None)?,
                None => Some(vec![0]), // 缺秒補 0
            };
        }
        None => {
            // 時間整體省略 → 00:00:00 (只要有 date 或 dow 存在)。
            spec.hour = Some(vec![0]);
            spec.minute = Some(vec![0]);
            spec.second = Some(vec![0]);
        }
    }

    Ok(spec)
}

/// 解析單個字段表達式。返回 `None` 表示 `*`。
fn parse_field(
    s: &str,
    min: u32,
    max: u32,
    field: &'static str,
    names: Option<&[&str]>,
) -> Result<Option<Vec<u32>>, CalendarError> {
    if s == "*" {
        return Ok(None);
    }
    let mut out = Vec::new();
    for item in s.split(',') {
        parse_item(item, min, max, field, names, &mut out)?;
    }
    if out.is_empty() {
        return Err(bad_field(field, s));
    }
    for &v in &out {
        if v < min || v > max {
            return Err(bad_field(field, s));
        }
    }
    out.sort_unstable();
    out.dedup();
    Ok(Some(out))
}

/// 解析字段中的一項: `值` / `a..b` / `a..b/n` / `*/n` / `a/n`。
fn parse_item(
    item: &str,
    min: u32,
    max: u32,
    field: &'static str,
    names: Option<&[&str]>,
    out: &mut Vec<u32>,
) -> Result<(), CalendarError> {
    let err = || bad_field(field, item);

    let (base, step) = match item.split_once('/') {
        Some((b, s)) => {
            let step: u32 = s.parse().map_err(|_| err())?;
            if step == 0 {
                return Err(err());
            }
            (b, Some(step))
        }
        None => (item, None),
    };

    let (start, end) = if base == "*" {
        (min, max)
    } else if let Some((a, b)) = base.split_once("..") {
        let a = parse_val(a, names).ok_or_else(err)?;
        let b = parse_val(b, names).ok_or_else(err)?;
        if a > b {
            return Err(err());
        }
        (a, b)
    } else {
        let v = parse_val(base, names).ok_or_else(err)?;
        // `a/n` = 從 a 重複到字段上限;單值 = start..=v
        match step {
            Some(_) => (v, max),
            None => (v, v),
        }
    };

    match step {
        Some(n) => {
            let mut v = start;
            while v <= end {
                out.push(v);
                v += n;
            }
        }
        None => out.extend(start..=end),
    }
    Ok(())
}

/// 解析單個值: 名稱 (若提供) 或非負整數。
fn parse_val(raw: &str, names: Option<&[&str]>) -> Option<u32> {
    if let Some(names) = names {
        let lower = raw.to_ascii_lowercase();
        if let Some(idx) = names
            .iter()
            .position(|n| lower.len() >= n.len() && lower.starts_with(n))
        {
            return Some(idx as u32 + 1);
        }
    }
    raw.parse::<u32>().ok()
}

/// 計算 `after` 之後 (嚴格大於) 的下一次觸發時刻。
///
/// 掃描上限約 20000 天 (~55 年);固定年份超過掃描窗口的規格返回 `None`。
pub fn next_elapse(spec: &CalendarSpec, after: NaiveDateTime) -> Option<NaiveDateTime> {
    let hours: Vec<u32> = spec.hour.clone().unwrap_or_else(|| (0..24).collect());
    let minutes: Vec<u32> = spec.minute.clone().unwrap_or_else(|| (0..60).collect());
    let seconds: Vec<u32> = spec.second.clone().unwrap_or_else(|| (0..60).collect());

    let mut t = after + Duration::seconds(1);
    for _ in 0..20000u32 {
        let date = t.date();
        if date_matches(spec, date) {
            let from = t.time();
            for &h in &hours {
                for &m in &minutes {
                    for &s in &seconds {
                        let cand = NaiveTime::from_hms_opt(h, m, s)?;
                        if cand >= from {
                            return Some(date.and_time(cand));
                        }
                    }
                }
            }
        }
        t = date.succ_opt()?.and_hms_opt(0, 0, 0)?;
    }
    None
}

/// 日期字段 (年/月/日/星期) 是否全部匹配。
fn date_matches(spec: &CalendarSpec, d: NaiveDate) -> bool {
    if let Some(years) = &spec.year {
        if !years.contains(&d.year()) {
            return false;
        }
    }
    if let Some(months) = &spec.month {
        if !months.contains(&d.month()) {
            return false;
        }
    }
    if let Some(days) = &spec.day {
        if !days.contains(&d.day()) {
            return false;
        }
    }
    if let Some(dows) = &spec.dow {
            if !dows.contains(&d.weekday().num_days_from_monday()) {
            return false;
        }
    }
    true
}
