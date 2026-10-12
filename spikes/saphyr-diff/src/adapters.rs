//! Spike-only adapters: small code changes around serde-saphyr that try to reproduce what
//! serde_norway does where no `Options` does. `main.rs` measures each one alone and all
//! together against today's behaviour. None of this is a design; it is what a node could
//! write, measured.
//!
//! Every adapter that needs the document's shape reads it from the contract's own JSON
//! Schema (`schema_bundle()`'s `budgets-file` root), never from a hand-written copy, with
//! two stated exceptions: the root struct's Rust name (`BudgetsFile`; the schema calls it
//! `budgets-file`) and `schema_version`'s integer width (`u32`; its schema has no `format`).

use std::borrow::Cow;

use saphyr::{AnnotatedMapping, MarkedYaml, ScalarStyle, YamlData, YamlLoader};
use serde_json::Value;

use crate::Failure;

/// Which adapters are on.
#[derive(Clone, Copy, Debug, Default)]
pub struct Adapters {
    /// Refuse a leading byte order mark before parsing, as today refuses such a file.
    pub bom: bool,
    /// Pre-pass over saphyr's marked tree, driven by the schema: refuse what serde-saphyr
    /// coerces but serde_norway refuses (quoted numbers, null for a sequence or a number).
    pub shape: bool,
    /// Pre-pass patch: quote a plain null scalar (`~`, `null`) standing where the schema
    /// wants a non-nullable string, so it reads as the text it is, as serde_norway reads it.
    pub null_strings: bool,
    /// Drop serde_path_to_error's `?` segments (an unknown map key) from the path.
    pub path: bool,
    /// Rewrite serde-saphyr's messages into serde_norway's wording and locations.
    pub message: bool,
}

impl Adapters {
    pub const ALL: Self = Self {
        bom: true,
        shape: true,
        null_strings: true,
        path: true,
        message: true,
    };

    pub fn singles() -> Vec<(&'static str, Self)> {
        let off = Self::default();
        vec![
            ("bom", Self { bom: true, ..off }),
            ("shape", Self { shape: true, ..off }),
            (
                "null_strings",
                Self {
                    null_strings: true,
                    ..off
                },
            ),
            ("path", Self { path: true, ..off }),
            (
                "message",
                Self {
                    message: true,
                    ..off
                },
            ),
        ]
    }
}

// ---------------------------------------------------------------------------------------
// The marked tree.

/// The first document of `text`, with raw scalars (`early_parse(false)`) and duplicate keys
/// allowed, so it loads wherever serde-saphyr reaches a semantic error.
///
/// A text with a syntax error past what serde reads first (serde streams; this loads it
/// whole) is loaded again up to the start of the line the error is on, so the nodes before
/// it still have their positions.
pub fn marked(text: &str) -> Option<MarkedYaml<'_>> {
    let load = |text| {
        let mut parser = saphyr_parser::Parser::new_from_str(text);
        let mut loader = YamlLoader::<MarkedYaml>::default();
        loader.allow_duplicate_keys(true);
        loader.early_parse(false);
        parser
            .load(&mut loader, true)
            .map(|()| loader.into_documents().into_iter().next())
    };
    match load(text) {
        Ok(doc) => doc,
        Err(error) => {
            let line_start = byte(text, error.marker().index() - error.marker().col());
            load(&text[..line_start]).ok().flatten()
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Pos {
    pub index: usize,
    /// 1-based.
    pub line: usize,
    /// 1-based, as serde_norway counts.
    pub col: usize,
}

fn start(node: &MarkedYaml) -> Pos {
    let m = node.span.start;
    Pos {
        index: m.index(),
        line: m.line(),
        col: m.col() + 1,
    }
}

/// serde_norway's location suffix: none at the very start of the input.
fn at(pos: Pos) -> String {
    if pos.index == 0 {
        String::new()
    } else {
        format!(" at line {} column {}", pos.line, pos.col)
    }
}

fn raw<'a>(node: &'a MarkedYaml) -> Option<(&'a str, ScalarStyle)> {
    match &node.data {
        YamlData::Representation(text, style, None) => Some((text.as_ref(), *style)),
        _ => None,
    }
}

fn is_plain_null(node: &MarkedYaml) -> bool {
    // Not the empty scalar: serde_norway reads `budgets:` with nothing after it as empty.
    matches!(
        raw(node),
        Some(("~" | "null" | "Null" | "NULL", ScalarStyle::Plain))
    )
}

fn key_text<'a>(node: &'a MarkedYaml) -> Option<&'a str> {
    raw(node).map(|(text, _)| text)
}

