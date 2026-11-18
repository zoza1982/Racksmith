//! Manifest rules, checked on the raw `Cargo.toml` text because `cargo metadata` resolves
//! `workspace = true` and target paths away.
//!
//! - Members inherit the workspace lint policy and workspace dependency versions, and keep every
//!   target and build script inside the crate directory.
//! - The root manifest uses no key that substitutes code behind the rules' back.
//! - The standalone fuzz project restates the workspace lint policy exactly.

use std::path::{Component, Path};

use anyhow::{Context as _, Result};
use toml::{Table, Value};

use crate::violation::Violation;

const DEPENDENCY_SECTIONS: [&str; 3] = ["dependencies", "dev-dependencies", "build-dependencies"];

/// Root-manifest keys that replace or exclude code, as `(table, key)`; an empty table means a
/// top-level key.
const FORBIDDEN_ROOT_KEYS: [(&str, &str); 3] =
    [("", "patch"), ("", "replace"), ("workspace", "exclude")];

/// Target tables whose `path` must stay inside the crate: `[lib]` and the arrays of tables.
const TARGET_TABLES: [&str; 5] = ["lib", "bin", "test", "bench", "example"];

/// Checks one member manifest.
///
/// # Errors
///
/// Returns an error if `manifest` is not valid TOML.
pub(crate) fn check(krate: &str, manifest: &str) -> Result<Vec<Violation>> {
    let manifest: Table = toml::from_str(manifest)
        .with_context(|| format!("Cargo.toml of `{krate}` is not valid TOML"))?;
    let mut violations = Vec::new();
    if !inherits_workspace_lints(&manifest) {
        violations.push(Violation::MissingWorkspaceLints {
            krate: krate.to_owned(),
        });
    }
    violations.extend(targets_outside_crate(krate, &manifest));
    for (section, deps) in dependency_tables(&manifest) {
        violations.extend(
            deps.iter()
                .filter(|(_, spec)| !inherits_workspace(spec))
                .map(|(dep, _)| Violation::LocalDependencySpec {
                    krate: krate.to_owned(),
                    section: section.clone(),
                    dep: dep.clone(),
                }),
        );
    }
    Ok(violations)
}

/// Checks the root manifest for keys that substitute code no workspace rule checks.
///
/// # Errors
///
/// Returns an error if `manifest` is not valid TOML.
pub(crate) fn check_root(manifest: &str) -> Result<Vec<Violation>> {
    let manifest: Table =
        toml::from_str(manifest).context("the root Cargo.toml is not valid TOML")?;
    Ok(FORBIDDEN_ROOT_KEYS
        .into_iter()
        .filter(|&(table, key)| {
            let scope = if table.is_empty() {
                Some(&manifest)
            } else {
                manifest.get(table).and_then(Value::as_table)
            };
            scope.is_some_and(|scope| scope.contains_key(key))
        })
        .map(|(table, key)| Violation::ForbiddenRootManifestKey {
            key: if table.is_empty() {
                key.to_owned()
            } else {
                format!("{table}.{key}")
            },
        })
        .collect())
}

/// Checks that `mirror` (at `mirror_path`) has a `[lints]` table equal to the root
/// `[workspace.lints]`, for a project that cannot inherit it.
///
/// # Errors
///
/// Returns an error if either manifest is not valid TOML.
pub(crate) fn check_mirrored_lints(
    root: &str,
    mirror: &str,
    mirror_path: &Path,
) -> Result<Option<Violation>> {
    let root: Table = toml::from_str(root).context("the root Cargo.toml is not valid TOML")?;
    let mirror: Table = toml::from_str(mirror)
        .with_context(|| format!("{} is not valid TOML", mirror_path.display()))?;
    let workspace_lints = root
        .get("workspace")
        .and_then(|workspace| workspace.get("lints"));
    Ok(
        (workspace_lints.is_none() || workspace_lints != mirror.get("lints")).then(|| {
            Violation::MirroredLintsDiffer {
                path: mirror_path.to_path_buf(),
            }
        }),
    )
}

/// Targets and build scripts whose path leaves the crate directory.
fn targets_outside_crate(krate: &str, manifest: &Table) -> Vec<Violation> {
    let build = manifest
        .get("package")
        .and_then(|package| package.get("build"))
        .and_then(Value::as_str)
        .map(|path| ("package.build".to_owned(), path));
    let targets = TARGET_TABLES.into_iter().flat_map(|name| {
        let tables: Vec<&Table> = match manifest.get(name) {
            Some(Value::Table(table)) => vec![table],
            Some(Value::Array(array)) => array.iter().filter_map(Value::as_table).collect(),
            _ => Vec::new(),
        };
        tables
            .into_iter()
            .filter_map(move |table| Some((format!("{name}.path"), table.get("path")?.as_str()?)))
    });
    build
        .into_iter()
        .chain(targets)
        .filter(|(_, path)| leaves_directory(path))
        .map(|(key, path)| Violation::TargetOutsideCrate {
            krate: krate.to_owned(),
            key,
            target: path.to_owned(),
        })
        .collect()
}

