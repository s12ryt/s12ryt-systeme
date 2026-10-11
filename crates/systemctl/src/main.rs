use std::process::ExitCode;

use systemctl::client::run_command;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = match systemctl::parse_args(&args) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(2);
        }
    };

    #[cfg(unix)]
    {
        use systemctl::client::{socket_path, UnixSocketTransport};
        let path = socket_path();
        match UnixSocketTransport::connect(&path) {
            Ok(mut t) => {
                let (out, code) = run_command(&cmd, &mut t);
                print!("{out}");
                ExitCode::from(code.clamp(0, 255) as u8)
            }
            Err(e) => match cmd {
                systemctl::Command::Help | systemctl::Command::Version => {
                    let mut null = NullTransport;
                    let (out, code) = run_command(&cmd, &mut null);
                    print!("{out}");
                    ExitCode::from(code.clamp(0, 255) as u8)
                }
                _ => {
                    eprintln!("System has not been booted with s12ryt-systeme ({e}).");
                    ExitCode::from(1)
                }
            },
        }
    }

    #[cfg(not(unix))]
    {
        eprintln!(
            "systemctl: IPC requires a Unix socket; only help/version available on this platform."
        );
        let mut null = NullTransport;
        let (out, code) = run_command(&cmd, &mut null);
        print!("{out}");
        ExitCode::from(code.clamp(0, 255) as u8)
    }
}

/// 恆失敗的空傳輸 — Help/Version 走 run_command 快路徑不會觸發它
struct NullTransport;

impl systemctl::client::IpcTransport for NullTransport {
    fn request(
        &mut self,
        _req: &systeme_core::ipc::Request,
    ) -> Result<systeme_core::ipc::Response, String> {
        Err("no transport".into())
    }
}
