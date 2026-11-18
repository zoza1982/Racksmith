//! Dependency-graph rules: declared crates, trusted sources, strict layering, allowlists and
//! bans.

use std::collections::BTreeSet;

use crate::architecture::{Architecture, CrateRules};
use crate::violation::{DepKind, Violation};
use crate::workspace::{DepOrigin, Dependency, WorkspaceCrate, crate_names};

/// Checks every crate's declared dependencies against the architecture policy.
pub(crate) fn check(architecture: &Architecture, crates: &[WorkspaceCrate]) -> Vec<Violation> {
    let members = crate_names(crates);
    let mut violations = stale_declarations(architecture, &members);
    for krate in crates {
        violations.extend(banned_dependencies(architecture, krate));
        let Some(rules) = architecture.crates.get(&krate.name) else {
            violations.push(Violation::UndeclaredCrate {
                krate: krate.name.clone(),
            });
            continue;
        };
        for dep in &krate.dependencies {
            let is_member = members.contains(dep.name.as_str());
            let violation = match (&dep.origin, is_member) {
                (DepOrigin::Member, true) => {
                    check_workspace_dep(architecture, &krate.name, rules, dep)
                }
                (DepOrigin::CratesIo, false) => check_external_dep(&krate.name, rules, dep),
                (origin, _) => Some(untrusted_source(&krate.name, dep, origin)),
            };
            violations.extend(violation);
        }
    }
    violations
}

/// A dependency whose code no rule checks: an external crate not from crates.io, or a crate
/// that borrows a workspace member's name without being that member.
fn untrusted_source(krate: &str, dep: &Dependency, origin: &DepOrigin) -> Violation {
    let origin = match origin {
        DepOrigin::Other(description) => description.clone(),
        DepOrigin::Member => "a workspace member with a different name".to_owned(),
        DepOrigin::CratesIo => "crates.io, but it shares a workspace member's name".to_owned(),
    };
    Violation::UntrustedSource {
        krate: krate.to_owned(),
        dep: dep.name.clone(),
        origin,
    }
}

fn stale_declarations(architecture: &Architecture, members: &BTreeSet<&str>) -> Vec<Violation> {
    architecture
        .crates
        .keys()
        .filter(|name| !members.contains(name.as_str()))
        .map(|name| Violation::StaleDeclaration {
            krate: name.clone(),
        })
        .collect()
}

fn banned_dependencies(architecture: &Architecture, krate: &WorkspaceCrate) -> Vec<Violation> {
    architecture
        .dependency_bans
        .iter()
        .filter(|ban| !ban.allowed_in.contains(&krate.name))
        .filter(|ban| krate.dependencies.iter().any(|dep| dep.name == ban.name))
        .map(|ban| Violation::BannedDependency {
            krate: krate.name.clone(),
            dep: ban.name.clone(),
            reason: ban.reason.clone(),
        })
        .collect()
}

fn check_workspace_dep(
    architecture: &Architecture,
    krate: &str,
    rules: &CrateRules,
    dep: &Dependency,
) -> Option<Violation> {
    if !rules.workspace_deps.contains(&dep.name) {
        return Some(Violation::WorkspaceDepNotAllowed {
            krate: krate.to_owned(),
            dep: dep.name.clone(),
        });
    }
    // An undeclared dependency target is reported as `UndeclaredCrate` on its own entry.
    let dep_layer = architecture.crates.get(&dep.name)?.layer;
    (dep_layer >= rules.layer).then(|| Violation::LayerInversion {
        krate: krate.to_owned(),
        layer: rules.layer,
        dep: dep.name.clone(),
        dep_layer,
    })
}

