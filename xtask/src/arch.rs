//! `cargo xtask arch`: enforce `architecture.toml` across the workspace.

use std::fs;
use std::path::Path;

use anyhow::{Context as _, Result, bail, ensure};

use crate::architecture::Architecture;
use crate::violation::Violation;
use crate::workspace::{self, SourceFile, WorkspaceCrate};
use crate::{dependency_rules, manifest_rules, msrv_rules, source_rules, term};

/// The standalone fuzz project: not a workspace member, but held to the same lint policy and
/// source rules.
const FUZZ_DIR: &str = "fuzz";

/// What a run examined, so that a run which silently examined nothing cannot pass.
#[derive(Debug, Default, PartialEq, Eq)]
struct Examined {
    /// Workspace crates whose manifests, dependencies and sources were checked.
    crates: usize,
    /// Source files checked, across the crates and the fuzz project.
    files: usize,
    /// Packages of the locked dependency graph checked against the MSRV.
    packages: usize,
    /// The MSRV comparison, as `MSRV >= highest (package)`.
    msrv: String,
}

impl Examined {
    /// Fails if a rule set had nothing to examine, which means xtask looked in the wrong place.
    fn ensure_nonempty(&self) -> Result<()> {
        ensure!(
            self.crates > 0,
            "architecture: examined no workspace crates; is the workspace root right?"
        );
        ensure!(self.files > 0, "architecture: examined no source files");
        ensure!(
            self.packages > 0,
            "architecture: examined no packages of the dependency graph"
        );
        Ok(())
    }

    fn summary(&self) -> String {
        format!(
            "{} crates, {} source files, {} locked packages ({})",
            self.crates, self.files, self.packages, self.msrv
        )
    }
}

/// Runs every architecture rule and reports all violations at once.
///
/// # Errors
///
/// Returns an error if the workspace cannot be inspected, nothing was examined, or any rule is
/// violated.
pub(crate) fn run(root: &Path) -> Result<()> {
    let (violations, examined) = collect_violations(root)?;
    examined.ensure_nonempty()?;
    if violations.is_empty() {
        term::info(&format!(
            "architecture: all rules pass; checked {}",
            examined.summary()
        ))?;
        return Ok(());
    }
    for violation in &violations {
        term::error(&violation.to_string())?;
    }
    bail!(
        "{} architecture violation(s) in {}; see architecture.toml",
        violations.len(),
        examined.summary()
    )
}

/// Gathers the violations of every rule, sorted for stable output, and what was examined.
///
/// # Errors
///
/// Returns an error if `architecture.toml`, a manifest, a source file or the dependency graph
/// cannot be read, or if a crate or the fuzz project has no source files.
fn collect_violations(root: &Path) -> Result<(Vec<Violation>, Examined)> {
    let policy = read(&root.join("architecture.toml"))?;
    let architecture = Architecture::parse(&policy)?;
    let crates = workspace::load_crates(root)?;
    let packages = workspace::load_resolved_packages(root)?;
    let msrv = msrv_rules::check(&packages);

    let mut examined = Examined {
        crates: crates.len(),
        packages: packages.len(),
        msrv: describe_msrv(&msrv),
        ..Examined::default()
    };
    let mut violations = dependency_rules::check(&architecture, &crates);
    violations.extend(msrv.violations);
    violations.extend(root_violations(root, &architecture, &mut examined)?);
    for krate in &crates {
        violations.extend(crate_violations(root, &architecture, krate, &mut examined)?);
    }
    violations.sort();
    Ok((violations, examined))
}

fn describe_msrv(report: &msrv_rules::MsrvReport<'_>) -> String {
    let msrv = report
        .msrv
        .map_or_else(|| "no MSRV".to_owned(), ToString::to_string);
    match report.highest {
        Some(package) => format!(
            "MSRV {msrv}; highest dependency rust-version {} in {} {}",
            package
                .rust_version
                .as_ref()
                .map_or_else(String::new, ToString::to_string),
            package.name,
            package.version
        ),
        None => format!("MSRV {msrv}; no dependency declares a rust-version"),
    }
}

/// Rules on the root manifest and on the fuzz project, which no `cargo metadata` call sees.
fn root_violations(
    root: &Path,
    architecture: &Architecture,
    examined: &mut Examined,
) -> Result<Vec<Violation>> {
    let root_manifest = read(&root.join("Cargo.toml"))?;
    let mut violations = manifest_rules::check_root(&root_manifest)?;
    let fuzz_dir = root.join(FUZZ_DIR);
    let fuzz_manifest_path = fuzz_dir.join("Cargo.toml");
    let fuzz_manifest = read(&fuzz_manifest_path)?;
    violations.extend(manifest_rules::check_mirrored_lints(
        &root_manifest,
        &fuzz_manifest,
        Path::new(FUZZ_DIR).join("Cargo.toml").as_path(),
    )?);
    let fuzz_sources = load_nonempty_sources(root, &fuzz_dir, FUZZ_DIR, examined)?;
    violations.extend(source_rules::check(architecture, FUZZ_DIR, &fuzz_sources));
    Ok(violations)
}

fn read(path: &Path) -> Result<String> {
    fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))
}

fn crate_violations(
    root: &Path,
    architecture: &Architecture,
    krate: &WorkspaceCrate,
    examined: &mut Examined,
) -> Result<Vec<Violation>> {
    let manifest = read(&krate.dir.join("Cargo.toml"))?;
    let mut violations = manifest_rules::check(&krate.name, &manifest)?;
    let sources = load_nonempty_sources(root, &krate.dir, &krate.name, examined)?;
    violations.extend(source_rules::check(architecture, &krate.name, &sources));
    Ok(violations)
}

/// Loads the sources under `dir` and counts them; a project without any is an error, because
/// every crate has at least a `lib.rs` or `main.rs`, so zero files means the scan missed it.
fn load_nonempty_sources(
    root: &Path,
    dir: &Path,
    name: &str,
    examined: &mut Examined,
) -> Result<Vec<SourceFile>> {
    let sources = workspace::load_sources(root, dir)?;
    ensure!(
        !sources.is_empty(),
        "architecture: found no source files for `{name}` in {}",
        dir.display()
    );
    examined.files += sources.len();
    Ok(sources)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn examined(crates: usize, files: usize, packages: usize) -> Examined {
        Examined {
            crates,
            files,
            packages,
            msrv: "MSRV 1.98.0".into(),
        }
    }

    #[test]
    fn a_run_that_examined_nothing_fails() {
        assert!(
            examined(4, 30, 130).ensure_nonempty().is_ok(),
            "complete run"
        );
        for empty in [
            examined(0, 30, 130),
            examined(4, 0, 130),
            examined(4, 30, 0),
        ] {
            assert!(empty.ensure_nonempty().is_err(), "{empty:?}");
        }
    }

    #[test]
    fn a_crate_without_source_files_is_an_error() {
        let dir = std::env::temp_dir().join(format!("xtask-empty-crate-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let mut examined = Examined::default();
        let result = load_nonempty_sources(&dir, &dir, "empty", &mut examined);
        fs::remove_dir_all(&dir).unwrap();
        let error = result.unwrap_err().to_string();
        assert!(error.contains("no source files for `empty`"), "{error}");
        assert_eq!(examined.files, 0, "nothing counted");
    }

    #[test]
    fn summary_is_one_terse_line() {
        assert_eq!(
            examined(4, 30, 130).summary(),
            "4 crates, 30 source files, 130 locked packages (MSRV 1.98.0)"
        );
    }
}
