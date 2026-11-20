//! Architecture violations and their actionable messages.
//!
//! Every message states what is wrong, where, and how to fix it, because the reader is as
//! likely to be an AI agent as a human.

use std::fmt;
use std::path::PathBuf;

use crate::exemption_ledger::{FillerProblem, ReferenceProblem, slash_path};
use crate::plan_tasks::TaskProblem;

/// The kind of a dependency edge, mirroring the manifest sections.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum DepKind {
    /// `[dependencies]`
    Normal,
    /// `[dev-dependencies]`
    Dev,
    /// `[build-dependencies]`
    Build,
}

impl DepKind {
    /// The `architecture.toml` key that allowlists external crates of this kind.
    pub(crate) fn allowlist_key(self) -> &'static str {
        match self {
            Self::Normal => "deps",
            Self::Dev => "dev-deps",
            Self::Build => "build-deps",
        }
    }
}

impl fmt::Display for DepKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Normal => "dependency",
            Self::Dev => "dev-dependency",
            Self::Build => "build-dependency",
        })
    }
}

/// A single breach of the architecture rules.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Violation {
    /// A workspace crate has no entry in `architecture.toml`.
    UndeclaredCrate { krate: String },
    /// `architecture.toml` declares a crate that is not a workspace member.
    StaleDeclaration { krate: String },
    /// A workspace dependency points to the same or a higher layer.
    LayerInversion {
        krate: String,
        layer: u8,
        dep: String,
        dep_layer: u8,
    },
    /// A workspace dependency is not in the crate's `workspace-deps`.
    WorkspaceDepNotAllowed { krate: String, dep: String },
    /// An external dependency is not in the allowlist for its kind.
    ExternalDepNotAllowed {
        krate: String,
        dep: String,
        kind: DepKind,
    },
    /// A dependency comes from somewhere other than crates.io or a workspace member.
    UntrustedSource {
        krate: String,
        dep: String,
        origin: String,
    },
    /// The root manifest uses a key that can substitute unchecked code.
    ForbiddenRootManifestKey { key: String },
    /// The fuzz project's lint table differs from the workspace lint policy.
    MirroredLintsDiffer { path: PathBuf },
    /// No workspace member declares a `rust-version`, so the MSRV cannot be checked.
    MissingRustVersion,
    /// A locked dependency declares a `rust-version` above the workspace MSRV.
    DependencyNeedsNewerRust {
        package: String,
        rust_version: String,
        msrv: String,
    },
    /// A crate's target or build script lives outside the crate directory.
    TargetOutsideCrate {
        krate: String,
        key: String,
        target: String,
    },
    /// A crate uses a dependency that is banned for it.
    BannedDependency {
        krate: String,
        dep: String,
        reason: String,
    },
    /// A member manifest lacks `[lints] workspace = true`.
    MissingWorkspaceLints { krate: String },
    /// A member manifest declares a dependency without `workspace = true`.
    LocalDependencySpec {
        krate: String,
        section: String,
        dep: String,
    },
    /// A banned code pattern appears in a crate's sources.
    BannedSourcePattern {
        path: PathBuf,
        line: usize,
        pattern: String,
        reason: String,
    },
    /// A lint level other than `expect` is set in source.
    LintLevelAttribute {
        path: PathBuf,
        line: usize,
        level: &'static str,
    },
    /// An `expect` attribute exempts a whole module.
    ModuleWideExpect { path: PathBuf, line: usize },
    /// Code is loaded from a file outside the standard module layout.
    HiddenModuleSource {
        path: PathBuf,
        line: usize,
        construct: &'static str,
    },
    /// A `clippy` cfg makes code differ between clippy runs and real builds.
    LinterCfg { path: PathBuf, line: usize },
    /// A `mod.rs` module file exists.
    ModRsFile { path: PathBuf },
    /// A `TODO`/`FIXME` comment is not linked to an issue.
    UntrackedMarker {
        path: PathBuf,
        line: usize,
        marker: String,
    },
    /// An `#[expect]` has no entry in the exemption ledger.
    UnlistedExemption {
        path: PathBuf,
        line: usize,
        item: String,
        lints: String,
    },
    /// Two `#[expect]` attributes of one file exempt the same lints on the same item.
    AmbiguousExemption {
        path: PathBuf,
        line: usize,
        item: String,
        lints: String,
    },
    /// A ledger entry matches no `#[expect]` attribute.
    StaleExemption {
        file: String,
        item: String,
        lints: String,
    },
    /// The ledger lists one exemption more than once.
    DuplicateExemptionEntry {
        file: String,
        item: String,
        lints: String,
    },
    /// A ledger entry's reference does not resolve.
    UnresolvedExemptionReference {
        file: String,
        item: String,
        reference: String,
        problem: ReferenceProblem,
    },
    /// A ledger entry's justification is filler.
    FillerExemptionJustification {
        file: String,
        item: String,
        problem: FillerProblem,
    },
    /// An `#[expect]` attribute's `reason` is missing or filler.
    FillerExemptionReason {
        path: PathBuf,
        line: usize,
        item: String,
        lints: String,
        problem: FillerProblem,
    },
    /// A subsystem lacks a field its status requires, or sets one it forbids.
    SubsystemField {
        subsystem: String,
        field: &'static str,
        status: &'static str,
        required: bool,
    },
    /// A subsystem's `entry` is not a single identifier.
    SubsystemEntryNotIdentifier { subsystem: String, entry: String },
    /// A wired subsystem names a package that is not a workspace member.
    SubsystemUnknownCrate {
        subsystem: String,
        field: &'static str,
        krate: String,
    },
    /// A wired subsystem's entry is not defined in its crate's non-test sources.
    SubsystemEntryNotDefined {
        subsystem: String,
        entry: String,
        krate: String,
    },
    /// A wired subsystem's entry is not used by its consumer's non-test sources.
    SubsystemNotWired {
        subsystem: String,
        entry: String,
        consumer: String,
    },
    /// A wired subsystem's consumer is its own crate, which is not the composition root.
    SubsystemSelfConsumer { subsystem: String, krate: String },
    /// A wired subsystem's consumer defines its own item named like the entry.
    SubsystemShadowed {
        subsystem: String,
        entry: String,
        consumer: String,
    },
    /// A wired subsystem's consumer does not depend on the defining crate.
    SubsystemConsumerLacksDependency {
        subsystem: String,
        consumer: String,
        krate: String,
    },
    /// A planned subsystem's tracking reference is not an open plan task.
    SubsystemTask {
        subsystem: String,
        task: String,
        problem: TaskProblem,
    },
    /// A planned subsystem's entry already appears in the workspace.
    SubsystemHalfWired {
        subsystem: String,
        entry: String,
        path: PathBuf,
        line: usize,
    },
}

