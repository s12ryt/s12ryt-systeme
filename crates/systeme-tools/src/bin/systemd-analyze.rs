//! systemd-analyze — 啟動分析工具子集 (二進制兼容名)。

use std::process::ExitCode;

use systeme_tools::analyze;

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    if argv.iter().any(|a| a == "--version" || a == "-V") {
        println!("systemd-analyze (systeme) {}", env!("CARGO_PKG_VERSION"));
        return ExitCode::SUCCESS;
    }
    let args: Vec<String> = argv.iter().filter(|a| !a.starts_with('-')).cloned().collect();
    let out = analyze::run(&args);
    println!("{out}");
    ExitCode::SUCCESS
}
