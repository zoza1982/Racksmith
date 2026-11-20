//! Rust items found on tokens: which code is test-only, what a crate defines and references,
//! and the inputs its deserializable and command-line types accept.
//!
//! Like the source rules, this works without name resolution, on the code view of
//! [`source_text`] (comments and literal contents removed), so a name in a doc comment or a
//! string is never a definition or a reference.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use crate::attributes::{self, Attribute};
use crate::source_text;
use crate::tokens::{self, Token};
use crate::workspace::SourceFile;

/// Keywords that introduce a named item; the name is the next identifier.
const ITEM_KEYWORDS: [&str; 9] = [
    "fn", "struct", "enum", "trait", "type", "const", "static", "mod", "union",
];

/// Words after `const` or `static` that are not the item's name: `const fn f`, `static mut S`,
/// `const _: () = ...`.
const NOT_ITEM_NAMES: [&str; 6] = ["fn", "unsafe", "async", "extern", "mut", "_"];

/// Derives that make a type an input: data deserialized from a request or file, or command-line
/// arguments parsed by clap.
const INPUT_DERIVES: [&str; 5] = ["Deserialize", "Parser", "Args", "Subcommand", "ValueEnum"];

/// Router methods whose first argument is a path pattern with `{param}` segments.
const ROUTE_METHODS: [&str; 4] = ["route", "route_service", "nest", "nest_service"];

/// The variable that `tracing_subscriber::EnvFilter` reads unless told otherwise
/// (`EnvFilter::DEFAULT_ENV`).
const DEFAULT_FILTER_ENV: &str = "RUST_LOG";

/// Attributes whose `skip` (or `skip_deserializing`) removes a field or variant from the input.
const SKIP_ATTRIBUTES: [&str; 5] = ["serde", "arg", "clap", "command", "value"];

/// The product code of one file: its tokens with test-only items removed.
#[derive(Debug)]
pub(crate) struct ProductCode {
    /// The file, as listed in its [`SourceFile`].
    pub(crate) path: PathBuf,
    /// Tokens of the code that is built outside tests.
    pub(crate) tokens: Vec<Token>,
}

/// A type deriving one of [`INPUT_DERIVES`], with the members a caller can set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct InputType {
    /// The type's name.
    pub(crate) name: String,
    /// Its fields and variants: `field`, `Variant`, `Variant.field` for the fields of a
    /// struct-like variant and `Variant.0` for the fields of a tuple variant. Tuple fields are
    /// named by position (`0`).
    pub(crate) members: Vec<String>,
    /// 1-based line of the type's name.
    pub(crate) line: usize,
    /// Whether the type derives `Deserialize`, has named fields (a struct, or an enum with a
    /// struct-like variant), and lets unknown fields through: it lacks
    /// `#[serde(deny_unknown_fields)]`, or a `#[serde(flatten)]` field defeats it.
    pub(crate) accepts_unknown_fields: bool,
}

/// Where the parts of an input type that decide whether it accepts unknown fields are.
#[derive(Debug, Default)]
struct Shape {
    /// The type has a `{ ... }` field list, of its own or of a variant.
    named_fields: bool,
    /// A field carries `#[serde(flatten)]`.
    flattens: bool,
}

/// The tokens of every file in `files` that is built outside tests.
///
/// Files under `tests/`, `benches/` and `examples/` are skipped, and so are the files of modules
/// declared `#[cfg(test)] mod name;`. In the remaining files, items gated by `#[cfg(test)]` (or
/// `#[cfg(kani)]`, the proof harnesses) are removed.
pub(crate) fn product_code(files: &[SourceFile]) -> Vec<ProductCode> {
    let parsed: Vec<(&SourceFile, Vec<Token>)> = files
        .iter()
        .filter(|file| !file.is_test_code)
        .map(|file| (file, tokens::tokenize(&source_text::split(&file.text))))
        .collect();
    let test_modules: Vec<PathBuf> = parsed
        .iter()
        .flat_map(|(file, tokens)| test_module_dirs(&file.path, tokens))
        .collect();
    parsed
        .into_iter()
        .filter(|(file, _)| !is_test_module_file(&file.path, &test_modules))
        .map(|(file, tokens)| ProductCode {
            path: file.path.clone(),
            tokens: strip_test_items(&tokens),
        })
        .collect()
}

/// For each `#[cfg(test)] mod name;` in the file at `path`, the module's path without
/// extension: `src/tests` for `src/lib.rs`, `src/routes/tests` for `src/routes.rs`.
fn test_module_dirs(path: &Path, tokens: &[Token]) -> Vec<PathBuf> {
    let parent = path.parent().unwrap_or_else(|| Path::new(""));
    let is_root = matches!(
        path.file_name().and_then(OsStr::to_str),
        Some("lib.rs" | "main.rs" | "build.rs")
    );
    let module_dir = match path.file_stem() {
        Some(stem) if !is_root => parent.join(stem),
        Some(_) | None => parent.to_path_buf(),
    };
    attributes::find(tokens)
        .iter()
        .filter(|attribute| !attribute.inner && is_test_gate(attribute))
        .filter_map(|attribute| declared_module(attribute.rest))
        .map(|name| module_dir.join(name))
        .collect()
}

