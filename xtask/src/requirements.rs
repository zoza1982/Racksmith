//! The typed model of `requirements.toml` and its rules.
//!
//! `[sections."<key>"]` classifies every leaf section of the system design, and
//! `[inputs."<package>::<Type>.<member>"]` every field and variant a caller can set. The rules
//! make every gap explicit: a section is built (with entry points and the tests that prove it),
//! partly built or planned (with plan tasks), or informative (with a reason); an input is
//! implemented (with a test proving it has an effect) or rejected at admission (with a task and a
//! test). Anything else is a [`Finding`].

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context as _, Result};
use serde::Deserialize;

use crate::design_sections::Section;
use crate::finding::Finding;
use crate::plan_tasks::PlanTasks;

/// The traceability file, relative to the workspace root.
pub(crate) const REQUIREMENTS_FILE: &str = "requirements.toml";

/// The complete contents of `requirements.toml`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Requirements {
    /// Design sections by section number.
    #[serde(default)]
    sections: BTreeMap<String, SectionEntry>,
    /// Inputs by `<package>::<Type>.<member>`.
    #[serde(default)]
    inputs: BTreeMap<String, InputEntry>,
}

/// The classification of one design section, before its fields are checked against its status.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SectionEntry {
    /// The heading text after the number, compared with the design to detect drift.
    title: String,
    status: SectionStatus,
    /// Plan tasks that build what is missing.
    tasks: Option<Vec<String>>,
    /// What is not built yet.
    missing: Option<String>,
    /// Paths of the code that implements the section.
    entry_points: Option<Vec<String>>,
    /// Tests that prove the section.
    tests: Option<Vec<String>>,
    /// Why the section states no buildable requirement.
    reason: Option<String>,
}

/// How far a design section is built.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
enum SectionStatus {
    Implemented,
    Partial,
    Planned,
    Informative,
}

impl SectionStatus {
    const ALL: [Self; 4] = [
        Self::Implemented,
        Self::Partial,
        Self::Planned,
        Self::Informative,
    ];

    fn as_str(self) -> &'static str {
        match self {
            Self::Implemented => "implemented",
            Self::Partial => "partial",
            Self::Planned => "planned",
            Self::Informative => "informative",
        }
    }

    /// The rule for each optional field, in the order of [`SectionEntry`].
    fn rules(self) -> [Rule; 5] {
        use Rule::{Forbidden, Optional, Required};
        // tasks, missing, entry_points, tests, reason
        match self {
            Self::Implemented => [Forbidden, Forbidden, Required, Required, Forbidden],
            Self::Partial => [Required, Required, Optional, Optional, Forbidden],
            Self::Planned => [Required, Forbidden, Forbidden, Forbidden, Forbidden],
            Self::Informative => [Forbidden, Forbidden, Forbidden, Forbidden, Required],
        }
    }
}

/// The classification of one input, before its fields are checked against its status.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct InputEntry {
    status: InputStatus,
    /// Tests proving that changing the input changes an observable outcome.
    effect_tests: Option<Vec<String>>,
    /// Plan tasks that will implement a rejected input.
    tasks: Option<Vec<String>>,
    /// Tests proving that admission rejects the input with a typed error naming it.
    tests: Option<Vec<String>>,
}

/// Whether an input is honoured.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
enum InputStatus {
    Implemented,
    Rejected,
}

impl InputStatus {
    const ALL: [Self; 2] = [Self::Implemented, Self::Rejected];

    fn as_str(self) -> &'static str {
        match self {
            Self::Implemented => "implemented",
            Self::Rejected => "rejected",
        }
    }

    /// The rule for each optional field, in the order of [`InputEntry`].
    fn rules(self) -> [Rule; 3] {
        use Rule::{Forbidden, Required};
        // effect_tests, tasks, tests
        match self {
            Self::Implemented => [Required, Forbidden, Forbidden],
            Self::Rejected => [Forbidden, Required, Required],
        }
    }
}

/// Whether a status needs, allows or forbids a field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Rule {
    Required,
    Optional,
    Forbidden,
}

