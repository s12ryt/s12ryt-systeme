//! systemctl — systemd-compatible control CLI for s12ryt-systeme

pub mod cli;
pub mod client;

pub use cli::{parse_args, Command, ParseError};
pub use client::{run_command, IpcTransport};
