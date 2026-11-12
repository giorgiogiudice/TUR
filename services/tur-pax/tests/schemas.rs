// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The published JSON schemas agree with the loaders.
//!
//! The loaders in this crate are authoritative; the schemas describe the same
//! formats for other tools. These tests hold the two together in three ways:
//!
//! 1. Example documents, valid and invalid, are validated against the schemas
//!    and loaded by the loaders, and the two must give the same answer.
//! 2. The few rules a schema cannot express are listed, each with a document
//!    that shows the difference, so that the list cannot grow unnoticed.
//! 3. The points where a schema and a loader state the same constant are
//!    compared directly.
//!
//! The validator is in this file. It implements the part of JSON Schema
//! 2020-12 the two schemas use and nothing else; a keyword or a piece of
//! regular-expression syntax it does not know stops the test, so a schema
//! cannot come to rely on something that is not checked here.

use std::collections::BTreeSet;

use serde_json::{Map, Value, json};
use tur_pax::manifest::{MANIFEST_SCHEMA_VERSION, SCHEMA_TYPES, ToolManifest};
use tur_pax::policy::POLICY_SCHEMA_VERSION;
use tur_pax::{Mode, RelaxationLevel, UserPolicy};

const MANIFEST_SCHEMA: &str = include_str!("../../../schemas/agent-tool-manifest.schema.json");
const POLICY_SCHEMA: &str = include_str!("../../../schemas/agent-policy.schema.json");

const TEST_SERVICE_MANIFEST: &str = include_str!("data/test-service.manifest.json");
/// The manifest the storage service ships: the first real one in the repository.
const STORAGE_MANIFEST: &str = include_str!("../../tur-storage/agent-tools.json");
const CLASS_4_MANIFEST: &str = include_str!("data/invalid-class-4.manifest.json");
const OBSERVE_ONLY_POLICY: &str = include_str!("data/policy-observe-only.json");
const ASK_EVERY_ACTION_POLICY: &str = include_str!("data/policy-ask-every-action.json");
const INVALID_RELAXATION_POLICY: &str = include_str!("data/policy-invalid-relaxation.json");

/// The example manifest of the addendum, section 11.
const ADDENDUM_MANIFEST: &str = r#"{
  "schema_version": 1,
  "service": "org.tur.Storage1",
  "tools": [
    {
      "name": "storage_list_volumes",
      "description": "List detected volumes with filesystem, state and mount point.",
      "class": 0,
      "method": "ListVolumes",
      "input_schema": { "type": "object", "properties": {}, "additionalProperties": false }
    },
    {
      "name": "storage_unmount",
      "description": "Unmount a volume by its TUR volume identifier.",
      "class": 1,
      "method": "Unmount",
      "input_schema": {
        "type": "object",
        "properties": { "volume_id": { "type": "string" } },
        "required": ["volume_id"],
        "additionalProperties": false
      }
    }
  ]
}"#;

/// The example policy of `docs/architecture/agent-policy.md`, section 7.
const DOCUMENTED_POLICY: &str = r#"{
  "schema_version": 1,
  "mode": "normal",
  "excluded_applications": ["org.example.Vault"],
  "sensitive_applications": ["org.mozilla.firefox"],
  "relaxations": [
    { "application": "org.kde.kmail2", "class": 2 }
  ]
}"#;

fn parse(text: &str) -> Value {
    serde_json::from_str(text).expect("the text is valid JSON")
}

// ---------------------------------------------------------------------------
// A regular expression matcher for the subset of ECMA-262 the schemas use:
// literal characters, `^` and `$`, character classes with ranges and negation,
// the escapes `\s`, `\S`, `\uXXXX` and escaped punctuation, groups, and the
// quantifiers `*`, `+`, `?` and `{m}`, `{m,}`, `{m,n}`. No alternation, no
// look-around, no lazy quantifiers, no back-references.
// ---------------------------------------------------------------------------

#[derive(Debug)]
enum Node {
    Start,
    End,
    Set(CharSet),
    Group(Vec<Node>),
    Repeat(Box<Node>, usize, Option<usize>),
}

#[derive(Debug)]
struct CharSet {
    negated: bool,
    items: Vec<SetItem>,
}

#[derive(Debug)]
enum SetItem {
    Range(char, char),
    /// `\s`: the white space and line terminators of ECMA-262.
    Space,
}

impl CharSet {
    fn of(c: char) -> CharSet {
        CharSet {
            negated: false,
            items: vec![SetItem::Range(c, c)],
        }
    }

    fn contains(&self, c: char) -> bool {
        let listed = self.items.iter().any(|item| match item {
            SetItem::Range(low, high) => (*low..=*high).contains(&c),
            SetItem::Space => matches!(
                c,
                '\t' | '\n' | '\u{b}' | '\u{c}' | '\r' | ' ' | '\u{a0}' | '\u{1680}' | '\u{2000}'
                    ..='\u{200a}'
                        | '\u{2028}'
                        | '\u{2029}'
                        | '\u{202f}'
                        | '\u{205f}'
                        | '\u{3000}'
                        | '\u{feff}'
            ),
        });
        listed != self.negated
    }
}

struct Pattern {
    nodes: Vec<Node>,
}

struct PatternParser {
    chars: Vec<char>,
    at: usize,
}

