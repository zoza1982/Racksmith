//! The exemption ledger, `exceptions.toml`: one code-owned entry per lint exemption.
//!
//! Clippy's `allow_attributes_without_reason` checks only that an `#[expect]` has a reason, so
//! `reason = "x"` passes it. The ledger makes every exemption visible in one code-owned file,
//! where adding an entry needs the owner's review. Each entry names the attribute (file, item and
//! lints, matched against [`expect_attributes`](crate::expect_attributes)), justifies it in full
//! sentences, and cites a reference that resolves offline: an ADR in `docs/adr/` that is not
//! superseded, an open M1 plan task, or a leaf section of the system design. The rules:
//!
//! - every exemption has exactly one entry, and every entry matches an exemption;
//! - two exemptions of one file with the same item and lints are ambiguous;
//! - references resolve;
//! - neither the justification nor the attribute's `reason` is filler: each has a minimum
//!   length and number of distinct words, is not a placeholder such as `x` or `needed`, and does
//!   not contain a phrase such as `fix later`, `bypass` or `temporary`.
//!
//! See ADR 0036.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::Path;

use anyhow::{Context as _, Result};
use serde::Deserialize;

use crate::design_sections::Section;
use crate::expect_attributes::Exemption;
use crate::plan_tasks::{PlanTasks, TaskProblem};
use crate::violation::Violation;

/// The ledger, relative to the workspace root.
pub(crate) const LEDGER_FILE: &str = "exceptions.toml";

/// The ADR directory, relative to the workspace root.
pub(crate) const ADR_DIR: &str = "docs/adr";

/// Whole texts that say nothing, compared after normalization (lowercase, words only).
const PLACEHOLDERS: [&str; 16] = [
    "x",
    "xx",
    "xxx",
    "needed",
    "required",
    "necessary",
    "see above",
    "see below",
    "n a",
    "none",
    "ok",
    "lint",
    "clippy",
    "false positive",
    "intentional",
    "by design",
];

/// Phrases that mark an exemption as a way around the rule rather than a reasoned exception,
/// matched as whole words anywhere in the text.
const RED_FLAGS: [&str; 20] = [
    "todo",
    "fixme",
    "tbd",
    "wip",
    "fix later",
    "fix it later",
    "clean up later",
    "for now",
    "temporary",
    "temporarily",
    "bypass",
    "bypasses",
    "hack",
    "silence clippy",
    "silence the lint",
    "make clippy happy",
    "appease clippy",
    "pass ci",
    "make ci pass",
    "make the build pass",
];

/// The minimum size of a text that can explain an exemption.
#[derive(Debug, Clone, Copy)]
struct TextRule {
    /// Minimum number of characters, ignoring surrounding whitespace.
    chars: usize,
    /// Minimum number of distinct words of two or more characters.
    words: usize,
}

/// A ledger justification: a real sentence or more.
const JUSTIFICATION: TextRule = TextRule {
    chars: 60,
    words: 8,
};

/// The `reason` of an attribute: a short clause, consistent with the justification.
const REASON: TextRule = TextRule {
    chars: 25,
    words: 4,
};

/// The complete contents of `exceptions.toml`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Ledger {
    /// One entry per exemption.
    #[serde(default)]
    exceptions: Vec<Entry>,
}

/// One ledger entry.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    /// The file of the attribute, relative to the workspace root, with `/` separators.
    file: String,
    /// The path of the exempted item inside the file.
    item: String,
    /// The exempted lints, exactly as the attribute lists them (in any order).
    lints: BTreeSet<String>,
    /// Why the lint is wrong for this item and the code should not change.
    justification: String,
    /// `ADR-NNNN`, an open plan task such as `M1-A-01`, or `design N.M`.
    reference: String,
}

/// The ADRs of `docs/adr/`, by four-digit number, with whether each is retired (superseded,
/// rejected or deprecated).
#[derive(Debug, Default)]
pub(crate) struct Adrs {
    retired: BTreeMap<String, bool>,
}

