//! Repository automation, invoked as `cargo xtask <command>`.
//!
//! Every check here is deterministic and runs identically on a laptop and in CI. Run
//! `cargo xtask help` for the list of commands.

mod arch;
mod architecture;
mod attributes;
mod ci;
mod cli;
mod dependency_rules;
mod design_sections;
mod docs;
mod exemption_ledger;
mod expect_attributes;
mod finding;
mod hooks;
mod manifest_rules;
mod msrv_rules;
mod paths;
mod plan_tasks;
mod process;
mod requirements;
mod rust_items;
mod source_rules;
mod source_text;
mod spec;
mod subsystem_rules;
mod term;
mod test_listing;
mod tokens;
mod tracked_files;
mod violation;
mod workspace;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context as _, Result};

use crate::cli::Command;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            // If stderr itself is gone there is nowhere left to report to; the exit code still
            // signals failure.
            let _ignored = term::error(&format!("{error:#}"));
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<()> {
    let command = cli::parse(std::env::args().skip(1))?;
    let root = workspace_root()?;
    match command {
        Command::Arch => arch::run(&root),
        Command::Docs { fix } => docs::run(&root, fix),
        Command::Spec { fix } => spec::run(&root, fix),
        Command::Ci => ci::run(&root),
        Command::InstallHooks => hooks::install(&root),
        Command::Help => term::info(cli::USAGE).context("failed to write usage"),
    }
}

/// Returns the workspace root: the parent of this crate's manifest directory.
fn workspace_root() -> Result<PathBuf> {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .context("xtask manifest directory has no parent; is xtask inside the workspace?")
}
