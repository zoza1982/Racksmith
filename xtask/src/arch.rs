//! `cargo xtask arch`: enforce `architecture.toml` across the workspace.

use std::fs;
use std::path::Path;

use anyhow::{Context as _, Result, bail};

use crate::architecture::Architecture;
use crate::violation::Violation;
use crate::workspace::{self, WorkspaceCrate};
use crate::{dependency_rules, manifest_rules, source_rules, term};

/// The standalone fuzz project: not a workspace member, but held to the same lint policy and
/// source rules.
const FUZZ_DIR: &str = "fuzz";

/// Runs every architecture rule and reports all violations at once.
///
/// # Errors
///
/// Returns an error if the workspace cannot be inspected or any rule is violated.
pub(crate) fn run(root: &Path) -> Result<()> {
    let violations = collect_violations(root)?;
    if violations.is_empty() {
        term::info("architecture: all rules pass")?;
        return Ok(());
    }
    for violation in &violations {
        term::error(&violation.to_string())?;
    }
    bail!(
        "{} architecture violation(s); see architecture.toml",
        violations.len()
    )
}

/// Gathers the violations of every rule, sorted for stable output.
///
/// # Errors
///
/// Returns an error if `architecture.toml`, a manifest or a source file cannot be read.
fn collect_violations(root: &Path) -> Result<Vec<Violation>> {
    let policy = read(&root.join("architecture.toml"))?;
    let architecture = Architecture::parse(&policy)?;
    let crates = workspace::load_crates(root)?;

    let mut violations = dependency_rules::check(&architecture, &crates);
    violations.extend(root_violations(root, &architecture)?);
    for krate in &crates {
        violations.extend(crate_violations(root, &architecture, krate)?);
    }
    violations.sort();
    Ok(violations)
}

/// Rules on the root manifest and on the fuzz project, which no `cargo metadata` call sees.
fn root_violations(root: &Path, architecture: &Architecture) -> Result<Vec<Violation>> {
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
    let fuzz_sources = workspace::load_sources(root, &fuzz_dir)?;
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
) -> Result<Vec<Violation>> {
    let manifest = read(&krate.dir.join("Cargo.toml"))?;
    let mut violations = manifest_rules::check(&krate.name, &manifest)?;
    let sources = workspace::load_sources(root, &krate.dir)?;
    violations.extend(source_rules::check(architecture, &krate.name, &sources));
    Ok(violations)
}
