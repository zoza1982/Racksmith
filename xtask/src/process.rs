//! Running external tools.
//!
//! xtask itself runs under `cargo run`, so its environment describes the xtask *package*
//! (`CARGO_PKG_NAME=xtask`, `CARGO_MANIFEST_DIR=<root>/xtask`, ...). A tool that inherits those
//! variables can mistake xtask for the thing it should examine and check the wrong target, or
//! nothing: cargo-machete, for one, parses its arguments differently when `CARGO_PKG_NAME` is
//! set. Every command spawned here therefore starts without them; see [`command`].

use std::ffi::{OsStr, OsString};
use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::{Context as _, Result, bail};

/// Variables that cargo sets for the program it builds and runs, describing that program's
/// package. They are xtask's, never the spawned tool's, so [`command`] removes them.
///
/// Deliberately kept, because they describe the toolchain rather than xtask's package:
/// - `CARGO`, the cargo binary that invoked xtask: [`cargo`] runs it so that nested commands use
///   the same cargo, and tools such as cargo-machete read it to detect a `cargo <tool>` call;
/// - `RUSTUP_TOOLCHAIN`, set by the rustup proxy: it pins nested `cargo` and `rustc` calls to the
///   toolchain xtask was started with (`rust-toolchain.toml`, or a `+toolchain` override).
const PACKAGE_VARS: [&str; 8] = [
    "CARGO_MANIFEST_DIR",
    "CARGO_MANIFEST_PATH",
    "CARGO_CRATE_NAME",
    "CARGO_BIN_NAME",
    "CARGO_PRIMARY_PACKAGE",
    "CARGO_TARGET_TMPDIR",
    "CARGO_RUSTC_CURRENT_DIR",
    "OUT_DIR",
];

/// Prefixes of further per-package variables: `CARGO_PKG_NAME`, `CARGO_PKG_VERSION_MAJOR`,
/// `CARGO_BIN_EXE_<name>`, ...
const PACKAGE_VAR_PREFIXES: [&str; 2] = ["CARGO_PKG_", "CARGO_BIN_EXE_"];

/// A command for `program` that does not inherit xtask's package variables.
pub(crate) fn command(program: impl AsRef<OsStr>) -> Command {
    let mut command = Command::new(program);
    for name in inherited_package_vars() {
        command.env_remove(name);
    }
    command
}

/// The package variables present in xtask's own environment, for spawners that do not build a
/// [`Command`] themselves (such as `cargo_metadata`).
pub(crate) fn inherited_package_vars() -> Vec<OsString> {
    package_vars(std::env::vars_os().map(|(name, _)| name))
}

/// The names in `names` that are package variables.
fn package_vars(names: impl IntoIterator<Item = OsString>) -> Vec<OsString> {
    names
        .into_iter()
        .filter(|name| is_package_var(name))
        .collect()
}

fn is_package_var(name: &OsStr) -> bool {
    name.to_str().is_some_and(|name| {
        PACKAGE_VARS.contains(&name)
            || PACKAGE_VAR_PREFIXES
                .iter()
                .any(|prefix| name.starts_with(prefix))
    })
}

/// A `cargo` command rooted at the workspace, using the same cargo that invoked xtask.
pub(crate) fn cargo(root: &Path) -> Command {
    let program = std::env::var_os("CARGO").unwrap_or_else(|| OsString::from("cargo"));
    let mut command = command(program);
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

    #[test]
    fn package_vars_are_scrubbed_but_toolchain_and_user_config_are_kept() {
        let names = [
            "CARGO_PKG_NAME",
            "CARGO_PKG_VERSION_MAJOR",
            "CARGO_MANIFEST_DIR",
            "CARGO_MANIFEST_PATH",
            "CARGO_PRIMARY_PACKAGE",
            "CARGO_CRATE_NAME",
            "CARGO_BIN_NAME",
            "CARGO_BIN_EXE_racksmith",
            "OUT_DIR",
            // Kept: the toolchain, and configuration the user set for every cargo call.
            "CARGO",
            "RUSTUP_TOOLCHAIN",
            "CARGO_HOME",
            "CARGO_TARGET_DIR",
            "CARGO_TERM_COLOR",
            "PATH",
        ]
        .map(OsString::from);
        let scrubbed = package_vars(names.clone());
        assert_eq!(
            scrubbed,
            names.get(..9).unwrap(),
            "exactly the package variables are removed"
        );
    }

    #[test]
    fn spawned_commands_start_without_package_vars() {
        // Test binaries run with the package variables of `xtask` set, as xtask itself does.
        let inherited = inherited_package_vars();
        assert!(
            inherited.contains(&OsString::from("CARGO_PKG_NAME")),
            "the test runner sets CARGO_PKG_NAME: {inherited:?}"
        );
        let command = cargo(Path::new("."));
        let removed: Vec<&OsStr> = command
            .get_envs()
            .filter_map(|(name, value)| value.is_none().then_some(name))
            .collect();
        for name in &inherited {
            assert!(
                removed.contains(&name.as_os_str()),
                "{name:?} leaks into spawned commands"
            );
        }
    }
}
