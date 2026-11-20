//! The `#[expect(...)]` attributes of the sources, each with the item it exempts, for the
//! exemption ledger (`exceptions.toml`, see [`exemption_ledger`](crate::exemption_ledger)).
//!
//! Like the source rules this works on tokens of the code view, so an attribute quoted in a
//! comment or a string literal (as in the fixtures of these tests) is not an exemption.
//!
//! The item of an attribute is named by its path inside the file: the enclosing `mod`, `impl`,
//! `trait`, `fn` and type bodies, then the item itself, as in `MachineState::apply` for a
//! method of `impl MachineState` or `Tools` for a struct. An `impl` block is named after its self
//! type (`impl Display for Finding` encloses `Finding::fmt`) and, as a target itself, written
//! `impl Finding`. A field or variant is named inside its type (`Config::port`). An attribute on
//! anything else (a statement, an expression, a `use`) takes the path of the item that encloses
//! it, or [`TOP_LEVEL`] at the top of the file.

use std::collections::BTreeSet;
use std::path::PathBuf;

use crate::attributes;
use crate::rust_items::{
    group_len, inside_group, is_arrow, opens, skip_attributes_and_visibility,
    skip_generics_and_where, split_top_level,
};
use crate::source_text;
use crate::tokens::{self, Token};
use crate::workspace::SourceFile;

/// The item of an attribute that has no enclosing named item in its file.
pub(crate) const TOP_LEVEL: &str = "(top level)";

/// Keywords of items that may have a `{ ... }` body, which is a named scope.
const SCOPE_KEYWORDS: [&str; 6] = ["fn", "struct", "enum", "union", "trait", "mod"];

/// Keywords of items whose body holds fields or variants.
const DATA_KEYWORDS: [&str; 3] = ["struct", "enum", "union"];

/// Keywords of items named by the identifier after them.
const NAMED_ITEM_KEYWORDS: [&str; 9] = [
    "fn", "struct", "enum", "union", "trait", "type", "mod", "static", "const",
];

/// Qualifiers that may stand before an item keyword, as in `default unsafe fn`.
const QUALIFIERS: [&str; 4] = ["default", "unsafe", "async", "extern"];

/// Words after `const` that make it a qualifier (`const fn`) rather than a constant item.
const CONST_QUALIFIED: [&str; 4] = ["fn", "unsafe", "async", "extern"];

/// Tokens after which `impl` starts an item rather than an `impl Trait` type.
const BEFORE_ITEM: [&str; 5] = ["}", ";", "]", "{", "unsafe"];

/// Tokens that may follow the name of a field or variant.
const AFTER_MEMBER: [&str; 6] = [":", ",", "(", "{", "}", "="];

/// One `expect(...)` lint exemption in the sources.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Exemption {
    /// The file, as listed in its [`SourceFile`].
    pub(crate) path: PathBuf,
    /// 1-based line of the attribute's `#`.
    pub(crate) line: usize,
    /// The path of the exempted item inside the file, as described in the module docs.
    pub(crate) item: String,
    /// The exempted lints, as written: `clippy::too_many_lines`, `dead_code`.
    pub(crate) lints: BTreeSet<String>,
    /// The `reason = "..."` text with escapes and line continuations resolved, if present.
    pub(crate) reason: Option<String>,
}

/// A `{ ... }` body of a named item.
#[derive(Debug)]
struct Scope {
    /// The item's name; the self type for an `impl` block.
    name: String,
    /// Whether the body holds fields or variants.
    holds_members: bool,
    /// Token index of the opening `{`.
    open: usize,
    /// Token index of the closing `}`.
    close: usize,
}

/// Every outer `expect` exemption in `files`, test code included. Inner `#![expect]` and
/// `expect` on a module are left out: the source rules reject them outright.
pub(crate) fn find(files: &[&SourceFile]) -> Vec<Exemption> {
    files.iter().flat_map(|file| in_file(file)).collect()
}

fn in_file(file: &SourceFile) -> Vec<Exemption> {
    let tokens = tokens::tokenize(&source_text::split(&file.text));
    let scopes = scopes(&tokens);
    let mut exemptions = Vec::new();
    for attribute in attributes::find(&tokens) {
        if attribute.inner || attribute.applies_to_module() {
            continue;
        }
        let groups = expect_groups(attribute.body);
        if groups.is_empty() {
            continue;
        }
        let item = item_path(&scopes, tokens.len() - attribute.rest.len(), attribute.rest);
        for group in groups {
            let (lints, reason) = lints_and_reason(group);
            exemptions.push(Exemption {
                path: file.path.clone(),
                line: attribute.line,
                item: item.clone(),
                lints,
                reason,
            });
        }
    }
    exemptions
}

