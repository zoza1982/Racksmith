//! The MSRV rule: no package in the locked dependency graph may need a newer Rust than the
//! workspace's declared `rust-version`.
//!
//! The CI `MSRV` job builds with that toolchain, but only on Linux and only after the fact; this
//! rule names the offending package on every platform, before anything is compiled.

use cargo_metadata::semver::Version;

use crate::violation::Violation;
use crate::workspace::ResolvedPackage;

/// The outcome of the MSRV rule, with what it compared for the summary line.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct MsrvReport<'a> {
    /// The workspace MSRV: the lowest `rust-version` any member declares.
    pub(crate) msrv: Option<&'a Version>,
    /// The dependency with the highest `rust-version`, if any declares one.
    pub(crate) highest: Option<&'a ResolvedPackage>,
    /// Violations found.
    pub(crate) violations: Vec<Violation>,
}

/// Checks every non-member package against the workspace MSRV.
pub(crate) fn check(packages: &[ResolvedPackage]) -> MsrvReport<'_> {
    let msrv = packages
        .iter()
        .filter(|package| package.is_member)
        .filter_map(|package| package.rust_version.as_ref())
        .min();
    let dependencies = packages.iter().filter(|package| !package.is_member);
    let highest = dependencies
        .clone()
        .filter(|package| package.rust_version.is_some())
        .max_by(|a, b| a.rust_version.cmp(&b.rust_version));
    let violations = match msrv {
        None => vec![Violation::MissingRustVersion],
        Some(msrv) => dependencies
            .filter_map(|package| {
                let needed = package.rust_version.as_ref()?;
                (needed > msrv).then(|| Violation::DependencyNeedsNewerRust {
                    package: format!("{} {}", package.name, package.version),
                    rust_version: needed.to_string(),
                    msrv: msrv.to_string(),
                })
            })
            .collect(),
    };
    MsrvReport {
        msrv,
        highest,
        violations,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn package(name: &str, rust_version: Option<&str>, is_member: bool) -> ResolvedPackage {
        ResolvedPackage {
            name: name.to_owned(),
            version: Version::new(1, 0, 0),
            rust_version: rust_version.map(|version| Version::parse(version).unwrap()),
            is_member,
        }
    }

    #[test]
    fn dependencies_within_the_msrv_pass() {
        let packages = [
            package("app", Some("1.98.0"), true),
            package("old", Some("1.70.0"), false),
            package("exact", Some("1.98.0"), false),
            package("undeclared", None, false),
        ];
        let report = check(&packages);
        assert_eq!(report.violations, vec![]);
        assert_eq!(report.msrv, Some(&Version::new(1, 98, 0)));
        assert_eq!(report.highest.map(|p| p.name.as_str()), Some("exact"));
    }

    #[test]
    fn dependency_needing_a_newer_rust_is_reported() {
        let packages = [
            package("app", Some("1.98.0"), true),
            package("tool", Some("1.98.0"), true),
            // A patch release counts: the MSRV job installs 1.98.0, not the newest 1.98.x.
            package("fresh", Some("1.98.1"), false),
            package("newer", Some("1.99.0"), false),
        ];
        let report = check(&packages);
        assert_eq!(
            report.violations,
            vec![
                Violation::DependencyNeedsNewerRust {
                    package: "fresh 1.0.0".into(),
                    rust_version: "1.98.1".into(),
                    msrv: "1.98.0".into(),
                },
                Violation::DependencyNeedsNewerRust {
                    package: "newer 1.0.0".into(),
                    rust_version: "1.99.0".into(),
                    msrv: "1.98.0".into(),
                },
            ]
        );
        assert_eq!(report.highest.map(|p| p.name.as_str()), Some("newer"));
    }

    #[test]
    fn members_without_a_rust_version_are_reported() {
        let packages = [
            package("app", None, true),
            package("dep", Some("1.0.0"), false),
        ];
        assert_eq!(
            check(&packages).violations,
            vec![Violation::MissingRustVersion]
        );
    }

    #[test]
    fn the_lowest_member_rust_version_is_the_msrv() {
        let packages = [
            package("app", Some("1.98.0"), true),
            package("tool", Some("1.99.0"), true),
            package("dep", Some("1.99.0"), false),
        ];
        assert_eq!(check(&packages).violations.len(), 1, "1.99 > 1.98");
    }
}
