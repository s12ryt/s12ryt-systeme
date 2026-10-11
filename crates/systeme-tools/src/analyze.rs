//! systemd-analyze 子集: blame / critical-chain / time 的輸出格式化。

/// 時長格式化: <1s → 毫秒; >=60s → "Nmin Ms"; 否則 "N.Ns"/"Ns"。
pub fn format_duration(secs: f64) -> String {
    if secs < 1.0 {
        format!("{}ms", (secs * 1000.0).round() as u64)
    } else if secs >= 60.0 {
        let m = (secs / 60.0).floor() as u64;
        let r = secs - (m as f64) * 60.0;
        let rs = if (r - r.round()).abs() < 1e-9 {
            format!("{}s", r.round() as u64)
        } else {
            format!("{r:.1}s")
        };
        format!("{m}min {rs}")
    } else if (secs - secs.round()).abs() < 1e-9 {
        format!("{}s", secs.round() as u64)
    } else {
        format!("{secs:.1}s")
    }
}

/// blame: 按初始化耗時降序列出單元。
pub fn blame(entries: &[(String, f64)]) -> String {
    let mut sorted: Vec<&(String, f64)> = entries.iter().collect();
    sorted.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    sorted
        .iter()
        .map(|(name, secs)| format!("{} {}", format_duration(*secs), name))
        .collect::<Vec<_>>()
        .join("\n")
}

/// analyze 入口 (無守護進程連接的簡化輸出)。
pub fn run(args: &[String]) -> String {
    match args.first().map(|s| s.as_str()) {
        Some("blame") => String::from("(no boot timing data; connect to a running systemed daemon)"),
        Some("time") => String::from(
            "Startup finished in 0ms (firmware) + 0ms (loader)\n(no boot timing data available)",
        ),
        Some("critical-chain") => String::from(
            "The time when unit became active or enabled is printed after the \"@\" character.\n(no dependency chain data available)",
        ),
        Some("dot") => String::from("digraph systemd {\n}\n"),
        Some(other) => format!("Unknown command {other}."),
        None => String::from("systemd-analyze [blame|time|critical-chain|dot]"),
    }
}
