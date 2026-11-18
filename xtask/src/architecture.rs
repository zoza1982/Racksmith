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
    fn parses_the_repository_policy() {
        let text = include_str!("../../architecture.toml");
        let architecture = Architecture::parse(text).unwrap();
        assert!(architecture.crates.contains_key("racksmith-domain"));
    }
}