fn get<'a, 'b>(
    map: &'b AnnotatedMapping<'a, MarkedYaml<'a>>,
    key: &str,
) -> Option<&'b MarkedYaml<'a>> {
    map.iter()
        .find(|(k, _)| key_text(k) == Some(key))
        .map(|(_, v)| v)
}

#[derive(Debug, Clone, PartialEq)]
enum Seg {
    Key(String),
    Index(usize),
}

fn segments(path: &str) -> Vec<Seg> {
    if path == "." {
        return Vec::new();
    }
    let mut out = Vec::new();
    for part in path.split('.') {
        let (key, rest) = part.split_once('[').map_or((part, ""), |(k, r)| (k, r));
        if !key.is_empty() {
            out.push(Seg::Key(key.to_owned()));
        }
        for index in rest.split('[') {
            if let Some(n) = index.strip_suffix(']').and_then(|n| n.parse().ok()) {
                out.push(Seg::Index(n));
            }
        }
    }
    out
}

fn node_at<'a, 'b>(root: &'b MarkedYaml<'a>, path: &[Seg]) -> Option<&'b MarkedYaml<'a>> {
    let mut node = root;
    for seg in path {
        node = match (&node.data, seg) {
            (YamlData::Mapping(map), Seg::Key(key)) => get(map, key)?,
            (YamlData::Sequence(items), Seg::Index(i)) => items.get(*i)?,
            _ => return None,
        };
    }
    Some(node)
}

fn parent(path: &str) -> String {
    let segs = segments(path);
    render(&segs[..segs.len().saturating_sub(1)])
}

fn render(segs: &[Seg]) -> String {
    let mut out = String::new();
    for seg in segs {
        match seg {
            Seg::Key(k) => {
                if !out.is_empty() {
                    out.push('.');
                }
                out.push_str(k);
            }
            Seg::Index(i) => out.push_str(&format!("[{i}]")),
        }
    }
    if out.is_empty() { ".".into() } else { out }
}

// ---------------------------------------------------------------------------------------
// The schema, read from the contract.

pub struct Schema {
    root: Value,
}

impl Schema {
    pub fn load() -> Self {
        Self {
            root: onebudgetspec_core::schema_bundle()["roots"]["budgets-file"].clone(),
        }
    }

    fn resolve<'s>(&'s self, schema: &'s Value) -> &'s Value {
        match schema.get("$ref").and_then(Value::as_str) {
            Some(r) => &self.root["$defs"][r.trim_start_matches("#/$defs/")],
            None => schema,
        }
    }

    /// The schema at `path`, resolved.
    fn at(&self, path: &[Seg]) -> Option<&Value> {
        let mut schema = &self.root;
        for seg in path {
            schema = self.resolve(schema);
            schema = match seg {
                Seg::Key(k) => schema.get("properties")?.get(k)?,
                Seg::Index(_) => schema.get("items")?,
            };
        }
        Some(self.resolve(schema))
    }

    /// The Rust name serde names a struct or sequence by in `expected ...`.
    fn expected(&self, path: &[Seg]) -> String {
        if path.is_empty() {
            return "struct BudgetsFile".into(); // stated exception: the root's Rust name
        }
        let mut schema = &self.root;
        let mut def = None;
        for seg in path {
            schema = self.resolve(schema);
            schema = match seg {
                Seg::Key(k) => &schema["properties"][k],
                Seg::Index(_) => &schema["items"],
            };
            def = schema
                .get("$ref")
                .and_then(Value::as_str)
                .map(|r| r.trim_start_matches("#/$defs/").to_owned());
        }
        match def {
            Some(name) => format!("struct {name}"),
            None if types(self.resolve(schema)).contains(&"array") => "a sequence".into(),
            None => numeric(self.resolve(schema)).unwrap_or("a string").into(),
        }
    }
}

fn types(schema: &Value) -> Vec<&str> {
    match schema.get("type") {
        Some(Value::String(t)) => vec![t.as_str()],
        Some(Value::Array(ts)) => ts.iter().filter_map(Value::as_str).collect(),
        _ => Vec::new(),
    }
}

