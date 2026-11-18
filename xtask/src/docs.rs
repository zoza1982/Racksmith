//! `cargo xtask docs`: keep generated sections of docs and models in sync with their source.
//!
//! A generated section lives between a BEGIN and an END marker, written as a comment in the
//! file's language (`<!-- BEGIN GENERATED: <name> -->` in Markdown, `\* BEGIN GENERATED: <name>`
//! in TLA+), and must equal the output of a command, so the files can never drift from the code
//! they describe.

use std::fs;
use std::path::Path;

use anyhow::{Context as _, Result, bail, ensure};

use crate::{process, term};

/// A section of a file whose contents are produced by a command.
struct GeneratedSection {
    /// Marker name, as in `<!-- BEGIN GENERATED: <name> -->`.
    name: &'static str,
    /// File containing the markers, relative to the workspace root.
    file: &'static str,
    /// How the file writes comments, and so its markers.
    style: CommentStyle,
    /// Arguments to `cargo` that print the section's contents on stdout.
    cargo_args: &'static [&'static str],
}

/// The comment syntax used for a file's markers.
#[derive(Debug, Clone, Copy)]
enum CommentStyle {
    /// `<!-- ... -->`
    Html,
    /// `\* ...`, a TLA+ line comment.
    Tla,
}

impl CommentStyle {
    /// The BEGIN and END markers of the section `name`.
    fn markers(self, name: &str) -> (String, String) {
        match self {
            Self::Html => (
                format!("<!-- BEGIN GENERATED: {name} -->"),
                format!("<!-- END GENERATED: {name} -->"),
            ),
            Self::Tla => (
                format!("\\* BEGIN GENERATED: {name}"),
                format!("\\* END GENERATED: {name}"),
            ),
        }
    }
}

const SECTIONS: &[GeneratedSection] = &[
    GeneratedSection {
        name: "lifecycle",
        file: "docs/lifecycle.md",
        style: CommentStyle::Html,
        cargo_args: &[
            "run",
            "--quiet",
            "--package",
            "racksmith-cli",
            "--",
            "lifecycle",
        ],
    },
    GeneratedSection {
        name: "transitions",
        file: "specs/tla/MachineLifecycle.tla",
        style: CommentStyle::Tla,
        cargo_args: &[
            "run",
            "--quiet",
            "--package",
            "racksmith-cli",
            "--",
            "lifecycle",
            "--format",
            "tla",
        ],
    },
];

/// Checks every generated section, or rewrites stale ones when `fix` is set.
///
/// # Errors
///
/// Returns an error if a generator fails, a file lacks its markers, or (without `fix`) a
/// section is out of date.
pub(crate) fn run(root: &Path, fix: bool) -> Result<()> {
    let mut stale = Vec::new();
    for section in SECTIONS {
        let path = root.join(section.file);
        let current = fs::read_to_string(&path).with_context(|| {
            format!(
                "failed to read {} (it must contain the generated-section markers)",
                section.file
            )
        })?;
        let generated = process::capture(process::cargo(root).args(section.cargo_args))?;
        let markers = section.style.markers(section.name);
        let expected = replace_section(&current, &markers, &generated)
            .with_context(|| format!("cannot update {}", section.file))?;
        if expected == current {
            continue;
        }
        if fix {
            fs::write(&path, expected)
                .with_context(|| format!("failed to write {}", section.file))?;
            term::info(&format!(
                "docs: regenerated `{}` in {}",
                section.name, section.file
            ))?;
        } else {
            stale.push(section.file);
        }
    }
    if !stale.is_empty() {
        bail!(
            "generated docs are out of date: {} — run `cargo xtask docs --fix`",
            stale.join(", ")
        );
    }
    term::info("docs: generated sections are up to date")?;
    Ok(())
}

/// Returns `doc` with the body between the `(begin, end)` markers replaced by `content`.
///
/// The body always starts on the line after the BEGIN marker and ends with a newline right
/// before the END marker.
///
/// # Errors
///
/// Returns an error if either marker is missing, appears more than once, or the END marker
/// precedes the BEGIN marker.
fn replace_section(doc: &str, (begin, end): &(String, String), content: &str) -> Result<String> {
    let begin_at = find_unique(doc, begin)?;
    let end_at = find_unique(doc, end)?;
    ensure!(begin_at < end_at, "`{end}` must come after `{begin}`");

    let head = doc
        .get(..begin_at + begin.len())
        .context("marker is not on a char boundary")?;
    let tail = doc
        .get(end_at..)
        .context("marker is not on a char boundary")?;
    let newline = if content.ends_with('\n') { "" } else { "\n" };
    Ok(format!("{head}\n{content}{newline}{tail}"))
}

fn find_unique(doc: &str, marker: &str) -> Result<usize> {
    let mut positions = doc.match_indices(marker).map(|(index, _)| index);
    let first = positions
        .next()
        .with_context(|| format!("missing marker `{marker}`"))?;
    ensure!(
        positions.next().is_none(),
        "marker `{marker}` appears more than once"
    );
    Ok(first)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC: &str =
        "# Title\n\n<!-- BEGIN GENERATED: t -->\nold\n<!-- END GENERATED: t -->\n\nafter\n";

    fn replace(doc: &str, content: &str) -> Result<String> {
        replace_section(doc, &CommentStyle::Html.markers("t"), content)
    }

    #[test]
    fn replaces_only_the_section_body() {
        let updated = replace(DOC, "new\n").unwrap();
        assert_eq!(
            updated,
            "# Title\n\n<!-- BEGIN GENERATED: t -->\nnew\n<!-- END GENERATED: t -->\n\nafter\n"
        );
    }

    #[test]
    fn replacement_is_idempotent() {
        let once = replace(DOC, "new").unwrap();
        assert_eq!(replace(&once, "new").unwrap(), once);
    }

    #[test]
    fn up_to_date_doc_is_unchanged() {
        assert_eq!(replace(DOC, "old\n").unwrap(), DOC);
    }

    #[test]
    fn tla_markers_are_line_comments() {
        let doc = "A == 1\n\\* BEGIN GENERATED: t\nold\n\\* END GENERATED: t\n====\n";
        let updated = replace_section(doc, &CommentStyle::Tla.markers("t"), "B == 2").unwrap();
        assert_eq!(
            updated,
            "A == 1\n\\* BEGIN GENERATED: t\nB == 2\n\\* END GENERATED: t\n====\n"
        );
    }

    #[test]
    fn missing_marker_is_an_error() {
        let error = replace("# no markers\n", "x").unwrap_err().to_string();
        assert!(
            error.contains("missing marker `<!-- BEGIN GENERATED: t -->`"),
            "{error}"
        );
    }

    #[test]
    fn duplicate_or_reversed_markers_are_errors() {
        let duplicated = format!("{DOC}<!-- END GENERATED: t -->\n");
        assert!(replace(&duplicated, "x").is_err(), "duplicate END marker");
        let reversed = "<!-- END GENERATED: t -->\n<!-- BEGIN GENERATED: t -->\n";
        assert!(replace(reversed, "x").is_err(), "END before BEGIN");
    }
}
