//! The leaf sections of the system design, the keys of `[sections]` in `requirements.toml`.
//!
//! A leaf is every numbered level-3 heading (`### 4.1 The machine aggregate`) plus every
//! numbered level-2 heading (`## 15. Decisions needing ADRs`) without numbered level-3
//! children. Unnumbered headings (`## References`) and headings inside code blocks are not
//! sections. A heading that starts with a digit but is not numbered in one of those two forms
//! (`## 17 New chapter`, `### 4.1`, `### 4.1.2 Detail`) is an error, and so is a `### N.M`
//! heading under a `## K.` heading with another major number, so that no section can drop out
//! of `requirements.toml` through a typo.

use std::collections::BTreeSet;

use anyhow::{Result, bail};

/// The authoritative design document, relative to the workspace root.
pub(crate) const DESIGN_FILE: &str = "docs/design/system-design.md";

/// One leaf section of the design.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Section {
    /// The section number, as `4.1` or `15`.
    pub(crate) key: String,
    /// The heading text after the number.
    pub(crate) title: String,
}

/// A level-2 or level-3 heading, before leaves are chosen.
enum Heading {
    /// A numbered heading of level 2 or 3.
    Numbered { level: u8, section: Section },
    /// An unnumbered level-2 heading: not a section, but it ends the children of the one
    /// before it.
    Unnumbered,
}

/// The leaf sections of `text`, in document order.
///
/// # Errors
///
/// Returns an error if two headings carry the same number, a heading starts with a digit but
/// is malformed, or a `### N.M` heading sits under a `## K.` heading with `K != N`.
pub(crate) fn leaf_sections(text: &str) -> Result<Vec<Section>> {
    let headings = headings(text)?;
    let mut leaves = Vec::new();
    let mut seen = BTreeSet::new();
    let mut parent: Option<&str> = None;
    for (index, heading) in headings.iter().enumerate() {
        let Heading::Numbered { level, section } = heading else {
            parent = None;
            continue;
        };
        if *level == 2 {
            parent = Some(section.key.as_str());
        } else if let Some(parent) = parent {
            let major = section.key.split_once('.').map(|(major, _)| major);
            if major != Some(parent) {
                bail!(
                    "{DESIGN_FILE}: section {} is under section {parent}. — renumber it or \
                     move it",
                    section.key
                );
            }
        }
        if !seen.insert(section.key.as_str()) {
            bail!(
                "{DESIGN_FILE}: section {} is numbered more than once",
                section.key
            );
        }
        let has_children = *level == 2
            && matches!(
                headings.get(index + 1),
                Some(Heading::Numbered { level: 3, .. })
            );
        if !has_children {
            leaves.push(section.clone());
        }
    }
    Ok(leaves)
}

/// Every level-2 heading and every numbered level-3 heading outside code blocks.
///
/// # Errors
///
/// Returns an error naming the line of a level-2 or level-3 heading that starts with a digit
/// but is not a well-formed numbered heading.
fn headings(text: &str) -> Result<Vec<Heading>> {
    let mut headings = Vec::new();
    let mut in_fence = false;
    for (index, line) in text.lines().enumerate() {
        if line.starts_with("```") || line.starts_with("~~~") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        let (level, rest) = if let Some(rest) = line.strip_prefix("## ") {
            (2, rest)
        } else if let Some(rest) = line.strip_prefix("### ") {
            (3, rest)
        } else {
            continue;
        };
        match numbered(level, rest) {
            Some(heading) => headings.push(heading),
            None if rest.trim_start().starts_with(|c: char| c.is_ascii_digit()) => bail!(
                "{DESIGN_FILE}:{}: heading `{line}` starts with a digit but is not numbered as \
                 `## N. Title` or `### N.M Title`",
                index + 1
            ),
            None if level == 2 => headings.push(Heading::Unnumbered),
            None => {}
        }
    }
    Ok(headings)
}

/// The heading `rest` if it starts with a section number: `N.` at level 2, `N.M` at level 3.
fn numbered(level: u8, rest: &str) -> Option<Heading> {
    let (number, title) = rest.trim().split_once(' ')?;
    if title.trim().is_empty() {
        return None;
    }
    let key = if level == 2 {
        number.strip_suffix('.').filter(|key| is_number(key))?
    } else {
        let key = number.strip_suffix('.').unwrap_or(number);
        let (major, minor) = key.split_once('.')?;
        (is_number(major) && is_number(minor)).then_some(key)?
    };
    Some(Heading::Numbered {
        level,
        section: Section {
            key: key.to_owned(),
            title: title.trim().to_owned(),
        },
    })
}

fn is_number(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(text: &str) -> Vec<(String, String)> {
        leaf_sections(text)
            .unwrap()
            .into_iter()
            .map(|section| (section.key, section.title))
            .collect()
    }

    fn pairs(expected: &[(&str, &str)]) -> Vec<(String, String)> {
        expected
            .iter()
            .map(|(key, title)| ((*key).to_owned(), (*title).to_owned()))
            .collect()
    }

    #[test]
    fn leaves_are_level_three_sections_and_childless_level_two_sections() {
        let text = "\
# Title

## 1. Overview

### 1.1 The design in one paragraph

### 1.2 `decide()`: a pure step

## 2. Decisions needing ADRs

Prose.

## 3. Storage

### 3.1 Engine

## References

### Unnumbered subsection
";
        assert_eq!(
            keys(text),
            pairs(&[
                ("1.1", "The design in one paragraph"),
                ("1.2", "`decide()`: a pure step"),
                ("2", "Decisions needing ADRs"),
                ("3.1", "Engine"),
            ])
        );
    }

    #[test]
    fn an_unnumbered_level_two_heading_ends_the_children_of_the_previous_one() {
        let text = "## 1. Alone\n\n## Interlude\n\n### 2.1 Orphan\n";
        assert_eq!(keys(text), pairs(&[("1", "Alone"), ("2.1", "Orphan")]));
    }

    #[test]
    fn headings_inside_code_blocks_are_ignored() {
        let text = "## 1. Real\n\n```toml\n## 2. Not a heading\n### 1.1 Nor this\n```\n";
        assert_eq!(keys(text), pairs(&[("1", "Real")]));
    }

    #[test]
    fn duplicate_numbers_are_an_error() {
        let text = "### 1.1 One\n### 1.1 Two\n";
        let error = leaf_sections(text).unwrap_err().to_string();
        assert!(
            error.contains("section 1.1 is numbered more than once"),
            "{error}"
        );
    }

    #[test]
    fn malformed_numbered_headings_are_errors() {
        for heading in [
            "## 17 New chapter",
            "## 5.Title",
            "### 4.1",
            "### 4.1.2 Detail",
            "### 5. Level-two number at level three",
            "## 4.1 Level-three number at level two",
        ] {
            let text = format!("## 1. Fine\n\n{heading}\n");
            let error = leaf_sections(&text).unwrap_err().to_string();
            assert!(error.contains(":3: heading"), "{heading}: {error}");
        }
    }

    #[test]
    fn a_child_must_carry_its_parent_number() {
        let text = "## 4. Domain\n\n### 4.1 Fine\n\n### 5.2 Stray\n";
        let error = leaf_sections(text).unwrap_err().to_string();
        assert!(error.contains("section 5.2 is under section 4."), "{error}");
    }

    #[test]
    fn parses_the_repository_design() {
        let sections = leaf_sections(include_str!("../../docs/design/system-design.md")).unwrap();
        assert!(sections.len() > 50, "found {} sections", sections.len());
        assert!(sections.iter().all(|section| !section.title.is_empty()));
    }
}