/// The Rust numeric type serde names, from the schema's `format`.
fn numeric(schema: &Value) -> Option<&'static str> {
    let types = types(schema);
    match schema.get("format").and_then(Value::as_str) {
        Some("double") => Some("f64"),
        Some("uint64") => Some("u64"),
        Some("uint32") => Some("u32"),
        // stated exception: schema_version's schema carries no format; its field is a u32.
        None if types.contains(&"integer") => Some("u32"),
        _ => None,
    }
}

// ---------------------------------------------------------------------------------------
// serde_norway's view of a plain scalar standing where a number is wanted.

/// `None` when serde_norway accepts `text` as `want`; otherwise how its error names it.
fn misread_number(text: &str, style: ScalarStyle, want: &str) -> Option<String> {
    if style != ScalarStyle::Plain {
        return Some(format!("string {text:?}"));
    }
    let int = parse_int(text);
    let float = parse_float(text);
    if want == "f64" {
        return match (float, int) {
            (Some(_), _) => None,
            (None, Some(i)) => Some(format!("integer `{i}`")),
            (None, None) => Some(format!("string {text:?}")),
        };
    }
    match (int, float) {
        (Some(i), _) if i >= 0 && (want == "u64" || i <= i128::from(u32::MAX)) => None,
        (Some(i), _) => Some(format!("integer `{i}`")),
        (None, Some(f)) => Some(format!("floating point `{f:?}`")),
        (None, None) => Some(format!("string {text:?}")),
    }
}

fn parse_int(text: &str) -> Option<i128> {
    let (sign, body) = match text.as_bytes().first() {
        Some(b'-') => (-1, &text[1..]),
        Some(b'+') => (1, &text[1..]),
        _ => (1, text),
    };
    let value = if let Some(hex) = body.strip_prefix("0x") {
        i128::from_str_radix(hex, 16).ok()?
    } else if let Some(oct) = body.strip_prefix("0o") {
        i128::from_str_radix(oct, 8).ok()?
    } else if !body.is_empty() && body.bytes().all(|b| b.is_ascii_digit()) {
        body.parse().ok()?
    } else {
        return None;
    };
    Some(sign * value)
}

fn parse_float(text: &str) -> Option<f64> {
    match text {
        ".inf" | ".Inf" | ".INF" | "+.inf" | "+.Inf" | "+.INF" => return Some(f64::INFINITY),
        "-.inf" | "-.Inf" | "-.INF" => return Some(f64::NEG_INFINITY),
        ".nan" | ".NaN" | ".NAN" => return Some(f64::NAN),
        _ => {}
    }
    if !text
        .bytes()
        .all(|b| b.is_ascii_digit() || b"+-.eE".contains(&b))
        || !text.bytes().any(|b| b.is_ascii_digit())
    {
        return None;
    }
    text.parse::<f64>().ok().filter(|f| f.is_finite())
}

// ---------------------------------------------------------------------------------------
// The pre-pass.

struct Problem {
    pos: Pos,
    failure: Failure,
}

struct Walk<'s> {
    text: &'s str,
    schema: &'s Schema,
    adapters: Adapters,
    problems: Vec<Problem>,
    /// Byte ranges of plain nulls to quote.
    patches: Vec<(usize, usize, String)>,
}

