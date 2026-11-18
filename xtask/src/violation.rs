//! Architecture violations and their actionable messages.
//!
//! Every message states what is wrong, where, and how to fix it, because the reader is as
//! likely to be an AI agent as a human.

use std::fmt;
use std::path::PathBuf;

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
    /// A `mod.rs` module file exists.
    ModRsFile { path: PathBuf },
    /// A `TODO`/`FIXME` comment is not linked to an issue.
    UntrackedMarker {
        path: PathBuf,
        line: usize,
        marker: String,
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
        }
    }
}
