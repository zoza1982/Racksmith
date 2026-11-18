//! Paths named by code, resolved through `use` declarations.
//!
//! A source ban on `std::fs` must also catch `use std::{fs as f};`, `use std as s; s::fs`,
//! `extern crate std as s;` and `use std::*; fs::read`, not only the literal text. This module
//! expands every `use` tree into full paths, records the names each import introduces, and
//! resolves every other path in the file through those names. It works on tokens, without name
//! resolution, so it may over-approximate; it never misses a path spelled in the file.

use std::collections::BTreeMap;

use crate::tokens::Token;

/// A path as its segment names, with the line it starts on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResolvedPath {
    /// Segment names, as `["std", "fs", "read"]`.
    pub(crate) segments: Vec<String>,
    /// 1-based line number of the first segment.
    pub(crate) line: usize,
}

/// One leaf of a `use` tree (or an `extern crate`), expanded to its full path.
#[derive(Debug)]
struct Import {
    path: ResolvedPath,
    /// The name the import binds (its `as` rename or last segment); `None` for `_` and globs.
    binds: Option<String>,
    /// Whether this is a glob import (`path::*`).
    glob: bool,
}

/// Every path in `tokens`, expanded through the file's imports.
///
/// The result contains each import's full path, each path as written, and each path whose first
/// segment is an imported name (or could come from a glob import) with that prefix substituted.
pub(crate) fn resolve(tokens: &[Token]) -> Vec<ResolvedPath> {
    let imports = imports(tokens);
    let bound: BTreeMap<&str, &[String]> = imports
        .iter()
        .filter_map(|import| Some((import.binds.as_deref()?, import.path.segments.as_slice())))
        .collect();
    let globs: Vec<&[String]> = imports
        .iter()
        .filter(|import| import.glob)
        .map(|import| import.path.segments.as_slice())
        .collect();

    let mut resolved: Vec<ResolvedPath> =
        imports.iter().map(|import| import.path.clone()).collect();
    for path in written_paths(tokens) {
        let first = path.segments.first().map(String::as_str);
        let prefixes = first
            .and_then(|name| bound.get(name))
            .map(|segments| (segments, 1))
            .into_iter()
            .chain(globs.iter().map(|segments| (segments, 0)));
        for (prefix, replaced) in prefixes {
            let tail = path.segments.get(replaced..).unwrap_or_default();
            resolved.push(ResolvedPath {
                segments: prefix.iter().chain(tail).cloned().collect(),
                line: path.line,
            });
        }
        resolved.push(path);
    }
    resolved
}

/// Every maximal `a::b::c` run of identifiers in `tokens`, as written.
fn written_paths(tokens: &[Token]) -> Vec<ResolvedPath> {
    let mut paths = Vec::new();
    let mut current: Option<ResolvedPath> = None;
    let mut after_separator = false;
    for token in tokens {
        if token.is("::") {
            after_separator = true;
            continue;
        }
        let continues = after_separator;
        after_separator = false;
        if !token.is_ident() {
            paths.extend(current.take());
        } else if let Some(path) = current.as_mut().filter(|_| continues) {
            path.segments.push(token.text.clone());
        } else {
            paths.extend(current.replace(new_path(token)));
        }
    }
    paths.extend(current);
    paths
}

fn new_path(first: &Token) -> ResolvedPath {
    ResolvedPath {
        segments: vec![first.text.clone()],
        line: first.line,
    }
}

/// Every import in `tokens`: the leaves of each `use` tree, and each `extern crate`.
fn imports(tokens: &[Token]) -> Vec<Import> {
    let mut imports = Vec::new();
    for (index, token) in tokens.iter().enumerate() {
        let rest = tokens.get(index + 1..).unwrap_or_default();
        if token.is("use") {
            use_tree(rest, &[], token.line, &mut imports);
        } else if token.is("extern")
            && let [krate, name, rest @ ..] = rest
            && krate.is("crate")
            && name.is_ident()
        {
            let path = ResolvedPath {
                segments: vec![name.text.clone()],
                line: token.line,
            };
            push_import(path, rename(rest).0, false, &mut imports);
        }
    }
    imports
}

