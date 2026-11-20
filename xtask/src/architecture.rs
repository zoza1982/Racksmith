//! The typed model of `architecture.toml`.
//!
//! Unknown keys are rejected so that a typo in the policy file fails loudly instead of
//! silently disabling a rule.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context as _, Result, bail};
use serde::Deserialize;

use crate::tokens;

/// The complete architecture policy for the workspace.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub(crate) struct Architecture {
    /// Per-crate rules, keyed by package name.
    pub(crate) crates: BTreeMap<String, CrateRules>,
    /// Crates that are banned everywhere except an explicit set of dependents.
    #[serde(default)]
    pub(crate) dependency_bans: Vec<DependencyBan>,
    /// Code patterns forbidden in the `src/` directory of specific crates.
    #[serde(default)]
    pub(crate) source_bans: Vec<SourceBan>,
    /// Designed subsystems, by name: each is wired into a consumer or planned (rule 7).
    #[serde(default)]
    pub(crate) subsystems: BTreeMap<String, Subsystem>,
}

/// Rules for a single workspace crate.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub(crate) struct CrateRules {
    /// Position in the layering; a crate may only depend on crates in strictly lower layers.
    pub(crate) layer: u8,
    /// Workspace crates this crate may depend on (any dependency kind).
    pub(crate) workspace_deps: BTreeSet<String>,
    /// External crates allowed as normal dependencies.
    pub(crate) deps: BTreeSet<String>,
    /// External crates allowed as dev-dependencies.
    pub(crate) dev_deps: BTreeSet<String>,
    /// External crates allowed as build-dependencies.
    pub(crate) build_deps: BTreeSet<String>,
}

/// A crate that only the listed dependents may use.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub(crate) struct DependencyBan {
    /// The banned crate's package name.
    pub(crate) name: String,
    /// The only workspace crates allowed to depend on it.
    pub(crate) allowed_in: BTreeSet<String>,
    /// Why the crate is banned, shown in violation messages.
    pub(crate) reason: String,
}

/// A code pattern that must not appear in the listed crates' `src/` directories.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub(crate) struct SourceBan {
    /// Crates the ban applies to.
    pub(crate) crates: BTreeSet<String>,
    /// The forbidden pattern, matched as tokens against code with comments and strings removed,
    /// and, if it is a path, against every path the code names or imports.
    pub(crate) pattern: String,
    /// Why the pattern is banned, shown in violation messages.
    pub(crate) reason: String,
}

/// A designed subsystem: an entry symbol that a consumer must call (`wired`), or that does not
/// exist yet because a plan task builds it (`planned`). Which fields a status needs is checked
/// by `cargo xtask arch`, so every problem is reported at once.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub(crate) struct Subsystem {
    /// Whether the subsystem is built and called.
    pub(crate) status: SubsystemStatus,
    /// The identifier of the subsystem's entry: a type, trait or function.
    pub(crate) entry: String,
    /// `wired`: the package whose non-test sources define `entry`.
    #[serde(rename = "crate")]
    pub(crate) krate: Option<String>,
    /// `wired`: the package whose non-test sources must use `entry`.
    pub(crate) consumer: Option<String>,
    /// `planned`: the plan tasks that build it, as `M1-A-01`.
    pub(crate) tasks: Option<Vec<String>>,
    /// One line on what the subsystem is.
    pub(crate) reason: String,
}

/// Whether a subsystem is wired into its consumer or still planned.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum SubsystemStatus {
    /// Built, and called by its consumer.
    Wired,
    /// Not built yet; tracked by plan tasks.
    Planned,
}

impl SubsystemStatus {
    /// The status as written in `architecture.toml`.
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Wired => "wired",
            Self::Planned => "planned",
        }
    }
}

impl Architecture {
    /// Parses the contents of `architecture.toml`.
    ///
    /// # Errors
    ///
    /// Returns an error if the text is not valid TOML or does not match the schema.
    pub(crate) fn parse(text: &str) -> Result<Self> {
        let architecture: Self =
            toml::from_str(text).context("architecture.toml does not match the expected schema")?;
        if let Some(ban) = architecture
            .source_bans
            .iter()
            .find(|ban| tokens::tokenize_str(&ban.pattern).is_empty())
        {
            bail!(
                "architecture.toml: source ban `{}` has an empty pattern",
                ban.reason
            );
        }
        Ok(architecture)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_complete_policy() {
        let architecture = Architecture::parse(
            r#"
            [crates.core]
            layer = 0
            workspace-deps = []
            deps = ["serde"]
            dev-deps = []
            build-deps = []

            [[dependency-bans]]
            name = "anyhow"
            allowed-in = ["app"]
            reason = "typed errors"

            [[source-bans]]
            crates = ["core"]
            pattern = "std::fs"
            reason = "pure"
            "#,
        )
        .unwrap();

        assert_eq!(architecture.crates["core"].layer, 0);
        assert!(architecture.crates["core"].deps.contains("serde"));
        assert_eq!(architecture.dependency_bans[0].name, "anyhow");
        assert_eq!(architecture.source_bans[0].pattern, "std::fs");
    }

    #[test]
    fn rejects_unknown_keys() {
        let result = Architecture::parse(
            r"
            [crates.core]
            layer = 0
            workspace-deps = []
            deps = []
            dev-deps = []
            build-deps = []
            dependencies = []
            ",
        );
        assert!(
            result.is_err(),
            "a misspelled key must not be silently ignored"
        );
    }

    #[test]
    fn rejects_empty_source_ban_patterns() {
        let result = Architecture::parse(
            r#"
            crates = {}

            [[source-bans]]
            crates = []
            pattern = "  "
            reason = "blank"
            "#,
        );
        assert!(result.is_err(), "an empty pattern would match nothing");
    }

    #[test]
    fn parses_subsystems_and_rejects_unknown_subsystem_keys() {
        let crates = "[crates.core]\nlayer = 0\nworkspace-deps = []\ndeps = []\n\
                      dev-deps = []\nbuild-deps = []\n";
        let architecture = Architecture::parse(&format!(
            "{crates}[subsystems.registry]\nstatus = \"wired\"\nentry = \"Registry\"\n\
             crate = \"core\"\nconsumer = \"app\"\nreason = \"stores machines\"\n\
             [subsystems.scheduler]\nstatus = \"planned\"\nentry = \"Scheduler\"\n\
             tasks = [\"M1-C-01\"]\nreason = \"places work\"\n"
        ))
        .unwrap();
        let registry = &architecture.subsystems["registry"];
        assert_eq!(registry.status, SubsystemStatus::Wired);
        assert_eq!(registry.krate.as_deref(), Some("core"));
        assert_eq!(
            architecture.subsystems["scheduler"].tasks,
            Some(vec!["M1-C-01".to_owned()])
        );
        for bad in [
            "status = \"wired\"\nentry = \"X\"\nreason = \"r\"\nissue = 12\n",
            "status = \"half\"\nentry = \"X\"\nreason = \"r\"\n",
            "status = \"planned\"\nentry = \"X\"\n",
        ] {
            let text = format!("{crates}[subsystems.x]\n{bad}");
            assert!(Architecture::parse(&text).is_err(), "accepted: {bad}");
        }
    }

    #[test]
    fn parses_the_repository_policy() {
        let text = include_str!("../../architecture.toml");
        let architecture = Architecture::parse(text).unwrap();
        assert!(architecture.crates.contains_key("racksmith-domain"));
    }
}