impl PatternParser {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.at).copied()
    }

    fn take(&mut self) -> char {
        let c = self
            .peek()
            .unwrap_or_else(|| panic!("the pattern ends too early"));
        self.at += 1;
        c
    }

    fn sequence(&mut self) -> Vec<Node> {
        let mut nodes = Vec::new();
        while let Some(c) = self.peek() {
            if c == ')' {
                break;
            }
            self.at += 1;
            let atom = match c {
                '^' => Node::Start,
                '$' => Node::End,
                '(' => {
                    assert_ne!(
                        self.peek(),
                        Some('?'),
                        "group syntax (?...) is not supported"
                    );
                    let inner = self.sequence();
                    assert_eq!(self.take(), ')', "a group is not closed");
                    Node::Group(inner)
                }
                '[' => Node::Set(self.class()),
                '\\' => {
                    let (item, negated) = self.escape();
                    Node::Set(CharSet {
                        negated,
                        items: vec![item],
                    })
                }
                '*' | '+' | '?' | '{' | '}' | '|' | '.' | ']' => {
                    panic!("{c:?} is not supported here")
                }
                literal => Node::Set(CharSet::of(literal)),
            };
            nodes.push(self.quantified(atom));
        }
        nodes
    }

    /// What follows a backslash: a set item, and whether it is negated.
    fn escape(&mut self) -> (SetItem, bool) {
        match self.take() {
            's' => (SetItem::Space, false),
            'S' => (SetItem::Space, true),
            'u' => {
                let digits: String = (0..4).map(|_| self.take()).collect();
                let code = u32::from_str_radix(&digits, 16)
                    .unwrap_or_else(|_| panic!("\\u{digits} is not four hexadecimal digits"));
                let c = char::from_u32(code)
                    .unwrap_or_else(|| panic!("\\u{digits} is not a character"));
                (SetItem::Range(c, c), false)
            }
            c if c.is_ascii_alphanumeric() => panic!("the escape \\{c} is not supported"),
            c => (SetItem::Range(c, c), false),
        }
    }

    /// One character of a class, or `None` after pushing `\s`.
    fn class_character(&mut self, first: char, items: &mut Vec<SetItem>) -> Option<char> {
        if first != '\\' {
            return Some(first);
        }
        match self.escape() {
            (SetItem::Range(c, _), false) => Some(c),
            (SetItem::Space, false) => {
                items.push(SetItem::Space);
                None
            }
            _ => panic!("\\S is not supported inside a class"),
        }
    }

    /// The rest of a character class, after its `[`.
    fn class(&mut self) -> CharSet {
        let negated = self.peek() == Some('^');
        if negated {
            self.at += 1;
        }
        let mut items = Vec::new();
        loop {
            let first = self.take();
            if first == ']' {
                break;
            }
            let Some(low) = self.class_character(first, &mut items) else {
                continue;
            };
            let is_range = self.peek() == Some('-')
                && self.chars.get(self.at + 1).is_some_and(|next| *next != ']');
            if is_range {
                self.at += 1;
                let second = self.take();
                let high = self
                    .class_character(second, &mut items)
                    .unwrap_or_else(|| panic!("a range cannot end in \\s"));
                assert!(low <= high, "the range {low:?}-{high:?} is reversed");
                items.push(SetItem::Range(low, high));
            } else {
                items.push(SetItem::Range(low, low));
            }
        }
        CharSet { negated, items }
    }

    fn quantified(&mut self, atom: Node) -> Node {
        let (min, max) = match self.peek() {
            Some('*') => {
                self.at += 1;
                (0, None)
            }
            Some('+') => {
                self.at += 1;
                (1, None)
            }
            Some('?') => {
                self.at += 1;
                (0, Some(1))
            }
            Some('{') => {
                self.at += 1;
                let mut text = String::new();
                loop {
                    let c = self.take();
                    if c == '}' {
                        break;
                    }
                    text.push(c);
                }
                match text.split_once(',') {
                    None => {
                        let exact = repetitions(&text);
                        (exact, Some(exact))
                    }
                    Some((min, "")) => (repetitions(min), None),
                    Some((min, max)) => (repetitions(min), Some(repetitions(max))),
                }
            }
            _ => return atom,
        };
        assert!(
            !matches!(atom, Node::Start | Node::End),
            "an anchor cannot be repeated"
        );
        assert!(
            !matches!(self.peek(), Some('?' | '*' | '+' | '{')),
            "a quantifier cannot follow a quantifier"
        );
        assert!(max.is_none_or(|max| min <= max), "a repetition is reversed");
        Node::Repeat(Box::new(atom), min, max)
    }
}

fn repetitions(text: &str) -> usize {
    text.parse()
        .unwrap_or_else(|_| panic!("{text:?} is not a repetition count"))
}

impl Pattern {
    fn new(pattern: &str) -> Pattern {
        let mut parser = PatternParser {
            chars: pattern.chars().collect(),
            at: 0,
        };
        let nodes = parser.sequence();
        assert_eq!(parser.at, parser.chars.len(), "unbalanced ) in {pattern:?}");
        Pattern { nodes }
    }

    /// Whether the pattern matches anywhere in the text, as `pattern` in JSON
    /// Schema does.
    fn is_match(&self, text: &str) -> bool {
        let text: Vec<char> = text.chars().collect();
        (0..=text.len()).any(|start| match_sequence(&self.nodes, &text, start, &|_| true))
    }
}

fn match_sequence(nodes: &[Node], text: &[char], at: usize, rest: &dyn Fn(usize) -> bool) -> bool {
    match nodes.split_first() {
        None => rest(at),
        Some((node, tail)) => match_node(node, text, at, &|next| {
            match_sequence(tail, text, next, rest)
        }),
    }
}

fn match_node(node: &Node, text: &[char], at: usize, rest: &dyn Fn(usize) -> bool) -> bool {
    match node {
        Node::Start => at == 0 && rest(at),
        Node::End => at == text.len() && rest(at),
        Node::Set(set) => text.get(at).is_some_and(|c| set.contains(*c)) && rest(at + 1),
        Node::Group(nodes) => match_sequence(nodes, text, at, rest),
        Node::Repeat(inner, min, max) => match &**inner {
            // A repeated set of characters is matched without recursion, so
            // that a long text does not use up the stack: take as many as
            // there are, then give them back one at a time.
            Node::Set(set) => {
                let available = text[at..].iter().take_while(|c| set.contains(**c)).count();
                let most = max.map_or(available, |max| available.min(max));
                *min <= most && (*min..=most).rev().any(|count| rest(at + count))
            }
            _ => match_repeat(inner, (*min, *max), 0, text, at, rest),
        },
    }
}

/// Greedy repetition with backtracking: one more repetition first, then what
/// follows.
fn match_repeat(
    node: &Node,
    bounds: (usize, Option<usize>),
    done: usize,
    text: &[char],
    at: usize,
    rest: &dyn Fn(usize) -> bool,
) -> bool {
    let (min, max) = bounds;
    let one_more = max.is_none_or(|max| done < max)
        && match_node(node, text, at, &|next| {
            // A repetition that consumes nothing would never end.
            next > at && match_repeat(node, bounds, done + 1, text, next, rest)
        });
    one_more || (done >= min && rest(at))
}

// ---------------------------------------------------------------------------
// A validator for the keywords the schemas use.
// ---------------------------------------------------------------------------

/// The keywords the validator implements. Anything else stops the test.
const KEYWORDS: &[&str] = &[
    "$schema",
    "$id",
    "$defs",
    "$ref",
    "title",
    "description",
    "type",
    "const",
    "enum",
    "minLength",
    "maxLength",
    "pattern",
    "minimum",
    "maximum",
    "minItems",
    "items",
    "required",
    "properties",
    "additionalProperties",
    "propertyNames",
    "oneOf",
];

/// Equality as JSON Schema defines it: numbers are equal if they have the same
/// mathematical value, so `1` equals `1.0`.
fn json_equal(left: &Value, right: &Value) -> bool {
    match (left, right) {
        (Value::Number(left), Value::Number(right)) => match (integer(left), integer(right)) {
            (Some(left), Some(right)) => left == right,
            _ => left.as_f64() == right.as_f64(),
        },
        (Value::Array(left), Value::Array(right)) => {
            left.len() == right.len()
                && left
                    .iter()
                    .zip(right)
                    .all(|(left, right)| json_equal(left, right))
        }
        (Value::Object(left), Value::Object(right)) => {
            left.len() == right.len()
                && left
                    .iter()
                    .all(|(key, left)| right.get(key).is_some_and(|right| json_equal(left, right)))
        }
        _ => left == right,
    }
}

fn integer(number: &serde_json::Number) -> Option<i128> {
    number
        .as_i64()
        .map(i128::from)
        .or_else(|| number.as_u64().map(i128::from))
}

fn has_type(instance: &Value, name: &str) -> bool {
    match name {
        "object" => instance.is_object(),
        "array" => instance.is_array(),
        "string" => instance.is_string(),
        "boolean" => instance.is_boolean(),
        "null" => instance.is_null(),
        "number" => instance.is_number(),
        // JSON Schema: any number with a zero fractional part is an integer.
        "integer" => {
            instance.is_i64()
                || instance.is_u64()
                || instance
                    .as_f64()
                    .is_some_and(|number| number.fract() == 0.0)
        }
        other => panic!("the validator does not know the type {other:?}"),
    }
}