impl Adrs {
    /// Reads ADRs from `(file name, contents)` pairs; files not named `NNNN-*.md` (such as
    /// `template.md`) are not ADRs.
    pub(crate) fn parse<'a>(files: impl IntoIterator<Item = (&'a str, &'a str)>) -> Self {
        let retired = files
            .into_iter()
            .filter_map(|(name, text)| {
                let (number, _) = name.split_once('-')?;
                let is_adr = number.len() == 4
                    && number.bytes().all(|b| b.is_ascii_digit())
                    && Path::new(name)
                        .extension()
                        .is_some_and(|extension| extension.eq_ignore_ascii_case("md"));
                is_adr.then(|| (number.to_owned(), is_retired(text)))
            })
            .collect();
        Self { retired }
    }
}

/// Whether the `- Status:` line of an ADR retires it.
fn is_retired(text: &str) -> bool {
    text.lines()
        .find_map(|line| line.trim_start().strip_prefix("- Status:"))
        .is_some_and(|status| {
            let status = status.to_lowercase();
            ["superseded", "rejected", "deprecated"]
                .iter()
                .any(|word| status.contains(word))
        })
}

/// What a ledger reference may resolve against.
#[derive(Debug, Clone, Copy)]
pub(crate) struct References<'a> {
    /// The ADRs of `docs/adr/`.
    pub(crate) adrs: &'a Adrs,
    /// The tasks of the M1 plan.
    pub(crate) tasks: &'a PlanTasks,
    /// The leaf sections of the system design.
    pub(crate) sections: &'a [Section],
}

/// Why a ledger reference does not resolve.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum ReferenceProblem {
    /// Not one of the accepted forms.
    Malformed,
    /// No ADR has this number.
    UnknownAdr,
    /// The ADR is superseded, rejected or deprecated.
    RetiredAdr,
    /// The plan task is malformed, unknown or done.
    Task(TaskProblem),
    /// The design has no leaf section with this number.
    UnknownSection,
}

impl fmt::Display for ReferenceProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Malformed => f.write_str(
                "is not `ADR-NNNN`, an M1 plan task such as `M1-A-01`, or a design section such \
                 as `design 4.4`",
            ),
            Self::UnknownAdr => write!(f, "names no ADR in {ADR_DIR}/"),
            Self::RetiredAdr => f.write_str(
                "names a superseded, rejected or deprecated ADR — cite the decision in force",
            ),
            Self::Task(problem) => problem.fmt(f),
            Self::UnknownSection => {
                f.write_str("is not a leaf section of docs/design/system-design.md")
            }
        }
    }
}

/// Why a justification or reason is not an explanation.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum FillerProblem {
    /// The attribute has no `reason`.
    Missing,
    /// The text is a placeholder or contains a red-flag phrase.
    DenyListed(&'static str),
    /// The text is shorter than the minimum.
    TooShort { min: usize },
    /// The text has fewer distinct words than the minimum.
    TooFewWords { min: usize },
}

impl fmt::Display for FillerProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing => f.write_str("is missing"),
            Self::DenyListed(phrase) => write!(f, "is filler (`{phrase}`)"),
            Self::TooShort { min } => write!(f, "is shorter than {min} characters"),
            Self::TooFewWords { min } => write!(f, "has fewer than {min} distinct words"),
        }
    }
}

impl Ledger {
    /// Parses `exceptions.toml`.
    ///
    /// # Errors
    ///
    /// Returns an error if the text is not valid TOML of the ledger's schema.
    pub(crate) fn parse(text: &str) -> Result<Self> {
        toml::from_str(text).with_context(|| format!("failed to parse {LEDGER_FILE}"))
    }

