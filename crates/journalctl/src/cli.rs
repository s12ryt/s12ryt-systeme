//! journalctl 命令行參數解析。
//!
//! 兼容常用 systemd journalctl 旗標子集:
//! `-u/--unit` (可多次)、`-p/--priority`、`--since/--until`、
//! `-o/--output`、`-n/--lines`、`-r`、`-f`、`--no-pager` (忽略)。

use systeme_journal::filter::{parse_datetime_arg, parse_priority};
use systeme_journal::format::OutputFormat;

/// 一次日誌查詢的全部參數。
#[derive(Debug, Clone, PartialEq)]
pub struct Query {
    /// 僅顯示這些單元 (-u, 可多次)
    pub units: Vec<String>,
    /// 最高優先級 (-p), None = 不限
    pub priority_max: Option<u8>,
    /// 起始時間 (UTC 微秒)
    pub since: Option<u64>,
    /// 結束時間 (UTC 微秒)
    pub until: Option<u64>,
    /// 輸出格式 (-o)
    pub format: OutputFormat,
    /// 尾部 N 條 (-n)
    pub lines: Option<usize>,
    /// 倒序顯示 (-r)
    pub reverse: bool,
    /// 跟隨輸出 (-f)
    pub follow: bool,
    /// 文本過濾 pattern (位置參數)
    pub patterns: Vec<String>,
}

impl Default for Query {
    fn default() -> Self {
        Query {
            units: Vec::new(),
            priority_max: None,
            since: None,
            until: None,
            format: OutputFormat::Short,
            lines: None,
            reverse: false,
            follow: false,
            patterns: Vec::new(),
        }
    }
}

/// 解析參數列表為 [`Query`]。壞值 (優先級/日期/格式/行數) 返回 Err。
pub fn parse_args(args: &[String]) -> Result<Query, String> {
    let mut q = Query::default();
    let mut i = 0;

    let value_of = |flag: &str, inline: Option<&str>, i: &mut usize| -> Result<String, String> {
        if let Some(v) = inline {
            return Ok(v.to_string());
        }
        *i += 1;
        args.get(*i)
            .cloned()
            .ok_or_else(|| format!("missing value for {flag}"))
    };

    while i < args.len() {
        let arg = args[i].as_str();
        let (name, inline) = match arg.split_once('=') {
            Some((n, v)) if n.starts_with('-') => (n, Some(v)),
            _ => (arg, None),
        };
        match name {
            "-u" | "--unit" => {
                let v = value_of(name, inline, &mut i)?;
                q.units.push(v);
            }
            "-p" | "--priority" => {
                let v = value_of(name, inline, &mut i)?;
                q.priority_max = Some(
                    parse_priority(&v).ok_or_else(|| format!("invalid priority: {v}"))?,
                );
            }
            "--since" => {
                let v = value_of(name, inline, &mut i)?;
                q.since = Some(
                    parse_datetime_arg(&v).ok_or_else(|| format!("invalid date: {v}"))?,
                );
            }
            "--until" => {
                let v = value_of(name, inline, &mut i)?;
                q.until = Some(
                    parse_datetime_arg(&v).ok_or_else(|| format!("invalid date: {v}"))?,
                );
            }
            "-o" | "--output" => {
                let v = value_of(name, inline, &mut i)?;
                q.format = OutputFormat::parse(&v)
                    .ok_or_else(|| format!("invalid output format: {v}"))?;
            }
            "-n" | "--lines" => {
                let v = value_of(name, inline, &mut i)?;
                let n: usize = v
                    .parse()
                    .map_err(|_| format!("invalid line count: {v}"))?;
                q.lines = Some(n);
            }
            "-r" => q.reverse = true,
            "-f" => q.follow = true,
            "--no-pager" => { /* 接受並忽略 */ }
            _ => {
                if arg.starts_with('-') {
                    // 未知旗標: 容忍跳過 (連帶其行內值已剝離)
                } else {
                    q.patterns.push(arg.to_string());
                }
            }
        }
        i += 1;
    }
    Ok(q)
}