/// The name of a `mod name;` declaration (with a body in another file) at the start of
/// `tokens`, after further attributes and a visibility.
fn declared_module(tokens: &[Token]) -> Option<&str> {
    match skip_attributes_and_visibility(tokens) {
        [keyword, name, semicolon, ..]
            if keyword.is("mod") && name.is_ident() && semicolon.is(";") =>
        {
            Some(name.text.as_str())
        }
        _ => None,
    }
}

/// Whether `path` is the file of a test module (`<dir>.rs`, `<dir>/mod.rs`) or below it.
fn is_test_module_file(path: &Path, test_modules: &[PathBuf]) -> bool {
    test_modules
        .iter()
        .any(|module| path == module.with_extension("rs") || path.starts_with(module))
}

/// Whether `attribute` gates its item to test or proof builds: `#[cfg(test)]`,
/// `#[cfg(kani)]`, `#[cfg(all(test, ...))]` with `test` or `kani` as a conjunct, or `any(...)`
/// whose every alternative is such a gate. `#[cfg(any(test, feature = "x"))]` is product code,
/// because the feature builds it outside tests.
fn is_test_gate(attribute: &Attribute<'_>) -> bool {
    match attribute.body {
        [cfg, predicate @ ..] if cfg.is("cfg") && opens(predicate, "(") => {
            group_len(predicate) == predicate.len() && is_test_predicate(inside_group(predicate))
        }
        _ => false,
    }
}

/// Whether the `cfg` predicate `tokens` holds only in test or proof builds.
fn is_test_predicate(tokens: &[Token]) -> bool {
    match tokens {
        [word] => word.is("test") || word.is("kani"),
        [combinator, group @ ..] if opens(group, "(") && group_len(group) == group.len() => {
            let operands = split_top_level(inside_group(group));
            if combinator.is("all") {
                operands.into_iter().any(is_test_predicate)
            } else if combinator.is("any") {
                !operands.is_empty() && operands.into_iter().all(is_test_predicate)
            } else {
                false
            }
        }
        _ => false,
    }
}

/// `tokens` without the items gated by a test attribute; a file that starts with an inner
/// `#![cfg(test)]` has no product code at all.
fn strip_test_items(tokens: &[Token]) -> Vec<Token> {
    let found = attributes::find(tokens);
    // `#`, `!`, `[`, the body and `]`, at the very start of the file.
    let gates_file = found.first().is_some_and(|first| {
        first.inner
            && is_test_gate(first)
            && tokens.len() == first.rest.len() + first.body.len() + 4
    });
    if gates_file {
        return Vec::new();
    }
    let mut removed: Vec<(usize, usize)> = Vec::new();
    for attribute in found.iter().filter(|a| !a.inner && is_test_gate(a)) {
        let attribute_end = tokens.len() - attribute.rest.len();
        // `#`, `[`, the body and `]`.
        let start = attribute_end.saturating_sub(attribute.body.len() + 3);
        let end = attribute_end + item_len(attribute.rest);
        removed.push((start, end));
    }
    tokens
        .iter()
        .enumerate()
        .filter(|(index, _)| {
            !removed
                .iter()
                .any(|&(start, end)| (start..end).contains(index))
        })
        .map(|(_, token)| token.clone())
        .collect()
}

/// The number of tokens of the item at the start of `tokens`, including its attributes: up to
/// a `;` or a closing `}` outside any bracket.
fn item_len(tokens: &[Token]) -> usize {
    let mut depth = 0_usize;
    for (index, token) in tokens.iter().enumerate() {
        match token.text.as_str() {
            "(" | "[" | "{" => depth += 1,
            ")" | "]" => depth = depth.saturating_sub(1),
            "}" => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return index + 1;
                }
            }
            ";" if depth == 0 => return index + 1,
            _ => {}
        }
    }
    tokens.len()
}

/// The indices of the tokens that name an item where it is defined.
fn definition_indices(tokens: &[Token]) -> Vec<usize> {
    let mut indices = Vec::new();
    for (index, token) in tokens.iter().enumerate() {
        let name_at = if token.is("macro_rules") {
            tokens
                .get(index + 1)
                .filter(|bang| bang.is("!"))
                .map(|_| index + 2)
        } else if ITEM_KEYWORDS.contains(&token.text.as_str()) {
            let skip_mut = token.is("static") && tokens.get(index + 1).is_some_and(|t| t.is("mut"));
            Some(index + if skip_mut { 2 } else { 1 })
        } else {
            None
        };
        let Some(name_at) = name_at else {
            continue;
        };
        let is_name = tokens
            .get(name_at)
            .is_some_and(|name| name.is_ident() && !NOT_ITEM_NAMES.contains(&name.text.as_str()));
        if is_name {
            indices.push(name_at);
        }
    }
    indices
}

/// The names of every item defined in `code`.
pub(crate) fn definitions(code: &[ProductCode]) -> BTreeSet<String> {
    code.iter()
        .flat_map(|file| {
            definition_indices(&file.tokens)
                .into_iter()
                .filter_map(|index| file.tokens.get(index))
                .map(|token| token.text.clone())
        })
        .collect()
}