/// The argument lists of every `expect(...)` the attribute body `meta` applies: its own, or
/// those inside `cfg_attr(predicate, ...)`, nested or not.
fn expect_groups(meta: &[Token]) -> Vec<&[Token]> {
    match meta {
        [name, args @ ..] if name.is("expect") && opens(args, "(") => vec![inside_group(args)],
        [name, args @ ..] if name.is("cfg_attr") && opens(args, "(") => {
            split_top_level(inside_group(args))
                .into_iter()
                .skip(1)
                .flat_map(expect_groups)
                .collect()
        }
        _ => Vec::new(),
    }
}

/// The lints and the resolved reason of one `expect(...)` argument list.
fn lints_and_reason(arguments: &[Token]) -> (BTreeSet<String>, Option<String>) {
    let mut lints = BTreeSet::new();
    let mut reason = None;
    for argument in split_top_level(arguments) {
        match argument {
            [key, equals, quote, ..] if key.is("reason") && equals.is("=") && quote.is("\"") => {
                reason = quote.literal.as_deref().map(unescape);
            }
            _ => {
                lints.insert(argument.iter().map(|token| token.text.as_str()).collect());
            }
        }
    }
    (lints, reason)
}

/// The text of a raw string literal body: a `\` before a line break drops the break and the
/// indentation after it, and any other escape stands for the escaped character.
fn unescape(raw: &str) -> String {
    let mut text = String::with_capacity(raw.len());
    let mut chars = raw.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\\' {
            text.push(c);
            continue;
        }
        match chars.next() {
            Some('\n') => while chars.next_if(|next| next.is_whitespace()).is_some() {},
            Some('n' | 't') => text.push(' '),
            Some(escaped) => text.push(escaped),
            None => {}
        }
    }
    text
}

/// Every named `{ ... }` body in `tokens`, in source order.
fn scopes(tokens: &[Token]) -> Vec<Scope> {
    let mut scopes = Vec::new();
    for (index, token) in tokens.iter().enumerate() {
        let after = tokens.get(index + 1..).unwrap_or_default();
        let named = match after {
            [name, header @ ..] if SCOPE_KEYWORDS.contains(&token.text.as_str()) => {
                name.is_ident().then(|| {
                    (
                        name.text.clone(),
                        DATA_KEYWORDS.contains(&token.text.as_str()),
                        header,
                    )
                })
            }
            [bang, name, header @ ..] if token.is("macro_rules") && bang.is("!") => {
                name.is_ident().then(|| (name.text.clone(), false, header))
            }
            _ if token.is("impl") && starts_item(tokens, index) => {
                impl_self_type(after).map(|name| (name, false, after))
            }
            _ => None,
        };
        let Some((name, holds_members, header)) = named else {
            continue;
        };
        let Some(offset) = body_offset(header) else {
            continue;
        };
        let open = tokens.len() - header.len() + offset;
        let body = tokens.get(open..).unwrap_or_default();
        scopes.push(Scope {
            name,
            holds_members,
            open,
            close: open + group_len(body).saturating_sub(1),
        });
    }
    scopes
}

/// Whether the `impl` at `index` starts an item, not an `impl Trait` type.
fn starts_item(tokens: &[Token], index: usize) -> bool {
    index
        .checked_sub(1)
        .and_then(|before| tokens.get(before))
        .is_none_or(|before| BEFORE_ITEM.contains(&before.text.as_str()) || before.is("default"))
}

/// The offset in `header` of the `{` that opens the item's body, or `None` if the item ends at
/// a `;` first, as `struct Unit;` and a trait method without a default body do.
fn body_offset(header: &[Token]) -> Option<usize> {
    let mut depth = 0_usize;
    for (index, token) in header.iter().enumerate() {
        match token.text.as_str() {
            "(" | "[" => depth += 1,
            ")" | "]" => depth = depth.saturating_sub(1),
            "{" if depth == 0 => return Some(index),
            ";" if depth == 0 => return None,
            _ => {}
        }
    }
    None
}