/// What the design, the plan and the code contain, to check `requirements.toml` against.
#[derive(Debug)]
pub(crate) struct Facts<'a> {
    /// Leaf sections of the design, in document order.
    pub(crate) sections: &'a [Section],
    /// Every input of the product crates, as `<package>::<Type>.<member>`.
    pub(crate) inputs: &'a BTreeSet<String>,
    /// The plan's tasks.
    pub(crate) tasks: &'a PlanTasks,
    /// Names of the items defined in each product crate's non-test sources, by package name.
    pub(crate) definitions: &'a BTreeMap<String, BTreeSet<String>>,
    /// Every test of the product crates, as `<package>::<libtest path>`.
    pub(crate) tests: &'a BTreeSet<String>,
    /// The listed tests marked `#[ignore]`, which the gate never runs.
    pub(crate) ignored_tests: &'a BTreeSet<String>,
    /// `Deserialize` types with named fields that accept unknown fields, as
    /// `<package>::<Type>`.
    pub(crate) lax_inputs: &'a BTreeSet<String>,
}

/// The outcome of the check.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Report {
    /// Every gap found, grouped by entry.
    pub(crate) findings: Vec<Finding>,
    /// Leaf sections that have an entry.
    pub(crate) sections_classified: usize,
    /// Inputs that have an entry.
    pub(crate) inputs_classified: usize,
    /// Distinct test names that exist.
    pub(crate) tests_resolved: usize,
    /// Distinct task references that are open plan tasks.
    pub(crate) tasks_resolved: usize,
}

impl Requirements {
    /// Parses the contents of `requirements.toml`.
    ///
    /// # Errors
    ///
    /// Returns an error if the text is not valid TOML or has an unknown key or status.
    pub(crate) fn parse(text: &str) -> Result<Self> {
        toml::from_str(text).context("requirements.toml does not match the expected schema")
    }

    /// Checks every entry against the facts, and every fact for an entry.
    pub(crate) fn check(&self, facts: &Facts<'_>) -> Report {
        let mut checker = Checker {
            facts,
            findings: Vec::new(),
            tests: BTreeSet::new(),
            tasks: BTreeSet::new(),
        };
        let leaves: BTreeMap<&str, &Section> = facts
            .sections
            .iter()
            .map(|section| (section.key.as_str(), section))
            .collect();
        for section in facts.sections {
            if !self.sections.contains_key(&section.key) {
                checker.findings.push(Finding::UnclassifiedSection {
                    key: section.key.clone(),
                    title: section.title.clone(),
                });
            }
        }
        for (key, entry) in &self.sections {
            match leaves.get(key.as_str()) {
                Some(section) => checker.section(section, entry),
                None => checker
                    .findings
                    .push(Finding::UnknownSection { key: key.clone() }),
            }
        }
        for key in facts.inputs {
            if !self.inputs.contains_key(key) {
                checker
                    .findings
                    .push(Finding::UnclassifiedInput { key: key.clone() });
            }
        }
        for (key, entry) in &self.inputs {
            checker.input(key, entry);
        }
        for ty in facts.lax_inputs {
            checker
                .findings
                .push(Finding::AcceptsUnknownFields { ty: ty.clone() });
        }
        Report {
            findings: checker.findings,
            sections_classified: count_keys(&self.sections, leaves.keys().copied()),
            inputs_classified: count_keys(&self.inputs, facts.inputs.iter().map(String::as_str)),
            tests_resolved: checker.tests.len(),
            tasks_resolved: checker.tasks.len(),
        }
    }

    /// The generated coverage page, `docs/requirements.md`. Call it only when [`Self::check`]
    /// found nothing, so that every section and input has an entry.
    pub(crate) fn render(&self, facts: &Facts<'_>) -> String {
        let section_counts = SectionStatus::ALL.map(|status| {
            let count = self
                .sections
                .values()
                .filter(|entry| entry.status == status)
                .count();
            format!("{count} {}", status.as_str())
        });
        let input_counts = InputStatus::ALL.map(|status| {
            let count = self
                .inputs
                .values()
                .filter(|entry| entry.status == status)
                .count();
            format!("{count} {}", status.as_str())
        });
        let mut lines =
            vec![
            "<!-- Generated by `cargo xtask spec --fix` from requirements.toml, \
             docs/design/system-design.md and docs/plan/m1.md. Do not edit: change \
             requirements.toml and regenerate. -->"
                .to_owned(),
            String::new(),
            "# Requirements coverage".to_owned(),
            String::new(),
            "Every leaf section of the [system design](design/system-design.md) and every input \
             of the product crates is classified in [`requirements.toml`](../requirements.toml), \
             and `cargo xtask spec` fails on a gap. See \
             [ADR 0035](adr/0035-completeness-layer.md)."
                .to_owned(),
            String::new(),
            "## Summary".to_owned(),
            String::new(),
            format!(
                "- {} design sections: {}",
                self.sections.len(),
                section_counts.join(", ")
            ),
            format!("- {} inputs: {}", self.inputs.len(), input_counts.join(", ")),
            String::new(),
            "## Design sections".to_owned(),
            String::new(),
            "| Section | Title | Status | Tasks | Tests |".to_owned(),
            "|---|---|---|---|---|".to_owned(),
        ];
        lines.extend(facts.sections.iter().filter_map(|section| {
            let entry = self.sections.get(&section.key)?;
            Some(format!(
                "| {} | {} | {} | {} | {} |",
                section.key,
                escape(&section.title),
                entry.status.as_str(),
                len(entry.tasks.as_ref()),
                len(entry.tests.as_ref()),
            ))
        }));
        lines.extend(
            [
                "",
                "## Inputs",
                "",
                "| Input | Status | Tasks | Tests |",
                "|---|---|---|---|",
            ]
            .map(str::to_owned),
        );
        lines.extend(self.inputs.iter().map(|(key, entry)| {
            format!(
                "| `{key}` | {} | {} | {} |",
                entry.status.as_str(),
                len(entry.tasks.as_ref()),
                len(entry.effect_tests.as_ref()) + len(entry.tests.as_ref()),
            )
        }));
        lines.push(String::new());
        lines.join("\n")
    }
}