/// Whether `code` uses the identifier `name` anywhere other than where an item of that name
/// is defined.
pub(crate) fn references<'a>(code: impl IntoIterator<Item = &'a ProductCode>, name: &str) -> bool {
    code.into_iter().any(|file| {
        let defined = definition_indices(&file.tokens);
        file.tokens
            .iter()
            .enumerate()
            .any(|(index, token)| token.is(name) && !defined.contains(&index))
    })
}

/// The first place `tokens` names the identifier `name`, as a line number.
pub(crate) fn first_mention(tokens: &[Token], name: &str) -> Option<usize> {
    tokens
        .iter()
        .find(|token| token.is(name))
        .map(|token| token.line)
}

/// Every type in `code` that derives one of [`INPUT_DERIVES`].
pub(crate) fn input_types(code: &[ProductCode]) -> Vec<InputType> {
    let mut types = Vec::new();
    for file in code {
        // The outer attributes of each item, by the index of the item's first token.
        let mut items: BTreeMap<usize, Vec<Attribute<'_>>> = BTreeMap::new();
        for attribute in attributes::find(&file.tokens) {
            if attribute.inner {
                continue;
            }
            let item = skip_attributes_and_visibility(attribute.rest);
            items
                .entry(file.tokens.len() - item.len())
                .or_default()
                .push(attribute);
        }
        for (&start, attached) in &items {
            if !attached.iter().any(|a| derives(a, &INPUT_DERIVES)) {
                continue;
            }
            let item = file.tokens.get(start..).unwrap_or_default();
            let Some((mut input, shape)) = input_type(item) else {
                continue;
            };
            let deserializes = attached.iter().any(|a| derives(a, &["Deserialize"]));
            let denies = attached.iter().any(|attribute| {
                attribute
                    .body
                    .iter()
                    .any(|token| token.is("deny_unknown_fields"))
            });
            input.accepts_unknown_fields =
                deserializes && shape.named_fields && (!denies || shape.flattens);
            types.push(input);
        }
    }
    types
}

/// Whether `attribute` derives one of `traits`, also through `cfg_attr` and with a path such as
/// `serde::Deserialize`.
fn derives(attribute: &Attribute<'_>, traits: &[&str]) -> bool {
    attribute
        .body
        .windows(2)
        .enumerate()
        .filter(|(_, pair)| matches!(pair, [derive, open] if derive.is("derive") && open.is("(")))
        .any(|(index, _)| {
            let group = attribute.body.get(index + 1..).unwrap_or_default();
            let inner = inside_group(group);
            inner.iter().enumerate().any(|(at, token)| {
                traits.contains(&token.text.as_str())
                    && !inner.get(at + 1).is_some_and(|next| next.is("::"))
            })
        })
}

/// The struct or enum at the start of `tokens`, its members and its shape.
fn input_type(tokens: &[Token]) -> Option<(InputType, Shape)> {
    let [keyword, name, rest @ ..] = tokens else {
        return None;
    };
    if !name.is_ident() {
        return None;
    }
    let body = skip_generics_and_where(rest);
    let inner = inside_group(body);
    let opens_with = |open: &str| body.first().is_some_and(|token| token.is(open));
    let mut shape = Shape::default();
    let members = match keyword.text.as_str() {
        "struct" | "union" if opens_with("{") => named_fields(inner, &mut shape),
        "struct" if opens_with("(") => tuple_fields(inner),
        "struct" => Vec::new(),
        "enum" if opens_with("{") => variants(inner, &mut shape),
        _ => return None,
    };
    let input = InputType {
        name: name.text.clone(),
        members,
        line: name.line,
        accepts_unknown_fields: false,
    };
    Some((input, shape))
}

/// The non-skipped field names of a `{ ... }` field list, noting in `shape` that the type has
/// named fields and whether one of them is flattened.
fn named_fields(inner: &[Token], shape: &mut Shape) -> Vec<String> {
    shape.named_fields = true;
    let fields = split_top_level(inner);
    shape.flattens |= fields.iter().any(|field| is_flattened(field));
    fields
        .into_iter()
        .filter(|field| !is_skipped(field))
        .filter_map(|field| match skip_attributes_and_visibility(field) {
            [name, colon, ..] if name.is_ident() && colon.is(":") => Some(name.text.clone()),
            _ => None,
        })
        .collect()
}

/// The positions of the non-skipped fields of a `( ... )` field list.
fn tuple_fields(inner: &[Token]) -> Vec<String> {
    split_top_level(inner)
        .into_iter()
        .enumerate()
        .filter(|(_, field)| !is_skipped(field))
        .map(|(position, _)| position.to_string())
        .collect()
}

/// The non-skipped variants of an enum body, each followed by its fields if it has any:
/// `Variant.field` for a struct-like variant, `Variant.0` for a tuple variant.
fn variants(inner: &[Token], shape: &mut Shape) -> Vec<String> {
    let mut members = Vec::new();
    for variant in split_top_level(inner) {
        if is_skipped(variant) {
            continue;
        }
        let [name, rest @ ..] = skip_attributes_and_visibility(variant) else {
            continue;
        };
        if !name.is_ident() {
            continue;
        }
        members.push(name.text.clone());
        let fields = if opens(rest, "{") {
            named_fields(inside_group(rest), shape)
        } else if opens(rest, "(") {
            tuple_fields(inside_group(rest))
        } else {
            Vec::new()
        };
        members.extend(
            fields
                .into_iter()
                .map(|field| format!("{}.{field}", name.text)),
        );
    }
    members
}

