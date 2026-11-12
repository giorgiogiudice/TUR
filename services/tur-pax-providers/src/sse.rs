// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! An incremental parser for server-sent events.
//!
//! All three providers stream their responses as `text/event-stream`. The
//! parser reads from any [`Read`], so a chunk boundary may fall anywhere:
//! inside a line, inside a multi-byte character or between the two bytes of a
//! line ending.
//!
//! What it implements of the event-stream format:
//!
//! * a line ends with `\n` or `\r\n`;
//! * a line starting with `:` is a comment (providers use it as a keep-alive)
//!   and is ignored;
//! * `event:` names the event, `data:` adds a line of data, any other field is
//!   ignored;
//! * an empty line ends the event. An event without data is dropped;
//! * an event that is still incomplete when the stream ends is dropped.
//!
//! A line that ends with a bare `\r` is not treated as a line ending. None of
//! the three providers sends one.

use std::io::{self, Read};

/// The most bytes buffered for one line, and the most data kept for one event.
pub(crate) const MAX_EVENT_BYTES: usize = 8 * 1024 * 1024;

const READ_CHUNK: usize = 8192;

/// One event of the stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SseEvent {
    /// The event name, if the stream gave one.
    pub(crate) event: Option<String>,
    /// The data lines, joined with a newline.
    pub(crate) data: String,
}

/// Why the stream could not be read further.
#[derive(Debug)]
pub(crate) enum SseError {
    /// Reading from the connection failed.
    Io(io::Error),
    /// A line was not valid UTF-8.
    InvalidUtf8,
    /// A line or an event was larger than [`MAX_EVENT_BYTES`].
    TooLong,
}

/// Reads events from a byte stream.
pub(crate) struct SseReader<R> {
    source: R,
    buffer: Vec<u8>,
    scanned: usize,
    at_end: bool,
    event: Option<String>,
    data: String,
    has_data: bool,
}

impl<R: Read> SseReader<R> {
    /// A reader over `source`.
    pub(crate) fn new(source: R) -> Self {
        SseReader {
            source,
            buffer: Vec::new(),
            scanned: 0,
            at_end: false,
            event: None,
            data: String::new(),
            has_data: false,
        }
    }

    /// The next complete event, or `None` when the stream has ended.
    pub(crate) fn next_event(&mut self) -> Result<Option<SseEvent>, SseError> {
        loop {
            let Some(line) = self.take_line()? else {
                return Ok(None);
            };
            if let Some(event) = self.accept_line(&line)? {
                return Ok(Some(event));
            }
        }
    }