    /// Checks the ledger against the `exemptions` found in the sources.
    pub(crate) fn check(
        &self,
        exemptions: &[Exemption],
        references: References<'_>,
    ) -> Vec<Violation> {
        let mut violations = Vec::new();
        let mut found: BTreeMap<Key<'_>, &Exemption> = BTreeMap::new();
        for exemption in exemptions {
            let key = (
                slash_path(&exemption.path),
                exemption.item.as_str(),
                &exemption.lints,
            );
            if let Some(problem) = reason_problem(exemption.reason.as_deref()) {
                violations.push(Violation::FillerExemptionReason {
                    path: exemption.path.clone(),
                    line: exemption.line,
                    item: exemption.item.clone(),
                    lints: join(&exemption.lints),
                    problem,
                });
            }
            if found.insert(key, exemption).is_some() {
                violations.push(Violation::AmbiguousExemption {
                    path: exemption.path.clone(),
                    line: exemption.line,
                    item: exemption.item.clone(),
                    lints: join(&exemption.lints),
                });
            }
        }
        let mut listed: BTreeSet<Key<'_>> = BTreeSet::new();
        for entry in &self.exceptions {
            let key = (entry.file.clone(), entry.item.as_str(), &entry.lints);
            violations.extend(entry_violations(
                entry,
                found.contains_key(&key),
                references,
            ));
            if !listed.insert(key) {
                violations.push(Violation::DuplicateExemptionEntry {
                    file: entry.file.clone(),
                    item: entry.item.clone(),
                    lints: join(&entry.lints),
                });
            }
        }
        violations.extend(found.iter().filter(|(key, _)| !listed.contains(*key)).map(
            |(_, exemption)| Violation::UnlistedExemption {
                path: exemption.path.clone(),
                line: exemption.line,
                item: exemption.item.clone(),
                lints: join(&exemption.lints),
            },
        ));
        violations
    }
}

/// What identifies an exemption: its file (with `/` separators), item and lints.
type Key<'a> = (String, &'a str, &'a BTreeSet<String>);

/// The violations of one entry on its own: stale, unresolved reference, filler justification.
fn entry_violations(entry: &Entry, matched: bool, references: References<'_>) -> Vec<Violation> {
    let mut violations = Vec::new();
    let file = || entry.file.clone();
    let item = || entry.item.clone();
    if !matched {
        violations.push(Violation::StaleExemption {
            file: file(),
            item: item(),
            lints: join(&entry.lints),
        });
    }
    if let Err(problem) = resolve(&entry.reference, references) {
        violations.push(Violation::UnresolvedExemptionReference {
            file: file(),
            item: item(),
            reference: entry.reference.clone(),
            problem,
        });
    }
    if let Some(problem) = filler(&entry.justification, JUSTIFICATION) {
        violations.push(Violation::FillerExemptionJustification {
            file: file(),
            item: item(),
            problem,
        });
    }
    violations
}

/// Checks that `reference` resolves.
///
/// # Errors
///
/// Returns the problem if it is malformed or names nothing in force.
fn resolve(reference: &str, references: References<'_>) -> Result<(), ReferenceProblem> {
    if let Some(number) = reference.strip_prefix("ADR-") {
        if number.len() != 4 || !number.bytes().all(|b| b.is_ascii_digit()) {
            return Err(ReferenceProblem::Malformed);
        }
        return match references.adrs.retired.get(number) {
            None => Err(ReferenceProblem::UnknownAdr),
            Some(true) => Err(ReferenceProblem::RetiredAdr),
            Some(false) => Ok(()),
        };
    }
    if let Some(key) = reference.strip_prefix("design ") {
        return if references.sections.iter().any(|section| section.key == key) {
            Ok(())
        } else {
            Err(ReferenceProblem::UnknownSection)
        };
    }
    if reference.starts_with("M1-") {
        return references
            .tasks
            .check(reference)
            .map_err(ReferenceProblem::Task);
    }
    Err(ReferenceProblem::Malformed)
}

fn reason_problem(reason: Option<&str>) -> Option<FillerProblem> {
    match reason {
        None => Some(FillerProblem::Missing),
        Some(reason) => filler(reason, REASON),
    }
}