/// Whether a field carries `#[serde(flatten)]`.
fn is_flattened(field: &[Token]) -> bool {
    own_attributes(field).iter().any(|attribute| {
        attribute.body.first().is_some_and(|name| name.is("serde"))
            && attribute.body.iter().any(|token| token.is("flatten"))
    })
}

/// The inputs of `code` that no derive declares, as the `<Type>.<member>` half of their keys:
/// `route.<pattern>.<param>` for every `{param}` segment of a router path pattern
/// (`.route("/machines/{id}", ...)`), and `env.<NAME>` for every environment variable read at
/// run time (`env::var("NAME")`, `env::var_os`, `EnvFilter::from_env*` and
/// `from_default_env`, `#[arg(env = "NAME")]`).
///
/// # Errors
///
/// Returns one message per read whose name cannot be known from the code: an environment read
/// through a non-literal name, `env::vars()`, a bare `#[arg(env)]`, or a malformed `{param}`.
pub(crate) fn undeclared_inputs(code: &[ProductCode]) -> Result<BTreeSet<String>, Vec<String>> {
    let mut found = Undeclared::default();
    for file in code {
        let place = |line: usize| format!("{}:{line}", file.path.display());
        let (routes, route_problems) = route_inputs(file);
        found.keys.extend(routes);
        found.problems.extend(route_problems);
        for (index, token) in file.tokens.iter().enumerate() {
            match env_read_at(&file.tokens, index) {
                Some(Ok(key)) => {
                    found.keys.insert(key);
                }
                Some(Err(problem)) => found
                    .problems
                    .push(format!("{}: {problem}", place(token.line))),
                None => {}
            }
        }
        for attribute in attributes::find(&file.tokens) {
            for (line, result) in clap_env_inputs(&attribute) {
                match result {
                    Ok(key) => {
                        found.keys.insert(key);
                    }
                    Err(problem) => found.problems.push(format!("{}: {problem}", place(line))),
                }
            }
        }
    }
    if found.problems.is_empty() {
        Ok(found.keys)
    } else {
        Err(found.problems)
    }
}

/// The undeclared inputs found so far, and the reads that cannot be named.
#[derive(Debug, Default)]
struct Undeclared {
    keys: BTreeSet<String>,
    problems: Vec<String>,
}

/// The environment variable that the token at `index` reads, if it reads one: `Ok` with its
/// `env.<NAME>` key, or `Err` with why its name cannot be known.
fn env_read_at(tokens: &[Token], index: usize) -> Option<Result<String, String>> {
    let token = tokens.get(index)?;
    let after = tokens.get(index + 1..).unwrap_or_default();
    let after_env_path = index
        .checked_sub(2)
        .and_then(|at| tokens.get(at..index))
        .is_some_and(|before| matches!(before, [env, path] if env.is("env") && path.is("::")));
    let text = token.text.as_str();
    let unnamed = || {
        Err(format!(
            "`{text}` reads an environment variable whose name is not a string literal; name it \
             literally so it can be classified"
        ))
    };
    if after_env_path && (text == "var" || text == "var_os") {
        return Some(
            literal_argument(after).map_or_else(unnamed, |name| Ok(format!("env.{name}"))),
        );
    }
    if after_env_path && (text == "vars" || text == "vars_os") {
        return Some(Err(format!(
            "`env::{text}` reads every environment variable, which cannot be classified; read \
             each variable by name"
        )));
    }
    if text == "from_default_env" || text == "try_from_default_env" {
        return Some(Ok(format!("env.{DEFAULT_FILTER_ENV}")));
    }
    let filter_read = matches!(text, "from_env" | "try_from_env" | "from_env_lossy")
        && after.first().is_some_and(|open| open.is("("));
    if !filter_read {
        return None;
    }
    let named = literal_argument(after).map(str::to_owned).or_else(|| {
        let empty = after.get(1).is_some_and(|close| close.is(")"));
        empty.then(|| builder_env_var(tokens, index))
    });
    Some(named.map_or_else(unnamed, |name| Ok(format!("env.{name}"))))
}

/// The `route.<pattern>.<param>` inputs of every router path pattern in `file`, and one
/// message per pattern with a malformed `{param}` segment.
fn route_inputs(file: &ProductCode) -> (Vec<String>, Vec<String>) {
    let mut keys = Vec::new();
    let mut problems = Vec::new();
    for (index, token) in file.tokens.iter().enumerate() {
        if !ROUTE_METHODS.contains(&token.text.as_str()) {
            continue;
        }
        let after = file.tokens.get(index + 1..).unwrap_or_default();
        let Some(pattern) = (match after {
            [open, quote, ..] if open.is("(") => quote.literal.as_deref(),
            _ => None,
        }) else {
            continue;
        };
        match route_params(pattern) {
            Ok(params) => keys.extend(
                params
                    .into_iter()
                    .map(|param| format!("route.{pattern}.{param}")),
            ),
            Err(problem) => {
                problems.push(format!("{}:{}: {problem}", file.path.display(), token.line));
            }
        }
    }
    (keys, problems)
}

