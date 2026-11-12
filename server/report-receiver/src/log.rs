// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The receiver's log: one line per event, `level=... event=... key=value`.
//!
//! Values are restricted by construction to identifiers and numbers: every
//! character outside `A-Z a-z 0-9 . _ : / + -` becomes `_`, and a value is
//! cut at 64 characters. Free text from a report, an address or a token
//! therefore cannot be written whole, and the call sites pass only report
//! identifiers, signatures, codes and counts. Client addresses and tokens
//! are never passed at all; the tests scan the log for them.

use std::io::{self, Write};
use std::sync::{Arc, Mutex};

/// Where log lines go.
#[derive(Clone)]
pub struct Log {
    sink: Arc<Mutex<Box<dyn Write + Send>>>,
}

impl std::fmt::Debug for Log {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Log")
    }
}

/// A sink that keeps what is written, for tests.
struct Memory(Arc<Mutex<Vec<u8>>>);

impl Write for Memory {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        self.0
            .lock()
            .map_err(|_| io::Error::other("poisoned"))?
            .extend_from_slice(data);
        Ok(data.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// The longest value written.
const MAX_VALUE: usize = 64;

/// A value made safe for the log.
pub fn safe(value: &str) -> String {
    value
        .chars()
        .take(MAX_VALUE)
        .map(|c| {
            if c.is_ascii_alphanumeric() || "._:/+-".contains(c) {
                c
            } else {
                '_'
            }
        })
        .collect()
}

impl Log {
    /// A log on standard error, which systemd and the container runtime
    /// collect.
    pub fn stderr() -> Log {
        Log {
            sink: Arc::new(Mutex::new(Box::new(io::stderr()))),
        }
    }

    /// A log kept in memory, and the memory, for tests.
    pub fn memory() -> (Log, Arc<Mutex<Vec<u8>>>) {
        let memory = Arc::new(Mutex::new(Vec::new()));
        let log = Log {
            sink: Arc::new(Mutex::new(Box::new(Memory(Arc::clone(&memory))))),
        };
        (log, memory)
    }

    fn write(&self, level: &str, event: &str, fields: &[(&str, &str)]) {
        let mut line = format!("level={level} event={}", safe(event));
        for (key, value) in fields {
            line.push(' ');
            line.push_str(&safe(key));
            line.push('=');
            line.push_str(&safe(value));
        }
        line.push('\n');
        if let Ok(mut sink) = self.sink.lock() {
            // A log that cannot be written must not stop the receiver.
            let _ = sink.write_all(line.as_bytes());
            let _ = sink.flush();
        }
    }

    /// Records something that happened as expected.
    pub fn info(&self, event: &str, fields: &[(&str, &str)]) {
        self.write("info", event, fields);
    }

    /// Records something that needs attention.
    pub fn warn(&self, event: &str, fields: &[(&str, &str)]) {
        self.write("warn", event, fields);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_cannot_carry_free_text() {
        let (log, memory) = Log::memory();
        log.info(
            "received",
            &[
                ("report", "R-0123456789abcdef"),
                ("note", "alice@example.org lives at /home/alice and said \"hi\"\nlevel=error"),
            ],
        );
        let text = String::from_utf8(memory.lock().expect("lock").clone()).expect("text");
        assert_eq!(text.lines().count(), 1);
        assert!(text.starts_with("level=info event=received report=R-0123456789abcdef note="));
        assert!(!text.contains('@'));
        assert!(!text.contains(' ') || text.matches(' ').count() == 3);
        assert!(!text.contains("level=error"));
        assert_eq!(safe(&"x".repeat(200)).len(), MAX_VALUE);
    }
}
