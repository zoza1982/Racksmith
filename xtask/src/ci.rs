//! `cargo xtask ci`: the local quality gate, run fail-fast in a fixed order.
//!
//! It runs every check of the required `ci-success` CI check that works on one machine, with the
//! same arguments (including `--locked`). Only the jobs that need other toolchains or platforms
//! run in CI alone: the MSRV build, the coverage floor, the macOS and Windows runs, and the
//! `Verify` workflow (TLA+, Kani, fuzzing).
//!
//! Optional tools that are not installed are skipped with a loud warning, so the gate stays
//! usable on a fresh clone. When the `CI` environment variable is set, a missing tool is an error
//! instead, so CI can never pass by skipping.

use std::path::Path;
use std::process::Command;

use anyhow::{Context as _, Result, bail};

use crate::{arch, docs, process, term};

/// One step of the gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Step {
    /// Header printed before the step runs.
    title: &'static str,
    /// What the step does.
    action: Action,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    /// `cargo <args>` with extra environment variables.
    Cargo {
        args: &'static [&'static str],
        env: &'static [(&'static str, &'static str)],
    },
    /// Another program on `PATH`.
    External {
        program: &'static str,
        args: &'static [&'static str],
    },
    /// `cargo xtask arch`, run in-process.
    Arch,
    /// `cargo xtask docs`, run in-process.
    Docs,
    /// A step whose tool is not installed.
    Skip { warning: &'static str },
}

/// Which optional tools are installed.
#[derive(Debug, Clone, Copy)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "one independent flag per optional tool; nothing is a state machine"
)]
struct Tools {
    nextest: bool,
    deny: bool,
    typos: bool,
    machete: bool,
    actionlint: bool,
    zizmor: bool,
}

impl Tools {
    /// Probes `PATH` (and cargo subcommands) for every optional tool.
    fn detect(root: &Path) -> Self {
        let cargo_has = |subcommand: &str| {
            process::succeeds(process::cargo(root).args([subcommand, "--version"]))
        };
        let program_has =
            |program: &str, flag: &str| process::succeeds(Command::new(program).arg(flag));
        Self {
            nextest: cargo_has("nextest"),
            deny: cargo_has("deny"),
            typos: program_has("typos", "--version"),
            machete: cargo_has("machete"),
            actionlint: program_has("actionlint", "-version"),
            zizmor: program_has("zizmor", "--version"),
        }
    }
}

/// Runs the gate, stopping at the first failing step.
///
/// # Errors
///
/// Returns the first step's failure, or, when `CI` is set, an error for a missing tool.
pub(crate) fn run(root: &Path) -> Result<()> {
    let strict = std::env::var_os("CI").is_some();
    for step in plan(Tools::detect(root)) {
        term::header(step.title)?;
        execute(root, step.action, strict)
            .with_context(|| format!("ci step `{}` failed", step.title))?;
    }
    term::info("\nci: all checks passed")?;
    Ok(())
}

fn execute(root: &Path, action: Action, strict: bool) -> Result<()> {
    match action {
        Action::Cargo { args, env } => {
            process::run(process::cargo(root).args(args).envs(env.iter().copied()))
        }
        Action::External { program, args } => {
            process::run(Command::new(program).args(args).current_dir(root))
        }
        Action::Arch => arch::run(root),
        Action::Docs => docs::run(root, false),
        Action::Skip { warning } if strict => {
            bail!("{warning}; `CI` is set, so nothing may be skipped")
        }
        Action::Skip { warning } => term::warn(warning).context("failed to write warning"),
    }
}

/// A step that runs `cargo <args>`, or is skipped with `warning` when its tool is missing.
const fn cargo_step(
    title: &'static str,
    installed: bool,
    args: &'static [&'static str],
    warning: &'static str,
) -> Step {
    let action = if installed {
        Action::Cargo { args, env: &[] }
    } else {
        Action::Skip { warning }
    };
    Step { title, action }
}

