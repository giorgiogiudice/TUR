// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! A small JSON reader and writer.
//!
//! This program has no third-party dependencies (see `Cargo.toml`), so it
//! carries the little JSON it needs: reading `bootc status --format=json` and
//! its own state files, and writing those state files. Objects keep the order
//! of their members, so a file written twice from the same data is identical.

use std::fmt::Write as _;

/// A JSON value.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    Array(Vec<Value>),
    Object(Vec<(String, Value)>),
}

/// How deep arrays and objects may nest before the reader gives up.
const MAX_DEPTH: usize = 64;

impl Value {
    /// The member of an object, if this is an object that has it.
    pub(crate) fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Object(members) => members
                .iter()
                .find(|(name, _)| name == key)
                .map(|(_, value)| value),
            _ => None,
        }
    }

    /// Follows a chain of object members.
    pub(crate) fn path(&self, keys: &[&str]) -> Option<&Value> {
        let mut current = self;
        for key in keys {
            current = current.get(key)?;
        }
        Some(current)
    }

    pub(crate) fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(text) => Some(text),
            _ => None,
        }
    }

    pub(crate) fn as_i64(&self) -> Option<i64> {
        match self {
            Value::Int(number) => Some(*number),
            _ => None,
        }
    }

    pub(crate) fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(flag) => Some(*flag),
            _ => None,
        }
    }

    pub(crate) fn as_array(&self) -> Option<&[Value]> {
        match self {
            Value::Array(items) => Some(items),
            _ => None,
        }
    }

    pub(crate) fn is_null(&self) -> bool {
        matches!(self, Value::Null)
    }

    /// A string member, or the empty string.
    pub(crate) fn str_or_empty(&self, key: &str) -> String {
        self.get(key)
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned()
    }

    /// A string value, or null for `None`.
    pub(crate) fn opt_str(text: Option<&str>) -> Value {
        match text {
            Some(text) => Value::Str(text.to_owned()),
            None => Value::Null,
        }
    }

    /// An object from its members, in the order given.
    pub(crate) fn object(members: Vec<(&str, Value)>) -> Value {
        Value::Object(
            members
                .into_iter()
                .map(|(name, value)| (name.to_owned(), value))
                .collect(),
        )
    }

    /// Text on one line.
    pub(crate) fn to_compact(&self) -> String {
        let mut out = String::new();
        write_value(&mut out, self, None, 0);
        out
    }

    /// Text indented by two spaces, ending in a newline.
    pub(crate) fn to_pretty(&self) -> String {
        let mut out = String::new();
        write_value(&mut out, self, Some(2), 0);
        out.push('\n');
        out
    }
}

impl From<&str> for Value {
    fn from(text: &str) -> Value {
        Value::Str(text.to_owned())
    }
}

impl From<String> for Value {
    fn from(text: String) -> Value {
        Value::Str(text)
    }
}

impl From<i64> for Value {
    fn from(number: i64) -> Value {
        Value::Int(number)
    }
}

impl From<bool> for Value {
    fn from(flag: bool) -> Value {
        Value::Bool(flag)
    }
}

fn write_string(out: &mut String, text: &str) {
    out.push('"');
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if (ch as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", ch as u32);
            }
            ch => out.push(ch),
        }
    }
    out.push('"');
}

fn newline(out: &mut String, indent: Option<usize>, level: usize) {
    if let Some(width) = indent {
        out.push('\n');
        for _ in 0..(width * level) {
            out.push(' ');
        }
    }
}

fn write_value(out: &mut String, value: &Value, indent: Option<usize>, level: usize) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(true) => out.push_str("true"),
        Value::Bool(false) => out.push_str("false"),
        Value::Int(number) => {
            let _ = write!(out, "{number}");
        }
        Value::Float(number) => {
            if number.is_finite() {
                let _ = write!(out, "{number}");
            } else {
                out.push_str("null");
            }
        }
        Value::Str(text) => write_string(out, text),
        Value::Array(items) => {
            if items.is_empty() {
                out.push_str("[]");
                return;
            }
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                newline(out, indent, level + 1);
                write_value(out, item, indent, level + 1);
            }
            newline(out, indent, level);
            out.push(']');
        }
        Value::Object(members) => {
            if members.is_empty() {
                out.push_str("{}");
                return;
            }
            out.push('{');
            for (index, (name, member)) in members.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                newline(out, indent, level + 1);
                write_string(out, name);
                out.push(':');
                if indent.is_some() {
                    out.push(' ');
                }
                write_value(out, member, indent, level + 1);
            }
            newline(out, indent, level);
            out.push('}');
        }
    }
}