fn resolve<'a>(root: &'a Value, reference: &Value) -> &'a Value {
    let reference = reference.as_str().expect("$ref is a string");
    let name = reference
        .strip_prefix("#/$defs/")
        .unwrap_or_else(|| panic!("the validator resolves only #/$defs/ references: {reference}"));
    root.get("$defs")
        .and_then(|definitions| definitions.get(name))
        .unwrap_or_else(|| panic!("{reference} is not defined"))
}

fn count(argument: &Value) -> u64 {
    argument
        .as_u64()
        .unwrap_or_else(|| panic!("{argument} is not a count"))
}

fn length(text: &str) -> u64 {
    u64::try_from(text.chars().count()).expect("a length fits in 64 bits")
}

fn list(argument: &Value) -> &Vec<Value> {
    argument
        .as_array()
        .unwrap_or_else(|| panic!("{argument} is not a list"))
}

fn object(argument: &Value) -> &Map<String, Value> {
    argument
        .as_object()
        .unwrap_or_else(|| panic!("{argument} is not an object"))
}

/// Whether `instance` is valid against `schema`, a schema inside `root`.
fn is_valid(root: &Value, schema: &Value, instance: &Value) -> bool {
    let schema = object(schema);
    schema
        .iter()
        .all(|(keyword, argument)| match keyword.as_str() {
            "$schema" | "$id" | "$defs" | "title" | "description" => true,
            "$ref" => is_valid(root, resolve(root, argument), instance),
            "type" => has_type(instance, argument.as_str().expect("type is one string")),
            "const" => json_equal(instance, argument),
            "enum" => list(argument)
                .iter()
                .any(|option| json_equal(instance, option)),
            "minLength" => instance
                .as_str()
                .is_none_or(|text| length(text) >= count(argument)),
            "maxLength" => instance
                .as_str()
                .is_none_or(|text| length(text) <= count(argument)),
            "pattern" => instance.as_str().is_none_or(|text| {
                Pattern::new(argument.as_str().expect("pattern is a string")).is_match(text)
            }),
            "minimum" => instance
                .as_f64()
                .is_none_or(|number| number >= argument.as_f64().expect("minimum is a number")),
            "maximum" => instance
                .as_f64()
                .is_none_or(|number| number <= argument.as_f64().expect("maximum is a number")),
            "minItems" => instance
                .as_array()
                .is_none_or(|items| items.len() as u64 >= count(argument)),
            "items" => instance
                .as_array()
                .is_none_or(|items| items.iter().all(|item| is_valid(root, argument, item))),
            "required" => instance.as_object().is_none_or(|instance| {
                list(argument)
                    .iter()
                    .all(|name| instance.contains_key(name.as_str().expect("a property name")))
            }),
            "properties" => instance.as_object().is_none_or(|instance| {
                object(argument).iter().all(|(name, property)| {
                    instance
                        .get(name)
                        .is_none_or(|value| is_valid(root, property, value))
                })
            }),
            "additionalProperties" => instance.as_object().is_none_or(|instance| {
                let declared = schema.get("properties").map(object);
                instance
                    .iter()
                    .filter(|(name, _)| declared.is_none_or(|known| !known.contains_key(*name)))
                    .all(|(_, value)| match argument {
                        Value::Bool(allowed) => *allowed,
                        additional => is_valid(root, additional, value),
                    })
            }),
            "propertyNames" => instance.as_object().is_none_or(|instance| {
                instance
                    .keys()
                    .all(|name| is_valid(root, argument, &Value::String(name.clone())))
            }),
            "oneOf" => {
                list(argument)
                    .iter()
                    .filter(|option| is_valid(root, option, instance))
                    .count()
                    == 1
            }
            other => panic!("the validator does not implement the keyword {other:?}"),
        })
}

/// Check every schema inside `schema`, reached or not by the examples: only
/// implemented keywords, and only patterns the matcher can read.
fn check_keywords(schema: &Value, path: &str) {
    for (keyword, argument) in object(schema) {
        assert!(
            KEYWORDS.contains(&keyword.as_str()),
            "{path}: the keyword {keyword:?} is not implemented by the validator"
        );
        let at = format!("{path}/{keyword}");
        match keyword.as_str() {
            "pattern" => {
                let _ = Pattern::new(argument.as_str().expect("pattern is a string"));
            }
            "$ref" => {
                assert!(
                    argument
                        .as_str()
                        .is_some_and(|reference| reference.starts_with("#/$defs/")),
                    "{at}"
                );
            }
            "items" | "propertyNames" => check_keywords(argument, &at),
            "additionalProperties" => {
                if !argument.is_boolean() {
                    check_keywords(argument, &at);
                }
            }
            "properties" | "$defs" => {
                for (name, inner) in object(argument) {
                    check_keywords(inner, &format!("{at}/{name}"));
                }
            }
            "oneOf" => {
                for (index, inner) in list(argument).iter().enumerate() {
                    check_keywords(inner, &format!("{at}/{index}"));
                }
            }
            _ => {}
        }
    }
}

fn manifest_schema_accepts(document: &Value) -> bool {
    let schema = parse(MANIFEST_SCHEMA);
    is_valid(&schema, &schema, document)
}

fn policy_schema_accepts(document: &Value) -> bool {
    let schema = parse(POLICY_SCHEMA);
    is_valid(&schema, &schema, document)
}

fn manifest_loads(document: &Value) -> bool {
    ToolManifest::from_json(&document.to_string()).is_ok()
}

fn policy_loads(document: &Value) -> bool {
    UserPolicy::from_json(&document.to_string()).is_ok()
}

// ---------------------------------------------------------------------------
// The matcher and the validator themselves.
// ---------------------------------------------------------------------------

#[test]
fn the_pattern_matcher_matches_what_the_schemas_ask_of_it() {
    let cases: [(&str, &[&str], &[&str]); 7] = [
        (
            "^[a-z][a-z0-9_]{0,63}$",
            &["a", "a_9", "storage_list_volumes"],
            &["", "A", "9a", "_a", "a-b", "a b", "a\n", " a"],
        ),
        (
            "^[A-Za-z_][A-Za-z0-9_]*(\\.[A-Za-z_][A-Za-z0-9_]*)+$",
            &["a.b", "org.tur.Storage1", "_a._b"],
            &["", "a", "a.", ".a", "a..b", "a.9b", "a.b c", "a.b."],
        ),
        (
            "^[!-~]([ -~]*[!-~])?$",
            &["a", "ab", "a b", "org.kde.konsole", "~!"],
            &["", " ", " a", "a ", "a\tb", "\u{e9}", "a\u{200b}"],
        ),
        (
            "^[^\\u0000-\\u001f\\u007f-\\u009f]*[^\\s\\u0000-\\u001f\\u007f-\\u009f][^\\u0000-\\u001f\\u007f-\\u009f]*$",
            &["x", " x ", "List volumes.", "\u{e9}", "a\u{a0}b"],
            &[
                "", " ", "\u{a0}", "\u{feff}", "a\nb", "a\tb", "a\u{7f}", "a\u{85}b",
            ],
        ),
        // Without anchors a pattern matches anywhere in the text.
        ("b+", &["b", "abbbc", "ab"], &["", "a", "ac"]),
        (
            "^a{2}b{1,}c?$",
            &["aab", "aabbc"],
            &["ab", "aaab", "aabcc", "aac"],
        ),
        ("^(ab)*\\S$", &["x", "abx", "ababb"], &["", "ab ", "ab"]),
    ];
    for (pattern, matching, failing) in cases {
        let compiled = Pattern::new(pattern);
        for text in matching {
            assert!(compiled.is_match(text), "{pattern} must match {text:?}");
        }
        for text in failing {
            assert!(
                !compiled.is_match(text),
                "{pattern} must not match {text:?}"
            );
        }
    }
    // The longest strings the schemas bound, without running out of stack.
    let long = "a".repeat(1025);
    assert!(Pattern::new("^[^\\u0000-\\u001f]*[^\\s][^\\u0000-\\u001f]*$").is_match(&long));
    assert!(!Pattern::new("^[a-z]{0,1024}$").is_match(&long));
}

