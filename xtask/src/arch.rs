//! `cargo xtask arch`: enforce `architecture.toml` across the workspace.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result, bail, ensure};

use crate::architecture::Architecture;
use crate::design_sections::{self, DESIGN_FILE};
use crate::exemption_ledger::{ADR_DIR, Adrs, LEDGER_FILE, Ledger, References};
use crate::expect_attributes;
use crate::plan_tasks::{PLAN_FILE, PlanTasks};
use crate::spec::PRODUCT_PREFIX;
use crate::subsystem_rules::{self, CrateSources};
use crate::violation::DepKind;
use crate::violation::Violation;
use crate::workspace::{self, SourceFile, WorkspaceCrate};
use crate::{dependency_rules, manifest_rules, msrv_rules, source_rules, term, tracked_files};

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
    /// Subsystems of `architecture.toml` checked by rule 7.
    subsystems: usize,
    /// `#[expect]` exemptions checked against the ledger. Zero is a valid count: the sources
    /// were scanned (`files > 0`), and an entry of the ledger that matched nothing would fail
    /// as stale, so a scan that missed real attributes cannot pass silently.
    exemptions: usize,
    /// Files of the git index checked by rule 9 for build output and size.
    tracked_files: usize,
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
        ensure!(
            self.subsystems > 0,
            "architecture: architecture.toml has no [subsystems]; rule 7 examined nothing"
        );
        ensure!(
            self.tracked_files > 0,
            "architecture: git lists no tracked files; rule 9 examined nothing"
        );
        Ok(())
    }

    fn summary(&self) -> String {
        format!(
            "{} crates, {} source files, {} locked packages, {} subsystems, {} exemptions, {} \
             tracked files ({})",
            self.crates,
            self.files,
            self.packages,
            self.subsystems,
            self.exemptions,
            self.tracked_files,
            self.msrv
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
/// Returns an error if `architecture.toml`, a manifest, a source file, the dependency graph or
/// the git index cannot be read, or if a crate or the fuzz project has no source files.
fn collect_violations(root: &Path) -> Result<(Vec<Violation>, Examined)> {
    let policy = read(&root.join("architecture.toml"))?;
    let architecture = Architecture::parse(&policy)?;
    let crates = workspace::load_crates(root)?;
    let packages = workspace::load_resolved_packages(root)?;
    let msrv = msrv_rules::check(&packages);
    let plan = PlanTasks::parse(&read(&root.join(PLAN_FILE))?)?;

    let mut examined = Examined {
        crates: crates.len(),
        packages: packages.len(),
        subsystems: architecture.subsystems.len(),
        msrv: describe_msrv(&msrv),
        ..Examined::default()
    };
    let mut violations = dependency_rules::check(&architecture, &crates);
    violations.extend(msrv.violations);
    let (fuzz_violations, fuzz_sources) = root_violations(root, &architecture, &mut examined)?;
    violations.extend(fuzz_violations);
    let mut sources = Vec::with_capacity(crates.len());
    for krate in &crates {
        let (crate_violations, files) =
            crate_violations(root, &architecture, krate, &mut examined)?;
        violations.extend(crate_violations);
        sources.push(files);
    }
    violations.extend(subsystem_violations(
        root,
        &architecture,
        &plan,
        &crates,
        &sources,
    ));
    let all_sources: Vec<&SourceFile> = sources.iter().flatten().chain(&fuzz_sources).collect();
    let (ledger_violations, exemptions) = ledger_violations(root, &plan, &all_sources)?;
    violations.extend(ledger_violations);
    examined.exemptions = exemptions;
    let tracked = tracked_files::load(root)?;
    examined.tracked_files = tracked.len();
    violations.extend(tracked_files::check(&tracked));
    violations.sort();
    Ok((violations, examined))
}

/// Rule 8, the exemption ledger: every `#[expect]` of `sources` (test code included) against
/// `exceptions.toml`, with references resolved against the ADRs, `plan` and the design. Returns
/// the violations and the number of exemptions checked.
///
/// # Errors
///
/// Returns an error if the ledger, an ADR or the design cannot be read or parsed.
fn ledger_violations(
    root: &Path,
    plan: &PlanTasks,
    sources: &[&SourceFile],
) -> Result<(Vec<Violation>, usize)> {
    let ledger = Ledger::parse(&read(&root.join(LEDGER_FILE))?)?;
    let adrs = load_adrs(&root.join(ADR_DIR))?;
    let sections = design_sections::leaf_sections(&read(&root.join(DESIGN_FILE))?)?;
    let exemptions = expect_attributes::find(sources);
    let references = References {
        adrs: &adrs,
        tasks: plan,
        sections: &sections,
    };
    Ok((ledger.check(&exemptions, references), exemptions.len()))
}

/// The ADRs in `dir`.
///
/// # Errors
///
/// Returns an error if the directory or a Markdown file in it cannot be read.
fn load_adrs(dir: &Path) -> Result<Adrs> {
    let mut files = Vec::new();
    let entries = fs::read_dir(dir).with_context(|| format!("failed to list {}", dir.display()))?;
    for entry in entries {
        let path = entry
            .with_context(|| format!("failed to list {}", dir.display()))?
            .path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if path.extension().is_some_and(|extension| extension == "md") {
            files.push((name.to_owned(), read(&path)?));
        }
    }
    Ok(Adrs::parse(
        files
            .iter()
            .map(|(name, text)| (name.as_str(), text.as_str())),
    ))
}

/// Rule 7, on every workspace crate's sources.
fn subsystem_violations(
    root: &Path,
    architecture: &Architecture,
    plan: &PlanTasks,
    crates: &[WorkspaceCrate],
    sources: &[Vec<SourceFile>],
) -> Vec<Violation> {
    if architecture.subsystems.is_empty() {
        return Vec::new();
    }
    let composition_root = composition_root(architecture);
    let crate_sources: Vec<CrateSources<'_>> = crates
        .iter()
        .zip(sources)
        .map(|(krate, files)| {
            let binary_files = binary_target_files(root, krate, files);
            CrateSources {
                name: &krate.name,
                is_composition_root: composition_root == Some(krate.name.as_str())
                    && !binary_files.is_empty(),
                binary_files,
                dependencies: krate
                    .dependencies
                    .iter()
                    .filter(|dependency| dependency.kind == DepKind::Normal)
                    .map(|dependency| dependency.name.as_str())
                    .collect(),
                files,
            }
        })
        .collect();
    subsystem_rules::check(architecture, plan, &crate_sources)
}

/// The composition root: the single product crate in the highest layer of `architecture`, or
/// `None` if several share that layer.
fn composition_root(architecture: &Architecture) -> Option<&str> {
    let product = || {
        architecture
            .crates
            .iter()
            .filter(|(name, _)| name.starts_with(PRODUCT_PREFIX))
    };
    let top = product().map(|(_, rules)| rules.layer).max()?;
    let mut at_top = product().filter(|(_, rules)| rules.layer == top);
    match (at_top.next(), at_top.next()) {
        (Some((name, _)), None) => Some(name.as_str()),
        _ => None,
    }
}

/// The non-test files of the crate's binary targets in the standard layout: `src/main.rs` and
/// `src/bin/`, or every non-test file of a crate without `src/lib.rs`. Empty for a library.
fn binary_target_files(root: &Path, krate: &WorkspaceCrate, files: &[SourceFile]) -> Vec<PathBuf> {
    let src = krate
        .dir
        .strip_prefix(root)
        .unwrap_or(&krate.dir)
        .join("src");
    let product = || files.iter().filter(|file| !file.is_test_code);
    let is_target = |file: &&SourceFile| {
        file.path == src.join("main.rs") || file.path.starts_with(src.join("bin"))
    };
    if !product().any(|file| is_target(&file)) {
        return Vec::new();
    }
    let has_library = product().any(|file| file.path == src.join("lib.rs"));
    product()
        .filter(|file| !has_library || is_target(file))
        .map(|file| file.path.clone())
        .collect()
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

/// Rules on the root manifest and on the fuzz project, which no `cargo metadata` call sees,
/// and the fuzz project's sources for the rules that span projects.
fn root_violations(
    root: &Path,
    architecture: &Architecture,
    examined: &mut Examined,
) -> Result<(Vec<Violation>, Vec<SourceFile>)> {
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
    Ok((violations, fuzz_sources))
}

fn read(path: &Path) -> Result<String> {
    fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))
}

/// The manifest and source violations of one crate, and its sources for the rules that span
/// crates.
fn crate_violations(
    root: &Path,
    architecture: &Architecture,
    krate: &WorkspaceCrate,
    examined: &mut Examined,
) -> Result<(Vec<Violation>, Vec<SourceFile>)> {
    let manifest = read(&krate.dir.join("Cargo.toml"))?;
    let mut violations = manifest_rules::check(&krate.name, &manifest)?;
    let sources = load_nonempty_sources(root, &krate.dir, &krate.name, examined)?;
    violations.extend(source_rules::check(architecture, &krate.name, &sources));
    Ok((violations, sources))
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
            subsystems: 2,
            exemptions: 4,
            tracked_files: 150,
            msrv: "MSRV 1.98.0".into(),
        }
    }

    #[test]
    fn a_run_that_examined_nothing_fails() {
        assert!(
            examined(4, 30, 130).ensure_nonempty().is_ok(),
            "complete run"
        );
        let no_exemptions = Examined {
            exemptions: 0,
            ..examined(4, 30, 130)
        };
        assert!(
            no_exemptions.ensure_nonempty().is_ok(),
            "a repository without exemptions is complete"
        );
        let no_subsystems = Examined {
            subsystems: 0,
            ..examined(4, 30, 130)
        };
        let no_tracked_files = Examined {
            tracked_files: 0,
            ..examined(4, 30, 130)
        };
        for empty in [
            examined(0, 30, 130),
            examined(4, 0, 130),
            examined(4, 30, 0),
            no_subsystems,
            no_tracked_files,
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
    fn binary_targets_are_main_and_bin_or_the_whole_of_a_binary_only_crate() {
        let root = Path::new("/repo");
        let krate = |name: &str| WorkspaceCrate {
            name: name.to_owned(),
            dir: root.join("crates").join(name),
            dependencies: Vec::new(),
        };
        let file = |path: &str, is_test_code| SourceFile {
            path: path.into(),
            is_test_code,
            text: String::new(),
        };
        let paths = |files: &[&str]| -> Vec<PathBuf> { files.iter().map(PathBuf::from).collect() };
        let cli = [
            file("crates/cli/src/main.rs", false),
            file("crates/cli/src/commands.rs", false),
            file("crates/cli/tests/cli.rs", true),
        ];
        assert_eq!(
            binary_target_files(root, &krate("cli"), &cli),
            paths(&["crates/cli/src/main.rs", "crates/cli/src/commands.rs"]),
            "a binary-only crate"
        );
        let mixed = [
            file("crates/tool/src/lib.rs", false),
            file("crates/tool/src/registry.rs", false),
            file("crates/tool/src/bin/extra.rs", false),
        ];
        assert_eq!(
            binary_target_files(root, &krate("tool"), &mixed),
            paths(&["crates/tool/src/bin/extra.rs"]),
            "only the binary half of a library with a stray src/bin/"
        );
        let lib = [
            file("crates/lib/src/lib.rs", false),
            file("crates/lib/examples/main.rs", true),
        ];
        assert!(
            binary_target_files(root, &krate("lib"), &lib).is_empty(),
            "a library with an example"
        );
    }

    #[test]
    fn the_composition_root_is_the_single_top_layer_product_crate() {
        let crates = |layers: &[(&str, u8)]| {
            let text = layers
                .iter()
                .map(|(name, layer)| {
                    format!(
                        "[crates.{name}]\nlayer = {layer}\nworkspace-deps = []\ndeps = []\n\
                         dev-deps = []\nbuild-deps = []\n"
                    )
                })
                .collect::<Vec<_>>()
                .concat();
            Architecture::parse(&text).unwrap()
        };
        let repo = crates(&[
            ("racksmith-domain", 0),
            ("racksmith-api", 1),
            ("racksmith-cli", 2),
            ("xtask", 3),
        ]);
        assert_eq!(composition_root(&repo), Some("racksmith-cli"));
        let tied = crates(&[("racksmith-api", 1), ("racksmith-server", 1)]);
        assert_eq!(composition_root(&tied), None, "no single root");
    }

    #[test]
    fn summary_is_one_terse_line() {
        assert_eq!(
            examined(4, 30, 130).summary(),
            "4 crates, 30 source files, 130 locked packages, 2 subsystems, 4 exemptions, 150 \
             tracked files (MSRV 1.98.0)"
        );
    }
}
