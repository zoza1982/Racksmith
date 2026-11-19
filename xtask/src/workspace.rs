//! A snapshot of the workspace — crates, their dependency edges and source files — gathered
//! once from `cargo metadata` and the filesystem so that every rule is a pure function of it.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result, bail};
use cargo_metadata::camino::Utf8Path;
use cargo_metadata::semver::Version;
use cargo_metadata::{DependencyKind, MetadataCommand, Package};

use crate::process;
use crate::violation::DepKind;

/// A workspace member as seen by the architecture rules.
#[derive(Debug)]
pub(crate) struct WorkspaceCrate {
    /// Package name.
    pub(crate) name: String,
    /// Directory containing the crate's `Cargo.toml`.
    pub(crate) dir: PathBuf,
    /// Declared dependency edges (not the resolved graph).
    pub(crate) dependencies: Vec<Dependency>,
}

/// One declared dependency of a workspace crate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Dependency {
    /// Package name of the dependency (not its `rename`).
    pub(crate) name: String,
    /// Which manifest section declares it.
    pub(crate) kind: DepKind,
    /// Where the dependency's code comes from.
    pub(crate) origin: DepOrigin,
}

/// Where a dependency's code comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DepOrigin {
    /// A path dependency on a workspace member.
    Member,
    /// The crates.io registry.
    CratesIo,
    /// Anything else (another registry, git, a path outside the workspace), described for
    /// messages.
    Other(String),
}

/// A Rust source file belonging to a workspace crate.
#[derive(Debug)]
pub(crate) struct SourceFile {
    /// Path relative to the workspace root, for readable messages.
    pub(crate) path: PathBuf,
    /// Whether the file is test-only code: under the crate's `tests/`, `benches/` or `examples/`.
    pub(crate) is_test_code: bool,
    /// File contents.
    pub(crate) text: String,
}

/// A package of the resolved dependency graph (`Cargo.lock`), for the MSRV rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResolvedPackage {
    /// Package name.
    pub(crate) name: String,
    /// Package version.
    pub(crate) version: Version,
    /// The `rust-version` the package declares, if any.
    pub(crate) rust_version: Option<Version>,
    /// Whether the package is a workspace member.
    pub(crate) is_member: bool,
}

/// A `cargo metadata` call for the manifest at `root`, without xtask's package variables.
fn metadata_command(root: &Path) -> MetadataCommand {
    let mut command = MetadataCommand::new();
    command.manifest_path(root.join("Cargo.toml"));
    for name in process::inherited_package_vars() {
        command.env_remove(name);
    }
    command
}

/// Loads every package of the locked dependency graph, members included, for every platform
/// and feature combination that `Cargo.lock` records.
///
/// # Errors
///
/// Returns an error if `cargo metadata --locked` fails, for example on a stale `Cargo.lock`.
pub(crate) fn load_resolved_packages(root: &Path) -> Result<Vec<ResolvedPackage>> {
    let metadata = metadata_command(root)
        .other_options(vec!["--locked".to_owned()])
        .exec()
        .context("`cargo metadata --locked` failed")?;
    Ok(metadata
        .packages
        .iter()
        .map(|package| ResolvedPackage {
            name: package.name.to_string(),
            version: package.version.clone(),
            rust_version: package.rust_version.clone(),
            is_member: metadata.workspace_members.contains(&package.id),
        })
        .collect())
}

/// Loads every workspace member from `cargo metadata`.
///
/// # Errors
///
/// Returns an error if `cargo metadata` fails or reports an unknown dependency kind.
pub(crate) fn load_crates(root: &Path) -> Result<Vec<WorkspaceCrate>> {
    let metadata = metadata_command(root)
        .no_deps()
        .exec()
        .context("`cargo metadata` failed")?;
    let packages = metadata.workspace_packages();
    let member_dirs: BTreeSet<&Utf8Path> = packages
        .iter()
        .filter_map(|package| package.manifest_path.parent())
        .collect();
    packages
        .iter()
        .map(|package| workspace_crate(package, &member_dirs))
        .collect()
}

/// The names of the given crates, for classifying dependency edges as internal or external.
pub(crate) fn crate_names(crates: &[WorkspaceCrate]) -> BTreeSet<&str> {
    crates.iter().map(|krate| krate.name.as_str()).collect()
}

fn workspace_crate(package: &Package, member_dirs: &BTreeSet<&Utf8Path>) -> Result<WorkspaceCrate> {
    let dir = package
        .manifest_path
        .parent()
        .with_context(|| format!("manifest of `{}` has no parent directory", package.name))?;
    let dependencies = package
        .dependencies
        .iter()
        .map(|dep| {
            Ok(Dependency {
                name: dep.name.clone(),
                kind: dep_kind(dep.kind, &package.name, &dep.name)?,
                origin: dep_origin(dep, member_dirs),
            })
        })
        .collect::<Result<_>>()?;
    Ok(WorkspaceCrate {
        name: package.name.to_string(),
        dir: dir.into(),
        dependencies,
    })
}