#[test]
fn the_pattern_matcher_refuses_syntax_it_does_not_implement() {
    for pattern in [
        "a|b", "(?=a)b", "(?!a)b", "(?:a)", "a.b", "\\d+", "\\w", "\\bword", "a*?", "a**", "^*",
        "[a", "(a", "a)", "a{x}", "a{2,1}", "[z-a]", "[\\S]", "\\u12",
    ] {
        let result = std::panic::catch_unwind(|| Pattern::new(pattern));
        assert!(result.is_err(), "{pattern:?} was accepted");
    }
}

#[test]
fn the_schemas_use_only_what_the_validator_implements() {
    for (name, text) in [("manifest", MANIFEST_SCHEMA), ("policy", POLICY_SCHEMA)] {
        let schema = parse(text);
        check_keywords(&schema, name);
        assert_eq!(
            schema["$schema"],
            "https://json-schema.org/draft/2020-12/schema"
        );
    }
}

#[test]
fn the_validator_implements_the_keywords_as_json_schema_defines_them() {
    let root = json!({
        "$defs": {
            "name": { "type": "string", "minLength": 2, "maxLength": 3, "pattern": "^[a-z]+$" },
            "pair": { "type": "array", "minItems": 2, "items": { "$ref": "#/$defs/name" } }
        }
    });
    let check = |schema: Value, instance: Value| is_valid(&root, &schema, &instance);

    // A keyword says nothing about an instance of another type.
    assert!(check(json!({ "minLength": 5 }), json!(1)));
    assert!(check(json!({ "minimum": 5 }), json!("x")));
    assert!(check(json!({ "required": ["a"] }), json!([])));
    assert!(check(json!({ "items": { "type": "string" } }), json!({})));

    assert!(check(json!({ "$ref": "#/$defs/name" }), json!("ab")));
    assert!(!check(json!({ "$ref": "#/$defs/name" }), json!("a")));
    assert!(!check(json!({ "$ref": "#/$defs/name" }), json!("abcd")));
    assert!(!check(json!({ "$ref": "#/$defs/name" }), json!("AB")));
    assert!(check(
        json!({ "$ref": "#/$defs/pair" }),
        json!(["ab", "cd"])
    ));
    assert!(!check(json!({ "$ref": "#/$defs/pair" }), json!(["ab"])));
    assert!(!check(json!({ "$ref": "#/$defs/pair" }), json!(["ab", 1])));

    // Length is counted in characters, not bytes.
    assert!(check(json!({ "maxLength": 1 }), json!("\u{1f5d1}")));

    // Numbers compare by value.
    assert!(check(json!({ "const": 1 }), json!(1.0)));
    assert!(check(json!({ "enum": [1, 2] }), json!(2.0)));
    assert!(!check(json!({ "enum": [1, 2] }), json!(3)));
    assert!(!check(json!({ "const": 1 }), json!("1")));
    assert!(check(json!({ "const": false }), json!(false)));
    assert!(check(json!({ "type": "integer" }), json!(2.0)));
    assert!(!check(json!({ "type": "integer" }), json!(2.5)));
    assert!(check(
        json!({ "type": "integer", "minimum": 0, "maximum": 3 }),
        json!(3)
    ));
    assert!(!check(json!({ "minimum": 0, "maximum": 3 }), json!(4)));
    assert!(!check(json!({ "minimum": 0, "maximum": 3 }), json!(-1)));

    let closed = json!({
        "type": "object",
        "required": ["a"],
        "properties": { "a": { "type": "boolean" } },
        "additionalProperties": false
    });
    assert!(check(closed.clone(), json!({ "a": true })));
    assert!(!check(closed.clone(), json!({})));
    assert!(!check(closed.clone(), json!({ "a": 1 })));
    assert!(!check(closed, json!({ "a": true, "b": 1 })));
    let open = json!({
        "properties": { "a": { "type": "boolean" } },
        "additionalProperties": { "type": "string" },
        "propertyNames": { "pattern": "^[a-z]$" }
    });
    assert!(check(open.clone(), json!({ "a": true, "b": "x" })));
    assert!(!check(open.clone(), json!({ "a": true, "b": 1 })));
    assert!(!check(open, json!({ "a": true, "bb": "x" })));

    let one_of = json!({ "oneOf": [{ "type": "integer" }, { "minimum": 10 }] });
    assert!(check(one_of.clone(), json!(1)));
    assert!(check(one_of.clone(), json!(10.5)));
    assert!(check(one_of.clone(), json!("x")));
    // Both match, so not exactly one.
    assert!(!check(one_of.clone(), json!(11)));
    // Neither matches.
    assert!(!check(one_of, json!(1.5)));
}

// ---------------------------------------------------------------------------
// Example documents: the schema and the loader give the same answer.
// ---------------------------------------------------------------------------

#[test]
fn every_valid_example_is_accepted_by_the_schema_and_by_the_loader() {
    for (name, text) in [
        ("the test service", TEST_SERVICE_MANIFEST),
        ("the example of the addendum", ADDENDUM_MANIFEST),
        ("the storage service", STORAGE_MANIFEST),
    ] {
        let document = parse(text);
        assert!(manifest_schema_accepts(&document), "{name}");
        assert!(ToolManifest::from_json(text).is_ok(), "{name}");
    }
    for (name, text) in [
        ("observe only", OBSERVE_ONLY_POLICY),
        ("ask every action", ASK_EVERY_ACTION_POLICY),
        ("the example of agent-policy.md", DOCUMENTED_POLICY),
        (
            "the shortest",
            r#"{ "schema_version": 1, "mode": "normal" }"#,
        ),
    ] {
        let document = parse(text);
        assert!(policy_schema_accepts(&document), "{name}");
        assert!(UserPolicy::from_json(text).is_ok(), "{name}");
    }
}

#[test]
fn the_invalid_fixtures_are_rejected_by_the_schema_and_by_the_loader() {
    let document = parse(CLASS_4_MANIFEST);
    assert!(!manifest_schema_accepts(&document));
    assert!(ToolManifest::from_json(CLASS_4_MANIFEST).is_err());

    let document = parse(INVALID_RELAXATION_POLICY);
    assert!(!policy_schema_accepts(&document));
    assert!(UserPolicy::from_json(INVALID_RELAXATION_POLICY).is_err());
}