/// The name of the self type of an `impl` block whose header (after `impl`) is `header`: the
/// last identifier outside generics, after `for` if the block implements a trait.
fn impl_self_type(header: &[Token]) -> Option<String> {
    let header = skip_generics_and_where(header);
    let mut depth = 0_usize;
    let mut name = None;
    for (index, token) in header.iter().enumerate() {
        match token.text.as_str() {
            "{" | ";" | "where" if depth == 0 => break,
            "<" | "(" | "[" => depth += 1,
            ">" if is_arrow(header, index) => {}
            ">" | ")" | "]" => depth = depth.saturating_sub(1),
            "for" if depth == 0 => name = None,
            "dyn" | "mut" => {}
            _ if depth == 0 && token.is_ident() && !follows_quote(header, index) => {
                name = Some(token.text.clone());
            }
            _ => {}
        }
    }
    name
}

/// Whether the token at `index` is the name of a lifetime, as `a` in `'a`.
fn follows_quote(tokens: &[Token], index: usize) -> bool {
    index
        .checked_sub(1)
        .and_then(|before| tokens.get(before))
        .is_some_and(|before| before.is("'"))
}

/// The path of the item exempted by an attribute whose closing `]` is just before token `at`,
/// with `rest` the tokens from there on.
fn item_path(scopes: &[Scope], at: usize, rest: &[Token]) -> String {
    let enclosing: Vec<&Scope> = scopes
        .iter()
        .filter(|scope| scope.open < at && at < scope.close)
        .collect();
    let in_members = enclosing.last().is_some_and(|scope| scope.holds_members);
    let mut segments: Vec<&str> = enclosing.iter().map(|scope| scope.name.as_str()).collect();
    let target = target(rest, in_members);
    if let Some(target) = &target {
        segments.push(target);
    }
    if segments.is_empty() {
        TOP_LEVEL.to_owned()
    } else {
        segments.join("::")
    }
}

