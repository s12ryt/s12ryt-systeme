//! journalctl 二進制入口: 讀 JSONL 日誌文件並按參數查詢輸出。

use std::process::ExitCode;
use std::thread::sleep;
use std::time::Duration;

use journalctl::cli::parse_args;
use journalctl::query::run_query;
use systeme_journal::journal_path;
use systeme_journal::store::read_jsonl;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let query = match parse_args(&args) {
        Ok(q) => q,
        Err(e) => {
            eprintln!("journalctl: {e}");
            return ExitCode::from(2);
        }
    };

    let path = journal_path();
    let entries = read_jsonl(&path);

    if entries.is_empty() && !query.follow {
        return ExitCode::from(1);
    }

    let out = run_query(&entries, &query);
    if !out.is_empty() {
        print!("{out}");
    }

    if query.follow {
        // 簡化 follow: 每 500ms 重讀, 打印新增行 (跨平台)。
        let mut printed = entries.len();
        loop {
            sleep(Duration::from_millis(500));
            let entries = read_jsonl(&path);
            if entries.len() > printed {
                let fresh = run_query(&entries[printed..], &query);
                if !fresh.is_empty() {
                    print!("{fresh}");
                }
                printed = entries.len();
            }
        }
    }

    ExitCode::SUCCESS
}
