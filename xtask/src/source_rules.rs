//! Source-level rules, checked on tokens so that formatting cannot hide a violation.
//!
//! - Lint levels are set only in `Cargo.toml`. In source the one lint attribute is an outer
//!   `#[expect(lint, reason = "...")]` on an item smaller than a module: `allow`, `warn`, `deny`
//!   and `forbid` are rejected (directly or inside `cfg_attr`), and so is a module-wide `expect`.
//! - Modules use the standard file layout, so every line of code is a file these rules scan: no
//!   `mod.rs`, no `#[path]`, no `include!`.
//! - The banned patterns of `architecture.toml` apply to every file of the listed crates except
//!   tests, benches and examples (so build scripts are covered), and are matched both as tokens
//!   and through `use` imports and aliases.
//! - `TODO`/`FIXME` comments reference a tracking issue, as in `TODO(#123)`.

use crate::architecture::{Architecture, SourceBan};
use crate::attributes::{self, Attribute};
use crate::paths;
use crate::source_text::{self, Line};
use crate::tokens::{self, Token};
use crate::violation::Violation;
use crate::workspace::SourceFile;

/// Comment markers that must reference a tracking issue, as in `TODO(#123)`.
const TRACKED_MARKERS: [&str; 2] = ["TODO", "FIXME"];

/// Lint-level attributes other than the self-checking `expect`.
const LINT_LEVELS: [&str; 4] = ["allow", "warn", "deny", "forbid"];

/// A source ban prepared for matching.
struct Ban<'a> {
    rule: &'a SourceBan,
    /// The pattern as tokens, matched against the file's tokens.
    tokens: Vec<String>,
    /// The pattern's segments if it is a path such as `std::fs`, matched against resolved paths.
    path: Option<Vec<String>>,
}

impl<'a> Ban<'a> {
    fn new(rule: &'a SourceBan) -> Self {
        let tokens = tokens::tokenize_str(&rule.pattern);
        let path = path_segments(&tokens);
        Self { rule, tokens, path }
    }
}

/// Checks every source file of one crate. Bans in `architecture` apply only if they list
/// `krate`.
pub(crate) fn check(
    architecture: &Architecture,
    krate: &str,
    files: &[SourceFile],
) -> Vec<Violation> {
    let bans: Vec<Ban<'_>> = architecture
        .source_bans
        .iter()
        .filter(|ban| ban.crates.contains(krate))
        .map(Ban::new)
        .collect();
    let mut violations: Vec<Violation> = files
        .iter()
        .flat_map(|file| check_file(file, &bans))
        .collect();
    violations.sort();
    violations.dedup();
    violations
}

