//! `cargo xtask install-hooks`: use the versioned git hooks in `.githooks/`.

use std::path::Path;
use std::process::Command;

use anyhow::Result;

use crate::{process, term};

/// Directory of the versioned hooks, relative to the workspace root.
const HOOKS_DIR: &str = ".githooks";

/// Points `core.hooksPath` at [`HOOKS_DIR`] for this clone.
///
/// # Errors
///
/// Returns an error if `git config` fails, e.g. outside a git checkout.
pub(crate) fn install(root: &Path) -> Result<()> {
    process::run(
        Command::new("git")
            .args(["config", "core.hooksPath", HOOKS_DIR])
            .current_dir(root),
    )?;
    term::info(&format!(
        "hooks: git now runs {HOOKS_DIR}/pre-commit (format check) and {HOOKS_DIR}/pre-push \
         (`cargo xtask ci`)"
    ))?;
    Ok(())
}