impl fmt::Display for Violation {
    #[expect(
        clippy::too_many_lines,
        reason = "one arm per violation keeps the whole message catalogue in one place"
    )]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UndeclaredCrate { krate } => write!(
                f,
                "crate `{krate}` is not declared in architecture.toml — add a [crates.{krate}] \
                 entry with its layer and allowlists (an architectural change: write an ADR)"
            ),
            Self::StaleDeclaration { krate } => write!(
                f,
                "architecture.toml declares `{krate}`, which is not a workspace member — \
                 remove the stale [crates.{krate}] entry"
            ),
            Self::LayerInversion {
                krate,
                layer,
                dep,
                dep_layer,
            } => write!(
                f,
                "crate `{krate}` (layer {layer}) depends on `{dep}` (layer {dep_layer}), but \
                 dependencies must point to a strictly lower layer — move the code or update \
                 architecture.toml via an ADR"
            ),
            Self::WorkspaceDepNotAllowed { krate, dep } => write!(
                f,
                "crate `{krate}` depends on workspace crate `{dep}`, which is not in its \
                 `workspace-deps` — remove the dependency or allow it in architecture.toml via \
                 an ADR"
            ),
            Self::ExternalDepNotAllowed { krate, dep, kind } => write!(
                f,
                "crate `{krate}` has {kind} `{dep}`, which is not in its `{key}` allowlist — \
                 remove it or add it to [crates.{krate}] {key} in architecture.toml (justify \
                 in the PR)",
                key = kind.allowlist_key()
            ),
            Self::UntrustedSource { krate, dep, origin } => write!(
                f,
                "crate `{krate}` depends on `{dep}` from {origin} — external crates must come \
                 from crates.io (checked by cargo-deny) and workspace crates must be members, so \
                 that every line of code in the build is linted or audited"
            ),
            Self::ForbiddenRootManifestKey { key } => write!(
                f,
                "the root Cargo.toml sets `{key}`, which can replace or hide code that no \
                 workspace rule checks — remove it (an exception needs an ADR and an xtask change)"
            ),
            Self::MirroredLintsDiffer { path } => write!(
                f,
                "{}: [lints] differs from the root [workspace.lints] — this project cannot \
                 inherit the workspace policy, so copy the root table exactly",
                path.display()
            ),
            Self::MissingRustVersion => write!(
                f,
                "no workspace member declares `rust-version` — set it once in the root \
                 [workspace.package] and inherit it with `rust-version.workspace = true`"
            ),
            Self::DependencyNeedsNewerRust {
                package,
                rust_version,
                msrv,
            } => write!(
                f,
                "dependency `{package}` requires Rust {rust_version}, above the workspace MSRV \
                 {msrv} — pin an older version of it in Cargo.lock, or raise `rust-version` \
                 (and clippy.toml `msrv`) deliberately in its own PR"
            ),
            Self::TargetOutsideCrate { krate, key, target } => write!(
                f,
                "crate `{krate}` sets `{key} = \"{target}\"`, outside the crate directory — keep \
                 every source file inside the crate so the architecture rules scan it"
            ),
            Self::BannedDependency { krate, dep, reason } => write!(
                f,
                "crate `{krate}` depends on banned crate `{dep}`: {reason}"
            ),
            Self::MissingWorkspaceLints { krate } => write!(
                f,
                "crate `{krate}` does not inherit the workspace lint policy — add \
                 `[lints] workspace = true` to its Cargo.toml"
            ),
            Self::LocalDependencySpec {
                krate,
                section,
                dep,
            } => write!(
                f,
                "crate `{krate}` declares `{dep}` in [{section}] without `workspace = true` — \
                 declare the version once in the root [workspace.dependencies] and use \
                 `{dep}.workspace = true`"
            ),
            Self::BannedSourcePattern {
                path,
                line,
                pattern,
                reason,
            } => write!(
                f,
                "{}:{line}: `{pattern}` is banned here: {reason}",
                path.display()
            ),
            Self::LintLevelAttribute { path, line, level } => write!(
                f,
                "{}:{line}: `{level}` attribute — lint levels are set only in the root \
                 [workspace.lints]; for a justified exception put \
                 `#[expect(lint, reason = \"...\")]` on the smallest item",
                path.display()
            ),
            Self::ModuleWideExpect { path, line } => write!(
                f,
                "{}:{line}: `expect` exempts a whole module — put \
                 `#[expect(lint, reason = \"...\")]` on the smallest item that needs it",
                path.display()
            ),
            Self::HiddenModuleSource {
                path,
                line,
                construct,
            } => write!(
                f,
                "{}:{line}: `{construct}` loads code from a file the architecture rules may not \
                 scan — declare modules with `mod name;` in the standard file layout",
                path.display()
            ),
            Self::LinterCfg { path, line } => write!(
                f,
                "{}:{line}: the `clippy` cfg hides code from clippy or shows it different code \
                 than the compiler builds — remove it; for a justified exception put \
                 `#[expect(lint, reason = \"...\")]` on the smallest item",
                path.display()
            ),
            Self::ModRsFile { path } => write!(
                f,
                "{}: `mod.rs` files are not allowed — name the module file after the module",
                path.display()
            ),
            Self::UntrackedMarker { path, line, marker } => write!(
                f,
                "{}:{line}: `{marker}` without an issue link — write `{marker}(#123)` referencing \
                 a tracking issue, or resolve it now",
                path.display()
            ),
            Self::UnlistedExemption {
                path,
                line,
                item,
                lints,
            } => write!(
                f,
                "{}:{line}: `#[expect({lints})]` on `{item}` has no entry in exceptions.toml — \
                 fix the code so the lint passes, or add an [[exceptions]] entry with \
                 file = \"{}\", item = \"{item}\", the lints, a justification and a reference \
                 that resolves (`ADR-NNNN`, an open plan task `M1-A-01` or `design N.M`) for \
                 code-owner review",
                path.display(),
                slash_path(path)
            ),
            Self::AmbiguousExemption {
                path,
                line,
                item,
                lints,
            } => write!(
                f,
                "{}:{line}: another `#[expect({lints})]` in this file also exempts `{item}`, so a \
                 ledger entry cannot tell them apart — merge them, or move one onto a smaller \
                 named item",
                path.display()
            ),
            Self::StaleExemption { file, item, lints } => write!(
                f,
                "exceptions.toml: the entry for `{item}` in {file} ({lints}) matches no \
                 `#[expect]` attribute — remove the stale entry, or fix its file, item or lints"
            ),
            Self::DuplicateExemptionEntry { file, item, lints } => write!(
                f,
                "exceptions.toml: `{item}` in {file} ({lints}) is listed more than once — keep \
                 one entry"
            ),
            Self::UnresolvedExemptionReference {
                file,
                item,
                reference,
                problem,
            } => write!(
                f,
                "exceptions.toml: the entry for `{item}` in {file} cites `{reference}`, which \
                 {problem}"
            ),
            Self::FillerExemptionJustification {
                file,
                item,
                problem,
            } => write!(
                f,
                "exceptions.toml: the justification for `{item}` in {file} {problem} — explain \
                 in full sentences why the lint is wrong for this item and why the code should \
                 not change"
            ),
            Self::FillerExemptionReason {
                path,
                line,
                item,
                lints,
                problem,
            } => write!(
                f,
                "{}:{line}: the reason of `#[expect({lints})]` on `{item}` {problem} — say why \
                 the lint does not apply here, consistently with its exceptions.toml \
                 justification",
                path.display()
            ),
            Self::SubsystemField {
                subsystem,
                field,
                status,
                required: true,
            } => write!(
                f,
                "[subsystems.{subsystem}]: status `{status}` requires a non-empty `{field}`"
            ),
            Self::SubsystemField {
                subsystem,
                field,
                status,
                required: false,
            } => write!(
                f,
                "[subsystems.{subsystem}]: `{field}` is not allowed with status `{status}` — \
                 remove it or change the status"
            ),
            Self::SubsystemEntryNotIdentifier { subsystem, entry } => write!(
                f,
                "[subsystems.{subsystem}]: entry `{entry}` must be one identifier, such as \
                 `MachineRegistry`"
            ),
            Self::SubsystemUnknownCrate {
                subsystem,
                field,
                krate,
            } => write!(
                f,
                "[subsystems.{subsystem}]: {field} `{krate}` is not a workspace member"
            ),
            Self::SubsystemEntryNotDefined {
                subsystem,
                entry,
                krate,
            } => write!(
                f,
                "[subsystems.{subsystem}]: `{entry}` is not defined in the non-test sources of \
                 `{krate}` — fix `entry` or `crate`, or mark the subsystem `planned` with a task"
            ),
            Self::SubsystemNotWired {
                subsystem,
                entry,
                consumer,
            } => write!(
                f,
                "[subsystems.{subsystem}]: `{entry}` is built but `{consumer}` never uses it \
                 outside tests — wire it in, or mark the subsystem `planned` with a task and \
                 remove the unused code"
            ),
            Self::SubsystemSelfConsumer { subsystem, krate } => write!(
                f,
                "[subsystems.{subsystem}]: consumer `{krate}` is the defining crate, which only \
                 the composition root (the single highest-layer product crate, a binary) may be \
                 — name the crate that calls it"
            ),
            Self::SubsystemShadowed {
                subsystem,
                entry,
                consumer,
            } => write!(
                f,
                "[subsystems.{subsystem}]: `{consumer}` defines its own `{entry}`, so its uses \
                 of that name prove nothing about the subsystem — rename one of them"
            ),
            Self::SubsystemConsumerLacksDependency {
                subsystem,
                consumer,
                krate,
            } => write!(
                f,
                "[subsystems.{subsystem}]: consumer `{consumer}` does not depend on `{krate}`, \
                 so it cannot use its entry — fix `consumer` or `crate`"
            ),
            Self::SubsystemTask {
                subsystem,
                task,
                problem,
            } => write!(f, "[subsystems.{subsystem}]: task `{task}` {problem}"),
            Self::SubsystemHalfWired {
                subsystem,
                entry,
                path,
                line,
            } => write!(
                f,
                "{}:{line}: `{entry}` of planned subsystem `{subsystem}` already exists — a \
                 planned subsystem has no code; finish and wire it (status `wired`), or remove \
                 it",
                path.display()
            ),
        }
    }
}
