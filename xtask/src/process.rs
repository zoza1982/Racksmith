//! Running external tools.

use std::ffi::OsString;
use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::{Context as _, Result, bail};

/// A `cargo` command rooted at the workspace, using the same cargo that invoked xtask.
pub(crate) fn cargo(root: &Path) -> Command {
    let program = std::env::var_os("CARGO").unwrap_or_else(|| OsString::from("cargo"));
    let mut command = Command::new(program);
    command.current_dir(root);
    command
}

/// Runs `command` with inherited stdio.
///
/// # Errors
///
/// Returns an error if the command cannot be spawned or exits unsuccessfully.
pub(crate) fn run(command: &mut Command) -> Result<()> {
    let status = command
        .status()
        .with_context(|| format!("failed to spawn {}", describe(command)))?;
    if !status.success() {
        bail!("{} failed ({status})", describe(command));
    }
    Ok(())
}

/// Runs `command` and returns its stdout; stderr is inherited so diagnostics stay visible.
///
/// # Errors
///
/// Returns an error if the command cannot be spawned, exits unsuccessfully, or prints
/// non-UTF-8 output.
pub(crate) fn capture(command: &mut Command) -> Result<String> {
    let output = command
        .stderr(Stdio::inherit())
        .output()
        .with_context(|| format!("failed to spawn {}", describe(command)))?;
    if !output.status.success() {
        bail!("{} failed ({})", describe(command), output.status);
    }
    String::from_utf8(output.stdout)
        .with_context(|| format!("{} printed non-UTF-8 output", describe(command)))
}

/// Whether `command` can be spawned and exits successfully, with all output discarded.
/// Used to probe for optional tools such as `cargo nextest --version`.
pub(crate) fn succeeds(command: &mut Command) -> bool {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

/// A human-readable rendering of a command line, for error messages.
pub(crate) fn describe(command: &Command) -> String {
    let mut words = vec![command.get_program().to_string_lossy().into_owned()];
    words.extend(
        command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned()),
    );
    format!("`{}`", words.join(" "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn describes_program_and_arguments() {
        let mut command = Command::new("cargo");
        command.args(["fmt", "--all"]);
        assert_eq!(describe(&command), "`cargo fmt --all`");
    }
}