/// The `env.<NAME>` inputs of a clap `#[arg(...)]` or `#[clap(...)]` attribute, with the line of
/// each `env`: `Err` for a bare `env` or a non-literal name.
fn clap_env_inputs(attribute: &Attribute<'_>) -> Vec<(usize, Result<String, String>)> {
    let is_clap = attribute
        .body
        .first()
        .is_some_and(|name| name.is("arg") || name.is("clap"));
    if !is_clap {
        return Vec::new();
    }
    attribute
        .body
        .iter()
        .enumerate()
        .filter(|(_, word)| word.is("env"))
        .map(|(at, word)| {
            let result = match attribute.body.get(at + 1..).unwrap_or_default() {
                [equals, quote, ..] if equals.is("=") => quote
                    .literal
                    .as_ref()
                    .map(|name| format!("env.{name}"))
                    .ok_or_else(|| {
                        "`env = ...` names its variable with a non-literal; write \
                         `env = \"NAME\"`"
                            .to_owned()
                    }),
                _ => Err(
                    "a bare `env` derives the variable's name from the field; write \
                          `env = \"NAME\"` so it can be classified"
                        .to_owned(),
                ),
            };
            (word.line, result)
        })
        .collect()
}

/// The `{param}` names of a path pattern; `{*rest}` names `rest`, and `{{`/`}}` are escapes.
fn route_params(pattern: &str) -> Result<Vec<String>, String> {
    let mut params = Vec::new();
    let mut rest = pattern;
    while let Some(open) = rest.find('{') {
        let after = rest.get(open + 1..).unwrap_or_default();
        if let Some(escaped) = after.strip_prefix('{') {
            rest = escaped;
            continue;
        }
        let Some(close) = after.find('}') else {
            return Err(format!("path pattern `{pattern}` has an unclosed `{{`"));
        };
        let raw = after.get(..close).unwrap_or_default();
        let name = raw.strip_prefix('*').unwrap_or(raw);
        let is_name = name
            .chars()
            .next()
            .is_some_and(|first| first.is_alphabetic() || first == '_')
            && name.chars().all(|c| c.is_alphanumeric() || c == '_');
        if !is_name {
            return Err(format!(
                "path pattern `{pattern}` has a segment `{{{raw}}}` that is not a parameter name"
            ));
        }
        params.push(name.to_owned());
        rest = after.get(close + 1..).unwrap_or_default();
    }
    Ok(params)
}

/// The string literal that is the whole argument list at the start of `tokens`: `("NAME")`.
fn literal_argument(tokens: &[Token]) -> Option<&str> {
    match tokens {
        [open, quote, _closing_quote, close, ..] if open.is("(") && close.is(")") => {
            quote.literal.as_deref()
        }
        _ => None,
    }
}

/// The variable an `EnvFilter` builder reads: the literal of a `with_env_var("NAME")` earlier
/// in the same statement, or [`DEFAULT_FILTER_ENV`].
fn builder_env_var(tokens: &[Token], index: usize) -> String {
    let statement = tokens
        .get(..index)
        .unwrap_or_default()
        .iter()
        .rposition(|token| token.is(";") || token.is("{") || token.is("}"))
        .map_or(0, |end| end + 1);
    tokens
        .get(statement..index)
        .unwrap_or_default()
        .iter()
        .enumerate()
        .filter(|(_, token)| token.is("with_env_var"))
        .rev()
        .find_map(|(at, _)| {
            let from = statement + at + 1;
            literal_argument(tokens.get(from..).unwrap_or_default())
        })
        .unwrap_or(DEFAULT_FILTER_ENV)
        .to_owned()
}

/// Whether a field or variant carries `#[serde(skip)]`, `#[serde(skip_deserializing)]`,
/// `#[arg(skip)]` or a similar attribute that removes it from the input.
fn is_skipped(member: &[Token]) -> bool {
    own_attributes(member).iter().any(|attribute| {
        let named = attribute
            .body
            .first()
            .is_some_and(|name| SKIP_ATTRIBUTES.contains(&name.text.as_str()));
        named
            && attribute.body.windows(2).any(|pair| {
                matches!(pair, [word, next]
                    if (word.is("skip") || word.is("skip_deserializing"))
                        && (next.is(",") || next.is(")") || next.is("=")))
            })
    })
}

/// The attributes in front of a field or variant, not those of the fields inside it.
fn own_attributes(member: &[Token]) -> Vec<Attribute<'_>> {
    let prefix = member.len() - skip_attributes_and_visibility(member).len();
    attributes::find(member.get(..prefix).unwrap_or_default())
}

/// Splits a list at commas outside brackets and generics.
pub(crate) fn split_top_level(tokens: &[Token]) -> Vec<&[Token]> {
    let mut parts = Vec::new();
    let mut depth = 0_usize;
    let mut start = 0;
    for (index, token) in tokens.iter().enumerate() {
        match token.text.as_str() {
            "(" | "[" | "{" | "<" => depth += 1,
            ">" if is_arrow(tokens, index) => {}
            ")" | "]" | "}" | ">" => depth = depth.saturating_sub(1),
            "," if depth == 0 => {
                parts.push(tokens.get(start..index).unwrap_or_default());
                start = index + 1;
            }
            _ => {}
        }
    }
    parts.push(tokens.get(start..).unwrap_or_default());
    parts.retain(|part| !part.is_empty());
    parts
}

