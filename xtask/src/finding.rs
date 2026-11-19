//! Completeness gaps found by `cargo xtask spec`, with actionable messages.
//!
//! Every message states what is missing or wrong, which entry of `requirements.toml` it concerns,
//! and how to fix it.

use std::fmt;

use crate::plan_tasks::TaskProblem;

/// One gap between the design, the code and `requirements.toml`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Finding {
    /// A leaf section of the design has no `[sections."<key>"]` entry.
    UnclassifiedSection { key: String, title: String },
    /// A `[sections]` key is not a leaf section of the design.
    UnknownSection { key: String },
    /// A section's `title` differs from its heading in the design.
    TitleMismatch {
        key: String,
        expected: String,
        found: String,
    },
    /// A field that the entry's status requires is absent or empty.
    MissingField {
        entry: String,
        field: &'static str,
        status: &'static str,
    },
    /// A field that the entry's status forbids is present.
    ForbiddenField {
        entry: String,
        field: &'static str,
        status: &'static str,
    },
    /// A tracking reference is not an open task of the plan.
    BadTask {
        entry: String,
        task: String,
        problem: TaskProblem,
    },
    /// A named test is not in the product crates' test listing.
    UnknownTest { entry: String, test: String },
    /// A named test is marked `#[ignore]`, so the gate never runs it.
    IgnoredTest { entry: String, test: String },
    /// An entry point is not a path of at least two identifiers.
    MalformedEntryPoint { entry: String, path: String },
    /// An entry point's first segment is not the identifier of a product crate.
    UnknownEntryPointCrate { entry: String, path: String },
    /// A later segment of an entry point is not defined in the crate it names.
    UnknownEntryPoint {
        entry: String,
        path: String,
        segment: String,
        krate: String,
    },
    /// An input field or variant of the code has no `[inputs."<key>"]` entry.
    UnclassifiedInput { key: String },
    /// An `[inputs]` key is well formed but names no input of the code.
    UnknownInput { key: String },
    /// An `[inputs]` key is not of the form `<package>::<Type>.<member>`.
    MalformedInput { key: String },
    /// A `Deserialize` type with named fields accepts and ignores unknown fields.
    AcceptsUnknownFields { ty: String },
}

impl fmt::Display for Finding {
    #[expect(
        clippy::too_many_lines,
        reason = "one arm per finding keeps the whole message catalogue in one place"
    )]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnclassifiedSection { key, title } => write!(
                f,
                "design section {key} \"{title}\" is not classified — add [sections.\"{key}\"] \
                 to requirements.toml with a status (implemented, partial, planned or \
                 informative)"
            ),
            Self::UnknownSection { key } => write!(
                f,
                "[sections.\"{key}\"] is not a leaf section of docs/design/system-design.md — \
                 fix the key or remove the stale entry"
            ),
            Self::TitleMismatch {
                key,
                expected,
                found,
            } => write!(
                f,
                "[sections.\"{key}\"] has title \"{found}\" but the design heading is \
                 \"{expected}\" — the section changed; re-check its status and copy the title"
            ),
            Self::MissingField {
                entry,
                field,
                status,
            } => write!(
                f,
                "{entry}: status `{status}` requires a non-empty `{field}`"
            ),
            Self::ForbiddenField {
                entry,
                field,
                status,
            } => write!(
                f,
                "{entry}: `{field}` is not allowed with status `{status}` — remove it or change \
                 the status"
            ),
            Self::BadTask {
                entry,
                task,
                problem,
            } => write!(f, "{entry}: task `{task}` {problem}"),
            Self::UnknownTest { entry, test } => write!(
                f,
                "{entry}: test `{test}` does not exist — name it as `<package>::<path>` exactly \
                 as `cargo test -p <package> -- --list` prints it, or write the test"
            ),
            Self::IgnoredTest { entry, test } => write!(
                f,
                "{entry}: test `{test}` is marked #[ignore] and never runs, so it proves \
                 nothing — remove the #[ignore] or cite a test that runs"
            ),
            Self::AcceptsUnknownFields { ty } => write!(
                f,
                "`{ty}` derives Deserialize with named fields but accepts and ignores unknown \
                 fields — add #[serde(deny_unknown_fields)] (and do not #[serde(flatten)], \
                 which defeats it)"
            ),
            Self::MalformedEntryPoint { entry, path } => write!(
                f,
                "{entry}: entry point `{path}` is not a path such as `racksmith_domain::Machine`"
            ),
            Self::UnknownEntryPointCrate { entry, path } => write!(
                f,
                "{entry}: entry point `{path}` does not start with a product crate such as \
                 `racksmith_api`"
            ),
            Self::UnknownEntryPoint {
                entry,
                path,
                segment,
                krate,
            } => write!(
                f,
                "{entry}: entry point `{path}`: `{segment}` is not defined in the non-test \
                 sources of {krate}"
            ),
            Self::UnclassifiedInput { key } => write!(
                f,
                "input `{key}` is not classified — add [inputs.\"{key}\"] to requirements.toml: \
                 `implemented` with an effect test that proves changing it changes an outcome, \
                 or `rejected` with a task and a test that admission rejects it"
            ),
            Self::UnknownInput { key } => write!(
                f,
                "[inputs.\"{key}\"] names no field or variant of a Deserialize or clap type, \
                 no router path parameter and no environment variable read — fix the key or \
                 remove the stale entry"
            ),
            Self::MalformedInput { key } => write!(
                f,
                "[inputs.\"{key}\"] is not of the form `<package>::<Type>.<field or Variant>`, \
                 `<package>::route.<pattern>.<param>` or `<package>::env.<NAME>`"
            ),
        }
    }
}
