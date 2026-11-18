//! The `racksmith` binary: the composition root that wires the API and domain together.
//!
//! `main` only parses arguments, installs logging and dispatches; each subcommand lives in its
//! own module under `commands`.

mod cli;
mod commands;

use std::io::{self, IsTerminal as _};
use std::process::ExitCode;

use clap::Parser as _;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::filter::LevelFilter;

use crate::cli::Cli;

fn main() -> anyhow::Result<ExitCode> {
    let cli = Cli::parse();
    init_tracing();
    commands::run(cli.command)
}

/// Sends logs to stderr, filtered by `RUST_LOG` (default `info`), keeping stdout for output.
///
/// Colours are used only when stderr is a terminal, so log files and journald get plain text.
fn init_tracing() {
    let filter = EnvFilter::builder()
        .with_default_directive(LevelFilter::INFO.into())
        .from_env_lossy();
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(io::stderr)
        .with_ansi(io::stderr().is_terminal())
        .init();
}