/// A step that runs `program <args>`, or is skipped with `warning` when it is missing.
const fn external_step(
    title: &'static str,
    installed: bool,
    program: &'static str,
    args: &'static [&'static str],
    warning: &'static str,
) -> Step {
    let action = if installed {
        Action::External { program, args }
    } else {
        Action::Skip { warning }
    };
    Step { title, action }
}

/// A `cargo <args>` step that always runs.
const fn always(title: &'static str, args: &'static [&'static str]) -> Step {
    cargo_step(title, true, args, "")
}

/// The ordered list of steps for the installed tools: cheapest and most common failures first.
fn plan(tools: Tools) -> Vec<Step> {
    let mut steps = lint_steps();
    steps.extend(test_steps(tools.nextest));
    steps.extend(policy_steps(tools));
    steps.extend(hygiene_steps(tools));
    steps
}

/// Formatting and clippy, for the workspace and the separate fuzz project.
fn lint_steps() -> Vec<Step> {
    vec![
        always("format", &["fmt", "--all", "--check"]),
        // `fuzz/` is a separate cargo workspace, so `--all` and `--workspace` do not reach it.
        always(
            "format (fuzz)",
            &["fmt", "--manifest-path", "fuzz/Cargo.toml", "--check"],
        ),
        always(
            "clippy",
            &[
                "clippy",
                "--workspace",
                "--all-targets",
                "--all-features",
                "--locked",
                "--",
                "-D",
                "warnings",
            ],
        ),
        always(
            "clippy (fuzz)",
            &[
                "clippy",
                "--manifest-path",
                "fuzz/Cargo.toml",
                "--all-targets",
                "--locked",
                "--",
                "-D",
                "warnings",
            ],
        ),
    ]
}

/// Documentation, supply-chain policy, architecture rules and generated sections.
fn policy_steps(tools: Tools) -> Vec<Step> {
    vec![
        Step {
            title: "rustdoc",
            action: Action::Cargo {
                args: &[
                    "doc",
                    "--workspace",
                    "--no-deps",
                    "--all-features",
                    "--locked",
                ],
                env: &[("RUSTDOCFLAGS", "-D warnings")],
            },
        },
        cargo_step(
            "cargo-deny",
            tools.deny,
            &["deny", "--locked", "check"],
            DENY_MISSING,
        ),
        cargo_step(
            "cargo-deny (fuzz)",
            tools.deny,
            &[
                "deny",
                "--manifest-path",
                "fuzz/Cargo.toml",
                "--locked",
                "check",
            ],
            DENY_MISSING,
        ),
        Step {
            title: "architecture",
            action: Action::Arch,
        },
        Step {
            title: "generated docs",
            action: Action::Docs,
        },
    ]
}

/// Spelling, unused dependencies and workflow lint.
fn hygiene_steps(tools: Tools) -> Vec<Step> {
    vec![
        external_step(
            "typos",
            tools.typos,
            "typos",
            &[],
            "SKIPPED: typos is not installed, so spelling was NOT checked \
             (install: `cargo install --locked typos-cli`)",
        ),
        cargo_step(
            "unused dependencies",
            tools.machete,
            &["machete"],
            "SKIPPED: cargo-machete is not installed, so unused dependencies were NOT checked \
             (install: `cargo install --locked cargo-machete`)",
        ),
        external_step(
            "workflow lint (actionlint)",
            tools.actionlint,
            "actionlint",
            &[],
            "SKIPPED: actionlint is not installed, so GitHub workflows were NOT linted \
             (install: https://github.com/rhysd/actionlint)",
        ),
        external_step(
            "workflow lint (zizmor)",
            tools.zizmor,
            "zizmor",
            &["--persona", "regular", "."],
            "SKIPPED: zizmor is not installed, so GitHub workflows were NOT audited \
             (install: `pipx install zizmor`)",
        ),
    ]
}

const DENY_MISSING: &str = "SKIPPED: cargo-deny is not installed, so licenses, advisories, bans \
                            and sources were NOT checked (install: `cargo install --locked cargo-deny`)";

/// Tests via nextest when available (doctests then need a separate `cargo test --doc`, which
/// nextest does not run); otherwise plain `cargo test`, which includes doctests.
fn test_steps(nextest: bool) -> Vec<Step> {
    if nextest {
        vec![
            always(
                "tests (nextest)",
                &[
                    "nextest",
                    "run",
                    "--workspace",
                    "--all-features",
                    "--locked",
                ],
            ),
            always(
                "doctests",
                &["test", "--doc", "--workspace", "--all-features", "--locked"],
            ),
        ]
    } else {
        vec![always(
            "tests",
            &["test", "--workspace", "--all-features", "--locked"],
        )]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL_TOOLS: Tools = Tools {
        nextest: true,
        deny: true,
        typos: true,
        machete: true,
        actionlint: true,
        zizmor: true,
    };
    const NO_TOOLS: Tools = Tools {
        nextest: false,
        deny: false,
        typos: false,
        machete: false,
        actionlint: false,
        zizmor: false,
    };

    fn titles(tools: Tools) -> Vec<&'static str> {
        plan(tools).into_iter().map(|step| step.title).collect()
    }

    #[test]
    fn runs_every_gate_in_order_when_all_tools_are_installed() {
        assert_eq!(
            titles(ALL_TOOLS),
            [
                "format",
                "format (fuzz)",
                "clippy",
                "clippy (fuzz)",
                "tests (nextest)",
                "doctests",
                "rustdoc",
                "cargo-deny",
                "cargo-deny (fuzz)",
                "architecture",
                "generated docs",
                "typos",
                "unused dependencies",
                "workflow lint (actionlint)",
                "workflow lint (zizmor)",
            ]
        );
        let skipped = plan(ALL_TOOLS)
            .into_iter()
            .filter(|step| matches!(step.action, Action::Skip { .. }))
            .count();
        assert_eq!(
            skipped, 0,
            "nothing is skipped when every tool is installed"
        );
    }

    #[test]
    fn falls_back_to_cargo_test_and_skips_missing_tools() {
        let steps = plan(NO_TOOLS);
        assert!(steps.contains(&Step {
            title: "tests",
            action: Action::Cargo {
                args: &["test", "--workspace", "--all-features", "--locked"],
                env: &[]
            },
        }));
        for title in [
            "cargo-deny",
            "cargo-deny (fuzz)",
            "typos",
            "unused dependencies",
            "workflow lint (actionlint)",
            "workflow lint (zizmor)",
        ] {
            let step = steps.iter().find(|step| step.title == title).unwrap();
            assert!(
                matches!(step.action, Action::Skip { .. }),
                "{title} must be skipped"
            );
        }
    }

    #[test]
    fn every_cargo_step_that_resolves_dependencies_is_locked() {
        for step in plan(ALL_TOOLS) {
            let Action::Cargo { args, .. } = step.action else {
                continue;
            };
            let resolves = ["clippy", "nextest", "test", "doc", "deny"].contains(&args[0]);
            assert!(
                !resolves || args.contains(&"--locked"),
                "{} must pass --locked, as CI does",
                step.title
            );
        }
    }

    #[test]
    fn a_skipped_step_fails_in_strict_mode() {
        let skip = Action::Skip {
            warning: "SKIPPED: x",
        };
        let error = execute(Path::new("."), skip, true).unwrap_err().to_string();
        assert!(error.contains("nothing may be skipped"), "{error}");
    }

    #[test]
    fn rustdoc_denies_warnings() {
        let rustdoc = plan(ALL_TOOLS)
            .into_iter()
            .find(|step| step.title == "rustdoc")
            .unwrap();
        let Action::Cargo { env, .. } = rustdoc.action else {
            panic!("rustdoc must be a cargo step");
        };
        assert_eq!(env, [("RUSTDOCFLAGS", "-D warnings")]);
    }
}