/// A valid manifest with one tool, changed by `change`.
fn manifest_with(change: impl FnOnce(&mut Value)) -> Value {
    let mut document = json!({
        "schema_version": 1,
        "service": "org.tur.Test1",
        "tools": [{
            "name": "test_tool",
            "description": "A tool for tests.",
            "class": 1,
            "method": "Test",
            "input_schema": {
                "type": "object",
                "additionalProperties": false,
                "required": ["id"],
                "properties": {
                    "id": { "type": "string", "minLength": 1, "maxLength": 8 },
                    "mode": { "type": "string", "enum": ["fast", "safe"], "title": "Mode" },
                    "count": { "type": "integer", "minimum": 1, "maximum": 10 },
                    "ratio": { "type": "number", "description": "A ratio." },
                    "force": { "type": "boolean" },
                    "nothing": { "type": "null" },
                    "labels": { "type": "array", "items": { "type": "string" } },
                    "options": {
                        "type": "object",
                        "additionalProperties": false,
                        "properties": { "quiet": { "type": "boolean" } }
                    }
                }
            }
        }]
    });
    change(&mut document);
    document
}

fn tool_with(field: &str, value: Value) -> Value {
    manifest_with(|document| {
        document["tools"][0][field] = value;
    })
}

fn input_schema(schema: Value) -> Value {
    tool_with("input_schema", schema)
}

fn property(schema: Value) -> Value {
    input_schema(json!({
        "type": "object",
        "additionalProperties": false,
        "properties": { "value": schema }
    }))
}

/// A manifest whose tool has one string property with the given name.
fn named_property(name: &str) -> Value {
    let mut properties = Map::new();
    properties.insert(name.to_owned(), json!({ "type": "string" }));
    input_schema(json!({
        "type": "object",
        "additionalProperties": false,
        "properties": properties
    }))
}

fn without(mut document: Value, pointer: &str, field: &str) -> Value {
    document
        .pointer_mut(pointer)
        .and_then(Value::as_object_mut)
        .expect("the pointer names an object")
        .remove(field)
        .expect("the field is present");
    document
}