fn dep_kind(kind: DependencyKind, krate: &str, dep: &str) -> Result<DepKind> {
    match kind {
        DependencyKind::Normal => Ok(DepKind::Normal),
        DependencyKind::Development => Ok(DepKind::Dev),
        DependencyKind::Build => Ok(DepKind::Build),
        DependencyKind::Unknown => {
            bail!("`{krate}` declares `{dep}` with a dependency kind xtask does not understand")
        }
    }
}

fn dep_origin(dep: &cargo_metadata::Dependency, member_dirs: &BTreeSet<&Utf8Path>) -> DepOrigin {
    match (&dep.path, &dep.source) {
        (Some(path), _) if member_dirs.contains(path.as_path()) => DepOrigin::Member,
        (Some(path), _) => DepOrigin::Other(format!("the path `{path}` outside the workspace")),
        (None, Some(source)) if source.is_crates_io() => DepOrigin::CratesIo,
        (None, Some(source)) => DepOrigin::Other(format!("the source `{source}`")),
        (None, None) => DepOrigin::Other("an unknown source".to_owned()),
    }
}

/// Directories under a crate that hold test-only code.
const TEST_CODE_DIRS: [&str; 3] = ["tests", "benches", "examples"];

/// Reads every `.rs` file under `crate_dir`, skipping only the crate's own `target/` directory.
///
/// # Errors
///
/// Returns an error if a directory cannot be listed or a file cannot be read.
pub(crate) fn load_sources(root: &Path, crate_dir: &Path) -> Result<Vec<SourceFile>> {
    let mut paths = Vec::new();
    collect_rust_files(crate_dir, &crate_dir.join("target"), &mut paths)?;
    paths.sort();
    paths
        .into_iter()
        .map(|path| {
            let text = fs::read_to_string(&path)
                .with_context(|| format!("failed to read {}", path.display()))?;
            Ok(SourceFile {
                is_test_code: TEST_CODE_DIRS
                    .iter()
                    .any(|dir| path.starts_with(crate_dir.join(dir))),
                path: path.strip_prefix(root).unwrap_or(&path).to_path_buf(),
                text,
            })
        })
        .collect()
}

fn collect_rust_files(dir: &Path, skip: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    let entries = fs::read_dir(dir).with_context(|| format!("failed to list {}", dir.display()))?;
    for entry in entries {
        let path = entry
            .with_context(|| format!("failed to list {}", dir.display()))?
            .path();
        if path.is_dir() {
            if path != skip {
                collect_rust_files(&path, skip, out)?;
            }
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::architecture::Architecture;
    use crate::dependency_rules;
    use crate::violation::Violation;

    /// A throwaway workspace on disk, removed on drop.
    struct Fixture(PathBuf);

    impl Fixture {
        fn new(name: &str, files: &[(&str, &str)]) -> Self {
            let root = std::env::temp_dir().join(format!("xtask-{name}-{}", std::process::id()));
            for (path, text) in files {
                let path = root.join(path);
                fs::create_dir_all(path.parent().unwrap()).unwrap();
                fs::write(path, text).unwrap();
            }
            Self(root)
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ignored = fs::remove_dir_all(&self.0);
        }
    }

    /// `alias = { package = "real" }`, in the member or in `[workspace.dependencies]`, is still
    /// checked as `real`: allowlists and bans match the package name, never the alias.
    #[test]
    fn renamed_dependencies_are_checked_by_package_name() {
        let fixture = Fixture::new(
            "renames",
            &[
                (
                    "Cargo.toml",
                    "[workspace]\nresolver = \"3\"\nmembers = [\"lib\"]\n\n\
                     [workspace.dependencies]\ntls = { package = \"openssl\", version = \"0.10\" }\n",
                ),
                (
                    "lib/Cargo.toml",
                    "[package]\nname = \"lib\"\nversion = \"0.0.0\"\nedition = \"2024\"\n\n\
                     [dependencies]\ntls.workspace = true\n\
                     serde = { package = \"anyhow\", version = \"1\" }\n",
                ),
                ("lib/src/lib.rs", ""),
            ],
        );
        let crates = load_crates(&fixture.0).unwrap();
        let names: Vec<&str> = crates[0]
            .dependencies
            .iter()
            .map(|dep| dep.name.as_str())
            .collect();
        assert_eq!(names, ["anyhow", "openssl"], "package names, not aliases");

        // `serde` is allowlisted, but the crate behind the alias is `anyhow`, which is banned.
        let policy = Architecture::parse(
            "[crates.lib]\nlayer = 0\nworkspace-deps = []\ndeps = [\"serde\", \"tls\"]\n\
             dev-deps = []\nbuild-deps = []\n\n\
             [[dependency-bans]]\nname = \"anyhow\"\nallowed-in = []\nreason = \"typed errors\"\n",
        )
        .unwrap();
        let violations = dependency_rules::check(&policy, &crates);
        assert!(
            violations.contains(&Violation::BannedDependency {
                krate: "lib".into(),
                dep: "anyhow".into(),
                reason: "typed errors".into(),
            }),
            "{violations:?}"
        );
        for dep in ["anyhow", "openssl"] {
            assert!(
                violations.contains(&Violation::ExternalDepNotAllowed {
                    krate: "lib".into(),
                    dep: dep.into(),
                    kind: DepKind::Normal,
                }),
                "an alias must not satisfy the allowlist: {violations:?}"
            );
        }
    }
}
