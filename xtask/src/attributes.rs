//! Attributes found in a token stream: `#[...]` and inner `#![...]`.

use crate::tokens::Token;

/// One attribute, located in the token stream of a file.
#[derive(Debug)]
pub(crate) struct Attribute<'a> {
    /// Whether this is an inner attribute (`#![...]`), which applies to the enclosing item.
    pub(crate) inner: bool,
    /// Tokens between the brackets.
    pub(crate) body: &'a [Token],
    /// Tokens after the closing bracket: the rest of the file.
    pub(crate) rest: &'a [Token],
    /// 1-based line number of the `#`.
    pub(crate) line: usize,
}

impl Attribute<'_> {
    /// Whether the body calls `name(...)` anywhere, as `cfg_attr(test, allow(x))` calls `allow`.
    pub(crate) fn calls(&self, name: &str) -> bool {
        self.contains_pair(name, "(")
    }

    /// Whether the body assigns `name = ...` anywhere, as `cfg_attr(unix, path = "x")` sets `path`.
    pub(crate) fn sets(&self, name: &str) -> bool {
        self.contains_pair(name, "=")
    }

    /// Whether a meta item of the attribute is a macro fragment, as in `#[$meta]` or
    /// `#[cfg_attr(test, $meta)]`: what it expands to (perhaps `path = "..."`) is invisible
    /// here. A fragment in value position, as in `#[doc = $text]`, is not a meta item.
    pub(crate) fn spells_meta_from_fragment(&self) -> bool {
        let starts_with_fragment = self.body.first().is_some_and(|token| token.is("$"));
        let fragment_in_cfg_attr = self.body.first().is_some_and(|token| token.is("cfg_attr"))
            && self
                .body
                .windows(2)
                .any(|pair| matches!(pair, [comma, dollar] if comma.is(",") && dollar.is("$")));
        starts_with_fragment || fragment_in_cfg_attr
    }

    fn contains_pair(&self, first: &str, second: &str) -> bool {
        self.body
            .windows(2)
            .any(|pair| matches!(pair, [a, b] if a.is(first) && b.is(second)))
    }

    /// Whether the item this outer attribute is attached to is a module (`mod name`), skipping
    /// any further attributes and a visibility such as `pub(crate)`.
    pub(crate) fn applies_to_module(&self) -> bool {
        let mut rest = self.rest;
        loop {
            rest = match rest {
                [hash, group @ ..] if hash.is("#") && opens(group, "[") => skip_group(group),
                [vis, group @ ..] if vis.is("pub") && opens(group, "(") => skip_group(group),
                [vis, tail @ ..] if vis.is("pub") => tail,
                [item, ..] => return item.is("mod"),
                [] => return false,
            };
        }
    }
}

/// Every attribute in `tokens`, in source order, including attributes inside macro bodies.
pub(crate) fn find(tokens: &[Token]) -> Vec<Attribute<'_>> {
    let mut attributes = Vec::new();
    for (index, token) in tokens.iter().enumerate() {
        if !token.is("#") {
            continue;
        }
        let after_hash = tokens.get(index + 1..).unwrap_or_default();
        let (inner, group) = match after_hash {
            [bang, group @ ..] if bang.is("!") && opens(group, "[") => (true, group),
            group if opens(group, "[") => (false, group),
            _ => continue,
        };
        let rest = skip_group(group);
        let body_len = group.len() - rest.len();
        attributes.push(Attribute {
            inner,
            body: group.get(1..body_len.saturating_sub(1)).unwrap_or_default(),
            rest,
            line: token.line,
        });
    }
    attributes
}

/// Whether `tokens` starts with the opening bracket `open`.
fn opens(tokens: &[Token], open: &str) -> bool {
    tokens.first().is_some_and(|token| token.is(open))
}

/// Skips the bracketed group that starts at `tokens[0]` and returns the tokens after it. An
/// unterminated group consumes everything.
fn skip_group(tokens: &[Token]) -> &[Token] {
    let mut depth = 0_usize;
    for (index, token) in tokens.iter().enumerate() {
        match token.text.as_str() {
            "(" | "[" | "{" => depth += 1,
            ")" | "]" | "}" => depth = depth.saturating_sub(1),
            _ => continue,
        }
        if depth == 0 {
            return tokens.get(index + 1..).unwrap_or_default();
        }
    }
    &[]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{source_text, tokens};

    fn with_attributes(code: &str, check: impl FnOnce(&[Attribute<'_>])) {
        let tokens = tokens::tokenize(&source_text::split(code));
        check(&find(&tokens));
    }

    fn name<'a>(attribute: &'a Attribute<'_>) -> Option<&'a str> {
        attribute.body.first().map(|token| token.text.as_str())
    }

    #[test]
    fn finds_outer_and_inner_attributes_with_their_names() {
        with_attributes(
            "#![no_std]\n#[derive(Debug, Clone)]\nstruct S;",
            |attributes| {
                let names: Vec<_> = attributes.iter().map(|a| (a.inner, name(a))).collect();
                assert_eq!(names, [(true, Some("no_std")), (false, Some("derive"))]);
                assert_eq!(attributes[1].line, 2, "line of the `#`");
            },
        );
    }

    #[test]
    fn whitespace_and_line_breaks_do_not_hide_an_attribute() {
        with_attributes("# ! [\n  allow (x) ]", |attributes| {
            assert_eq!(attributes.len(), 1, "one attribute");
            assert!(attributes[0].inner, "inner");
            assert_eq!(name(&attributes[0]), Some("allow"));
        });
    }

    #[test]
    fn calls_finds_nested_lint_levels() {
        with_attributes("#[cfg_attr(all(),\n warn(x))]", |attributes| {
            assert!(attributes[0].calls("warn"), "nested call");
            assert!(!attributes[0].calls("allow"), "absent call");
        });
        with_attributes("#[cfg_attr(unix, path = \"u.rs\")]", |attributes| {
            assert!(attributes[0].sets("path"), "nested assignment");
        });
    }

    #[test]
    fn detects_attributes_on_modules() {
        for (code, on_module) in [
            ("#[expect(x)] mod m;", true),
            ("#[expect(x)]\n#[cfg(test)]\npub(crate) mod m {}", true),
            ("#[expect(x)] pub fn f() {}", false),
            ("#[expect(x)]", false),
        ] {
            with_attributes(code, |attributes| {
                assert_eq!(attributes[0].applies_to_module(), on_module, "{code}");
            });
        }
    }

    #[test]
    fn unterminated_attribute_does_not_panic() {
        with_attributes("#[allow(", |attributes| {
            assert_eq!(name(&attributes[0]), Some("allow"));
        });
    }
}
