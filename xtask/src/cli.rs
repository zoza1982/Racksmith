//! Command-line parsing for `cargo xtask`.
//!
//! The grammar is tiny, so it is parsed by hand to keep xtask's dependency footprint minimal.

use anyhow::{Result, bail};

/// Usage text printed by `cargo xtask help`.
pub(crate) const USAGE: &str = "\
Usage: cargo xtask <COMMAND>

Commands:
  arch            Enforce architecture.toml (layering, dependency policy, manifest and source
                  rules) and the exemption ledger exceptions.toml
  docs [--fix]    Check (or with --fix, regenerate) generated sections of the docs
  spec [--fix]    Check requirements.toml against the design, plan, code and tests, and
                  (with --fix) regenerate docs/requirements.md
  ci              Run the local quality gate: every ci-success check that runs on one machine
  install-hooks   Point git at the versioned hooks in .githooks/
  help            Print this message
";

/// A parsed `cargo xtask` invocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Command {
    /// `arch`: enforce the architecture rules.
    Arch,
    /// `docs`: verify generated documentation, rewriting it when `fix` is set.
    Docs {
        /// Rewrite stale generated sections instead of failing.
        fix: bool,
    },
    /// `spec`: check requirements traceability, rewriting the coverage page when `fix` is set.
    Spec {
        /// Rewrite a stale `docs/requirements.md` instead of failing.
        fix: bool,
    },
    /// `ci`: run every quality gate in order.
    Ci,
    /// `install-hooks`: configure git to use `.githooks/`.
    InstallHooks,
    /// `help`: print usage.
    Help,
}

/// Parses the arguments that follow `cargo xtask`.
///
/// # Errors
///
/// Returns an error naming the offending argument when the command or a flag is unknown.
pub(crate) fn parse(args: impl IntoIterator<Item = String>) -> Result<Command> {
    let mut args = args.into_iter();
    let Some(name) = args.next() else {
        return Ok(Command::Help);
    };
    let flags: Vec<String> = args.collect();
    let command = match name.as_str() {
        "arch" => Command::Arch,
        "docs" => Command::Docs {
            fix: flags.iter().any(|flag| flag == "--fix"),
        },
        "spec" => Command::Spec {
            fix: flags.iter().any(|flag| flag == "--fix"),
        },
        "ci" => Command::Ci,
        "install-hooks" => Command::InstallHooks,
        "help" | "--help" | "-h" => Command::Help,
        unknown => bail!("unknown command `{unknown}`\n\n{USAGE}"),
    };
    reject_unknown_flags(&name, &flags, accepted_flags(command))?;
    Ok(command)
}

/// The flags each command understands.
fn accepted_flags(command: Command) -> &'static [&'static str] {
    match command {
        Command::Docs { .. } | Command::Spec { .. } => &["--fix"],
        Command::Arch | Command::Ci | Command::InstallHooks | Command::Help => &[],
    }
}

fn reject_unknown_flags(command: &str, flags: &[String], accepted: &[&str]) -> Result<()> {
    if let Some(flag) = flags.iter().find(|flag| !accepted.contains(&flag.as_str())) {
        bail!("`{command}` does not accept `{flag}`\n\n{USAGE}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_strs(args: &[&str]) -> Result<Command> {
        parse(args.iter().map(|arg| (*arg).to_owned()))
    }

    #[test]
    fn no_arguments_prints_help() {
        assert_eq!(parse_strs(&[]).unwrap(), Command::Help);
    }

    #[test]
    fn parses_every_command() {
        assert_eq!(parse_strs(&["arch"]).unwrap(), Command::Arch);
        assert_eq!(parse_strs(&["docs"]).unwrap(), Command::Docs { fix: false });
        assert_eq!(
            parse_strs(&["docs", "--fix"]).unwrap(),
            Command::Docs { fix: true }
        );
        assert_eq!(parse_strs(&["spec"]).unwrap(), Command::Spec { fix: false });
        assert_eq!(
            parse_strs(&["spec", "--fix"]).unwrap(),
            Command::Spec { fix: true }
        );
        assert_eq!(parse_strs(&["ci"]).unwrap(), Command::Ci);
        assert_eq!(
            parse_strs(&["install-hooks"]).unwrap(),
            Command::InstallHooks
        );
        assert_eq!(parse_strs(&["--help"]).unwrap(), Command::Help);
    }

    #[test]
    fn rejects_unknown_command() {
        let error = parse_strs(&["deploy"]).unwrap_err().to_string();
        assert!(error.contains("unknown command `deploy`"), "got: {error}");
    }

    #[test]
    fn rejects_flag_the_command_does_not_accept() {
        let error = parse_strs(&["arch", "--fix"]).unwrap_err().to_string();
        assert!(
            error.contains("`arch` does not accept `--fix`"),
            "got: {error}"
        );
    }
}
