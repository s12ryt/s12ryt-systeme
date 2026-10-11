//! systemd-escape — 單元名轉義工具 (二進制兼容名)。

use std::process::ExitCode;

use systeme_tools::escape;

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    if argv.iter().any(|a| a == "--version" || a == "-V" || a == "--help" || a == "-h") {
        println!("systemd-escape (systeme) {}", env!("CARGO_PKG_VERSION"));
        return ExitCode::SUCCESS;
    }
    let flags: Vec<String> = argv.iter().filter(|a| a.starts_with('-')).cloned().collect();
    let args: Vec<String> = argv.iter().filter(|a| !a.starts_with('-')).cloned().collect();
    let out = escape::run(&args, &flags);
    if !out.is_empty() {
        println!("{out}");
    }
    ExitCode::SUCCESS
}