/// Whether the `>` at `index` is the second half of `->`.
pub(crate) fn is_arrow(tokens: &[Token], index: usize) -> bool {
    index
        .checked_sub(1)
        .and_then(|before| tokens.get(before))
        .is_some_and(|before| before.is("-"))
}

/// Skips `#[...]` attributes and a visibility such as `pub(crate)`.
pub(crate) fn skip_attributes_and_visibility(tokens: &[Token]) -> &[Token] {
    let mut rest = tokens;
    loop {
        rest = match rest {
            [hash, group @ ..] if hash.is("#") && opens(group, "[") => after_group(group),
            [vis, group @ ..] if vis.is("pub") && opens(group, "(") => after_group(group),
            [vis, tail @ ..] if vis.is("pub") => tail,
            _ => return rest,
        };
    }
}

/// Skips generic parameters and a `where` clause, up to the item's body.
pub(crate) fn skip_generics_and_where(tokens: &[Token]) -> &[Token] {
    let mut rest = tokens;
    if opens(rest, "<") {
        let mut depth = 0_usize;
        for (index, token) in rest.iter().enumerate() {
            match token.text.as_str() {
                "<" => depth += 1,
                ">" if !is_arrow(rest, index) => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        rest = rest.get(index + 1..).unwrap_or_default();
                        break;
                    }
                }
                _ => {}
            }
        }
    }
    if opens(rest, "where") {
        let body_at = rest
            .iter()
            .position(|token| token.is("{") || token.is(";"))
            .unwrap_or(rest.len());
        rest = rest.get(body_at..).unwrap_or_default();
    }
    rest
}

/// Whether `tokens` starts with the opening bracket or keyword `open`.
pub(crate) fn opens(tokens: &[Token], open: &str) -> bool {
    tokens.first().is_some_and(|token| token.is(open))
}

/// The length of the bracketed group that starts at `tokens[0]`, brackets included; an
/// unterminated group extends to the end.
pub(crate) fn group_len(tokens: &[Token]) -> usize {
    let mut depth = 0_usize;
    for (index, token) in tokens.iter().enumerate() {
        match token.text.as_str() {
            "(" | "[" | "{" => depth += 1,
            ")" | "]" | "}" => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return index + 1;
                }
            }
            _ => {}
        }
    }
    tokens.len()
}

/// The tokens inside the bracketed group that starts at `tokens[0]`, brackets excluded.
pub(crate) fn inside_group(tokens: &[Token]) -> &[Token] {
    tokens
        .get(1..group_len(tokens).saturating_sub(1))
        .unwrap_or_default()
}