impl Walk<'_> {
    fn node(&mut self, node: &MarkedYaml, path: &mut Vec<Seg>) {
        let Some(schema) = self.schema.at(path) else {
            return;
        };
        let ts = types(schema);
        let nullable = ts.contains(&"null");
        let here = render(path);
        let problem = |message: String| Problem {
            pos: start(node),
            failure: Failure {
                path: here.clone(),
                message: if here == "." {
                    message.clone()
                } else {
                    format!("{here}: {message}")
                },
            },
        };
        if is_plain_null(node) && !nullable {
            if self.adapters.shape && (ts.contains(&"array") || numeric(schema).is_some()) {
                let expected = self.schema.expected(path);
                self.problems.push(problem(format!(
                    "invalid type: unit value, expected {expected}{}",
                    at(start(node))
                )));
            }
            if self.adapters.null_strings && ts.contains(&"string") {
                let (text, _) = raw(node).unwrap();
                let span = node.span;
                self.patches
                    .push((span.start.index(), span.end.index(), format!("'{text}'")));
            }
            return;
        }
        // An empty plain scalar where a string is wanted reads as "" today.
        if self.adapters.null_strings
            && !nullable
            && ts.contains(&"string")
            && matches!(raw(node), Some(("", ScalarStyle::Plain)))
        {
            // Its span sits where its key ends; the value goes after the `:` that follows.
            let from = node.span.start.index();
            let colon = self
                .text
                .chars()
                .skip(from)
                .take_while(|c| *c != '\n')
                .position(|c| c == ':');
            if let Some(offset) = colon {
                self.patches
                    .push((from + offset + 1, from + offset + 1, " ''".into()));
            }
            return;
        }
        match &node.data {
            YamlData::Mapping(map) => {
                for (key, value) in map {
                    if let Some(k) = key_text(key) {
                        path.push(Seg::Key(k.to_owned()));
                        self.node(value, path);
                        path.pop();
                    }
                }
            }
            YamlData::Sequence(items) => {
                for (i, item) in items.iter().enumerate() {
                    path.push(Seg::Index(i));
                    self.node(item, path);
                    path.pop();
                }
            }
            YamlData::Representation(text, style, None) if self.adapters.shape => {
                if let Some(want) = numeric(schema)
                    && !(nullable && is_plain_null(node))
                    && let Some(found) = misread_number(text, *style, want)
                {
                    self.problems.push(problem(format!(
                        "invalid type: {found}, expected {want}{}",
                        at(start(node))
                    )));
                }
            }
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------------------
// The adapted parse.

pub fn parse<T: serde::de::DeserializeOwned>(
    text: &str,
    options: impl Fn() -> serde_saphyr::Options,
    adapters: Adapters,
    schema: &Schema,
) -> Result<T, Failure> {
    if adapters.bom && text.starts_with('\u{feff}') {
        return Err(Failure {
            path: ".".into(),
            message: "a budgets file must not begin with a byte order mark".into(),
        });
    }
    let tree = marked(text);
    let mut problems = Vec::new();
    let mut patched: Cow<str> = Cow::Borrowed(text);
    if (adapters.shape || adapters.null_strings)
        && let Some(root) = &tree
    {
        let mut walk = Walk {
            text,
            schema,
            adapters,
            problems: Vec::new(),
            patches: Vec::new(),
        };
        walk.node(root, &mut Vec::new());
        problems = walk.problems;
        if !walk.patches.is_empty() {
            let mut owned = text.to_owned();
            walk.patches.sort_by_key(|p| std::cmp::Reverse(p.0));
            for (from, to, with) in walk.patches {
                owned.replace_range(byte(text, from)..byte(text, to), &with);
            }
            patched = Cow::Owned(owned);
        }
    }
    problems.sort_by_key(|p| p.pos);

    let read = crate::saphyr::<T>(&patched, options());
    let mut failure = match read {
        Ok(value) => match problems.into_iter().next() {
            Some(first) => first.failure,
            None => return Ok(value),
        },
        Err(failure) => {
            let reported = location(&failure.message);
            match problems.into_iter().next() {
                Some(first)
                    if reported.is_none_or(|(line, col)| {
                        (first.pos.line, first.pos.col) <= (line, col)
                    }) =>
                {
                    first.failure
                }
                _ => failure,
            }
        }
    };
    if adapters.path {
        failure.path = strip_unknown_keys(&failure.path);
    }
    if adapters.message {
        failure = reword(failure, text, tree.as_ref(), schema, options);
    }
    Err(failure)
}

/// A marker index as a byte offset: saphyr counts characters.
fn byte(text: &str, chars: usize) -> usize {
    text.char_indices()
        .nth(chars)
        .map_or(text.len(), |(b, _)| b)
}

fn strip_unknown_keys(path: &str) -> String {
    let segs: Vec<_> = segments(path)
        .into_iter()
        .filter(|s| *s != Seg::Key("?".into()))
        .collect();
    render(&segs)
}

/// serde-saphyr's `at line N, column M` (with_snippet: false) or `error: line N column M:`.
fn location(message: &str) -> Option<(usize, usize)> {
    let tail = message
        .rsplit_once(" at line ")
        .map(|(_, t)| t.replace(',', ""))
        .or_else(|| {
            message
                .strip_prefix("error: line ")
                .map(|t| t.split(':').next().unwrap_or("").to_owned())
        })?;
    let mut parts = tail.split(" column ");
    Some((
        parts.next()?.trim().parse().ok()?,
        parts.next()?.trim().parse().ok()?,
    ))
}

fn split_location(message: &str) -> (&str, Option<(usize, usize)>) {
    match message.rsplit_once(" at line ") {
        Some((head, _)) => (head, location(message)),
        None => (message, None),
    }
}

/// serde's own `expected` phrasing for a list of alternatives.
fn one_of(list: &str, kind: &str) -> String {
    let names: Vec<_> = list.split(", ").filter(|n| !n.is_empty()).collect();
    match names.as_slice() {
        [] => format!("there are no {kind}"),
        [a] => format!("expected `{a}`"),
        [a, b] => format!("expected `{a}` or `{b}`"),
        _ => format!(
            "expected one of {}",
            names
                .iter()
                .map(|n| format!("`{n}`"))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

fn reword(
    failure: Failure,
    text: &str,
    tree: Option<&MarkedYaml>,
    schema: &Schema,
    options: impl Fn() -> serde_saphyr::Options,
) -> Failure {
    let (message, loc) = split_location(&failure.message);
    let segs = segments(&failure.path);
    let loc_text = loc.map_or(String::new(), |(l, c)| format!(" at line {l} column {c}"));
    let prefixed = |prefix: &str, message: String| {
        if prefix == "." {
            message
        } else {
            format!("{prefix}: {message}")
        }
    };
    let node_start = |segs: &[Seg]| tree.and_then(|t| node_at(t, segs)).map(start);
    let reworded = if let Some(rest) = message.strip_prefix("unknown field `") {
        let (name, list) = rest
            .split_once("`, expected one of ")
            .unwrap_or((rest.trim_end_matches('`'), ""));
        let parent = parent(&failure.path);
        Some(prefixed(
            &parent,
            format!(
                "unknown field `{name}`, {}{loc_text}",
                one_of(list, "fields")
            ),
        ))
    } else if let Some(rest) = message.strip_prefix("unknown variant `") {
        let (name, list) = rest
            .split_once("`, expected one of ")
            .unwrap_or((rest.trim_end_matches('`'), ""));
        Some(prefixed(
            &failure.path,
            format!(
                "unknown variant `{name}`, {}{loc_text}",
                one_of(list, "variants")
            ),
        ))
    } else if message.starts_with("missing field `") {
        let pos = node_start(&segs).map_or(loc_text.clone(), at);
        Some(prefixed(&failure.path, format!("{message}{pos}")))
    } else if let Some(rest) = message.strip_prefix("duplicate mapping key: ") {
        let key = rest
            .split(", set DuplicateKeyPolicy")
            .next()
            .unwrap_or(rest);
        let owner = strip_unknown_keys(&failure.path);
        let pos = node_start(&segments(&owner)).map_or(String::new(), at);
        Some(prefixed(&owner, format!("duplicate field `{key}`{pos}")))
    } else if message == "unexpected end of input" {
        // An empty document reads as an empty mapping today: the first field it misses.
        let first = crate::saphyr::<onebudgetspec_core::BudgetsFile>("{}", options())
            .err()
            .map(|f| split_location(&f.message).0.to_owned());
        let end = Pos {
            index: text.chars().count(),
            line: text.lines().count() + 1,
            col: 1,
        };
        first.map(|m| format!("{m}{}", at(end)))
    } else if message == "expected mapping start"
        || message == "expected sequence start"
        || message.starts_with("invalid ")
    {
        tree.and_then(|t| node_at(t, &segs)).and_then(|node| {
            let found = describe(node, schema.at(&segs).and_then(numeric))?;
            Some(prefixed(
                &failure.path,
                format!(
                    "invalid type: {found}, expected {}{}",
                    schema.expected(&segs),
                    at(start(node))
                ),
            ))
        })
    } else if message.starts_with("multiple YAML documents detected") {
        Some("deserializing from YAML containing more than one document is not supported".into())
    } else {
        None
    };
    match reworded {
        Some(message) => Failure { message, ..failure },
        None => failure,
    }
}

/// How serde_norway names a node it refused.
fn describe(node: &MarkedYaml, want: Option<&str>) -> Option<String> {
    Some(match &node.data {
        YamlData::Sequence(_) => "sequence".into(),
        YamlData::Mapping(_) => "map".into(),
        YamlData::Representation(text, style, None) => match want {
            Some(want) => misread_number(text, *style, want)?,
            None => format!("string {text:?}"),
        },
        _ => return None,
    })
}

// ---------------------------------------------------------------------------------------
// Schema-less reads: non-finite floats as JSON null, as serde_norway's Value has them.

/// saphyr's (early-parsed) tree as JSON, a non-finite float becoming null.
pub fn tree_value_with_null_non_finite(text: &str) -> Option<Value> {
    use saphyr::{LoadableYamlNode, Scalar, Yaml};
    fn node(yaml: &Yaml) -> Option<Value> {
        Some(match yaml {
            Yaml::Value(Scalar::Null) => Value::Null,
            Yaml::Value(Scalar::Boolean(b)) => Value::Bool(*b),
            Yaml::Value(Scalar::Integer(i)) => Value::from(*i),
            Yaml::Value(Scalar::FloatingPoint(f)) => {
                serde_json::Number::from_f64(f.into_inner()).map_or(Value::Null, Value::Number)
            }
            Yaml::Value(Scalar::String(s)) => Value::String(s.to_string()),
            Yaml::Sequence(items) => Value::Array(items.iter().map(node).collect::<Option<_>>()?),
            Yaml::Mapping(map) => Value::Object(
                map.iter()
                    .map(|(k, v)| {
                        Some((
                            match node(k)? {
                                Value::String(s) => s,
                                other => other.to_string(),
                            },
                            node(v)?,
                        ))
                    })
                    .collect::<Option<_>>()?,
            ),
            _ => return None,
        })
    }
    let docs = Yaml::load_from_str(text).ok()?;
    node(docs.first()?)
}

/// serde-saphyr's Value with `reject_non_finite_typeless_float: false`, then its canonical
/// non-finite strings mapped to null. It cannot tell `.inf` from `".inf"`.
pub fn saphyr_value_with_null_non_finite(text: &str) -> Result<Value, Failure> {
    fn walk(value: Value) -> Value {
        match value {
            Value::String(s) if matches!(s.as_str(), ".inf" | "-.inf" | ".nan") => Value::Null,
            Value::Array(items) => Value::Array(items.into_iter().map(walk).collect()),
            Value::Object(map) => {
                Value::Object(map.into_iter().map(|(k, v)| (k, walk(v))).collect())
            }
            other => other,
        }
    }
    crate::saphyr::<Value>(
        text,
        serde_saphyr::options! { reject_non_finite_typeless_float: false },
    )
    .map(walk)
}

// ---------------------------------------------------------------------------------------
// Writes: serde-saphyr's double-quoted scalars re-quoted with single quotes.

/// Each line whose value is one double-quoted scalar that needs no escape is rewritten
/// single-quoted, as serde_norway quotes.
pub fn single_quoted(yaml: &str) -> String {
    let mut out = String::with_capacity(yaml.len());
    for line in yaml.split_inclusive('\n') {
        let (body, newline) = line.strip_suffix('\n').map_or((line, ""), |b| (b, "\n"));
        out.push_str(&requote(body).unwrap_or_else(|| body.to_owned()));
        out.push_str(newline);
    }
    out
}

fn requote(line: &str) -> Option<String> {
    let trimmed = line.trim_start();
    let indent = &line[..line.len() - trimmed.len()];
    let (lead, value) = match trimmed.strip_prefix("- ") {
        Some(item) => ("- ", item),
        None => {
            let (key, value) = trimmed.split_once(": ")?;
            if key.starts_with('"') || key.starts_with('\'') {
                return None;
            }
            (&trimmed[..key.len() + 2], value)
        }
    };
    if !(value.len() >= 2 && value.starts_with('"') && value.ends_with('"')) {
        return None;
    }
    let decoded: String = serde_saphyr::from_str(value).ok()?;
    if decoded.chars().any(|c| c.is_control()) {
        return None;
    }
    Some(format!("{indent}{lead}'{}'", decoded.replace('\'', "''")))
}