/// Whether the relative-path text `path` is absolute or climbs out with `..`.
fn leaves_directory(path: &str) -> bool {
    Path::new(path).components().any(|component| {
        matches!(
            component,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    })
}

fn inherits_workspace_lints(manifest: &Table) -> bool {
    manifest
        .get("lints")
        .and_then(|lints| lints.get("workspace"))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn inherits_workspace(spec: &Value) -> bool {
    spec.get("workspace")
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

/// Every dependency table in the manifest with its section name, including
/// `[target.<cfg>.*dependencies]` tables.
fn dependency_tables(manifest: &Table) -> Vec<(String, &Table)> {
    let top_level = sections_of(manifest).map(|(name, table)| (name.to_owned(), table));
    let per_target = manifest
        .get("target")
        .and_then(Value::as_table)
        .into_iter()
        .flatten()
        .filter_map(|(target, value)| value.as_table().map(|table| (target, table)))
        .flat_map(|(target, table)| {
            sections_of(table).map(move |(name, deps)| (format!("target.'{target}'.{name}"), deps))
        });
    top_level.chain(per_target).collect()
}

fn sections_of(table: &Table) -> impl Iterator<Item = (&'static str, &Table)> {
    DEPENDENCY_SECTIONS.into_iter().filter_map(|name| {
        table
            .get(name)
            .and_then(Value::as_table)
            .map(|deps| (name, deps))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compliant_manifest_has_no_violations() {
        let manifest = r#"
            [package]
            name = "demo"

            [dependencies]
            serde = { workspace = true, features = ["derive"] }
            thiserror.workspace = true

            [lints]
            workspace = true
        "#;
        assert_eq!(check("demo", manifest).unwrap(), vec![]);
    }

    #[test]
    fn missing_lints_table_is_reported() {
        let violations = check("demo", "[package]\nname = \"demo\"\n").unwrap();
        assert_eq!(
            violations,
            vec![Violation::MissingWorkspaceLints {
                krate: "demo".into()
            }]
        );
    }

    #[test]
    fn targets_outside_the_crate_are_reported() {
        let manifest = r#"
            [package]
            build = "../shared/build.rs"

            [lib]
            path = "src/lib.rs"

            [[bin]]
            name = "tool"
            path = "/tmp/tool.rs"

            [lints]
            workspace = true
        "#;
        let keys: Vec<String> = check("demo", manifest)
            .unwrap()
            .into_iter()
            .map(|violation| {
                let Violation::TargetOutsideCrate { key, .. } = violation else {
                    panic!("unexpected violation: {violation}");
                };
                key
            })
            .collect();
        assert_eq!(keys, ["package.build", "bin.path"]);
    }

    #[test]
    fn root_keys_that_substitute_code_are_reported() {
        let manifest = r#"
            [workspace]
            members = ["crates/*"]
            exclude = ["vendor"]

            [patch.crates-io]
            serde = { path = "vendor/serde" }
        "#;
        let keys: Vec<String> = check_root(manifest)
            .unwrap()
            .into_iter()
            .map(|violation| violation.to_string())
            .collect();
        assert_eq!(keys.len(), 2, "{keys:?}");
        assert!(keys[0].contains("`patch`"), "{keys:?}");
        assert!(keys[1].contains("`workspace.exclude`"), "{keys:?}");
        assert_eq!(check_root("[workspace]\nmembers = []\n").unwrap(), vec![]);
    }

    #[test]
    fn mirrored_lints_must_equal_the_workspace_policy() {
        let root = "[workspace.lints.clippy]\nunwrap_used = \"deny\"\n";
        let path = Path::new("fuzz/Cargo.toml");
        let same = "[lints.clippy]\nunwrap_used = \"deny\"\n";
        assert_eq!(check_mirrored_lints(root, same, path).unwrap(), None);
        let weaker = "[lints.clippy]\nunwrap_used = \"warn\"\n";
        assert_eq!(
            check_mirrored_lints(root, weaker, path).unwrap(),
            Some(Violation::MirroredLintsDiffer {
                path: path.to_path_buf()
            })
        );
    }

    #[test]
    fn crate_local_versions_are_reported_in_every_section() {
        let manifest = r#"
            [dependencies]
            serde = "1"

            [dev-dependencies]
            proptest = { version = "1", workspace = false }

            [target.'cfg(unix)'.build-dependencies]
            cc = { path = "../cc" }

            [lints]
            workspace = true
        "#;
        let sections: Vec<String> = check("demo", manifest)
            .unwrap()
            .into_iter()
            .map(|violation| {
                let Violation::LocalDependencySpec { section, dep, .. } = violation else {
                    panic!("unexpected violation: {violation}");
                };
                format!("{section}:{dep}")
            })
            .collect();
        assert_eq!(
            sections,
            [
                "dependencies:serde",
                "dev-dependencies:proptest",
                "target.'cfg(unix)'.build-dependencies:cc"
            ]
        );
    }
}