fn count_keys<'a, V>(entries: &BTreeMap<String, V>, keys: impl Iterator<Item = &'a str>) -> usize {
    keys.filter(|key| entries.contains_key(*key)).count()
}

fn len(list: Option<&Vec<String>>) -> usize {
    list.map_or(0, Vec::len)
}

/// Escapes `|` so a title cannot break a table row.
fn escape(text: &str) -> String {
    text.replace('|', "\\|")
}

/// Accumulates findings and resolved references while entries are checked.
struct Checker<'a, 'f> {
    facts: &'a Facts<'f>,
    findings: Vec<Finding>,
    tests: BTreeSet<String>,
    tasks: BTreeSet<String>,
}

impl Checker<'_, '_> {
    fn section(&mut self, section: &Section, entry: &SectionEntry) {
        let name = format!("[sections.\"{}\"]", section.key);
        if entry.title.trim() != section.title {
            self.findings.push(Finding::TitleMismatch {
                key: section.key.clone(),
                expected: section.title.clone(),
                found: entry.title.clone(),
            });
        }
        let status = entry.status.as_str();
        let fields = [
            ("tasks", presence_of_list(entry.tasks.as_ref())),
            ("missing", presence_of_text(entry.missing.as_deref())),
            (
                "entry_points",
                presence_of_list(entry.entry_points.as_ref()),
            ),
            ("tests", presence_of_list(entry.tests.as_ref())),
            ("reason", presence_of_text(entry.reason.as_deref())),
        ];
        self.fields(&name, status, &fields, entry.status.rules());
        self.tasks(&name, entry.tasks.as_ref());
        self.tests(&name, entry.tests.as_ref());
        for path in entry.entry_points.iter().flatten() {
            self.entry_point(&name, path);
        }
    }

    fn input(&mut self, key: &str, entry: &InputEntry) {
        let name = format!("[inputs.\"{key}\"]");
        if !is_input_key(key) {
            self.findings.push(Finding::MalformedInput {
                key: key.to_owned(),
            });
        } else if !self.facts.inputs.contains(key) {
            self.findings.push(Finding::UnknownInput {
                key: key.to_owned(),
            });
        }
        let fields = [
            (
                "effect_tests",
                presence_of_list(entry.effect_tests.as_ref()),
            ),
            ("tasks", presence_of_list(entry.tasks.as_ref())),
            ("tests", presence_of_list(entry.tests.as_ref())),
        ];
        self.fields(&name, entry.status.as_str(), &fields, entry.status.rules());
        self.tasks(&name, entry.tasks.as_ref());
        self.tests(&name, entry.effect_tests.as_ref());
        self.tests(&name, entry.tests.as_ref());
    }

