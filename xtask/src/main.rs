//! Workspace tooling: `cargo xtask <command>`.
//!
//! Pure Rust (std only). External tools (`cargo`) are run through `std::process::Command`.
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

const USAGE: &str = "\
usage: cargo xtask <command>

commands:
  ci      fmt --check, clippy -D warnings, test, layers, wasm (stops at the first failure)
  layers  check that nothing below the UI depends on a UI toolkit
  wasm    cargo check --target wasm32-unknown-unknown for the crates that must build for the web
";

/// Packages that must build for wasm32-unknown-unknown.
const WASM_PACKAGES: &[&str] = &["prosecraft-core", "prosecraft-ui-egui", "prosecraft-rs-web"];

/// Crates below the UI, and the UI and windowing crates they must never depend on.
const BELOW_UI: &[&str] = &["crates/core", "crates/native"];
const UI_CRATES: &[&str] = &[
    "egui",
    "eframe",
    "winit",
    "rfd",
    "egui_kittest",
    "egui_extras",
];

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("ci") => cmd_ci(),
        Some("layers") => cmd_layers(),
        Some("wasm") => cmd_wasm(),
        Some("-h" | "--help" | "help") | None => {
            print!("{USAGE}");
            Ok(())
        }
        Some(other) => Err(format!("unknown command `{other}`\n\n{USAGE}")),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

/// The workspace root (the parent of the xtask crate).
fn root() -> PathBuf {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest_dir.parent().unwrap_or(manifest_dir).to_path_buf()
}

fn cargo() -> Command {
    let mut command = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()));
    command.current_dir(root());
    command
}

fn cargo_step(args: &[&str]) -> Result<(), String> {
    let what = format!("cargo {}", args.join(" "));
    eprintln!("$ {what}");
    let status = cargo()
        .args(args)
        .status()
        .map_err(|e| format!("{what}: failed to start: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{what}: exited with {status}"))
    }
}

fn cmd_wasm() -> Result<(), String> {
    let mut failed = Vec::new();
    for package in WASM_PACKAGES {
        if cargo_step(&["check", "--target", "wasm32-unknown-unknown", "-p", package]).is_err() {
            failed.push(*package);
        }
    }
    if failed.is_empty() {
        Ok(())
    } else {
        Err(format!("wasm check failed for: {}", failed.join(", ")))
    }
}

/// Reads each lower crate's manifest and refuses a dependency on a UI crate. The manifest is the
/// source of truth for direct dependencies; the wasm check catches most indirect ones.
fn cmd_layers() -> Result<(), String> {
    let mut problems = Vec::new();
    for dir in BELOW_UI {
        let path = root().join(dir).join("Cargo.toml");
        let manifest =
            std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        for line in manifest.lines() {
            let name = line.split('=').next().unwrap_or_default().trim();
            if UI_CRATES.contains(&name) {
                problems.push(format!("{dir} depends on `{name}`"));
            }
        }
    }
    if problems.is_empty() {
        eprintln!("layers: ok");
        Ok(())
    } else {
        Err(format!("layering broken: {}", problems.join("; ")))
    }
}

fn cmd_ci() -> Result<(), String> {
    type Step = (&'static str, fn() -> Result<(), String>);
    let steps: [Step; 5] = [
        ("fmt", || cargo_step(&["fmt", "--all", "--", "--check"])),
        ("clippy", || {
            cargo_step(&[
                "clippy",
                "--workspace",
                "--all-targets",
                "--",
                "-D",
                "warnings",
            ])
        }),
        ("test", || cargo_step(&["test", "--workspace"])),
        ("layers", cmd_layers),
        ("wasm", cmd_wasm),
    ];
    for (index, (name, step)) in steps.iter().enumerate() {
        eprintln!("\n=== ci: {name} ===");
        if let Err(error) = step() {
            println!("\nCI summary:");
            for (done, _) in steps.iter().take(index) {
                println!("  ok    {done}");
            }
            println!("  FAIL  {name}: {error}");
            for (skipped, _) in steps.iter().skip(index.saturating_add(1)) {
                println!("  skip  {skipped}");
            }
            return Err(format!("ci failed at `{name}`"));
        }
    }
    println!("\nCI summary: all {} steps passed", steps.len());
    Ok(())
}