fn check_file(file: &SourceFile, bans: &[Ban<'_>]) -> Vec<Violation> {
    let lines = source_text::split(&file.text);
    let tokens = tokens::tokenize(&lines);
    let mut violations = Vec::new();
    if file.path.file_name().is_some_and(|name| name == "mod.rs") {
        violations.push(Violation::ModRsFile {
            path: file.path.clone(),
        });
    }
    violations.extend(lines.iter().flat_map(|line| untracked_markers(file, line)));
    violations.extend(
        attributes::find(&tokens)
            .iter()
            .filter_map(|attribute| attribute_violation(file, attribute)),
    );
    violations.extend(include_macros(file, &tokens));
    if !file.is_test_code {
        violations.extend(banned_patterns(file, &tokens, bans));
    }
    violations
}

fn attribute_violation(file: &SourceFile, attribute: &Attribute<'_>) -> Option<Violation> {
    let path = file.path.clone();
    let line = attribute.line;
    if let Some(level) = LINT_LEVELS.into_iter().find(|level| attribute.calls(level)) {
        return Some(Violation::LintLevelAttribute { path, line, level });
    }
    if attribute.sets("path") {
        return Some(Violation::HiddenModuleSource {
            path,
            line,
            construct: "#[path]",
        });
    }
    let module_wide = attribute.inner || attribute.applies_to_module();
    (attribute.calls("expect") && module_wide).then_some(Violation::ModuleWideExpect { path, line })
}

fn include_macros(file: &SourceFile, tokens: &[Token]) -> Vec<Violation> {
    tokens
        .windows(2)
        .filter_map(|pair| match pair {
            [name, bang] if name.is("include") && bang.is("!") => {
                Some(Violation::HiddenModuleSource {
                    path: file.path.clone(),
                    line: name.line,
                    construct: "include!",
                })
            }
            _ => None,
        })
        .collect()
}

fn banned_patterns(file: &SourceFile, tokens: &[Token], bans: &[Ban<'_>]) -> Vec<Violation> {
    let resolved = paths::resolve(tokens);
    let mut violations = Vec::new();
    for ban in bans.iter().filter(|ban| !ban.tokens.is_empty()) {
        let token_lines = tokens
            .windows(ban.tokens.len())
            .filter(|window| window.iter().map(|token| &token.text).eq(&ban.tokens))
            .filter_map(|window| window.first().map(|token| token.line));
        let path_lines = ban.path.iter().flat_map(|segments| {
            resolved
                .iter()
                .filter(|path| path.segments.starts_with(segments))
                .map(|path| path.line)
        });
        violations.extend(token_lines.chain(path_lines).map(|line| {
            Violation::BannedSourcePattern {
                path: file.path.clone(),
                line,
                pattern: ban.rule.pattern.clone(),
                reason: ban.rule.reason.clone(),
            }
        }));
    }
    violations
}

/// The segments of `tokens` if they spell a path such as `std :: fs`.
fn path_segments(tokens: &[String]) -> Option<Vec<String>> {
    let mut segments = Vec::new();
    for (index, token) in tokens.iter().enumerate() {
        let expected_segment = index % 2 == 0;
        let is_segment = token.starts_with(|c: char| c.is_alphanumeric() || c == '_');
        match (expected_segment, is_segment) {
            (true, true) => segments.push(token.clone()),
            (false, false) if token == "::" => {}
            _ => return None,
        }
    }
    (tokens.len() % 2 == 1).then_some(segments)
}

fn untracked_markers(file: &SourceFile, line: &Line) -> Vec<Violation> {
    TRACKED_MARKERS
        .iter()
        .filter(|marker| has_untracked_marker(&line.comment, marker))
        .map(|marker| Violation::UntrackedMarker {
            path: file.path.clone(),
            line: line.number,
            marker: (*marker).to_owned(),
        })
        .collect()
}

/// Whether `comment` contains `marker` as a word that is neither quoted in backticks (prose
/// about the marker) nor followed by an issue link such as `(#123)`.
fn has_untracked_marker(comment: &str, marker: &str) -> bool {
    word_matches(comment, marker)
        .any(|(before, after)| !before.ends_with('`') && !starts_with_issue_link(after))
}

fn starts_with_issue_link(text: &str) -> bool {
    text.strip_prefix("(#")
        .and_then(|rest| rest.split_once(')'))
        .is_some_and(|(number, _)| !number.is_empty() && number.bytes().all(|b| b.is_ascii_digit()))
}

/// The text before and after each occurrence of the word `word` in `haystack`, skipping
/// occurrences glued to an adjacent identifier character.
fn word_matches<'a>(haystack: &'a str, word: &'a str) -> impl Iterator<Item = (&'a str, &'a str)> {
    haystack.match_indices(word).filter_map(move |(index, _)| {
        let before = haystack.get(..index)?;
        let after = haystack.get(index + word.len()..)?;
        let glued_before = before.chars().next_back().is_some_and(is_ident_char);
        let glued_after = after.chars().next().is_some_and(is_ident_char);
        (!glued_before && !glued_after).then_some((before, after))
    })
}

fn is_ident_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    const POLICY: &str = r#"
        [crates.domain]
        layer = 0
        workspace-deps = []
        deps = []
        dev-deps = []
        build-deps = []

        [[source-bans]]
        crates = ["domain"]
        pattern = "std::fs"
        reason = "pure"

        [[source-bans]]
        crates = ["domain"]
        pattern = "async fn"
        reason = "sync"
    "#;

    fn file(path: &str, is_test_code: bool, text: &str) -> SourceFile {
        SourceFile {
            path: PathBuf::from(path),
            is_test_code,
            text: text.to_owned(),
        }
    }

    fn check_one(krate: &str, file: SourceFile) -> Vec<Violation> {
        check(&Architecture::parse(POLICY).unwrap(), krate, &[file])
    }

    /// The `(line, pattern)` of every banned-pattern violation in `text`, a domain source file.
    fn banned(text: &str) -> Vec<(usize, String)> {
        check_one("domain", file("src/lib.rs", false, text))
            .into_iter()
            .filter_map(|violation| {
                let Violation::BannedSourcePattern { line, pattern, .. } = violation else {
                    return None;
                };
                Some((line, pattern))
            })
            .collect()
    }

    #[test]
    fn banned_pattern_in_code_is_reported_with_its_reason() {
        let violations = check_one(
            "domain",
            file("src/lib.rs", false, "\nuse std::fs::File;\n"),
        );
        assert_eq!(
            violations,
            vec![Violation::BannedSourcePattern {
                path: PathBuf::from("src/lib.rs"),
                line: 2,
                pattern: "std::fs".into(),
                reason: "pure".into(),
            }]
        );
    }

    #[test]
    fn formatting_imports_and_aliases_do_not_hide_a_banned_pattern() {
        for text in [
            "use std::{fs as f, net::TcpStream};",
            "fn f() { std :: fs :: read(); }",
            "fn f() { std::r#fs::read(); }",
            "use std as s;\nfn f() { s::fs::read(); }",
            "use std::*;\nfn f() { fs::read(); }",
            "pub(crate) async  fn e() {}",
            "async\nfn e() {}",
        ] {
            assert!(!banned(text).is_empty(), "not caught: {text}");
        }
    }

    #[test]
    fn banned_pattern_in_comments_strings_or_other_identifiers_is_ignored() {
        let text = "//! Never touches std::fs.\nconst S: &str = \"async fn\";\nfn std_fs() {}\n";
        assert_eq!(banned(text), vec![]);
    }

    #[test]
    fn source_bans_cover_build_scripts_but_not_tests_or_other_crates() {
        let text = "use std::fs;\n";
        assert_eq!(check_one("api", file("src/lib.rs", false, text)), vec![]);
        assert_eq!(check_one("domain", file("tests/it.rs", true, text)), vec![]);
        assert_eq!(check_one("domain", file("build.rs", false, text)).len(), 1);
    }

    #[test]
    fn lint_level_attributes_are_reported_everywhere() {
        for (text, level) in [
            ("#[allow(dead_code)]\nfn f() {}", "allow"),
            ("#![ allow(clippy::unwrap_used, reason = \"x\")]", "allow"),
            ("#![\nallow(unused)]", "allow"),
            ("#![warn(warnings, clippy::all)]", "warn"),
            ("#[cfg_attr(test, allow(unused))]", "allow"),
            ("#![cfg_attr(all(),\n warn(clippy::panic))]", "warn"),
            ("#[deny(missing_docs)]\nfn f() {}", "deny"),
            ("#![forbid(unsafe_code)]", "forbid"),
        ] {
            let violations = check_one("api", file("tests/it.rs", true, text));
            assert_eq!(
                violations,
                vec![Violation::LintLevelAttribute {
                    path: PathBuf::from("tests/it.rs"),
                    line: 1,
                    level,
                }],
                "input: {text}"
            );
        }
    }

    #[test]
    fn expect_is_reported_only_when_it_covers_a_module() {
        let item =
            "/// Never write #[allow(x)].\n#[expect(dead_code, reason = \"demo\")]\nfn f() {}";
        assert_eq!(check_one("api", file("src/lib.rs", false, item)), vec![]);
        for text in [
            "#![expect(clippy::unwrap_used, reason = \"x\")]",
            "#[expect(clippy::unwrap_used, reason = \"x\")]\npub mod m;",
        ] {
            let violations = check_one("api", file("src/lib.rs", false, text));
            assert_eq!(
                violations,
                vec![Violation::ModuleWideExpect {
                    path: PathBuf::from("src/lib.rs"),
                    line: 1,
                }],
                "input: {text}"
            );
        }
    }

    #[test]
    fn hidden_module_sources_are_reported() {
        for (text, construct) in [
            ("#[path = \"inc/evil.inc\"]\nmod evil;", "#[path]"),
            ("#[cfg_attr(unix, path = \"u.rs\")]\nmod m;", "#[path]"),
            ("include!(\"generated.rs\");", "include!"),
        ] {
            let violations = check_one("api", file("src/lib.rs", false, text));
            assert_eq!(
                violations,
                vec![Violation::HiddenModuleSource {
                    path: PathBuf::from("src/lib.rs"),
                    line: 1,
                    construct,
                }],
                "input: {text}"
            );
        }
        let fixture = "const S: &str = include_str!(\"fixture.txt\");";
        assert_eq!(check_one("api", file("src/lib.rs", false, fixture)), vec![]);
    }

    #[test]
    fn mod_rs_files_are_reported() {
        let violations = check_one("api", file("src/routes/mod.rs", false, ""));
        assert_eq!(
            violations,
            vec![Violation::ModRsFile {
                path: PathBuf::from("src/routes/mod.rs")
            }]
        );
    }

    #[test]
    fn markers_need_an_issue_link() {
        assert!(has_untracked_marker(" TODO: later", "TODO"));
        assert!(has_untracked_marker(" FIXME(#) broken", "FIXME"));
        assert!(has_untracked_marker(" TODO(#12 not closed", "TODO"));
        assert!(!has_untracked_marker(" TODO(#123): tracked", "TODO"));
        assert!(!has_untracked_marker(" rules about `TODO` markers", "TODO"));
        assert!(!has_untracked_marker(" TODOS and MYTODO are words", "TODO"));
    }

    #[test]
    fn untracked_marker_reports_its_line() {
        let violations = check_one(
            "api",
            file("src/lib.rs", false, "fn f() {}\n// FIXME: racy\n"),
        );
        assert_eq!(
            violations,
            vec![Violation::UntrackedMarker {
                path: PathBuf::from("src/lib.rs"),
                line: 2,
                marker: "FIXME".into(),
            }]
        );
    }

    #[test]
    fn recognizes_path_patterns() {
        let segments = |pattern: &str| path_segments(&tokens::tokenize_str(pattern));
        assert_eq!(segments("std::fs"), Some(vec!["std".into(), "fs".into()]));
        assert_eq!(segments("tokio"), Some(vec!["tokio".into()]));
        assert_eq!(segments("async fn"), None);
        assert_eq!(segments("std::"), None);
    }
}