    /// Reports each required field that is absent or empty and each forbidden field present.
    fn fields<const N: usize>(
        &mut self,
        entry: &str,
        status: &'static str,
        fields: &[(&'static str, Presence); N],
        rules: [Rule; N],
    ) {
        for (&(field, presence), rule) in fields.iter().zip(rules) {
            let finding = match (rule, presence) {
                (Rule::Required, Presence::Absent | Presence::Empty) => Finding::MissingField {
                    entry: entry.to_owned(),
                    field,
                    status,
                },
                (Rule::Forbidden, Presence::Empty | Presence::Filled) => Finding::ForbiddenField {
                    entry: entry.to_owned(),
                    field,
                    status,
                },
                (Rule::Required, Presence::Filled)
                | (Rule::Optional, _)
                | (Rule::Forbidden, Presence::Absent) => continue,
            };
            self.findings.push(finding);
        }
    }

    fn tasks(&mut self, entry: &str, tasks: Option<&Vec<String>>) {
        for task in tasks.into_iter().flatten() {
            match self.facts.tasks.check(task) {
                Ok(()) => {
                    self.tasks.insert(task.clone());
                }
                Err(problem) => self.findings.push(Finding::BadTask {
                    entry: entry.to_owned(),
                    task: task.clone(),
                    problem,
                }),
            }
        }
    }

    fn tests(&mut self, entry: &str, tests: Option<&Vec<String>>) {
        for test in tests.into_iter().flatten() {
            if self.facts.ignored_tests.contains(test) {
                self.findings.push(Finding::IgnoredTest {
                    entry: entry.to_owned(),
                    test: test.clone(),
                });
            } else if self.facts.tests.contains(test) {
                self.tests.insert(test.clone());
            } else {
                self.findings.push(Finding::UnknownTest {
                    entry: entry.to_owned(),
                    test: test.clone(),
                });
            }
        }
    }

    /// Checks that `path` names a product crate by its identifier (`racksmith_api`) and that
    /// every later segment is defined in that crate's non-test sources.
    fn entry_point(&mut self, entry: &str, path: &str) {
        let segments: Vec<&str> = path.split("::").collect();
        let finding = if segments.len() < 2 || !segments.iter().all(|s| is_identifier(s)) {
            Finding::MalformedEntryPoint {
                entry: entry.to_owned(),
                path: path.to_owned(),
            }
        } else {
            let krate = segments.first().map(|first| first.replace('_', "-"));
            match krate
                .as_ref()
                .and_then(|krate| self.facts.definitions.get(krate))
            {
                None => Finding::UnknownEntryPointCrate {
                    entry: entry.to_owned(),
                    path: path.to_owned(),
                },
                Some(defined) => {
                    let Some(undefined) = segments
                        .iter()
                        .skip(1)
                        .find(|segment| !defined.contains(**segment))
                    else {
                        return;
                    };
                    Finding::UnknownEntryPoint {
                        entry: entry.to_owned(),
                        path: path.to_owned(),
                        segment: (*undefined).to_owned(),
                        krate: krate.unwrap_or_default(),
                    }
                }
            }
        };
        self.findings.push(finding);
    }
}

/// Whether an optional field is absent, present but empty, or filled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Presence {
    Absent,
    Empty,
    Filled,
}

fn presence_of_list(list: Option<&Vec<String>>) -> Presence {
    match list {
        None => Presence::Absent,
        Some(items) if items.iter().all(|item| item.trim().is_empty()) => Presence::Empty,
        Some(_) => Presence::Filled,
    }
}

fn presence_of_text(text: Option<&str>) -> Presence {
    match text {
        None => Presence::Absent,
        Some(text) if text.trim().is_empty() => Presence::Empty,
        Some(_) => Presence::Filled,
    }
}

/// Whether `text` is a Rust identifier.
fn is_identifier(text: &str) -> bool {
    text.chars()
        .next()
        .is_some_and(|first| first.is_alphabetic() || first == '_')
        && text.chars().all(|c| c.is_alphanumeric() || c == '_')
}

/// Whether `key` has the form `<package>::<Type>.<member>` or `<package>::<Type>.<Variant>.<field>`,
/// or is an undeclared input: `<package>::route.<pattern>.<param>` or `<package>::env.<NAME>`.
fn is_input_key(key: &str) -> bool {
    let Some((package, path)) = key.split_once("::") else {
        return false;
    };
    let package_ok = !package.is_empty()
        && package
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    package_ok && (is_member_path(path) || is_undeclared_input(path))
}

/// `<Type>.<member>` or `<Type>.<Variant>.<field>`.
fn is_member_path(path: &str) -> bool {
    let parts: Vec<&str> = path.split('.').collect();
    let member_ok = |part: &&str| is_identifier(part) || part.bytes().all(|b| b.is_ascii_digit());
    (2..=3).contains(&parts.len())
        && parts.first().is_some_and(|ty| is_identifier(ty))
        && parts
            .iter()
            .skip(1)
            .all(|part| !part.is_empty() && member_ok(part))
}

/// `route.<pattern>.<param>`, where the pattern is a path starting with `/`, or `env.<NAME>`.
fn is_undeclared_input(path: &str) -> bool {
    if let Some(route) = path.strip_prefix("route.") {
        return route.rsplit_once('.').is_some_and(|(pattern, param)| {
            let names_param = pattern.contains(&format!("{{{param}}}"))
                || pattern.contains(&format!("{{*{param}}}"));
            pattern.starts_with('/')
                && !pattern.contains(char::is_whitespace)
                && names_param
                && is_identifier(param)
        });
    }
    path.strip_prefix("env.").is_some_and(|name| {
        !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan_tasks::TaskProblem;

    const PLAN: &str = "| A-01 | open |\n| ~~A-02~~ | done |\n| M1-H-01 | lab |\n";

    struct World {
        sections: Vec<Section>,
        inputs: BTreeSet<String>,
        tasks: PlanTasks,
        definitions: BTreeMap<String, BTreeSet<String>>,
        tests: BTreeSet<String>,
        ignored_tests: BTreeSet<String>,
        lax_inputs: BTreeSet<String>,
    }

    impl World {
        fn new() -> Self {
            let set = |items: &[&str]| items.iter().map(|item| (*item).to_owned()).collect();
            Self {
                sections: [
                    ("4.1", "The machine aggregate"),
                    ("4.2", "Storage"),
                    ("15", "Decisions needing ADRs"),
                ]
                .map(|(key, title)| Section {
                    key: key.to_owned(),
                    title: title.to_owned(),
                })
                .into(),
                inputs: set(&[
                    "racksmith-api::Req.mac",
                    "racksmith-cli::Command.Serve.listen",
                ]),
                tasks: PlanTasks::parse(PLAN).unwrap(),
                definitions: [
                    (
                        "racksmith-domain".to_owned(),
                        set(&["Machine", "machine", "new"]),
                    ),
                    ("racksmith-api".to_owned(), set(&["Registry", "serve"])),
                    ("racksmith-cli".to_owned(), set(&["serve"])),
                ]
                .into(),
                tests: set(&[
                    "racksmith-domain::machine::tests::new_machine",
                    "racksmith-api::create_rejects_bad_mac",
                    "racksmith-cli::serve_listens",
                    "racksmith-cli::slow_serve",
                ]),
                ignored_tests: set(&["racksmith-cli::slow_serve"]),
                lax_inputs: BTreeSet::new(),
            }
        }

        fn facts(&self) -> Facts<'_> {
            Facts {
                sections: &self.sections,
                inputs: &self.inputs,
                tasks: &self.tasks,
                definitions: &self.definitions,
                tests: &self.tests,
                ignored_tests: &self.ignored_tests,
                lax_inputs: &self.lax_inputs,
            }
        }

        fn check(&self, text: &str) -> Report {
            Requirements::parse(text).unwrap().check(&self.facts())
        }

        fn findings(&self, text: &str) -> Vec<Finding> {
            self.check(text).findings
        }
    }

    const COMPLETE: &str = r#"
        [sections."4.1"]
        title = "The machine aggregate"
        status = "implemented"
        entry_points = ["racksmith_domain::Machine"]
        tests = ["racksmith-domain::machine::tests::new_machine"]

        [sections."4.2"]
        title = "Storage"
        status = "partial"
        missing = "the redb engine"
        tasks = ["M1-A-01"]
        entry_points = ["racksmith_api::Registry"]

        [sections."15"]
        title = "Decisions needing ADRs"
        status = "informative"
        reason = "a list of decisions, each recorded as an ADR"

        [inputs."racksmith-api::Req.mac"]
        status = "rejected"
        tasks = ["M1-H-01"]
        tests = ["racksmith-api::create_rejects_bad_mac"]

        [inputs."racksmith-cli::Command.Serve.listen"]
        status = "implemented"
        effect_tests = ["racksmith-cli::serve_listens"]
    "#;

    #[test]
    fn a_complete_file_passes_and_counts_what_it_resolved() {
        let report = World::new().check(COMPLETE);
        assert_eq!(
            report,
            Report {
                findings: vec![],
                sections_classified: 3,
                inputs_classified: 2,
                tests_resolved: 3,
                tasks_resolved: 2,
            }
        );
    }

    #[test]
    fn unclassified_unknown_and_renamed_sections_are_reported() {
        let text = r#"
            [sections."4.1"]
            title = "The machine aggregate (old title)"
            status = "planned"
            tasks = ["M1-A-01"]

            [sections."4"]
            title = "Domain"
            status = "planned"
            tasks = ["M1-A-01"]
        "#;
        let findings = World::new().findings(text);
        for expected in [
            Finding::UnclassifiedSection {
                key: "4.2".into(),
                title: "Storage".into(),
            },
            Finding::UnclassifiedSection {
                key: "15".into(),
                title: "Decisions needing ADRs".into(),
            },
            Finding::UnknownSection { key: "4".into() },
            Finding::TitleMismatch {
                key: "4.1".into(),
                expected: "The machine aggregate".into(),
                found: "The machine aggregate (old title)".into(),
            },
        ] {
            assert!(findings.contains(&expected), "{expected:?} in {findings:?}");
        }
    }

    fn section(status_and_fields: &str) -> String {
        format!(
            "[sections.\"4.1\"]\ntitle = \"The machine aggregate\"\n{status_and_fields}\n\
             [sections.\"4.2\"]\ntitle = \"Storage\"\nstatus = \"planned\"\ntasks = [\"M1-A-01\"]\n\
             [sections.\"15\"]\ntitle = \"Decisions needing ADRs\"\nstatus = \"informative\"\n\
             reason = \"r\"\n\
             [inputs.\"racksmith-api::Req.mac\"]\nstatus = \"implemented\"\n\
             effect_tests = [\"racksmith-api::create_rejects_bad_mac\"]\n\
             [inputs.\"racksmith-cli::Command.Serve.listen\"]\nstatus = \"implemented\"\n\
             effect_tests = [\"racksmith-cli::serve_listens\"]\n"
        )
    }

    fn field_findings(status_and_fields: &str) -> Vec<(String, &'static str)> {
        World::new()
            .findings(&section(status_and_fields))
            .into_iter()
            .map(|finding| match finding {
                Finding::MissingField { field, .. } => ("missing".to_owned(), field),
                Finding::ForbiddenField { field, .. } => ("forbidden".to_owned(), field),
                other @ (Finding::UnclassifiedSection { .. }
                | Finding::UnknownSection { .. }
                | Finding::TitleMismatch { .. }
                | Finding::BadTask { .. }
                | Finding::UnknownTest { .. }
                | Finding::IgnoredTest { .. }
                | Finding::MalformedEntryPoint { .. }
                | Finding::UnknownEntryPointCrate { .. }
                | Finding::UnknownEntryPoint { .. }
                | Finding::UnclassifiedInput { .. }
                | Finding::UnknownInput { .. }
                | Finding::MalformedInput { .. }
                | Finding::AcceptsUnknownFields { .. }) => (other.to_string(), ""),
            })
            .collect()
    }

    #[test]
    fn each_status_requires_and_forbids_its_fields() {
        let missing = |field| ("missing".to_owned(), field);
        let forbidden = |field| ("forbidden".to_owned(), field);
        assert_eq!(
            field_findings("status = \"implemented\"\ntasks = [\"M1-A-01\"]"),
            [
                forbidden("tasks"),
                missing("entry_points"),
                missing("tests")
            ]
        );
        assert_eq!(
            field_findings("status = \"partial\"\ntasks = []\nmissing = \" \""),
            [missing("tasks"), missing("missing")]
        );
        assert_eq!(
            field_findings(
                "status = \"planned\"\ntasks = [\"M1-A-01\"]\n\
                 entry_points = [\"racksmith_domain::Machine\"]"
            ),
            [forbidden("entry_points")]
        );
        assert_eq!(
            field_findings("status = \"informative\"\ntasks = [\"M1-A-01\"]"),
            [forbidden("tasks"), missing("reason")]
        );
        assert_eq!(
            field_findings("status = \"planned\"\ntasks = [\"M1-A-01\"]"),
            []
        );
    }

    #[test]
    fn task_references_must_be_open_plan_tasks() {
        let findings = World::new().findings(&section(
            "status = \"planned\"\ntasks = [\"M1-A-02\", \"M1-Z-09\", \"#12\"]",
        ));
        let problems: Vec<(String, TaskProblem)> = findings
            .into_iter()
            .filter_map(|finding| {
                if let Finding::BadTask { task, problem, .. } = finding {
                    Some((task, problem))
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(
            problems,
            [
                ("M1-A-02".to_owned(), TaskProblem::Done),
                ("M1-Z-09".to_owned(), TaskProblem::Unknown),
                ("#12".to_owned(), TaskProblem::Malformed),
            ]
        );
    }

    #[test]
    fn implemented_claims_need_existing_tests_and_defined_entry_points() {
        let findings = World::new().findings(&section(
            "status = \"implemented\"\nentry_points = [\"racksmith_domain::Ghost\", \"a::\", \
             \"racksmith_domain::machine::Machine::new\"]\n\
             tests = [\"racksmith-domain::machine::tests::nope\", \"machine::tests::new_machine\"]",
        ));
        let entry = "[sections.\"4.1\"]".to_owned();
        assert_eq!(
            findings,
            [
                Finding::UnknownTest {
                    entry: entry.clone(),
                    test: "racksmith-domain::machine::tests::nope".into(),
                },
                Finding::UnknownTest {
                    entry: entry.clone(),
                    test: "machine::tests::new_machine".into(),
                },
                Finding::UnknownEntryPoint {
                    entry: entry.clone(),
                    path: "racksmith_domain::Ghost".into(),
                    segment: "Ghost".into(),
                    krate: "racksmith-domain".into(),
                },
                Finding::MalformedEntryPoint {
                    entry,
                    path: "a::".into(),
                },
            ]
        );
    }

    #[test]
    fn an_entry_point_resolves_in_the_crate_it_names() {
        let findings = World::new().findings(&section(
            "status = \"implemented\"\nentry_points = [\"racksmith_domain::Registry\", \
             \"nonexistent::serve\", \"Machine\", \"racksmith_api::serve\"]\n\
             tests = [\"racksmith-domain::machine::tests::new_machine\"]",
        ));
        let entry = "[sections.\"4.1\"]".to_owned();
        assert_eq!(
            findings,
            [
                Finding::UnknownEntryPoint {
                    entry: entry.clone(),
                    path: "racksmith_domain::Registry".into(),
                    segment: "Registry".into(),
                    krate: "racksmith-domain".into(),
                },
                Finding::UnknownEntryPointCrate {
                    entry: entry.clone(),
                    path: "nonexistent::serve".into(),
                },
                Finding::MalformedEntryPoint {
                    entry,
                    path: "Machine".into(),
                },
            ],
            "Registry lives in racksmith-api; a name defined elsewhere does not count"
        );
    }

    #[test]
    fn an_ignored_test_proves_nothing() {
        let findings = World::new().findings(&section(
            "status = \"implemented\"\nentry_points = [\"racksmith_domain::Machine\"]\n\
             tests = [\"racksmith-cli::slow_serve\"]",
        ));
        assert_eq!(
            findings,
            [Finding::IgnoredTest {
                entry: "[sections.\"4.1\"]".into(),
                test: "racksmith-cli::slow_serve".into(),
            }]
        );
    }

    #[test]
    fn a_type_that_accepts_unknown_fields_is_a_finding() {
        let mut world = World::new();
        world.lax_inputs = ["racksmith-api::Req".to_owned()].into();
        assert_eq!(
            world.findings(COMPLETE),
            [Finding::AcceptsUnknownFields {
                ty: "racksmith-api::Req".into()
            }]
        );
    }

    #[test]
    fn inputs_must_be_classified_known_and_well_formed() {
        let text = r#"
            [inputs."racksmith-api::Req.mac"]
            status = "implemented"
            effect_tests = ["racksmith-api::create_rejects_bad_mac"]

            [inputs."racksmith-api::Req.ghost"]
            status = "implemented"
            effect_tests = ["racksmith-api::create_rejects_bad_mac"]

            [inputs."Req.mac"]
            status = "rejected"
            tasks = ["M1-A-01"]
            tests = ["racksmith-api::create_rejects_bad_mac"]
        "#;
        let findings: Vec<Finding> = World::new()
            .findings(text)
            .into_iter()
            .filter(|finding| !matches!(finding, Finding::UnclassifiedSection { .. }))
            .collect();
        assert_eq!(
            findings,
            [
                Finding::UnclassifiedInput {
                    key: "racksmith-cli::Command.Serve.listen".into()
                },
                Finding::MalformedInput {
                    key: "Req.mac".into()
                },
                Finding::UnknownInput {
                    key: "racksmith-api::Req.ghost".into()
                },
            ]
        );
    }

    #[test]
    fn input_statuses_require_and_forbid_their_fields() {
        let text = r#"
            [inputs."racksmith-api::Req.mac"]
            status = "implemented"
            tests = ["racksmith-api::create_rejects_bad_mac"]

            [inputs."racksmith-cli::Command.Serve.listen"]
            status = "rejected"
            effect_tests = ["racksmith-cli::serve_listens"]
            tests = []
        "#;
        let fields: Vec<(String, &'static str)> = World::new()
            .findings(text)
            .into_iter()
            .filter_map(|finding| match finding {
                Finding::MissingField { entry, field, .. } => {
                    Some((format!("missing {entry}"), field))
                }
                Finding::ForbiddenField { entry, field, .. } => {
                    Some((format!("forbidden {entry}"), field))
                }
                Finding::UnclassifiedSection { .. }
                | Finding::UnknownSection { .. }
                | Finding::TitleMismatch { .. }
                | Finding::BadTask { .. }
                | Finding::UnknownTest { .. }
                | Finding::IgnoredTest { .. }
                | Finding::MalformedEntryPoint { .. }
                | Finding::UnknownEntryPointCrate { .. }
                | Finding::UnknownEntryPoint { .. }
                | Finding::UnclassifiedInput { .. }
                | Finding::UnknownInput { .. }
                | Finding::MalformedInput { .. }
                | Finding::AcceptsUnknownFields { .. } => None,
            })
            .collect();
        let api = "[inputs.\"racksmith-api::Req.mac\"]";
        let cli = "[inputs.\"racksmith-cli::Command.Serve.listen\"]";
        assert_eq!(
            fields,
            [
                (format!("missing {api}"), "effect_tests"),
                (format!("forbidden {api}"), "tests"),
                (format!("forbidden {cli}"), "effect_tests"),
                (format!("missing {cli}"), "tasks"),
                (format!("missing {cli}"), "tests"),
            ]
        );
    }

    #[test]
    fn unknown_keys_and_statuses_are_schema_errors() {
        for text in [
            "[sections.\"4.1\"]\ntitle = \"t\"\nstatus = \"planned\"\nissue = 12\n",
            "[sections.\"4.1\"]\ntitle = \"t\"\nstatus = \"done\"\n",
            "[inputs.\"a::B.c\"]\nstatus = \"ignored\"\n",
            "[requirements]\n",
        ] {
            assert!(Requirements::parse(text).is_err(), "accepted: {text}");
        }
    }

    #[test]
    fn input_keys_have_a_package_a_type_and_a_member() {
        for key in [
            "racksmith-api::CreateMachineRequest.hostname",
            "racksmith-cli::Command.Serve.listen",
            "racksmith-api::Pair.0",
            "racksmith-api::Action.Rename.0",
            "racksmith-api::route./machines/{id}.id",
            "racksmith-api::route./files/{*path}.path",
            "racksmith-cli::env.RUST_LOG",
        ] {
            assert!(is_input_key(key), "{key}");
        }
        for key in [
            "CreateMachineRequest.hostname",
            "racksmith-api::CreateMachineRequest",
            "racksmith-api::Request.a.b.c",
            "racksmith-api::0Type.x",
            "racksmith api::T.x",
            "::T.x",
            "racksmith-api::T.",
            "racksmith-api::route./machines/{id}.name",
            "racksmith-api::route.machines/{id}.id",
            "racksmith-api::route./a b/{id}.id",
            "racksmith-cli::env.",
            "racksmith-cli::env.MY-VAR",
        ] {
            assert!(!is_input_key(key), "{key}");
        }
    }

    #[test]
    fn renders_a_stable_coverage_page() {
        let world = World::new();
        let requirements = Requirements::parse(COMPLETE).unwrap();
        let page = requirements.render(&world.facts());
        assert!(
            page.starts_with("<!-- Generated by `cargo xtask spec --fix`"),
            "{page}"
        );
        assert!(
            page.contains(
                "- 3 design sections: 1 implemented, 1 partial, 0 planned, 1 informative"
            ),
            "{page}"
        );
        assert!(
            page.contains("- 2 inputs: 1 implemented, 1 rejected"),
            "{page}"
        );
        assert!(
            page.contains("| 4.1 | The machine aggregate | implemented | 0 | 1 |"),
            "{page}"
        );
        assert!(
            page.contains("| 4.2 | Storage | partial | 1 | 0 |"),
            "{page}"
        );
        let order: Vec<usize> = ["| 4.1 |", "| 4.2 |", "| 15 |"]
            .iter()
            .map(|row| page.find(row).unwrap())
            .collect();
        assert!(
            order.is_sorted(),
            "document order, not key order: {order:?}"
        );
        assert!(
            page.contains("| `racksmith-api::Req.mac` | rejected | 1 | 1 |"),
            "{page}"
        );
        assert_eq!(page, requirements.render(&world.facts()), "deterministic");
    }

    #[test]
    fn titles_with_pipes_do_not_break_the_table() {
        assert_eq!(escape("a | b"), "a \\| b");
    }
}