#[test]
fn the_manifest_schema_and_the_loader_agree_on_every_example() {
    let long = |length: usize| "a".repeat(length);
    let examples: Vec<(&str, Value)> = vec![
        ("unchanged", manifest_with(|_| {})),
        // The manifest itself.
        (
            "no schema_version",
            without(manifest_with(|_| {}), "", "schema_version"),
        ),
        ("no service", without(manifest_with(|_| {}), "", "service")),
        ("no tools", without(manifest_with(|_| {}), "", "tools")),
        (
            "unknown field",
            manifest_with(|d| d["confirm"] = json!(false)),
        ),
        (
            "version 0",
            manifest_with(|d| d["schema_version"] = json!(0)),
        ),
        (
            "version 2",
            manifest_with(|d| d["schema_version"] = json!(2)),
        ),
        (
            "version as text",
            manifest_with(|d| d["schema_version"] = json!("1")),
        ),
        (
            "version null",
            manifest_with(|d| d["schema_version"] = json!(null)),
        ),
        ("service empty", manifest_with(|d| d["service"] = json!(""))),
        (
            "service one element",
            manifest_with(|d| d["service"] = json!("nodots")),
        ),
        (
            "service empty element",
            manifest_with(|d| d["service"] = json!("org..tur")),
        ),
        (
            "service trailing dot",
            manifest_with(|d| d["service"] = json!("org.tur.")),
        ),
        (
            "service leading dot",
            manifest_with(|d| d["service"] = json!(".org.tur")),
        ),
        (
            "service digit first",
            manifest_with(|d| d["service"] = json!("org.9tur")),
        ),
        (
            "service with space",
            manifest_with(|d| d["service"] = json!("org.tur x")),
        ),
        (
            "service with slash",
            manifest_with(|d| d["service"] = json!("org.tur/x")),
        ),
        (
            "service with hyphen",
            manifest_with(|d| d["service"] = json!("org.tur-x")),
        ),
        (
            "service not text",
            manifest_with(|d| d["service"] = json!(1)),
        ),
        (
            "service shortest",
            manifest_with(|d| d["service"] = json!("a.b")),
        ),
        (
            "service underscores",
            manifest_with(|d| d["service"] = json!("_a._b")),
        ),
        (
            "service longest",
            manifest_with(|d| d["service"] = json!(format!("a.{}", long(253)))),
        ),
        (
            "service too long",
            manifest_with(|d| d["service"] = json!(format!("a.{}", long(254)))),
        ),
        ("tools empty", manifest_with(|d| d["tools"] = json!([]))),
        (
            "tools not a list",
            manifest_with(|d| d["tools"] = json!({})),
        ),
        (
            "tool not an object",
            manifest_with(|d| d["tools"] = json!(["x"])),
        ),
        // A tool.
        (
            "no name",
            without(manifest_with(|_| {}), "/tools/0", "name"),
        ),
        (
            "no description",
            without(manifest_with(|_| {}), "/tools/0", "description"),
        ),
        (
            "no class",
            without(manifest_with(|_| {}), "/tools/0", "class"),
        ),
        (
            "no method",
            without(manifest_with(|_| {}), "/tools/0", "method"),
        ),
        (
            "no input_schema",
            without(manifest_with(|_| {}), "/tools/0", "input_schema"),
        ),
        ("unknown tool field", tool_with("confirm", json!(false))),
        ("name empty", tool_with("name", json!(""))),
        ("name upper case", tool_with("name", json!("Storage"))),
        ("name digit first", tool_with("name", json!("9lives"))),
        ("name underscore first", tool_with("name", json!("_hidden"))),
        ("name with hyphen", tool_with("name", json!("has-dash"))),
        ("name with space", tool_with("name", json!("has space"))),
        ("name with dot", tool_with("name", json!("dot.name"))),
        ("name with newline", tool_with("name", json!("new\nline"))),
        ("name not ASCII", tool_with("name", json!("na\u{ef}ve"))),
        ("name not text", tool_with("name", json!(1))),
        ("name shortest", tool_with("name", json!("a"))),
        ("name longest", tool_with("name", json!(long(64)))),
        ("name too long", tool_with("name", json!(long(65)))),
        ("description empty", tool_with("description", json!(""))),
        ("description blank", tool_with("description", json!("   "))),
        (
            "description no-break space",
            tool_with("description", json!("\u{a0}")),
        ),
        (
            "description byte-order mark",
            tool_with("description", json!("\u{feff}")),
        ),
        (
            "description with newline",
            tool_with("description", json!("line\nbreak")),
        ),
        (
            "description with tab",
            tool_with("description", json!("tab\there")),
        ),
        (
            "description with DEL",
            tool_with("description", json!("a\u{7f}")),
        ),
        (
            "description with C1 control",
            tool_with("description", json!("a\u{85}b")),
        ),
        ("description not text", tool_with("description", json!(1))),
        ("description shortest", tool_with("description", json!("x"))),
        ("description padded", tool_with("description", json!(" x "))),
        (
            "description not ASCII",
            tool_with("description", json!("\u{e9} \u{5220}\u{9664}")),
        ),
        (
            "description longest",
            tool_with("description", json!(long(1024))),
        ),
        (
            "description too long",
            tool_with("description", json!(long(1025))),
        ),
        ("class -1", tool_with("class", json!(-1))),
        ("class 0", tool_with("class", json!(0))),
        ("class 2", tool_with("class", json!(2))),
        ("class 3", tool_with("class", json!(3))),
        ("class 4", tool_with("class", json!(4))),
        ("class 5", tool_with("class", json!(5))),
        ("class 256", tool_with("class", json!(256))),
        ("class as text", tool_with("class", json!("1"))),
        ("class with a fraction", tool_with("class", json!(1.5))),
        ("class null", tool_with("class", json!(null))),
        ("class true", tool_with("class", json!(true))),
        ("method empty", tool_with("method", json!(""))),
        ("method digit first", tool_with("method", json!("9Method"))),
        ("method with dot", tool_with("method", json!("Has.Dot"))),
        ("method with space", tool_with("method", json!("has space"))),
        ("method not text", tool_with("method", json!(1))),
        ("method shortest", tool_with("method", json!("M"))),
        ("method longest", tool_with("method", json!(long(255)))),
        ("method too long", tool_with("method", json!(long(256)))),
        // The input schema.
        ("schema true", input_schema(json!(true))),
        ("schema a list", input_schema(json!([]))),
        ("schema empty", input_schema(json!({}))),
        ("schema open", input_schema(json!({ "type": "object" }))),
        (
            "schema open explicitly",
            input_schema(json!({ "type": "object", "additionalProperties": true })),
        ),
        (
            "schema open to a schema",
            input_schema(json!({ "type": "object", "additionalProperties": { "type": "string" } })),
        ),
        (
            "schema not an object schema",
            input_schema(json!({ "type": "string" })),
        ),
        (
            "schema with a list of types",
            input_schema(json!({ "type": ["object", "null"], "additionalProperties": false })),
        ),
        (
            "schema closed",
            input_schema(json!({ "type": "object", "additionalProperties": false })),
        ),
        (
            "schema with every annotation",
            input_schema(json!({
                "type": "object", "additionalProperties": false, "title": "t",
                "description": "d", "properties": {}, "required": []
            })),
        ),
        (
            "schema with an enum of objects",
            input_schema(json!({
                "type": "object", "additionalProperties": false,
                "properties": { "level": { "type": "integer" } },
                "enum": [{ "level": 1 }, { "level": 2 }]
            })),
        ),
        (
            "properties a list",
            input_schema(
                json!({ "type": "object", "additionalProperties": false, "properties": [] }),
            ),
        ),
        (
            "required a string",
            input_schema(
                json!({ "type": "object", "additionalProperties": false, "required": "x" }),
            ),
        ),
        (
            "required a number",
            input_schema(
                json!({ "type": "object", "additionalProperties": false, "required": [1] }),
            ),
        ),
        (
            "unsupported keyword at the top",
            input_schema(
                json!({ "type": "object", "additionalProperties": false, "minProperties": 1 }),
            ),
        ),
        ("property a string", property(json!({ "type": "string" }))),
        ("property without a type", property(json!({}))),
        ("property not a schema", property(json!("string"))),
        (
            "property of an unknown type",
            property(json!({ "type": "text" })),
        ),
        (
            "property with a list of types",
            property(json!({ "type": ["string", "null"] })),
        ),
        (
            "property with anyOf",
            property(json!({ "type": "string", "anyOf": [{ "type": "string" }] })),
        ),
        (
            "property with oneOf",
            property(json!({ "type": "string", "oneOf": [{ "type": "string" }] })),
        ),
        (
            "property with $ref",
            property(json!({ "type": "string", "$ref": "#/x" })),
        ),
        (
            "property with pattern",
            property(json!({ "type": "string", "pattern": "^a$" })),
        ),
        (
            "property with format",
            property(json!({ "type": "string", "format": "uri" })),
        ),
        (
            "property with default",
            property(json!({ "type": "string", "default": "x" })),
        ),
        (
            "property with const",
            property(json!({ "type": "string", "const": "x" })),
        ),
        (
            "string with minimum",
            property(json!({ "type": "string", "minimum": 1 })),
        ),
        (
            "integer with maxLength",
            property(json!({ "type": "integer", "maxLength": 1 })),
        ),
        (
            "boolean with minimum",
            property(json!({ "type": "boolean", "minimum": 0 })),
        ),
        (
            "string with items",
            property(json!({ "type": "string", "items": { "type": "string" } })),
        ),
        (
            "object with items",
            property(json!({
                "type": "object", "additionalProperties": false, "items": { "type": "string" }
            })),
        ),
        ("array without items", property(json!({ "type": "array" }))),
        (
            "array with minItems",
            property(json!({
                "type": "array", "items": { "type": "string" }, "minItems": 1
            })),
        ),
        (
            "array of arrays",
            property(json!({
                "type": "array", "items": { "type": "array", "items": { "type": "null" } }
            })),
        ),
        (
            "array of open objects",
            property(json!({ "type": "array", "items": { "type": "object" } })),
        ),
        (
            "array of closed objects",
            property(json!({
                "type": "array", "items": { "type": "object", "additionalProperties": false }
            })),
        ),
        ("nested open object", property(json!({ "type": "object" }))),
        (
            "nested closed object",
            property(json!({ "type": "object", "additionalProperties": false })),
        ),
        (
            "minLength negative",
            property(json!({ "type": "string", "minLength": -1 })),
        ),
        (
            "minLength as text",
            property(json!({ "type": "string", "minLength": "1" })),
        ),
        (
            "maxLength with a fraction",
            property(json!({ "type": "string", "maxLength": 1.5 })),
        ),
        (
            "minimum as text",
            property(json!({ "type": "integer", "minimum": "1" })),
        ),
        (
            "minimum with a fraction",
            property(json!({ "type": "number", "minimum": 0.5 })),
        ),
        (
            "enum empty",
            property(json!({ "type": "string", "enum": [] })),
        ),
        (
            "enum not a list",
            property(json!({ "type": "string", "enum": "a" })),
        ),
        (
            "enum of anything",
            property(json!({ "type": "string", "enum": ["a", 1, null, [], {}] })),
        ),
        (
            "title not text",
            property(json!({ "type": "string", "title": 1 })),
        ),
        (
            "description not text",
            property(json!({ "type": "string", "description": 1 })),
        ),
        (
            "null with an enum",
            property(json!({ "type": "null", "enum": [null] })),
        ),
        ("property name empty", named_property("")),
        ("property name with space", named_property("has space")),
        ("property name digit first", named_property("9x")),
        ("property name with hyphen", named_property("pass-word")),
        ("property name not ASCII", named_property("na\u{ef}ve")),
        ("property name shortest", named_property("_")),
        ("property name longest", named_property(&long(64))),
        ("property name too long", named_property(&long(65))),
    ];

    let mut accepted = 0_usize;
    let mut rejected = 0_usize;
    for (name, document) in &examples {
        let by_schema = manifest_schema_accepts(document);
        let by_loader = manifest_loads(document);
        assert_eq!(
            by_schema, by_loader,
            "{name}: the schema says {by_schema}, the loader says {by_loader}\n{document}"
        );
        if by_loader {
            accepted += 1;
        } else {
            rejected += 1;
        }
    }
    // The examples are not all of one kind.
    assert!(accepted >= 20, "only {accepted} examples are valid");
    assert!(rejected >= 80, "only {rejected} examples are invalid");
}