    /// The next complete line without its line ending, or `None` at the end.
    fn take_line(&mut self) -> Result<Option<String>, SseError> {
        loop {
            let newline = self.buffer[self.scanned..]
                .iter()
                .position(|byte| *byte == b'\n');
            if let Some(offset) = newline {
                let end = self.scanned + offset;
                let mut line: Vec<u8> = self.buffer.drain(..=end).collect();
                self.scanned = 0;
                line.pop();
                if line.last() == Some(&b'\r') {
                    line.pop();
                }
                return String::from_utf8(line)
                    .map(Some)
                    .map_err(|_| SseError::InvalidUtf8);
            }
            self.scanned = self.buffer.len();
            if self.at_end {
                // A last line without a line ending belongs to an incomplete
                // event, which the format says to drop.
                self.buffer.clear();
                self.scanned = 0;
                return Ok(None);
            }
            if self.buffer.len() > MAX_EVENT_BYTES {
                return Err(SseError::TooLong);
            }
            let mut chunk = [0u8; READ_CHUNK];
            match self.source.read(&mut chunk) {
                Ok(0) => self.at_end = true,
                Ok(count) => self.buffer.extend_from_slice(&chunk[..count]),
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) => return Err(SseError::Io(error)),
            }
        }
    }

    /// Apply one line. Returns the event the line completed, if any.
    fn accept_line(&mut self, line: &str) -> Result<Option<SseEvent>, SseError> {
        if line.is_empty() {
            let event = self.event.take();
            if !self.has_data {
                return Ok(None);
            }
            self.has_data = false;
            return Ok(Some(SseEvent {
                event,
                data: std::mem::take(&mut self.data),
            }));
        }
        if line.starts_with(':') {
            return Ok(None);
        }
        let (field, value) = match line.split_once(':') {
            Some((field, value)) => (field, value.strip_prefix(' ').unwrap_or(value)),
            None => (line, ""),
        };
        match field {
            "event" => self.event = Some(value.to_owned()),
            "data" => {
                if self.has_data {
                    self.data.push('\n');
                }
                self.data.push_str(value);
                self.has_data = true;
                if self.data.len() > MAX_EVENT_BYTES {
                    return Err(SseError::TooLong);
                }
            }
            _ => {}
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A source that hands out at most `step` bytes per read.
    struct Drip<'a> {
        bytes: &'a [u8],
        step: usize,
    }

    impl Read for Drip<'_> {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            let count = self.step.min(self.bytes.len()).min(buffer.len());
            buffer[..count].copy_from_slice(&self.bytes[..count]);
            self.bytes = &self.bytes[count..];
            Ok(count)
        }
    }

    fn events(bytes: &[u8], step: usize) -> Vec<SseEvent> {
        let mut reader = SseReader::new(Drip { bytes, step });
        let mut found = Vec::new();
        while let Some(event) = reader.next_event().expect("the stream is readable") {
            found.push(event);
        }
        found
    }

    fn event(name: Option<&str>, data: &str) -> SseEvent {
        SseEvent {
            event: name.map(str::to_owned),
            data: data.to_owned(),
        }
    }

    const STREAM: &str = concat!(
        ": keep-alive\n",
        "\n",
        "event: first\n",
        "data: {\"text\":\"wörld ✓\"}\n",
        "\n",
        ":\n",
        "event: second\r\n",
        "id: 7\r\n",
        "retry: 1000\r\n",
        "data: line one\r\n",
        "data: line two\r\n",
        "\r\n",
        "data:no space\n",
        "\n",
        "event: no-data\n",
        "\n",
        "data\n",
        "\n",
        "data: after an empty data line\n",
        "\n",
    );

    fn expected() -> Vec<SseEvent> {
        vec![
            event(Some("first"), "{\"text\":\"wörld ✓\"}"),
            event(Some("second"), "line one\nline two"),
            event(None, "no space"),
            event(None, ""),
            event(None, "after an empty data line"),
        ]
    }

    #[test]
    fn parses_events_comments_and_both_line_endings() {
        assert_eq!(events(STREAM.as_bytes(), usize::MAX), expected());
    }

    #[test]
    fn the_result_does_not_depend_on_where_chunks_split() {
        for step in 1..=17 {
            assert_eq!(events(STREAM.as_bytes(), step), expected(), "step {step}");
        }
    }

    #[test]
    fn an_incomplete_last_event_is_dropped() {
        let cut = "data: complete\n\nevent: partial\ndata: {\"te";
        assert_eq!(events(cut.as_bytes(), 3), vec![event(None, "complete")]);

        let no_blank_line = "data: complete\n\ndata: never dispatched\n";
        assert_eq!(
            events(no_blank_line.as_bytes(), 5),
            vec![event(None, "complete")]
        );
    }

    #[test]
    fn unknown_fields_and_plain_text_produce_no_events() {
        let html = "<html>\n<body>captive portal</body>\n\n</html>\n\n";
        assert!(events(html.as_bytes(), 4).is_empty());
        assert!(events(b"", 1).is_empty());
    }

    #[test]
    fn invalid_utf8_is_an_error() {
        let mut reader = SseReader::new(Drip {
            bytes: b"data: \xff\xfe\n\n",
            step: 2,
        });
        assert!(matches!(reader.next_event(), Err(SseError::InvalidUtf8)));
    }

    #[test]
    fn a_read_failure_is_reported() {
        struct Failing;
        impl Read for Failing {
            fn read(&mut self, _buffer: &mut [u8]) -> io::Result<usize> {
                Err(io::Error::new(io::ErrorKind::ConnectionReset, "reset"))
            }
        }
        let mut reader = SseReader::new(Failing);
        match reader.next_event() {
            Err(SseError::Io(error)) => assert_eq!(error.kind(), io::ErrorKind::ConnectionReset),
            other => panic!("unexpected result: {other:?}"),
        }
    }

    #[test]
    fn an_endless_line_is_cut_off() {
        struct Endless;
        impl Read for Endless {
            fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
                buffer.fill(b'x');
                Ok(buffer.len())
            }
        }
        let mut reader = SseReader::new(Endless);
        assert!(matches!(reader.next_event(), Err(SseError::TooLong)));
    }
}