/// The name of the item at the start of `rest`, after further attributes, a visibility and
/// qualifiers, or `None` if it is not a named item (nor, when `in_members`, a field or variant).
fn target(rest: &[Token], in_members: bool) -> Option<String> {
    let mut rest = skip_attributes_and_visibility(rest);
    loop {
        rest = match rest {
            [qualifier, tail @ ..] if QUALIFIERS.contains(&qualifier.text.as_str()) => tail,
            // The ABI of `extern "C" fn`, a `""` pair in the code view.
            [open, close, tail @ ..] if open.is("\"") && close.is("\"") => tail,
            [keyword, tail @ ..]
                if keyword.is("const")
                    && tail
                        .first()
                        .is_some_and(|next| CONST_QUALIFIED.contains(&next.text.as_str())) =>
            {
                tail
            }
            _ => break,
        };
    }
    match rest {
        [keyword, mutable, name, ..]
            if keyword.is("static") && mutable.is("mut") && name.is_ident() =>
        {
            Some(name.text.clone())
        }
        [keyword, name, ..]
            if NAMED_ITEM_KEYWORDS.contains(&keyword.text.as_str()) && name.is_ident() =>
        {
            Some(name.text.clone())
        }
        [keyword, bang, name, ..] if keyword.is("macro_rules") && bang.is("!") => {
            Some(name.text.clone())
        }
        [keyword, header @ ..] if keyword.is("impl") => {
            impl_self_type(header).map(|name| format!("impl {name}"))
        }
        [name, next, ..]
            if in_members && name.is_ident() && AFTER_MEMBER.contains(&next.text.as_str()) =>
        {
            Some(name.text.clone())
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exemptions(text: &str) -> Vec<Exemption> {
        let file = SourceFile {
            path: PathBuf::from("src/lib.rs"),
            is_test_code: false,
            text: text.to_owned(),
        };
        find(&[&file])
    }

    fn items(text: &str) -> Vec<String> {
        exemptions(text).into_iter().map(|e| e.item).collect()
    }

    #[test]
    fn reads_lints_and_reason_with_line_continuations() {
        let found = exemptions(
            "\n#[expect(\n    clippy::too_many_lines,\n    dead_code,\n    reason = \"one long \\\n                  match\"\n)]\nfn f() {}",
        );
        assert_eq!(found.len(), 1, "{found:?}");
        let exemption = &found[0];
        assert_eq!(exemption.line, 2, "line of the `#`");
        assert_eq!(exemption.item, "f");
        assert_eq!(
            exemption.lints,
            BTreeSet::from(["clippy::too_many_lines".to_owned(), "dead_code".to_owned()])
        );
        assert_eq!(exemption.reason.as_deref(), Some("one long match"));
    }

    #[test]
    fn a_missing_reason_is_none() {
        let found = exemptions("#[expect(dead_code)]\nfn f() {}");
        assert_eq!(found[0].reason, None, "{found:?}");
    }

    #[test]
    fn attributes_in_comments_and_strings_are_not_exemptions() {
        let text = "// #[expect(dead_code, reason = \"x\")]\n\
                    /// #[expect(dead_code, reason = \"x\")]\n\
                    const FIXTURE: &str = \"#[expect(clippy::unwrap_used, reason = \\\"x\\\")]\\nfn f() {}\";\n\
                    const RAW: &str = r#\"#[expect(dead_code)] fn g() {}\"#;";
        assert_eq!(
            exemptions(text),
            vec![],
            "only literal and comment mentions"
        );
    }

    #[test]
    fn other_attributes_and_module_wide_expects_are_not_ledger_items() {
        let text = "#![expect(dead_code, reason = \"rejected by the source rules\")]\n\
                    #[expect(dead_code, reason = \"rejected by the source rules\")]\nmod m {}\n\
                    #[derive(Debug)]\n#[should_panic(expected = \"boom\")]\nfn t() {}";
        assert_eq!(exemptions(text), vec![], "none is an item exemption");
    }

    #[test]
    fn expects_inside_cfg_attr_are_found() {
        let found = exemptions(
            "#[cfg_attr(test, expect(dead_code, reason = \"a\"), expect(unused, reason = \"b\"))]\n\
             #[cfg_attr(unix, cfg_attr(test, expect(clippy::panic, reason = \"c\")))]\nfn f() {}",
        );
        let lints: Vec<Vec<String>> = found
            .iter()
            .map(|e| e.lints.iter().cloned().collect())
            .collect();
        assert_eq!(
            lints,
            [vec!["dead_code"], vec!["unused"], vec!["clippy::panic"]],
            "{found:?}"
        );
        assert!(found.iter().all(|e| e.item == "f"), "{found:?}");
    }

    #[test]
    fn items_are_named_by_their_path_in_the_file() {
        let text = "impl MachineState {\n\
                    #[expect(clippy::too_many_lines, reason = \"r\")]\n\
                    pub const fn apply(self) {}\n}\n\
                    impl<'a, T: Into<u8>> fmt::Display for &'a Finding<T> where T: Copy {\n\
                    #[expect(clippy::too_many_lines, reason = \"r\")]\n\
                    fn fmt(&self) -> impl Iterator<Item = u8> { todo() }\n}\n\
                    #[derive(Debug, Clone, Copy)]\n#[expect(clippy::struct_excessive_bools, reason = \"r\")]\n\
                    struct Tools { a: bool }\n\
                    mod outer { pub(crate) trait T { #[expect(dead_code, reason = \"r\")] fn f(); } }\n\
                    #[expect(dead_code, reason = \"r\")]\nimpl<T> Trait for Wrapper<T> {}\n\
                    #[expect(dead_code, reason = \"r\")]\npub(crate) unsafe extern \"C\" fn ffi() {}\n\
                    #[expect(dead_code, reason = \"r\")]\nstatic mut COUNT: u8 = 0;\n\
                    #[expect(dead_code, reason = \"r\")]\nmacro_rules! gen { () => {} }";
        assert_eq!(
            items(text),
            [
                "MachineState::apply",
                "Finding::fmt",
                "Tools",
                "outer::T::f",
                "impl Wrapper",
                "ffi",
                "COUNT",
                "gen",
            ]
        );
    }

    #[test]
    fn fields_and_variants_are_named_inside_their_type() {
        let text = "struct Config {\n    #[expect(dead_code, reason = \"r\")]\n    pub port: u16,\n}\n\
                    enum Mode {\n    #[expect(dead_code, reason = \"r\")]\n    Fast { x: u8 },\n\
                    #[expect(dead_code, reason = \"r\")]\n    Slow,\n}";
        assert_eq!(items(text), ["Config::port", "Mode::Fast", "Mode::Slow"]);
    }

    #[test]
    fn statements_take_the_path_of_the_enclosing_item() {
        let text = "fn run() {\n    #[expect(clippy::cast_possible_truncation, reason = \"r\")]\n\
                    let x = y as u8;\n    let f = || {\n        #[expect(unused, reason = \"r\")]\n\
                    call();\n    };\n}\n\
                    #[expect(unused_imports, reason = \"r\")]\nuse std::fmt;";
        assert_eq!(items(text), ["run", "run", TOP_LEVEL]);
    }

    #[test]
    fn unterminated_input_does_not_panic() {
        for text in [
            "#[expect(",
            "#[expect(x)] fn",
            "impl X { #[expect(y)]",
            "struct S {",
        ] {
            let _found = exemptions(text);
        }
    }
}
