//! `prosecraft-mcp`: serves the open sync folder named by `PROSECRAFT_OPEN_DIR` over stdio.
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

use std::process::ExitCode;

use prosecraft_mcp::env::SystemEnv;
use prosecraft_mcp::{ProseCraftTools, resolve_root, serve};

fn main() -> ExitCode {
    let value = std::env::var_os("PROSECRAFT_OPEN_DIR").map(|v| v.to_string_lossy().into_owned());
    // Fail fast with a clear message instead of hanging the MCP client.
    let root = match resolve_root(value.as_deref()) {
        Ok(root) => root,
        Err(message) => {
            eprintln!("{message}");
            return ExitCode::FAILURE;
        }
    };
    let tools = ProseCraftTools::new(root, Box::new(SystemEnv));
    let stdin = std::io::stdin();
    match serve(tools, stdin.lock(), std::io::stdout().lock()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