/// A valid policy, changed by `change`.
fn policy_with(change: impl FnOnce(&mut Value)) -> Value {
    let mut document = json!({
        "schema_version": 1,
        "mode": "normal",
        "excluded_applications": ["org.example.Vault"],
        "sensitive_applications": ["org.example.Bank"],
        "relaxations": [{ "application": "org.example.Mail", "class": 2 }]
    });
    change(&mut document);
    document
}

#[test]
fn the_policy_schema_and_the_loader_agree_on_every_example() {
    let long = |length: usize| "a".repeat(length);
    let applications: Vec<(&str, Value)> = vec![
        ("empty", json!("")),
        ("leading space", json!(" a")),
        ("trailing space", json!("a ")),
        ("only a space", json!(" ")),
        ("tab", json!("a\tb")),
        ("newline", json!("a\nb")),
        ("DEL", json!("a\u{7f}")),
        ("zero-width space", json!("org.example.Vault\u{200b}")),
        ("byte-order mark", json!("\u{feff}org.example.Vault")),
        ("no-break space", json!("org.example.Vault\u{a0}")),
        ("Cyrillic letter", json!("org.ex\u{430}mple.Vault")),
        ("accented letter", json!("org.example.V\u{e4}ult")),
        ("direction override", json!("org.example\u{202e}.Vault")),
        ("too long", json!(long(256))),
        ("not text", json!(1)),
        ("null", json!(null)),
        ("one character", json!("a")),
        ("space inside", json!("VirtualBox Manager")),
        ("punctuation", json!("!~")),
        ("longest", json!(long(255))),
    ];

    let mut examples: Vec<(String, Value)> = vec![
        ("unchanged".to_owned(), policy_with(|_| {})),
        (
            "only the required fields".to_owned(),
            json!({ "schema_version": 1, "mode": "normal" }),
        ),
        (
            "no schema_version".to_owned(),
            without(policy_with(|_| {}), "", "schema_version"),
        ),
        (
            "no mode".to_owned(),
            without(policy_with(|_| {}), "", "mode"),
        ),
        (
            "unknown field".to_owned(),
            policy_with(|d| d["allow_everything"] = json!(true)),
        ),
        (
            "version 0".to_owned(),
            policy_with(|d| d["schema_version"] = json!(0)),
        ),
        (
            "version 2".to_owned(),
            policy_with(|d| d["schema_version"] = json!(2)),
        ),
        (
            "version as text".to_owned(),
            policy_with(|d| d["schema_version"] = json!("1")),
        ),
        (
            "mode unknown".to_owned(),
            policy_with(|d| d["mode"] = json!("relaxed")),
        ),
        (
            "mode in upper case".to_owned(),
            policy_with(|d| d["mode"] = json!("Normal")),
        ),
        (
            "mode empty".to_owned(),
            policy_with(|d| d["mode"] = json!("")),
        ),
        (
            "mode null".to_owned(),
            policy_with(|d| d["mode"] = json!(null)),
        ),
        (
            "mode a number".to_owned(),
            policy_with(|d| d["mode"] = json!(1)),
        ),
        (
            "excluded not a list".to_owned(),
            policy_with(|d| d["excluded_applications"] = json!("x")),
        ),
        (
            "sensitive not a list".to_owned(),
            policy_with(|d| d["sensitive_applications"] = json!({})),
        ),
        (
            "empty lists".to_owned(),
            policy_with(|d| {
                d["excluded_applications"] = json!([]);
                d["sensitive_applications"] = json!([]);
                d["relaxations"] = json!([]);
            }),
        ),
        (
            "relaxations not a list".to_owned(),
            policy_with(|d| d["relaxations"] = json!({})),
        ),
        (
            "relaxation not an object".to_owned(),
            policy_with(|d| d["relaxations"] = json!(["x"])),
        ),
        (
            "relaxation empty".to_owned(),
            policy_with(|d| d["relaxations"] = json!([{}])),
        ),
        (
            "relaxation without a class".to_owned(),
            policy_with(|d| d["relaxations"] = json!([{ "application": "org.example.Mail" }])),
        ),
        (
            "relaxation without an application".to_owned(),
            policy_with(|d| d["relaxations"] = json!([{ "class": 2 }])),
        ),
        (
            "relaxation with an unknown field".to_owned(),
            policy_with(|d| d["relaxations"][0]["until"] = json!("tomorrow")),
        ),
    ];
    for mode in Mode::ALL {
        examples.push((
            format!("mode {}", mode.as_str()),
            policy_with(|d| d["mode"] = json!(mode.as_str())),
        ));
    }
    for class in [
        json!(-1),
        json!(0),
        json!(1),
        json!(2),
        json!(3),
        json!(4),
        json!("2"),
        json!(null),
        json!(2.5),
    ] {
        examples.push((
            format!("relaxation of class {class}"),
            policy_with(|d| d["relaxations"][0]["class"] = class.clone()),
        ));
    }
    for (name, application) in &applications {
        examples.push((
            format!("excluded application: {name}"),
            policy_with(|d| d["excluded_applications"] = json!([application])),
        ));
        examples.push((
            format!("sensitive application: {name}"),
            policy_with(|d| d["sensitive_applications"] = json!(["org.example.Bank", application])),
        ));
        examples.push((
            format!("relaxed application: {name}"),
            policy_with(|d| d["relaxations"][0]["application"] = application.clone()),
        ));
    }

    let mut accepted = 0_usize;
    let mut rejected = 0_usize;
    for (name, document) in &examples {
        let by_schema = policy_schema_accepts(document);
        let by_loader = policy_loads(document);
        assert_eq!(
            by_schema, by_loader,
            "{name}: the schema says {by_schema}, the loader says {by_loader}\n{document}"
        );
        if by_loader {
            accepted += 1;
        } else {
            rejected += 1;
        }
    }
    assert!(accepted >= 15, "only {accepted} examples are valid");
    assert!(rejected >= 60, "only {rejected} examples are invalid");
}

