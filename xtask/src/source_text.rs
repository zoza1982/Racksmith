//! A minimal Rust lexer that separates code from comments, line by line.
//!
//! Source rules need two views of a file: the *code* (so a pattern mentioned in a doc comment
//! or a string literal is not a violation) and the *comments* (for `TODO` tracking). String
//! and character literals are replaced by empty placeholders (`""`, `''`) in the code view.

/// One source line split into its code and comment parts.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Line {
    /// 1-based line number.
    pub(crate) number: usize,
    /// Code on this line, with literal contents removed.
    pub(crate) code: String,
    /// Comment text on this line, without the comment delimiters.
    pub(crate) comment: String,
    /// The raw contents (escapes kept as written) of the string literals that start on this
    /// line, in order; each one is the `""` placeholder of the code view.
    pub(crate) literals: Vec<String>,
}

/// Splits Rust source text into per-line code and comment views.
pub(crate) fn split(text: &str) -> Vec<Line> {
    let mut lexer = Lexer {
        chars: text.chars().collect(),
        pos: 0,
        lines: Vec::new(),
        current: Line {
            number: 1,
            ..Line::default()
        },
    };
    lexer.run();
    lexer.lines
}

struct Lexer {
    chars: Vec<char>,
    pos: usize,
    lines: Vec<Line>,
    current: Line,
}

impl Lexer {
    fn run(&mut self) {
        while let Some(c) = self.peek(0) {
            match (c, self.peek(1)) {
                ('/', Some('/')) => self.line_comment(),
                ('/', Some('*')) => self.block_comment(),
                ('"', _) => self.string(0),
                ('\'', _) => self.char_or_lifetime(),
                _ if is_ident_start(c) => self.identifier_or_prefixed_literal(),
                _ => self.code_char(),
            }
        }
        let last = std::mem::take(&mut self.current);
        self.lines.push(last);
    }

    fn peek(&self, offset: usize) -> Option<char> {
        self.chars.get(self.pos + offset).copied()
    }

    /// Consumes one character, starting a new line on `\n`. Returns `None` at end of input.
    fn bump(&mut self) -> Option<char> {
        let c = self.peek(0)?;
        self.pos += 1;
        if c == '\n' {
            let number = self.current.number + 1;
            let finished = std::mem::replace(
                &mut self.current,
                Line {
                    number,
                    ..Line::default()
                },
            );
            self.lines.push(finished);
        }
        Some(c)
    }

    fn code_char(&mut self) {
        if let Some(c) = self.bump().filter(|&c| c != '\n') {
            self.current.code.push(c);
        }
    }

    fn line_comment(&mut self) {
        self.pos += 2;
        while let Some(c) = self.peek(0).filter(|&c| c != '\n') {
            self.current.comment.push(c);
            self.pos += 1;
        }
    }

    /// Consumes a (possibly nested) block comment.
    fn block_comment(&mut self) {
        self.pos += 2;
        let mut depth = 1_usize;
        while depth > 0 {
            match (self.peek(0), self.peek(1)) {
                (None, _) => return,
                (Some('*'), Some('/')) => {
                    self.pos += 2;
                    depth -= 1;
                }
                (Some('/'), Some('*')) => {
                    self.pos += 2;
                    depth += 1;
                }
                (Some(_), _) => {
                    if let Some(c) = self.bump().filter(|&c| c != '\n') {
                        self.current.comment.push(c);
                    }
                }
            }
        }
    }

    /// Consumes a string literal whose opening quote is at the cursor. `hashes` is the number
    /// of `#` delimiters of a raw string; `0` means an ordinary string with escapes.
    fn string(&mut self, hashes: usize) {
        self.pos += 1;
        self.current.code.push('"');
        let starts_on = self.lines.len();
        let mut contents = String::new();
        while let Some(c) = self.bump() {
            if c == '\\' && hashes == 0 {
                contents.push(c);
                if let Some(escaped) = self.bump() {
                    contents.push(escaped);
                }
            } else if c == '"' && self.closes_raw_string(hashes) {
                self.pos += hashes;
                break;
            } else {
                contents.push(c);
            }
        }
        match self.lines.get_mut(starts_on) {
            Some(line) => line.literals.push(contents),
            None => self.current.literals.push(contents),
        }
        self.current.code.push('"');
    }