fn after_group(tokens: &[Token]) -> &[Token] {
    tokens.get(group_len(tokens)..).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(path: &str, is_test_code: bool, text: &str) -> SourceFile {
        SourceFile {
            path: PathBuf::from(path),
            is_test_code,
            text: text.to_owned(),
        }
    }

    fn code(text: &str) -> Vec<ProductCode> {
        product_code(&[file("src/lib.rs", false, text)])
    }

    fn inputs(text: &str) -> Vec<(String, Vec<String>)> {
        input_types(&code(text))
            .into_iter()
            .map(|input| (input.name, input.members))
            .collect()
    }

    fn input(name: &str, members: &[&str]) -> (String, Vec<String>) {
        (
            name.to_owned(),
            members.iter().map(|member| (*member).to_owned()).collect(),
        )
    }

    #[test]
    fn finds_definitions_of_every_item_kind() {
        let text = "pub struct Machine; enum State {} pub(crate) trait Store {}\n\
                    type Id = u64; const MAX: u8 = 1; static mut COUNT: u8 = 0;\n\
                    pub const fn limit() {} mod routes {} macro_rules! gen { () => {} }\n\
                    const _: () = ();";
        let defined = definitions(&code(text));
        let expected: BTreeSet<String> = [
            "COUNT", "Id", "MAX", "Machine", "State", "Store", "gen", "limit", "routes",
        ]
        .map(str::to_owned)
        .into();
        assert_eq!(defined, expected);
    }

    #[test]
    fn names_in_comments_strings_and_tests_are_not_definitions() {
        let text = "/// struct Ghost;\nconst S: &str = \"fn phantom\";\n\
                    #[cfg(test)]\nmod tests { struct Fixture; }\n\
                    #[cfg(kani)]\nfn proof() {}";
        let defined = definitions(&code(text));
        assert_eq!(defined, ["S".to_owned()].into(), "{defined:?}");
    }

    #[test]
    fn references_exclude_the_definition_and_test_code() {
        let defining =
            code("pub struct Registry;\n#[cfg(test)]\nmod tests { fn t(_: Registry) {} }");
        assert!(
            !references(&defining, "Registry"),
            "definition and tests only"
        );
        let using = code("fn serve() { let _r = Registry::new(); }");
        assert!(references(&using, "Registry"), "a real use");
        let mentioned = code("// Registry\nconst S: &str = \"Registry\";\nfn registry_x() {}");
        assert!(
            !references(&mentioned, "Registry"),
            "comments, strings, other words"
        );
    }

    #[test]
    fn test_modules_in_their_own_files_and_test_directories_are_skipped() {
        let files = [
            file("src/lib.rs", false, "#[cfg(test)]\nmod tests;\nmod routes;"),
            file("src/tests.rs", false, "struct LibFixture;"),
            file(
                "src/routes.rs",
                false,
                "struct Route;\n#[cfg(test)]\nmod tests;",
            ),
            file("src/routes/tests.rs", false, "struct RouteFixture;"),
            file("src/routes/tests/deep.rs", false, "struct DeepFixture;"),
            file("tests/http.rs", true, "struct HttpFixture;"),
        ];
        let defined = definitions(&product_code(&files));
        assert_eq!(
            defined,
            ["Route".to_owned(), "routes".to_owned()].into(),
            "{defined:?}"
        );
    }

    #[test]
    fn a_file_gated_by_an_inner_cfg_test_has_no_product_code() {
        let files = [file(
            "src/fixtures.rs",
            false,
            "#![cfg(test)]\nstruct Fixture;",
        )];
        assert!(definitions(&product_code(&files)).is_empty());
    }

    #[test]
    fn extracts_fields_of_deserializable_structs() {
        let text = "#[derive(Debug, serde::Deserialize)]\n#[serde(deny_unknown_fields)]\n\
                    pub(crate) struct CreateMachineRequest {\n\
                    /// MAC.\n    mac: String,\n    #[serde(default)]\n    pub hostname: Option<String>,\n\
                    #[serde(skip)]\n    cache: BTreeMap<String, u8>,\n\
                    #[serde(skip_deserializing, default)]\n    derived: u8,\n\
                    #[serde(skip_serializing_if = \"Option::is_none\")]\n    tags: Option<Vec<String>>,\n}";
        assert_eq!(
            inputs(text),
            [input("CreateMachineRequest", &["mac", "hostname", "tags"])]
        );
    }

    #[test]
    fn extracts_variants_and_variant_fields_of_clap_enums() {
        let text = "#[derive(Debug, Subcommand)]\npub(crate) enum Command {\n\
                    /// Serve.\n    Serve {\n        #[arg(long, default_value = \"x\")]\n        listen: SocketAddr,\n    },\n\
                    Validate { #[command(subcommand)] target: Target },\n\
                    Version,\n    Raw(String),\n    #[command(skip)]\n    Hidden,\n}\n\
                    #[derive(Clone, Copy, clap::ValueEnum)]\nenum Format { Markdown, Tla }";
        assert_eq!(
            inputs(text),
            [
                input(
                    "Command",
                    &[
                        "Serve",
                        "Serve.listen",
                        "Validate",
                        "Validate.target",
                        "Version",
                        "Raw",
                        "Raw.0"
                    ]
                ),
                input("Format", &["Markdown", "Tla"]),
            ]
        );
    }

    #[test]
    fn generic_and_tuple_and_unit_types_are_handled() {
        let text = "#[derive(Deserialize)]\nstruct Page<T: Clone, const N: usize> where T: Ord {\n\
                    items: BTreeMap<String, fn(u8) -> T>,\n    next: Option<String>,\n}\n\
                    #[derive(Deserialize)]\nstruct Pair(u8, #[serde(skip)] u8, String);\n\
                    #[derive(Deserialize)]\nstruct Marker;";
        assert_eq!(
            inputs(text),
            [
                input("Page", &["items", "next"]),
                input("Pair", &["0", "2"]),
                input("Marker", &[]),
            ]
        );
    }

    #[test]
    fn only_input_derives_count_once_per_item() {
        let text = "#[derive(Debug, Serialize)]\nstruct Response { id: u8 }\n\
                    #[derive(Debug)]\n#[derive(Parser)]\n#[cfg_attr(feature, derive(Deserialize))]\n\
                    struct Cli { verbose: bool }\n\
                    #[derive(serde::Deserialize::Other)]\nstruct NotSerde { x: u8 }\n\
                    #[cfg(test)]\nmod tests {\n#[derive(Deserialize)]\nstruct Fixture { y: u8 }\n}";
        assert_eq!(inputs(text), [input("Cli", &["verbose"])]);
    }

    #[test]
    fn extracts_the_payload_of_tuple_variants() {
        let text = "#[derive(Deserialize)]\nenum Action { Rename(String), Resize(u64, #[serde(skip)] u8), Stop }";
        assert_eq!(
            inputs(text),
            [input(
                "Action",
                &["Rename", "Rename.0", "Resize", "Resize.0", "Stop"]
            )]
        );
    }

    fn open_to_unknown_fields(text: &str) -> Vec<(String, bool)> {
        input_types(&code(text))
            .into_iter()
            .map(|input| (input.name, input.accepts_unknown_fields))
            .collect()
    }

    #[test]
    fn finds_deserializable_types_that_accept_unknown_fields() {
        let text = "#[serde(deny_unknown_fields)]\n#[derive(Deserialize)]\nstruct Strict { a: u8 }\n\
                    #[derive(Deserialize)]\nstruct Lax { a: u8 }\n\
                    #[derive(Deserialize)]\n#[serde(deny_unknown_fields)]\n\
                    struct Flat { #[serde(flatten)] rest: Other }\n\
                    #[derive(Deserialize)]\nenum LaxVariant { A { x: u8 }, B }\n\
                    #[derive(Deserialize)]\nenum Unit { A, B(u8) }\n\
                    #[derive(Deserialize)]\nstruct Pair(u8, u8);\n\
                    #[derive(Parser)]\nstruct Cli { verbose: bool }";
        assert_eq!(
            open_to_unknown_fields(text),
            [
                ("Strict".to_owned(), false),
                ("Lax".to_owned(), true),
                ("Flat".to_owned(), true),
                ("LaxVariant".to_owned(), true),
                ("Unit".to_owned(), false),
                ("Pair".to_owned(), false),
                ("Cli".to_owned(), false),
            ]
        );
    }

    #[test]
    fn a_cfg_that_a_feature_also_enables_is_product_code() {
        let text = "#[cfg(any(test, feature = \"f\"))]\n#[derive(Deserialize)]\nstruct Shared { a: u8 }\n\
                    #[cfg(all(test, unix))]\nstruct TestOnly;\n\
                    #[cfg(any(test, kani))]\nstruct ProofOnly;\n\
                    #[cfg(not(test))]\nstruct NotTest;\n\
                    #[cfg(feature = \"test\")]\nstruct Featured;";
        assert_eq!(inputs(text), [input("Shared", &["a"])]);
        let defined = definitions(&code(text));
        let expected: BTreeSet<String> =
            ["Featured", "NotTest", "Shared"].map(str::to_owned).into();
        assert_eq!(defined, expected);
    }

    fn undeclared(text: &str) -> Result<Vec<String>, Vec<String>> {
        undeclared_inputs(&code(text)).map(|keys| keys.into_iter().collect())
    }

    #[test]
    fn route_parameters_are_inputs() {
        let text = "fn router() -> Router {\n\
                    Router::new().route(\"/machines\", get(list))\n\
                    .route(\"/machines/{id}\", get(one))\n\
                    .route(\"/machines/{id}/disks/{*path}\", get(disk))\n\
                    .nest(\"/racks/{rack}\", inner)\n\
                    .route(\"/literal/{{braces}}\", get(x))\n\
                    // .route(\"/commented/{no}\", get(y))\n}";
        assert_eq!(
            undeclared(text),
            Ok(vec![
                "route./machines/{id}.id".to_owned(),
                "route./machines/{id}/disks/{*path}.id".to_owned(),
                "route./machines/{id}/disks/{*path}.path".to_owned(),
                "route./racks/{rack}.rack".to_owned(),
            ])
        );
        let bad = undeclared("fn r() { x.route(\"/a/{1x}\", h); }").unwrap_err();
        assert!(bad[0].contains("not a parameter name"), "{bad:?}");
    }

    #[test]
    fn environment_reads_are_inputs() {
        let text = "fn main() {\n\
                    let a = std::env::var(\"RACKSMITH_CONFIG\");\n\
                    let b = env::var_os(\"HOME\");\n\
                    let f = EnvFilter::builder().with_default_directive(d).from_env_lossy();\n\
                    let g = EnvFilter::builder().with_env_var(\"APP_LOG\").from_env_lossy();\n\
                    let h = EnvFilter::try_from_env(\"OTHER_LOG\");\n\
                    let i = EnvFilter::from_default_env();\n\
                    let v = env!(\"CARGO_PKG_VERSION\");\n}\n\
                    #[derive(Parser)]\nstruct Cli {\n#[arg(long, env = \"RACKSMITH_LISTEN\")]\nlisten: String }\n\
                    #[cfg(test)]\nmod tests { fn t() { std::env::var(\"TEST_ONLY\"); } }";
        assert_eq!(
            undeclared(text),
            Ok([
                "env.APP_LOG",
                "env.HOME",
                "env.OTHER_LOG",
                "env.RACKSMITH_CONFIG",
                "env.RACKSMITH_LISTEN",
                "env.RUST_LOG",
            ]
            .map(str::to_owned)
            .into())
        );
    }

    #[test]
    fn environment_reads_without_a_literal_name_are_errors() {
        let text = "fn main() {\nlet a = env::var(name);\nlet all = env::vars();\n}\n\
                    #[derive(Parser)]\nstruct Cli {\n#[arg(long, env)]\nlisten: String }";
        let problems = undeclared(text).unwrap_err();
        assert_eq!(problems.len(), 3, "{problems:?}");
        assert!(problems[0].contains("src/lib.rs:2"), "{problems:?}");
        assert!(
            problems[1].contains("every environment variable"),
            "{problems:?}"
        );
        assert!(problems[2].contains("bare `env`"), "{problems:?}");
    }

    #[test]
    fn first_mention_reports_the_line() {
        let tokens = tokens::tokenize(&source_text::split(
            "fn a() {}\n// Planned\nPlanned::new();",
        ));
        assert_eq!(first_mention(&tokens, "Planned"), Some(3));
        assert_eq!(first_mention(&tokens, "Absent"), None);
    }
}