/// The rules of the loaders that the schemas cannot express. Each document is
/// valid against its schema and rejected by its loader. The schema
/// descriptions name these rules; nothing else may differ.
#[test]
fn what_the_schemas_cannot_express_is_listed_and_nothing_else() {
    let tool = |name: &str, class: u8, method: &str| {
        json!({
            "name": name,
            "description": "A tool for tests.",
            "class": class,
            "method": method,
            "input_schema": { "type": "object", "additionalProperties": false }
        })
    };
    let manifest =
        |tools: Value| json!({ "schema_version": 1, "service": "org.tur.Test1", "tools": tools });

    let manifests: Vec<(&str, Value)> = vec![
        (
            "a tool name declared twice",
            manifest(json!([tool("a", 1, "First"), tool("a", 1, "Second")])),
        ),
        (
            "a method declared with two classes",
            manifest(json!([tool("a", 3, "Format"), tool("b", 1, "Format")])),
        ),
        (
            "a required entry that names no declared property",
            input_schema(json!({
                "type": "object", "additionalProperties": false, "required": ["missing"]
            })),
        ),
        // JSON Schema counts 1.0 as an integer; the loader wants an integer
        // written as one.
        (
            "a class written with a fraction",
            tool_with("class", json!(1.0)),
        ),
        (
            "a schema version written with a fraction",
            manifest_with(|d| d["schema_version"] = json!(1.0)),
        ),
        (
            "a length written with a fraction",
            property(json!({ "type": "string", "maxLength": 8.0 })),
        ),
    ];
    for (name, document) in &manifests {
        assert!(manifest_schema_accepts(document), "{name}: {document}");
        assert!(!manifest_loads(document), "{name}: {document}");
    }

    let policies: Vec<(&str, Value)> = vec![
        (
            "an application relaxed twice",
            policy_with(|d| {
                d["relaxations"] = json!([
                    { "application": "org.example.Mail", "class": 2 },
                    { "application": "org.example.Mail", "class": 2 }
                ]);
            }),
        ),
        (
            "an application relaxed twice in another letter case",
            policy_with(|d| {
                d["relaxations"] = json!([
                    { "application": "org.example.Mail", "class": 2 },
                    { "application": "ORG.EXAMPLE.MAIL.desktop", "class": 2 }
                ]);
            }),
        ),
        (
            "a class written with a fraction",
            policy_with(|d| d["relaxations"][0]["class"] = json!(2.0)),
        ),
        (
            "a schema version written with a fraction",
            policy_with(|d| d["schema_version"] = json!(1.0)),
        ),
    ];
    for (name, document) in &policies {
        assert!(policy_schema_accepts(document), "{name}: {document}");
        assert!(!policy_loads(document), "{name}: {document}");
    }

    // A repeated key is not a property of the document a schema sees: a
    // parser has already kept one of the two values. The loaders read the
    // text and refuse it.
    let repeated_manifest =
        TEST_SERVICE_MANIFEST.replacen("\"class\": 3,", "\"class\": 3, \"class\": 0,", 1);
    assert_ne!(repeated_manifest, TEST_SERVICE_MANIFEST);
    assert!(manifest_schema_accepts(&parse(&repeated_manifest)));
    assert!(ToolManifest::from_json(&repeated_manifest).is_err());

    let repeated_schema = TEST_SERVICE_MANIFEST.replacen(
        "{ \"type\": \"string\", \"maxLength\": 64 }",
        "{ \"type\": \"string\", \"maxLength\": 64, \"maxLength\": 64000 }",
        1,
    );
    assert_ne!(repeated_schema, TEST_SERVICE_MANIFEST);
    assert!(manifest_schema_accepts(&parse(&repeated_schema)));
    assert!(ToolManifest::from_json(&repeated_schema).is_err());

    let repeated_policy = r#"{ "schema_version": 1, "mode": "observe_only", "mode": "normal" }"#;
    assert!(policy_schema_accepts(&parse(repeated_policy)));
    assert!(UserPolicy::from_json(repeated_policy).is_err());
}

// ---------------------------------------------------------------------------
// Constants stated on both sides.
// ---------------------------------------------------------------------------

#[test]
fn the_manifest_schema_agrees_with_the_loader() {
    let schema = parse(MANIFEST_SCHEMA);
    assert_eq!(
        schema["$id"],
        "https://github.com/giorgiogiudice/TUR/schemas/agent-tool-manifest.schema.json"
    );
    assert_eq!(schema["additionalProperties"], false);
    assert_eq!(
        schema["properties"]["schema_version"]["const"],
        MANIFEST_SCHEMA_VERSION
    );

    let tool = &schema["$defs"]["tool"];
    assert_eq!(tool["additionalProperties"], false);
    assert_eq!(tool["properties"]["class"]["minimum"], 0);
    // Class 4 cannot be declared.
    assert_eq!(tool["properties"]["class"]["maximum"], 3);
    assert_eq!(
        tool["properties"]["name"]["pattern"],
        "^[a-z][a-z0-9_]{0,63}$"
    );
    assert_eq!(
        tool["properties"]["input_schema"]["$ref"],
        "#/$defs/object_schema"
    );

    // The input schema must be closed.
    let object_schema = &schema["$defs"]["object_schema"];
    assert_eq!(
        object_schema["properties"]["additionalProperties"]["const"],
        false
    );
    assert!(
        object_schema["required"]
            .as_array()
            .expect("required is a list")
            .contains(&Value::from("additionalProperties"))
    );

    // The value types of the schema subset are the ones the loader accepts.
    let mut types = BTreeSet::new();
    for name in [
        "object_schema",
        "array_schema",
        "string_schema",
        "number_schema",
        "plain_schema",
    ] {
        let definition = &schema["$defs"][name];
        // No keyword outside the subset.
        assert_eq!(definition["additionalProperties"], false, "{name}");
        let kind = &definition["properties"]["type"];
        if let Some(single) = kind["const"].as_str() {
            types.insert(single.to_owned());
        }
        if let Some(options) = kind["enum"].as_array() {
            for option in options {
                types.insert(option.as_str().expect("a type name").to_owned());
            }
        }
    }
    let accepted: BTreeSet<String> = SCHEMA_TYPES.iter().map(|kind| (*kind).to_owned()).collect();
    assert_eq!(types, accepted);
}

#[test]
fn the_policy_schema_agrees_with_the_loader() {
    let schema = parse(POLICY_SCHEMA);
    assert_eq!(
        schema["$id"],
        "https://github.com/giorgiogiudice/TUR/schemas/agent-policy.schema.json"
    );
    assert_eq!(schema["additionalProperties"], false);
    assert_eq!(
        schema["properties"]["schema_version"]["const"],
        POLICY_SCHEMA_VERSION
    );

    let modes: Vec<&str> = schema["properties"]["mode"]["enum"]
        .as_array()
        .expect("mode is an enum")
        .iter()
        .map(|mode| mode.as_str().expect("a mode name"))
        .collect();
    let known: Vec<&str> = Mode::ALL.iter().map(|mode| mode.as_str()).collect();
    assert_eq!(modes, known);

    // Only class 2 can be relaxed.
    let relaxation = &schema["properties"]["relaxations"]["items"];
    assert_eq!(relaxation["additionalProperties"], false);
    let classes: Vec<u64> = relaxation["properties"]["class"]["enum"]
        .as_array()
        .expect("class is an enum")
        .iter()
        .map(|class| class.as_u64().expect("a class number"))
        .collect();
    let levels: Vec<u64> = RelaxationLevel::ALL
        .iter()
        .map(|level| u64::from(level.class_number()))
        .collect();
    assert_eq!(classes, levels);
    assert_eq!(classes, [2]);
}

#[test]
fn the_test_fixtures_load() {
    let manifest =
        ToolManifest::from_json(TEST_SERVICE_MANIFEST).expect("the test service manifest is valid");
    let classes: Vec<u8> = manifest
        .tools()
        .iter()
        .map(|tool| tool.class().as_u8())
        .collect();
    assert_eq!(classes, [0, 1, 2, 3]);

    assert!(ToolManifest::from_json(CLASS_4_MANIFEST).is_err());

    let observe_only =
        UserPolicy::from_json(OBSERVE_ONLY_POLICY).expect("the observe-only policy is valid");
    assert_eq!(observe_only.mode, Mode::ObserveOnly);
    let ask_every = UserPolicy::from_json(ASK_EVERY_ACTION_POLICY)
        .expect("the ask-every-action policy is valid");
    assert_eq!(ask_every.mode, Mode::AskEveryAction);
    assert!(UserPolicy::from_json(INVALID_RELAXATION_POLICY).is_err());
}
