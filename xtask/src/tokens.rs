//! Tokens of the code view produced by [`source_text`](crate::source_text).
//!
//! Source rules match token sequences rather than raw text, so whitespace, line breaks and raw
//! identifiers (`r#name`) cannot hide a banned pattern.

use crate::source_text::Line;

/// One token of code: an identifier or number, `::`, or a single punctuation character.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Token {
    /// The token text; a raw identifier is stored without its `r#` prefix.
    pub(crate) text: String,
    /// 1-based line number of the token.
    pub(crate) line: usize,
    /// For the opening `"` of a string literal, the literal's raw contents.
    pub(crate) literal: Option<String>,
}

impl Token {
    /// Whether this token is the identifier, keyword or punctuation `text`.
    pub(crate) fn is(&self, text: &str) -> bool {
        self.text == text
    }

    /// Whether this token is an identifier or keyword (not punctuation).
    pub(crate) fn is_ident(&self) -> bool {
        self.text.chars().next().is_some_and(is_ident_char)
    }
}

/// Splits the code of every line into tokens, in source order.
pub(crate) fn tokenize(lines: &[Line]) -> Vec<Token> {
    let mut tokens = Vec::new();
    for line in lines {
        tokenize_into(&line.code, line.number, &mut tokens);
    }
    attach_literals(&mut tokens, lines);
    merge_raw_identifiers(tokens)
}

/// Gives the opening `"` of every string literal its contents. The code view writes each
/// literal as a `""` pair, so the quotes alternate between opening and closing ones.
fn attach_literals(tokens: &mut [Token], lines: &[Line]) {
    let mut literals = lines.iter().flat_map(|line| line.literals.iter());
    let mut opening = true;
    for token in tokens.iter_mut().filter(|token| token.is("\"")) {
        if opening {
            token.literal = literals.next().cloned();
        }
        opening = !opening;
    }
}

/// Splits a single string, such as a pattern from `architecture.toml`, into token texts.
pub(crate) fn tokenize_str(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    tokenize_into(text, 1, &mut tokens);
    merge_raw_identifiers(tokens)
        .into_iter()
        .map(|token| token.text)
        .collect()
}

fn tokenize_into(code: &str, line: usize, out: &mut Vec<Token>) {
    let mut rest = code;
    while let Some(c) = rest.chars().next() {
        let len = if is_ident_char(c) {
            rest.find(|next: char| !is_ident_char(next))
                .unwrap_or(rest.len())
        } else if rest.starts_with("::") {
            2
        } else {
            c.len_utf8()
        };
        let (text, tail) = rest.split_at(len);
        if !c.is_whitespace() {
            out.push(Token {
                text: text.to_owned(),
                line,
                literal: None,
            });
        }
        rest = tail;
    }
}

/// Rewrites `r # name` token runs into the plain identifier `name`.
fn merge_raw_identifiers(tokens: Vec<Token>) -> Vec<Token> {
    let mut merged: Vec<Token> = Vec::with_capacity(tokens.len());
    for token in tokens {
        let is_raw = token.is_ident()
            && merged.len() >= 2
            && merged.last().is_some_and(|hash| hash.is("#"))
            && merged.iter().rev().nth(1).is_some_and(|r| r.is("r"));
        if is_raw {
            merged.truncate(merged.len() - 2);
        }
        merged.push(token);
    }
    merged
}

fn is_ident_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source_text;

    fn texts(code: &str) -> Vec<String> {
        tokenize(&source_text::split(code))
            .into_iter()
            .map(|token| token.text)
            .collect()
    }

    #[test]
    fn splits_identifiers_paths_and_punctuation() {
        assert_eq!(
            texts("use std :: fs;"),
            ["use", "std", "::", "fs", ";"],
            "whitespace around `::` is irrelevant"
        );
        assert_eq!(
            texts("#![ allow(x)]"),
            ["#", "!", "[", "allow", "(", "x", ")", "]"]
        );
    }

    #[test]
    fn tokens_carry_their_line_numbers() {
        let tokens = tokenize(&source_text::split("#![\nallow(x)]"));
        let allow = tokens.iter().find(|token| token.is("allow")).unwrap();
        assert_eq!(allow.line, 2, "token on the second line");
    }

    #[test]
    fn raw_identifiers_lose_their_prefix() {
        assert_eq!(texts("std::r#fs::read"), ["std", "::", "fs", "::", "read"]);
        assert_eq!(tokenize_str("r#async fn"), ["async", "fn"]);
    }

    #[test]
    fn the_opening_quote_carries_the_literal() {
        let tokens = tokenize(&source_text::split("f(\"a\", '\"', \"b\nc\");\ng(\"d\");"));
        let literals: Vec<(Option<&str>, usize)> = tokens
            .iter()
            .filter(|token| token.is("\""))
            .map(|token| (token.literal.as_deref(), token.line))
            .collect();
        assert_eq!(
            literals,
            [
                (Some("a"), 1),
                (None, 1),
                (Some("b\nc"), 1),
                (None, 2),
                (Some("d"), 3),
                (None, 3),
            ]
        );
    }

    #[test]
    fn literals_and_comments_produce_no_tokens() {
        assert_eq!(
            texts("f(\"std::fs\"); // std::fs"),
            ["f", "(", "\"", "\"", ")", ";"]
        );
    }
}