fn check_external_dep(krate: &str, rules: &CrateRules, dep: &Dependency) -> Option<Violation> {
    let allowlist = match dep.kind {
        DepKind::Normal => &rules.deps,
        DepKind::Dev => &rules.dev_deps,
        DepKind::Build => &rules.build_deps,
    };
    (!allowlist.contains(&dep.name)).then(|| Violation::ExternalDepNotAllowed {
        krate: krate.to_owned(),
        dep: dep.name.clone(),
        kind: dep.kind,
    })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    const POLICY: &str = r#"
        [crates.domain]
        layer = 0
        workspace-deps = []
        deps = ["serde"]
        dev-deps = ["proptest"]
        build-deps = []

        [crates.api]
        layer = 1
        workspace-deps = ["domain"]
        deps = ["axum", "anyhow"]
        dev-deps = []
        build-deps = []

        [crates.cli]
        layer = 2
        workspace-deps = ["api", "domain"]
        deps = ["anyhow"]
        dev-deps = []
        build-deps = []

        [[dependency-bans]]
        name = "anyhow"
        allowed-in = ["cli"]
        reason = "libraries use typed errors"
    "#;

    /// Crate names that the test workspaces treat as members; everything else is external.
    const MEMBERS: [&str; 4] = ["domain", "api", "cli", "daemon"];

    /// A crate whose dependencies come from their usual origin: members by path, the rest from
    /// crates.io.
    fn krate(name: &str, deps: &[(&str, DepKind)]) -> WorkspaceCrate {
        let origin = |dep: &str| {
            if MEMBERS.contains(&dep) {
                DepOrigin::Member
            } else {
                DepOrigin::CratesIo
            }
        };
        WorkspaceCrate {
            name: name.to_owned(),
            dir: PathBuf::from(name),
            dependencies: deps
                .iter()
                .map(|(dep, kind)| Dependency {
                    name: (*dep).to_owned(),
                    kind: *kind,
                    origin: origin(dep),
                })
                .collect(),
        }
    }

    fn check_policy(crates: &[WorkspaceCrate]) -> Vec<Violation> {
        check(&Architecture::parse(POLICY).unwrap(), crates)
    }

    fn compliant_workspace() -> Vec<WorkspaceCrate> {
        vec![
            krate(
                "domain",
                &[("serde", DepKind::Normal), ("proptest", DepKind::Dev)],
            ),
            krate(
                "api",
                &[("domain", DepKind::Normal), ("axum", DepKind::Normal)],
            ),
            krate(
                "cli",
                &[("api", DepKind::Normal), ("anyhow", DepKind::Normal)],
            ),
        ]
    }

    #[test]
    fn compliant_workspace_has_no_violations() {
        assert_eq!(check_policy(&compliant_workspace()), vec![]);
    }

    #[test]
    fn allowlisted_name_from_an_untrusted_source_is_reported() {
        let mut crates = compliant_workspace();
        crates[0].dependencies[0].origin =
            DepOrigin::Other("the path `vendor/serde` outside the workspace".into());
        assert_eq!(
            check_policy(&crates),
            vec![Violation::UntrustedSource {
                krate: "domain".into(),
                dep: "serde".into(),
                origin: "the path `vendor/serde` outside the workspace".into(),
            }]
        );
    }

    #[test]
    fn registry_crate_named_like_a_member_is_reported() {
        let mut crates = compliant_workspace();
        crates[1].dependencies[0].origin = DepOrigin::CratesIo;
        let violations = check_policy(&crates);
        assert!(
            matches!(&violations[..], [Violation::UntrustedSource { dep, .. }] if dep == "domain"),
            "{violations:?}"
        );
    }

    #[test]
    fn upward_dependency_is_a_layer_violation() {
        let mut crates = compliant_workspace();
        crates[0] = krate("domain", &[("api", DepKind::Normal)]);
        let violations = check_policy(&crates);
        assert_eq!(
            violations,
            vec![Violation::WorkspaceDepNotAllowed {
                krate: "domain".into(),
                dep: "api".into()
            }]
        );
    }

    #[test]
    fn allowlisted_same_layer_dependency_is_still_a_layer_violation() {
        // Move `api` up into `cli`'s layer; `cli -> api` stays allowlisted.
        let policy = POLICY.replace("layer = 1", "layer = 2");
        let violations = check(
            &Architecture::parse(&policy).unwrap(),
            &compliant_workspace(),
        );
        assert_eq!(
            violations,
            vec![Violation::LayerInversion {
                krate: "cli".into(),
                layer: 2,
                dep: "api".into(),
                dep_layer: 2,
            }]
        );
    }

    #[test]
    fn external_dependency_must_be_allowlisted_for_its_kind() {
        let mut crates = compliant_workspace();
        // `proptest` is allowed as a dev-dependency of `domain`, not as a normal one.
        crates[0] = krate("domain", &[("proptest", DepKind::Normal)]);
        assert_eq!(
            check_policy(&crates),
            vec![Violation::ExternalDepNotAllowed {
                krate: "domain".into(),
                dep: "proptest".into(),
                kind: DepKind::Normal,
            }]
        );
    }

    #[test]
    fn ban_applies_even_when_allowlisted() {
        let mut crates = compliant_workspace();
        crates[1] = krate("api", &[("anyhow", DepKind::Normal)]);
        assert_eq!(
            check_policy(&crates),
            vec![Violation::BannedDependency {
                krate: "api".into(),
                dep: "anyhow".into(),
                reason: "libraries use typed errors".into(),
            }]
        );
    }

    #[test]
    fn undeclared_and_stale_crates_are_reported() {
        let mut crates = compliant_workspace();
        crates[2] = krate("daemon", &[]);
        let violations = check_policy(&crates);
        assert!(violations.contains(&Violation::UndeclaredCrate {
            krate: "daemon".into()
        }));
        assert!(violations.contains(&Violation::StaleDeclaration {
            krate: "cli".into()
        }));
    }

    #[test]
    fn layer_violation_message_is_actionable() {
        let message = Violation::LayerInversion {
            krate: "domain".into(),
            layer: 0,
            dep: "api".into(),
            dep_layer: 1,
        }
        .to_string();
        assert!(
            message.contains("`domain` (layer 0) depends on `api` (layer 1)"),
            "{message}"
        );
        assert!(message.contains("ADR"), "{message}");
    }
}