/// Reads one JSON document. Anything but white space after it is an error.
pub(crate) fn parse(text: &str) -> Result<Value, String> {
    let mut reader = Reader {
        bytes: text.as_bytes(),
        position: 0,
    };
    reader.skip_space();
    let value = reader.value(0)?;
    reader.skip_space();
    if reader.position != reader.bytes.len() {
        return Err(format!("unexpected text at byte {}", reader.position));
    }
    Ok(value)
}

struct Reader<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl Reader<'_> {
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.position).copied()
    }

    fn skip_space(&mut self) {
        while let Some(byte) = self.peek() {
            if byte == b' ' || byte == b'\n' || byte == b'\r' || byte == b'\t' {
                self.position += 1;
            } else {
                break;
            }
        }
    }

    fn fail<T>(&self, what: &str) -> Result<T, String> {
        Err(format!("{what} at byte {}", self.position))
    }

    fn literal(&mut self, word: &str, value: Value) -> Result<Value, String> {
        let end = self.position + word.len();
        if self.bytes.len() >= end && &self.bytes[self.position..end] == word.as_bytes() {
            self.position = end;
            Ok(value)
        } else {
            self.fail("unknown literal")
        }
    }

    fn value(&mut self, depth: usize) -> Result<Value, String> {
        if depth > MAX_DEPTH {
            return self.fail("nesting too deep");
        }
        match self.peek() {
            None => self.fail("unexpected end"),
            Some(b'n') => self.literal("null", Value::Null),
            Some(b't') => self.literal("true", Value::Bool(true)),
            Some(b'f') => self.literal("false", Value::Bool(false)),
            Some(b'"') => Ok(Value::Str(self.string()?)),
            Some(b'[') => self.array(depth),
            Some(b'{') => self.object(depth),
            Some(byte) if byte == b'-' || byte.is_ascii_digit() => self.number(),
            Some(_) => self.fail("unexpected character"),
        }
    }

    fn array(&mut self, depth: usize) -> Result<Value, String> {
        self.position += 1;
        let mut items = Vec::new();
        self.skip_space();
        if self.peek() == Some(b']') {
            self.position += 1;
            return Ok(Value::Array(items));
        }
        loop {
            self.skip_space();
            items.push(self.value(depth + 1)?);
            self.skip_space();
            match self.peek() {
                Some(b',') => self.position += 1,
                Some(b']') => {
                    self.position += 1;
                    return Ok(Value::Array(items));
                }
                _ => return self.fail("expected ',' or ']'"),
            }
        }
    }

    fn object(&mut self, depth: usize) -> Result<Value, String> {
        self.position += 1;
        let mut members = Vec::new();
        self.skip_space();
        if self.peek() == Some(b'}') {
            self.position += 1;
            return Ok(Value::Object(members));
        }
        loop {
            self.skip_space();
            if self.peek() != Some(b'"') {
                return self.fail("expected a member name");
            }
            let name = self.string()?;
            self.skip_space();
            if self.peek() != Some(b':') {
                return self.fail("expected ':'");
            }
            self.position += 1;
            self.skip_space();
            let member = self.value(depth + 1)?;
            members.push((name, member));
            self.skip_space();
            match self.peek() {
                Some(b',') => self.position += 1,
                Some(b'}') => {
                    self.position += 1;
                    return Ok(Value::Object(members));
                }
                _ => return self.fail("expected ',' or '}'"),
            }
        }
    }

    fn number(&mut self) -> Result<Value, String> {
        let start = self.position;
        let mut integer = true;
        while let Some(byte) = self.peek() {
            if byte.is_ascii_digit() || byte == b'-' || byte == b'+' {
                self.position += 1;
            } else if byte == b'.' || byte == b'e' || byte == b'E' {
                integer = false;
                self.position += 1;
            } else {
                break;
            }
        }
        let text = String::from_utf8_lossy(&self.bytes[start..self.position]).into_owned();
        if integer && let Ok(number) = text.parse::<i64>() {
            return Ok(Value::Int(number));
        }
        match text.parse::<f64>() {
            Ok(number) => Ok(Value::Float(number)),
            Err(_) => {
                self.position = start;
                self.fail("malformed number")
            }
        }
    }

    fn hex4(&mut self) -> Result<u32, String> {
        let end = self.position + 4;
        if end > self.bytes.len() {
            return self.fail("truncated \\u escape");
        }
        let text = String::from_utf8_lossy(&self.bytes[self.position..end]).into_owned();
        match u32::from_str_radix(&text, 16) {
            Ok(code) => {
                self.position = end;
                Ok(code)
            }
            Err(_) => self.fail("malformed \\u escape"),
        }
    }

    fn string(&mut self) -> Result<String, String> {
        self.position += 1;
        let mut out: Vec<u8> = Vec::new();
        loop {
            let Some(byte) = self.peek() else {
                return self.fail("unterminated string");
            };
            self.position += 1;
            match byte {
                b'"' => break,
                b'\\' => {
                    let Some(escape) = self.peek() else {
                        return self.fail("unterminated escape");
                    };
                    self.position += 1;
                    let ch = match escape {
                        b'"' => '"',
                        b'\\' => '\\',
                        b'/' => '/',
                        b'b' => '\u{8}',
                        b'f' => '\u{c}',
                        b'n' => '\n',
                        b'r' => '\r',
                        b't' => '\t',
                        b'u' => {
                            let mut code = self.hex4()?;
                            if (0xD800..0xDC00).contains(&code) {
                                // A surrogate pair: the low half must follow.
                                if self.peek() == Some(b'\\')
                                    && self.bytes.get(self.position + 1) == Some(&b'u')
                                {
                                    self.position += 2;
                                    let low = self.hex4()?;
                                    if (0xDC00..0xE000).contains(&low) {
                                        code = 0x10000 + ((code - 0xD800) << 10) + (low - 0xDC00);
                                    } else {
                                        code = 0xFFFD;
                                    }
                                } else {
                                    code = 0xFFFD;
                                }
                            }
                            char::from_u32(code).unwrap_or('\u{FFFD}')
                        }
                        _ => return self.fail("unknown escape"),
                    };
                    let mut buffer = [0u8; 4];
                    out.extend_from_slice(ch.encode_utf8(&mut buffer).as_bytes());
                }
                byte => out.push(byte),
            }
        }
        Ok(String::from_utf8_lossy(&out).into_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_what_bootc_status_prints() {
        let text = r#"{
            "apiVersion": "org.containers.bootc/v1",
            "status": {
                "booted": {
                    "downloadOnly": false,
                    "image": {"image": {"image": "localhost/tur:ci", "transport": "registry"},
                              "imageDigest": "sha256:afe8", "version": "0.1.0-dev"},
                    "ostree": {"checksum": "24d7", "deploySerial": 0, "stateroot": "default"}
                },
                "rollback": null,
                "rollbackQueued": false,
                "staged": null
            }
        }"#;
        let value = parse(text).unwrap();
        let booted = value.path(&["status", "booted"]).unwrap();
        assert_eq!(
            booted.path(&["ostree", "checksum"]).unwrap().as_str(),
            Some("24d7")
        );
        assert_eq!(
            booted.path(&["ostree", "deploySerial"]).unwrap().as_i64(),
            Some(0)
        );
        assert_eq!(booted.get("downloadOnly").unwrap().as_bool(), Some(false));
        assert!(value.path(&["status", "staged"]).unwrap().is_null());
        assert!(value.path(&["status", "missing"]).is_none());
    }

    #[test]
    fn what_is_written_is_read_back_the_same() {
        let value = Value::object(vec![
            ("schema_version", Value::Int(1)),
            ("text", Value::from("line one\nline \"two\"\ttab \\ back")),
            ("control", Value::from("\u{1}")),
            ("unicode", Value::from("Größe 文 🙂")),
            ("flag", Value::Bool(true)),
            ("nothing", Value::Null),
            ("negative", Value::Int(-1)),
            ("list", Value::Array(vec![Value::Int(1), Value::from("two"), Value::Array(vec![])])),
            ("empty", Value::Object(vec![])),
        ]);
        assert_eq!(parse(&value.to_compact()).unwrap(), value);
        assert_eq!(parse(&value.to_pretty()).unwrap(), value);
        assert!(value.to_pretty().ends_with("}\n"));
        assert!(!value.to_compact().contains('\n'));
    }

    #[test]
    fn escapes_are_understood() {
        let value = parse(r#"["a\u0041\n", "\ud83d\ude42", "\/"]"#).unwrap();
        let items = value.as_array().unwrap();
        assert_eq!(items[0].as_str(), Some("aA\n"));
        assert_eq!(items[1].as_str(), Some("🙂"));
        assert_eq!(items[2].as_str(), Some("/"));
    }

    #[test]
    fn numbers_keep_their_kind() {
        assert_eq!(parse("42").unwrap(), Value::Int(42));
        assert_eq!(parse("-7").unwrap(), Value::Int(-7));
        assert_eq!(parse("1.5").unwrap(), Value::Float(1.5));
        assert_eq!(parse("1e3").unwrap(), Value::Float(1000.0));
    }

    #[test]
    fn malformed_documents_are_refused() {
        for text in [
            "",
            "{",
            "[1,",
            "{\"a\" 1}",
            "{\"a\":1,}",
            "[1 2]",
            "nul",
            "\"open",
            "1 2",
            "{\"a\":\"\\x\"}",
            "--",
        ] {
            assert!(parse(text).is_err(), "{text:?} was accepted");
        }
    }

    #[test]
    fn deep_nesting_is_refused_not_overflowed() {
        let text = "[".repeat(10_000);
        assert!(parse(&text).is_err());
    }
}
