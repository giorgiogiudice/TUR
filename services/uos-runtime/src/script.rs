// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

//! Script detection from the interpreter (`#!`) line.

use serde::Serialize;

use crate::limits::MAX_SHEBANG_LEN;

/// What was read from a script's interpreter line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ScriptInfo {
    /// The interpreter path, for example `/bin/sh` or `/usr/bin/env`.
    pub interpreter: String,
    /// The rest of the line, which the kernel passes as one argument. For
    /// `#!/usr/bin/env python3` this is `python3`.
    pub argument: Option<String>,
}

/// Reads the interpreter line from the start of a file.
///
/// Returns `None` unless the file starts with `#!` followed by an absolute
/// path made of printable characters.
pub(crate) fn parse(head: &[u8]) -> Option<ScriptInfo> {
    let rest = head.strip_prefix(b"#!")?;
    let window = rest.get(..rest.len().min(MAX_SHEBANG_LEN))?;
    let line_end = window
        .iter()
        .position(|&byte| byte == b'\n')
        .unwrap_or(window.len());
    let line = std::str::from_utf8(window.get(..line_end)?).ok()?.trim();
    if line.chars().any(|c| c.is_control() && c != '\t') {
        return None;
    }
    let (interpreter, argument) = match line.split_once([' ', '\t']) {
        Some((interpreter, argument)) => (interpreter, argument.trim()),
        None => (line, ""),
    };
    if !interpreter.starts_with('/') {
        return None;
    }
    Some(ScriptInfo {
        interpreter: interpreter.to_owned(),
        argument: (!argument.is_empty()).then(|| argument.to_owned()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_and_env_interpreter_lines() {
        let script = parse(b"#!/bin/sh\necho hello\n").unwrap();
        assert_eq!(script.interpreter, "/bin/sh");
        assert_eq!(script.argument, None);

        let script = parse(b"#! /usr/bin/env python3\r\nprint(1)\n").unwrap();
        assert_eq!(script.interpreter, "/usr/bin/env");
        assert_eq!(script.argument.as_deref(), Some("python3"));

        let script = parse(b"#!/usr/bin/env -S deno run").unwrap();
        assert_eq!(script.argument.as_deref(), Some("-S deno run"));
    }

    #[test]
    fn lines_that_are_not_interpreter_lines() {
        assert_eq!(parse(b""), None);
        assert_eq!(parse(b"#"), None);
        assert_eq!(parse(b"#!"), None);
        assert_eq!(parse(b"#!\n/bin/sh"), None);
        assert_eq!(parse(b"#!python3\n"), None);
        assert_eq!(parse(b"# comment\n"), None);
        assert_eq!(parse(b"#!/bin/\xff\xfe\n"), None);
        assert_eq!(parse(b"#!/bin/sh\x1b[31m\n"), None);
    }

    #[test]
    fn only_the_first_bytes_are_examined() {
        let mut long = b"#!/".to_vec();
        long.extend(std::iter::repeat_n(b'a', 10_000));
        let script = parse(&long).unwrap();
        assert_eq!(script.interpreter.len(), MAX_SHEBANG_LEN);
    }
}