/// Parses one `use` tree after `prefix` and returns the tokens after it.
fn use_tree<'a>(
    tokens: &'a [Token],
    prefix: &[String],
    line: usize,
    out: &mut Vec<Import>,
) -> &'a [Token] {
    let mut segments = prefix.to_vec();
    let mut rest = tokens;
    let mut expect_segment = true;
    loop {
        rest = match rest {
            [separator, tail @ ..] if separator.is("::") => {
                expect_segment = true;
                tail
            }
            [open, tail @ ..] if expect_segment && open.is("{") => {
                return use_group(tail, &segments, line, out);
            }
            [star, tail @ ..] if expect_segment && star.is("*") => {
                push_import(ResolvedPath { segments, line }, None, true, out);
                return tail;
            }
            [segment, tail @ ..] if expect_segment && segment.is_ident() => {
                segments.push(segment.text.clone());
                expect_segment = false;
                tail
            }
            _ => break,
        };
    }
    if segments.last().is_some_and(|last| last == "self") {
        segments.pop();
    }
    let (alias, rest) = rename(rest);
    push_import(ResolvedPath { segments, line }, alias, false, out);
    rest
}

/// Parses the trees of a `{...}` group, whose opening brace is already consumed.
fn use_group<'a>(
    tokens: &'a [Token],
    prefix: &[String],
    line: usize,
    out: &mut Vec<Import>,
) -> &'a [Token] {
    let mut rest = tokens;
    loop {
        rest = match rest {
            [close, tail @ ..] if close.is("}") => return tail,
            [comma, tail @ ..] if comma.is(",") => tail,
            [] => return rest,
            _ => {
                let tail = use_tree(rest, prefix, line, out);
                if tail.len() == rest.len() {
                    return tail;
                }
                tail
            }
        };
    }
}

/// Splits an optional `as name` rename off the front of `tokens`.
fn rename(tokens: &[Token]) -> (Option<String>, &[Token]) {
    match tokens {
        [keyword, name, tail @ ..] if keyword.is("as") && name.is_ident() => {
            (Some(name.text.clone()), tail)
        }
        _ => (None, tokens),
    }
}

fn push_import(path: ResolvedPath, alias: Option<String>, glob: bool, out: &mut Vec<Import>) {
    if path.segments.is_empty() {
        return;
    }
    let binds = if glob {
        None
    } else {
        alias
            .or_else(|| path.segments.last().cloned())
            .filter(|name| name != "_")
    };
    out.push(Import { path, binds, glob });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{source_text, tokens};

    fn resolved(code: &str) -> Vec<String> {
        resolve(&tokens::tokenize(&source_text::split(code)))
            .into_iter()
            .map(|path| path.segments.join("::"))
            .collect()
    }

    fn resolves_to(code: &str, expected: &str) -> bool {
        resolved(code).iter().any(|path| path == expected)
    }

    #[test]
    fn expands_grouped_and_nested_use_trees() {
        let paths = resolved("use std::{fs as f, net::{TcpStream, self}, io::*};");
        for expected in ["std::fs", "std::net::TcpStream", "std::net", "std::io"] {
            assert!(
                paths.iter().any(|path| path == expected),
                "{expected} in {paths:?}"
            );
        }
    }

    #[test]
    fn resolves_paths_through_aliases_and_imported_names() {
        assert!(resolves_to(
            "use std as s;\nfn f() { s::fs::read(); }",
            "std::fs::read"
        ));
        assert!(resolves_to("extern crate std as s;\nuse s::fs;", "std::fs"));
        assert!(resolves_to(
            "use std::fs;\nfn f() { fs::read(); }",
            "std::fs::read"
        ));
    }

    #[test]
    fn resolves_paths_through_glob_imports() {
        assert!(resolves_to(
            "use std::*;\nfn f() { fs::read(); }",
            "std::fs::read"
        ));
    }

    #[test]
    fn keeps_written_paths_with_leading_separators_and_spaces() {
        assert!(resolves_to(
            "fn f() { :: std :: fs :: read(); }",
            "std::fs::read"
        ));
    }

    #[test]
    fn underscore_imports_bind_nothing() {
        assert!(!resolves_to(
            "use std::fs as _;\nfn f() { _::x(); }",
            "std::fs::x"
        ));
    }

    #[test]
    fn malformed_use_trees_do_not_loop_or_panic() {
        for code in ["use", "use {", "use a::{b, ;", "use a::{<}", "extern crate"] {
            let _paths = resolved(code);
        }
    }
}