/// Why `text` is filler under `rule`, if it is.
fn filler(text: &str, rule: TextRule) -> Option<FillerProblem> {
    let lowercase = text.to_lowercase();
    let words: Vec<&str> = lowercase
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .collect();
    let normalized = words.join(" ");
    let padded = format!(" {normalized} ");
    let placeholder = PLACEHOLDERS
        .iter()
        .copied()
        .find(|phrase| normalized == *phrase);
    let red_flag = || {
        RED_FLAGS
            .iter()
            .copied()
            .find(|phrase| padded.contains(&format!(" {phrase} ")))
    };
    if let Some(phrase) = placeholder.or_else(red_flag) {
        return Some(FillerProblem::DenyListed(phrase));
    }
    if text.trim().chars().count() < rule.chars {
        return Some(FillerProblem::TooShort { min: rule.chars });
    }
    let distinct: BTreeSet<&str> = words
        .into_iter()
        .filter(|word| word.chars().count() >= 2)
        .collect();
    (distinct.len() < rule.words).then_some(FillerProblem::TooFewWords { min: rule.words })
}

/// `path` with `/` separators, as ledger entries write it on every platform.
pub(crate) fn slash_path(path: &Path) -> String {
    path.components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

fn join(lints: &BTreeSet<String>) -> String {
    lints
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    const PLAN: &str = "| ID | Task |\n|---|---|\n| A-01 | open |\n| ~~A-02~~ | done |\n";
    const GOOD_REASON: &str = "one arm per variant keeps the catalogue together";
    const GOOD_JUSTIFICATION: &str = "The message catalogue is one exhaustive match with one \
                                      arm per variant, so its length grows with the variants.";

    struct Fixture {
        adrs: Adrs,
        tasks: PlanTasks,
        sections: Vec<Section>,
    }

    impl Fixture {
        fn new() -> Self {
            Self {
                adrs: Adrs::parse([
                    ("0003-trust-pyramid.md", "# 0003\n\n- Status: Accepted\n"),
                    ("0007-old.md", "# 0007\n\n- Status: Superseded by 0008\n"),
                    ("0036-ledger.md", "# 0036\n\n- Status: Proposed\n"),
                    (
                        "template.md",
                        "- Status: Proposed | Accepted | Superseded by NNNN\n",
                    ),
                ]),
                tasks: PlanTasks::parse(PLAN).unwrap(),
                sections: vec![Section {
                    key: "4.4".into(),
                    title: "decide()".into(),
                }],
            }
        }

        fn references(&self) -> References<'_> {
            References {
                adrs: &self.adrs,
                tasks: &self.tasks,
                sections: &self.sections,
            }
        }

        fn check(&self, ledger: &str, exemptions: &[Exemption]) -> Vec<Violation> {
            let mut violations = Ledger::parse(ledger)
                .unwrap()
                .check(exemptions, self.references());
            violations.sort();
            violations
        }
    }

    fn exemption(item: &str, lints: &[&str], reason: Option<&str>) -> Exemption {
        Exemption {
            path: PathBuf::from("xtask/src/violation.rs"),
            line: 7,
            item: item.into(),
            lints: lints.iter().map(|&lint| lint.to_owned()).collect(),
            reason: reason.map(str::to_owned),
        }
    }

    fn entry(item: &str, lints: &str, justification: &str, reference: &str) -> String {
        format!(
            "[[exceptions]]\nfile = \"xtask/src/violation.rs\"\nitem = \"{item}\"\n\
             lints = {lints}\njustification = \"{justification}\"\nreference = \"{reference}\"\n\n"
        )
    }

    fn good_entry(reference: &str) -> String {
        entry(
            "Violation::fmt",
            "[\"clippy::too_many_lines\"]",
            GOOD_JUSTIFICATION,
            reference,
        )
    }

    fn good_exemption() -> Exemption {
        exemption(
            "Violation::fmt",
            &["clippy::too_many_lines"],
            Some(GOOD_REASON),
        )
    }

    #[test]
    fn a_listed_justified_exemption_passes_with_any_reference_form() {
        let fixture = Fixture::new();
        for reference in ["ADR-0003", "ADR-0036", "M1-A-01", "design 4.4"] {
            assert_eq!(
                fixture.check(&good_entry(reference), &[good_exemption()]),
                vec![],
                "{reference}"
            );
        }
        assert_eq!(
            fixture.check("", &[]),
            vec![],
            "an empty ledger for no exemptions"
        );
    }

    #[test]
    fn an_unlisted_exemption_is_reported_with_its_location() {
        let fixture = Fixture::new();
        assert_eq!(
            fixture.check("", &[good_exemption()]),
            vec![Violation::UnlistedExemption {
                path: PathBuf::from("xtask/src/violation.rs"),
                line: 7,
                item: "Violation::fmt".into(),
                lints: "clippy::too_many_lines".into(),
            }]
        );
    }

    #[test]
    fn an_entry_must_match_file_item_and_the_whole_lint_set() {
        let fixture = Fixture::new();
        let other_lints = exemption(
            "Violation::fmt",
            &["clippy::too_many_lines", "dead_code"],
            Some(GOOD_REASON),
        );
        let other_item = exemption(
            "Finding::fmt",
            &["clippy::too_many_lines"],
            Some(GOOD_REASON),
        );
        let other_file = Exemption {
            path: PathBuf::from("xtask/src/finding.rs"),
            ..good_exemption()
        };
        for found in [other_lints, other_item, other_file] {
            let violations = fixture.check(&good_entry("ADR-0003"), std::slice::from_ref(&found));
            assert_eq!(violations.len(), 2, "{found:?}: {violations:?}");
            assert!(
                matches!(violations[0], Violation::UnlistedExemption { .. }),
                "{violations:?}"
            );
            assert!(
                matches!(violations[1], Violation::StaleExemption { .. }),
                "{violations:?}"
            );
        }
        let reordered = entry(
            "Violation::fmt",
            "[\"dead_code\", \"clippy::too_many_lines\"]",
            GOOD_JUSTIFICATION,
            "ADR-0003",
        );
        let both = exemption(
            "Violation::fmt",
            &["clippy::too_many_lines", "dead_code"],
            Some(GOOD_REASON),
        );
        assert_eq!(fixture.check(&reordered, &[both]), vec![], "order is free");
    }

    #[test]
    fn a_stale_entry_is_reported() {
        assert_eq!(
            Fixture::new().check(&good_entry("ADR-0003"), &[]),
            vec![Violation::StaleExemption {
                file: "xtask/src/violation.rs".into(),
                item: "Violation::fmt".into(),
                lints: "clippy::too_many_lines".into(),
            }]
        );
    }

    #[test]
    fn duplicate_entries_and_ambiguous_exemptions_are_reported() {
        let fixture = Fixture::new();
        let twice = good_entry("ADR-0003").repeat(2);
        assert_eq!(
            fixture.check(&twice, &[good_exemption()]),
            vec![Violation::DuplicateExemptionEntry {
                file: "xtask/src/violation.rs".into(),
                item: "Violation::fmt".into(),
                lints: "clippy::too_many_lines".into(),
            }]
        );
        let second = Exemption {
            line: 9,
            ..good_exemption()
        };
        assert_eq!(
            fixture.check(&good_entry("ADR-0003"), &[good_exemption(), second]),
            vec![Violation::AmbiguousExemption {
                path: PathBuf::from("xtask/src/violation.rs"),
                line: 9,
                item: "Violation::fmt".into(),
                lints: "clippy::too_many_lines".into(),
            }]
        );
    }

    #[test]
    fn references_must_resolve() {
        let fixture = Fixture::new();
        for (reference, problem) in [
            ("ADR-9999", ReferenceProblem::UnknownAdr),
            ("ADR-0007", ReferenceProblem::RetiredAdr),
            ("ADR-3", ReferenceProblem::Malformed),
            ("adr 0003", ReferenceProblem::Malformed),
            ("#12", ReferenceProblem::Malformed),
            ("", ReferenceProblem::Malformed),
            ("M1-ZZ-01", ReferenceProblem::Task(TaskProblem::Malformed)),
            ("M1-Z-01", ReferenceProblem::Task(TaskProblem::Unknown)),
            ("M1-A-02", ReferenceProblem::Task(TaskProblem::Done)),
            ("design 9.9", ReferenceProblem::UnknownSection),
            ("design 4", ReferenceProblem::UnknownSection),
        ] {
            assert_eq!(
                fixture.check(&good_entry(reference), &[good_exemption()]),
                vec![Violation::UnresolvedExemptionReference {
                    file: "xtask/src/violation.rs".into(),
                    item: "Violation::fmt".into(),
                    reference: reference.into(),
                    problem,
                }],
                "{reference}"
            );
        }
    }

    #[test]
    fn filler_justifications_are_reported() {
        let fixture = Fixture::new();
        for (justification, problem) in [
            ("x", FillerProblem::DenyListed("x")),
            ("Needed.", FillerProblem::DenyListed("needed")),
            ("see above", FillerProblem::DenyListed("see above")),
            (
                "Temporary exemption while the dispatcher is refactored into smaller pieces.",
                FillerProblem::DenyListed("temporary"),
            ),
            (
                "We will fix later once the registry has settled and the tests are green again.",
                FillerProblem::DenyListed("fix later"),
            ),
            (
                "This is how we bypass the lint on the message catalogue of the arch command.",
                FillerProblem::DenyListed("bypass"),
            ),
            (
                "The match is long because it is long.",
                FillerProblem::TooShort { min: 60 },
            ),
            (
                "long long long long long long long long long long long long long long",
                FillerProblem::TooFewWords { min: 8 },
            ),
        ] {
            let ledger = entry(
                "Violation::fmt",
                "[\"clippy::too_many_lines\"]",
                justification,
                "ADR-0003",
            );
            assert_eq!(
                fixture.check(&ledger, &[good_exemption()]),
                vec![Violation::FillerExemptionJustification {
                    file: "xtask/src/violation.rs".into(),
                    item: "Violation::fmt".into(),
                    problem,
                }],
                "{justification}"
            );
        }
    }

    #[test]
    fn filler_and_missing_reasons_are_reported() {
        let fixture = Fixture::new();
        for (reason, problem) in [
            (None, FillerProblem::Missing),
            (Some("x"), FillerProblem::DenyListed("x")),
            (
                Some("todo: split this up"),
                FillerProblem::DenyListed("todo"),
            ),
            (
                Some("for now, until the split"),
                FillerProblem::DenyListed("for now"),
            ),
            (
                Some("too long to split"),
                FillerProblem::TooShort { min: 25 },
            ),
            (
                Some("long long long long long long"),
                FillerProblem::TooFewWords { min: 4 },
            ),
        ] {
            let found = exemption("Violation::fmt", &["clippy::too_many_lines"], reason);
            assert_eq!(
                fixture.check(&good_entry("ADR-0003"), &[found]),
                vec![Violation::FillerExemptionReason {
                    path: PathBuf::from("xtask/src/violation.rs"),
                    line: 7,
                    item: "Violation::fmt".into(),
                    lints: "clippy::too_many_lines".into(),
                    problem,
                }],
                "{reason:?}"
            );
        }
    }

    #[test]
    fn red_flags_match_whole_words_only() {
        assert_eq!(
            filler(
                "Hackers aside, the temporaryfile helper bypassed nothing: one match, by design.",
                JUSTIFICATION
            ),
            None,
            "glued words are not red flags"
        );
    }

    #[test]
    fn the_schema_rejects_unknown_and_missing_fields() {
        for text in [
            "[[exceptions]]\nfile = \"a.rs\"\nitem = \"f\"\nlints = []\njustification = \"j\"\n",
            "[[exceptions]]\nfile = \"a.rs\"\nitem = \"f\"\nlints = []\njustification = \"j\"\n\
             reference = \"ADR-0003\"\nissue = \"#1\"\n",
            "[[exemptions]]\n",
        ] {
            assert!(Ledger::parse(text).is_err(), "{text}");
        }
    }

    #[test]
    fn only_numbered_markdown_files_are_adrs() {
        let fixture = Fixture::new();
        assert_eq!(fixture.adrs.retired.len(), 3, "{:?}", fixture.adrs);
    }

    #[test]
    fn ledger_files_use_forward_slashes() {
        assert_eq!(
            slash_path(&Path::new("crates").join("x").join("src").join("lib.rs")),
            "crates/x/src/lib.rs"
        );
    }

    #[test]
    fn parses_the_repository_ledger() {
        let ledger = Ledger::parse(include_str!("../../exceptions.toml")).unwrap();
        assert!(
            !ledger.exceptions.is_empty(),
            "the repository has exemptions"
        );
    }
}