    fn closes_raw_string(&self, hashes: usize) -> bool {
        (0..hashes).all(|offset| self.peek(offset) == Some('#'))
    }

    /// Distinguishes `'x'` / `'\n'` character literals from `'a` lifetimes and labels.
    fn char_or_lifetime(&mut self) {
        let is_char_literal = self.peek(1) == Some('\\') || self.peek(2) == Some('\'');
        if !is_char_literal {
            self.code_char();
            return;
        }
        self.pos += 1;
        while let Some(c) = self.bump() {
            if c == '\\' {
                self.bump();
            } else if c == '\'' {
                break;
            }
        }
        self.current.code.push_str("''");
    }

    /// Consumes an identifier, or a literal with a prefix such as `b"…"`, `r#"…"#` or `b'x'`.
    fn identifier_or_prefixed_literal(&mut self) {
        let start = self.pos;
        while self.peek(0).is_some_and(is_ident_continue) {
            self.pos += 1;
        }
        let word: String = self
            .chars
            .get(start..self.pos)
            .unwrap_or_default()
            .iter()
            .collect();
        let raw_hashes = self
            .chars
            .get(self.pos..)
            .unwrap_or_default()
            .iter()
            .take_while(|&&c| c == '#')
            .count();
        match (word.as_str(), self.peek(0), self.peek(raw_hashes)) {
            ("r" | "br" | "cr", _, Some('"')) => {
                self.pos += raw_hashes;
                self.string(raw_hashes);
            }
            ("b" | "c", Some('"'), _) => self.string(0),
            ("b", Some('\''), _) => self.char_or_lifetime(),
            _ => self.current.code.push_str(&word),
        }
    }
}

fn is_ident_start(c: char) -> bool {
    c.is_alphabetic() || c == '_'
}

fn is_ident_continue(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

#[cfg(test)]
mod tests {
    use super::*;

    fn code(text: &str) -> Vec<String> {
        split(text).into_iter().map(|line| line.code).collect()
    }

    fn comments(text: &str) -> Vec<String> {
        split(text).into_iter().map(|line| line.comment).collect()
    }

    #[test]
    fn separates_line_comments_from_code() {
        let lines = split("let x = 1; // note\n/// doc\n");
        assert_eq!(lines[0].code, "let x = 1; ");
        assert_eq!(lines[0].comment, " note");
        assert_eq!(lines[1].comment, "/ doc");
        assert_eq!(
            lines.len(),
            3,
            "the trailing newline starts an empty final line"
        );
    }

    #[test]
    fn handles_nested_multiline_block_comments() {
        assert_eq!(code("a /* x /* y */\n z */ b"), ["a ", " b"]);
        assert_eq!(comments("a /* x /* y */\n z */ b"), [" x  y ", " z "]);
    }

    #[test]
    fn removes_string_contents_but_keeps_line_numbers() {
        assert_eq!(
            code("f(\"std::fs // no\");\ng(\"a\\\"\nb\");"),
            ["f(\"\");", "g(\"", "\");"]
        );
    }

    #[test]
    fn keeps_string_contents_on_the_line_where_they_start() {
        let lines = split("f(\"/a/{id}\", r#\"b\"#);\ng(\"multi\nline\"); // \"no\"");
        assert_eq!(lines[0].literals, ["/a/{id}", "b"]);
        assert_eq!(lines[1].literals, ["multi\nline"]);
        assert!(lines[2].literals.is_empty(), "comments hold no literals");
    }

    #[test]
    fn handles_raw_and_byte_strings() {
        let text = r##"r#"a "quoted" b"#; br"x"; b"y"; c"z";"##;
        assert_eq!(code(text), ["\"\"; \"\"; \"\"; \"\";"]);
    }

    #[test]
    fn distinguishes_char_literals_from_lifetimes() {
        assert_eq!(
            code(r"fn f<'a>(c: char) { '\''; '/'; b'x'; }"),
            [r"fn f<'a>(c: char) { ''; ''; ''; }"]
        );
    }

    #[test]
    fn keeps_identifiers_that_merely_start_like_prefixes() {
        assert_eq!(
            code("let br = r; let raw = bytes;"),
            ["let br = r; let raw = bytes;"]
        );
    }

    #[test]
    fn unterminated_literals_do_not_panic() {
        for text in ["\"open", "r#\"open", "/* open", "'", "b'"] {
            let _lines = split(text);
        }
    }
}
