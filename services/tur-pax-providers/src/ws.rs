// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! A small blocking WebSocket client (RFC 6455), for the live sessions.
//!
//! Live speech-to-speech services speak JSON over a WebSocket. This module is
//! that much and no more: the opening handshake over TLS (rustls, with the
//! bundled web PKI roots, as the HTTP client of this crate uses) or, for a
//! model server on this computer only, over plain TCP to a loopback address;
//! masked client frames; text, binary, ping, pong and close; messages in
//! fragments put back together; a limit on the size of one message; and a
//! read that waits at most a given time and keeps what it has read so far.
//!
//! It uses no crate that the provider layer did not already depend on
//! through its HTTP client, so that adding it changes no lockfile but the
//! direct dependency lists. The SHA-1 that checks the server's handshake
//! answer and the base64 the live families send audio in are written here.
//!
//! No unsafe code; nothing here holds or prints a key: the caller passes the
//! headers, and errors name the host, never a header.

use std::fs::File;
use std::io::{self, Read, Write};
use std::net::{IpAddr, TcpStream, ToSocketAddrs};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::error::{ErrorKind, ProviderError};

/// The longest message accepted from a server, in bytes.
pub(crate) const MAX_MESSAGE: usize = 16 * 1024 * 1024;

/// The longest handshake answer accepted, in bytes.
const MAX_HANDSHAKE: usize = 16 * 1024;

/// The GUID of RFC 6455 section 1.3.
const GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

const OP_CONTINUATION: u8 = 0x0;
const OP_TEXT: u8 = 0x1;
const OP_BINARY: u8 = 0x2;
const OP_CLOSE: u8 = 0x8;
const OP_PING: u8 = 0x9;
const OP_PONG: u8 = 0xA;

// ------------------------------------------------------------------ SHA-1

/// SHA-1 of `data`. Used only to check the server's handshake answer, as
/// RFC 6455 requires; it protects nothing.
pub(crate) fn sha1(data: &[u8]) -> [u8; 20] {
    let mut h: [u32; 5] = [0x6745_2301, 0xEFCD_AB89, 0x98BA_DCFE, 0x1032_5476, 0xC3D2_E1F0];
    let bit_length = (data.len() as u64).wrapping_mul(8);
    let mut message = data.to_vec();
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&bit_length.to_be_bytes());
    for block in message.chunks_exact(64) {
        let mut w = [0u32; 80];
        for (index, word) in block.chunks_exact(4).enumerate() {
            w[index] = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
        }
        for index in 16..80 {
            w[index] = (w[index - 3] ^ w[index - 8] ^ w[index - 14] ^ w[index - 16]).rotate_left(1);
        }
        let [mut a, mut b, mut c, mut d, mut e] = h;
        for (index, word) in w.iter().enumerate() {
            let (f, k) = match index {
                0..=19 => ((b & c) | ((!b) & d), 0x5A82_7999),
                20..=39 => (b ^ c ^ d, 0x6ED9_EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1B_BCDC),
                _ => (b ^ c ^ d, 0xCA62_C1D6),
            };
            let next = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(*word);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = next;
        }
        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
    }
    let mut digest = [0u8; 20];
    for (index, word) in h.iter().enumerate() {
        digest[index * 4..index * 4 + 4].copy_from_slice(&word.to_be_bytes());
    }
    digest
}

// ----------------------------------------------------------------- base64

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Standard base64 with padding.
pub(crate) fn base64_encode(data: &[u8]) -> String {
    let mut text = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b0 = u32::from(chunk[0]);
        let b1 = chunk.get(1).map_or(0, |b| u32::from(*b));
        let b2 = chunk.get(2).map_or(0, |b| u32::from(*b));
        let triple = (b0 << 16) | (b1 << 8) | b2;
        text.push(char::from(ALPHABET[(triple >> 18) as usize & 63]));
        text.push(char::from(ALPHABET[(triple >> 12) as usize & 63]));
        if chunk.len() > 1 {
            text.push(char::from(ALPHABET[(triple >> 6) as usize & 63]));
        } else {
            text.push('=');
        }
        if chunk.len() > 2 {
            text.push(char::from(ALPHABET[triple as usize & 63]));
        } else {
            text.push('=');
        }
    }
    text
}

fn base64_value(byte: u8) -> Option<u32> {
    match byte {
        b'A'..=b'Z' => Some(u32::from(byte - b'A')),
        b'a'..=b'z' => Some(u32::from(byte - b'a') + 26),
        b'0'..=b'9' => Some(u32::from(byte - b'0') + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

/// Decode standard base64, with or without padding. `None` for anything
/// that is not base64.
pub(crate) fn base64_decode(text: &str) -> Option<Vec<u8>> {
    let trimmed = text.trim_end_matches('=');
    if text.len() - trimmed.len() > 2 {
        return None;
    }
    let bytes = trimmed.as_bytes();
    if bytes.len() % 4 == 1 {
        return None;
    }
    let mut out = Vec::with_capacity(bytes.len() * 3 / 4);
    for chunk in bytes.chunks(4) {
        let mut accumulated: u32 = 0;
        for (index, byte) in chunk.iter().enumerate() {
            accumulated |= base64_value(*byte)? << (18 - 6 * index);
        }
        out.push((accumulated >> 16) as u8);
        if chunk.len() > 2 {
            out.push((accumulated >> 8) as u8);
        }
        if chunk.len() > 3 {
            out.push(accumulated as u8);
        }
    }
    Some(out)
}

/// The `Sec-WebSocket-Accept` value a server must answer for `key`.
pub(crate) fn accept_for(key: &str) -> String {
    let mut text = key.to_owned();
    text.push_str(GUID);
    base64_encode(&sha1(text.as_bytes()))
}

// ----------------------------------------------------------------- frames

/// One frame as read from a server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Frame {
    pub(crate) fin: bool,
    pub(crate) opcode: u8,
    pub(crate) payload: Vec<u8>,
}

/// Why a frame could not be read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FrameError {
    /// A reserved bit, an unknown opcode, a masked server frame, a control
    /// frame that is fragmented or too long.
    Protocol,
    /// The frame is longer than [`MAX_MESSAGE`].
    TooLarge,
}

/// Encode one client frame: always masked with `mask`.
pub(crate) fn encode_frame(opcode: u8, payload: &[u8], mask: [u8; 4]) -> Vec<u8> {
    let mut frame = Vec::with_capacity(payload.len() + 14);
    frame.push(0x80 | (opcode & 0x0F));
    let length = payload.len();
    if length < 126 {
        frame.push(0x80 | length as u8);
    } else if u16::try_from(length).is_ok() {
        frame.push(0x80 | 126);
        frame.extend_from_slice(&(length as u16).to_be_bytes());
    } else {
        frame.push(0x80 | 127);
        frame.extend_from_slice(&(length as u64).to_be_bytes());
    }
    frame.extend_from_slice(&mask);
    frame.extend(
        payload
            .iter()
            .enumerate()
            .map(|(index, byte)| byte ^ mask[index % 4]),
    );
    frame
}

/// Read one server frame from the start of `buffer`: the frame and the
/// number of bytes it took, or `None` when the buffer does not yet hold a
/// whole frame.
pub(crate) fn parse_frame(buffer: &[u8]) -> Result<Option<(Frame, usize)>, FrameError> {
    if buffer.len() < 2 {
        return Ok(None);
    }
    let first = buffer[0];
    let second = buffer[1];
    if first & 0x70 != 0 {
        return Err(FrameError::Protocol);
    }
    let fin = first & 0x80 != 0;
    let opcode = first & 0x0F;
    if !matches!(
        opcode,
        OP_CONTINUATION | OP_TEXT | OP_BINARY | OP_CLOSE | OP_PING | OP_PONG
    ) {
        return Err(FrameError::Protocol);
    }
    // A server never masks (RFC 6455 section 5.1).
    if second & 0x80 != 0 {
        return Err(FrameError::Protocol);
    }
    let (length, header) = match second & 0x7F {
        126 => {
            if buffer.len() < 4 {
                return Ok(None);
            }
            (u64::from(u16::from_be_bytes([buffer[2], buffer[3]])), 4)
        }
        127 => {
            if buffer.len() < 10 {
                return Ok(None);
            }
            let mut bytes = [0u8; 8];
            bytes.copy_from_slice(&buffer[2..10]);
            (u64::from_be_bytes(bytes), 10)
        }
        short => (u64::from(short), 2),
    };
    let control = opcode >= OP_CLOSE;
    if control && (!fin || length > 125) {
        return Err(FrameError::Protocol);
    }
    if length > MAX_MESSAGE as u64 {
        return Err(FrameError::TooLarge);
    }
    let length = length as usize;
    if buffer.len() < header + length {
        return Ok(None);
    }
    let payload = buffer[header..header + length].to_vec();
    Ok(Some((
        Frame {
            fin,
            opcode,
            payload,
        },
        header + length,
    )))
}

/// A whole message from the server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Incoming {
    /// A text message.
    Text(String),
    /// A binary message.
    Binary(Vec<u8>),
    /// The server closed the connection, with its status code if it gave one.
    Close(Option<u16>),
}

/// Puts fragmented messages back together.
#[derive(Debug, Default)]
pub(crate) struct Assembler {
    partial: Option<(u8, Vec<u8>)>,
}

impl Assembler {
    /// Take one data frame. A whole message when it is complete.
    pub(crate) fn push(&mut self, frame: Frame) -> Result<Option<Incoming>, FrameError> {
        let (opcode, mut payload) = match (frame.opcode, self.partial.take()) {
            (OP_CONTINUATION, Some((opcode, mut so_far))) => {
                so_far.extend_from_slice(&frame.payload);
                (opcode, so_far)
            }
            (OP_CONTINUATION, None) => return Err(FrameError::Protocol),
            (OP_TEXT | OP_BINARY, None) => (frame.opcode, frame.payload),
            // A new message started before the last one ended.
            _ => return Err(FrameError::Protocol),
        };
        if payload.len() > MAX_MESSAGE {
            return Err(FrameError::TooLarge);
        }
        if !frame.fin {
            self.partial = Some((opcode, std::mem::take(&mut payload)));
            return Ok(None);
        }
        Ok(Some(if opcode == OP_TEXT {
            Incoming::Text(String::from_utf8(payload).map_err(|_| FrameError::Protocol)?)
        } else {
            Incoming::Binary(payload)
        }))
    }
}

// -------------------------------------------------------------- addresses

/// A WebSocket address, read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WsUrl {
    pub(crate) secure: bool,
    pub(crate) host: String,
    pub(crate) port: u16,
    /// The path and query, starting with `/`.
    pub(crate) target: String,
}

fn config_invalid(detail: &str) -> ProviderError {
    ProviderError::new(ErrorKind::ConfigInvalid).with_detail(detail)
}

/// Whether `host` is this computer: `localhost` or a loopback address.
pub(crate) fn is_loopback_host(host: &str) -> bool {
    let bare = host.trim_start_matches('[').trim_end_matches(']');
    bare == "localhost" || bare.parse::<IpAddr>().is_ok_and(|address| address.is_loopback())
}

/// Read a `wss://` address, or a `ws://` address of a loopback host (a model
/// server on this computer: a key is never sent unencrypted over a
/// network). User information and fragments are refused.
pub(crate) fn parse_url(url: &str) -> Result<WsUrl, ProviderError> {
    let problem = || {
        config_invalid(
            "a live address must be wss://host[:port]/path; ws:// is accepted for a loopback host only",
        )
    };
    let (scheme, rest) = url.split_once("://").ok_or_else(problem)?;
    let secure = match scheme {
        "wss" => true,
        "ws" => false,
        _ => return Err(problem()),
    };
    if rest.is_empty() || !rest.bytes().all(|b| b.is_ascii_graphic()) || rest.contains(['#', '@', '\\']) {
        return Err(problem());
    }
    let split = rest.find(['/', '?']).unwrap_or(rest.len());
    let (authority, target) = rest.split_at(split);
    let target = if target.is_empty() {
        "/".to_owned()
    } else if target.starts_with('?') {
        format!("/{target}")
    } else {
        target.to_owned()
    };
    let (host, port) = if let Some(bracketed) = authority.strip_prefix('[') {
        let (address, after) = bracketed.split_once(']').ok_or_else(problem)?;
        let port = match after.strip_prefix(':') {
            Some(port) => port.parse::<u16>().map_err(|_| problem())?,
            None if after.is_empty() => {
                if secure {
                    443
                } else {
                    80
                }
            }
            None => return Err(problem()),
        };
        (format!("[{address}]"), port)
    } else {
        match authority.split_once(':') {
            Some((host, port)) => (host.to_owned(), port.parse::<u16>().map_err(|_| problem())?),
            None => (authority.to_owned(), if secure { 443 } else { 80 }),
        }
    };
    if host.is_empty() || host == "[]" {
        return Err(problem());
    }
    if !secure && !is_loopback_host(&host) {
        return Err(problem());
    }
    Ok(WsUrl {
        secure,
        host,
        port,
        target,
    })
}

// -------------------------------------------------------------- transport

enum Transport {
    Plain(TcpStream),
    Tls(Box<rustls::StreamOwned<rustls::ClientConnection, TcpStream>>),
}

impl Transport {
    fn socket(&self) -> &TcpStream {
        match self {
            Transport::Plain(stream) => stream,
            Transport::Tls(stream) => &stream.sock,
        }
    }
}

impl Read for Transport {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        match self {
            Transport::Plain(stream) => stream.read(buffer),
            Transport::Tls(stream) => stream.read(buffer),
        }
    }
}

impl Write for Transport {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        match self {
            Transport::Plain(stream) => stream.write(buffer),
            Transport::Tls(stream) => stream.write(buffer),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            Transport::Plain(stream) => stream.flush(),
            Transport::Tls(stream) => stream.flush(),
        }
    }
}

fn network(detail: impl AsRef<str>) -> ProviderError {
    ProviderError::new(ErrorKind::Network).with_detail(detail)
}

fn malformed(detail: impl AsRef<str>) -> ProviderError {
    ProviderError::new(ErrorKind::MalformedResponse).with_detail(detail)
}

fn tls_config() -> Result<Arc<rustls::ClientConfig>, ProviderError> {
    let roots: rustls::RootCertStore = webpki_roots::TLS_SERVER_ROOTS.iter().cloned().collect();
    let config = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .map_err(|error| network(format!("TLS could not be set up: {error}")))?
    .with_root_certificates(roots)
    .with_no_client_auth();
    Ok(Arc::new(config))
}

/// The status of a refused handshake as an error of the right kind.
fn refused_handshake(status: u16) -> ProviderError {
    let kind = match status {
        401 | 403 => ErrorKind::AuthenticationFailed,
        402 => ErrorKind::QuotaExhausted,
        429 => ErrorKind::RateLimited,
        500..=599 => ErrorKind::Overloaded,
        _ => ErrorKind::RequestRejected,
    };
    ProviderError::new(kind)
        .with_status(status)
        .with_detail(format!("the live session was refused with HTTP {status}"))
}

/// A connection.
pub(crate) struct WebSocket {
    transport: Transport,
    buffer: Vec<u8>,
    assembler: Assembler,
    random: Option<File>,
    closed: bool,
}

impl std::fmt::Debug for WebSocket {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WebSocket")
            .field("closed", &self.closed)
            .finish_non_exhaustive()
    }
}

/// Read random bytes. Falls back to the clock and the address of the
/// buffer only if `/dev/urandom` cannot be read: a mask hides nothing from
/// the server, and the handshake key is only a nonce.
fn random_bytes(source: &mut Option<File>, out: &mut [u8]) {
    if let Some(file) = source
        && file.read_exact(out).is_ok()
    {
        return;
    }
    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |time| time.as_nanos() as u64);
    let mut state = (seed ^ (out.as_ptr() as usize as u64)) | 1;
    for byte in out.iter_mut() {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        *byte = state as u8;
    }
}

impl WebSocket {
    /// Open a connection to `url` with the extra `headers`, and complete the
    /// opening handshake within `timeout`.
    pub(crate) fn connect(
        url: &WsUrl,
        headers: &[(String, String)],
        timeout: Duration,
    ) -> Result<WebSocket, ProviderError> {
        let host_for_dns = url.host.trim_start_matches('[').trim_end_matches(']');
        let addresses: Vec<_> = (host_for_dns, url.port)
            .to_socket_addrs()
            .map_err(|error| network(format!("{} could not be found: {error}", url.host)))?
            .collect();
        let mut last = None;
        let mut stream = None;
        for address in addresses {
            match TcpStream::connect_timeout(&address, timeout) {
                Ok(connected) => {
                    stream = Some(connected);
                    break;
                }
                Err(error) => last = Some(error),
            }
        }
        let stream = stream.ok_or_else(|| {
            network(format!(
                "{} could not be reached: {}",
                url.host,
                last.map_or_else(|| "no address".to_owned(), |error| error.to_string())
            ))
        })?;
        let _ = stream.set_nodelay(true);
        stream
            .set_read_timeout(Some(timeout))
            .and_then(|()| stream.set_write_timeout(Some(timeout)))
            .map_err(|error| network(error.to_string()))?;
        let transport = if url.secure {
            let name = rustls::pki_types::ServerName::try_from(host_for_dns.to_owned())
                .map_err(|_| config_invalid("the live address's host is not a valid name"))?;
            let connection = rustls::ClientConnection::new(tls_config()?, name)
                .map_err(|error| network(format!("TLS could not start: {error}")))?;
            Transport::Tls(Box::new(rustls::StreamOwned::new(connection, stream)))
        } else {
            Transport::Plain(stream)
        };
        let mut socket = WebSocket {
            transport,
            buffer: Vec::new(),
            assembler: Assembler::default(),
            random: File::open("/dev/urandom").ok(),
            closed: false,
        };
        socket.handshake(url, headers)?;
        Ok(socket)
    }

    fn handshake(&mut self, url: &WsUrl, headers: &[(String, String)]) -> Result<(), ProviderError> {
        let mut nonce = [0u8; 16];
        random_bytes(&mut self.random, &mut nonce);
        let key = base64_encode(&nonce);
        let default_port = if url.secure { 443 } else { 80 };
        let host = if url.port == default_port {
            url.host.clone()
        } else {
            format!("{}:{}", url.host, url.port)
        };
        let mut request = format!(
            "GET {} HTTP/1.1\r\nHost: {host}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\
             Sec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\nUser-Agent: tur-pax-providers\r\n",
            url.target
        );
        for (name, value) in headers {
            if name.bytes().any(|b| !b.is_ascii_graphic() || b == b':')
                || value.bytes().any(|b| b == b'\r' || b == b'\n')
            {
                return Err(config_invalid("a header of the live session cannot be sent"));
            }
            request.push_str(name);
            request.push_str(": ");
            request.push_str(value);
            request.push_str("\r\n");
        }
        request.push_str("\r\n");
        self.transport
            .write_all(request.as_bytes())
            .and_then(|()| self.transport.flush())
            .map_err(|error| network(format!("the handshake could not be sent: {error}")))?;

        // The answer's head, up to the empty line. What follows is kept: a
        // server may send its first message at once.
        let mut head = Vec::new();
        let mut chunk = [0u8; 4096];
        let end = loop {
            if let Some(at) = head.windows(4).position(|window| window == b"\r\n\r\n") {
                break at;
            }
            if head.len() > MAX_HANDSHAKE {
                return Err(malformed("the handshake answer is too long"));
            }
            let read = match self.transport.read(&mut chunk) {
                Ok(0) => return Err(network("the server closed the connection during the handshake")),
                Ok(read) => read,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) if matches!(error.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut) => {
                    return Err(ProviderError::new(ErrorKind::TimedOut)
                        .with_detail("the live service did not answer the handshake in time"));
                }
                Err(error) => return Err(network(format!("the handshake failed: {error}"))),
            };
            head.extend_from_slice(&chunk[..read]);
        };
        self.buffer = head[end + 4..].to_vec();
        let text = String::from_utf8_lossy(&head[..end]).into_owned();
        let mut lines = text.split("\r\n");
        let status_line = lines.next().unwrap_or_default();
        let status = status_line
            .split(' ')
            .nth(1)
            .and_then(|code| code.parse::<u16>().ok())
            .ok_or_else(|| malformed("the handshake answer has no status"))?;
        if status != 101 {
            return Err(refused_handshake(status));
        }
        let mut upgrade = false;
        let mut accept = None;
        for line in lines {
            let Some((name, value)) = line.split_once(':') else {
                continue;
            };
            let value = value.trim();
            if name.eq_ignore_ascii_case("upgrade") && value.eq_ignore_ascii_case("websocket") {
                upgrade = true;
            } else if name.eq_ignore_ascii_case("sec-websocket-accept") {
                accept = Some(value.to_owned());
            }
        }
        if !upgrade || accept.as_deref() != Some(accept_for(&key).as_str()) {
            return Err(malformed("the server's handshake answer is not a WebSocket upgrade"));
        }
        Ok(())
    }

    fn send_frame(&mut self, opcode: u8, payload: &[u8]) -> Result<(), ProviderError> {
        if self.closed {
            return Err(network("the live session is closed"));
        }
        let mut mask = [0u8; 4];
        random_bytes(&mut self.random, &mut mask);
        let frame = encode_frame(opcode, payload, mask);
        self.transport
            .write_all(&frame)
            .and_then(|()| self.transport.flush())
            .map_err(|error| {
                self.closed = true;
                network(format!("the live session's connection was lost: {error}"))
            })
    }

    /// Send a text message.
    pub(crate) fn send_text(&mut self, text: &str) -> Result<(), ProviderError> {
        self.send_frame(OP_TEXT, text.as_bytes())
    }

    /// Read the next whole message, waiting at most `wait`. `None` when
    /// nothing whole arrived in that time; what was read is kept for the
    /// next call. Pings are answered here.
    pub(crate) fn read(&mut self, wait: Duration) -> Result<Option<Incoming>, ProviderError> {
        if self.closed {
            return Ok(None);
        }
        let deadline = Instant::now() + wait;
        loop {
            while let Some((frame, used)) = parse_frame(&self.buffer).map_err(|problem| {
                self.closed = true;
                match problem {
                    FrameError::Protocol => malformed("the server broke the WebSocket protocol"),
                    FrameError::TooLarge => malformed("the server sent a message that is too large"),
                }
            })? {
                self.buffer.drain(..used);
                match frame.opcode {
                    OP_PING => self.send_frame(OP_PONG, &frame.payload)?,
                    OP_PONG => {}
                    OP_CLOSE => {
                        let code = (frame.payload.len() >= 2)
                            .then(|| u16::from_be_bytes([frame.payload[0], frame.payload[1]]));
                        let _ = self.send_frame(OP_CLOSE, &frame.payload[..frame.payload.len().min(2)]);
                        self.closed = true;
                        return Ok(Some(Incoming::Close(code)));
                    }
                    _ => {
                        let message = self.assembler.push(frame).map_err(|_| {
                            self.closed = true;
                            malformed("the server sent a message that could not be read")
                        })?;
                        if let Some(message) = message {
                            return Ok(Some(message));
                        }
                    }
                }
            }
            let now = Instant::now();
            if now >= deadline {
                return Ok(None);
            }
            let left = (deadline - now).max(Duration::from_millis(1));
            self.transport
                .socket()
                .set_read_timeout(Some(left))
                .map_err(|error| network(error.to_string()))?;
            let mut chunk = [0u8; 16 * 1024];
            match self.transport.read(&mut chunk) {
                Ok(0) => {
                    self.closed = true;
                    return Err(network("the live service closed the connection"));
                }
                Ok(read) => {
                    if self.buffer.len() + read > MAX_MESSAGE + 16 {
                        self.closed = true;
                        return Err(malformed("the server sent a message that is too large"));
                    }
                    self.buffer.extend_from_slice(&chunk[..read]);
                }
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error)
                    if matches!(error.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut) =>
                {
                    return Ok(None);
                }
                Err(error) => {
                    self.closed = true;
                    return Err(network(format!("the live session's connection was lost: {error}")));
                }
            }
        }
    }

    /// Close the connection, saying so to the server first.
    pub(crate) fn close(&mut self) {
        if !self.closed {
            let _ = self.send_frame(OP_CLOSE, &1000u16.to_be_bytes());
            self.closed = true;
        }
        let _ = self.transport.socket().shutdown(std::net::Shutdown::Both);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    #[test]
    fn sha1_matches_the_standard_vectors() {
        assert_eq!(hex(&sha1(b"")), "da39a3ee5e6b4b0d3255bfef95601890afd80709");
        assert_eq!(hex(&sha1(b"abc")), "a9993e364706816aba3e25717850c26c9cd0d89d");
        assert_eq!(
            hex(&sha1(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq")),
            "84983e441c3bd26ebaae4aa1f95129e5e54670f1"
        );
        let million = vec![b'a'; 1_000_000];
        assert_eq!(hex(&sha1(&million)), "34aa973cd4c4daa4f61eeb2bdbad27316534016f");
    }

    #[test]
    fn the_accept_value_is_the_one_of_rfc_6455() {
        assert_eq!(accept_for("dGhlIHNhbXBsZSBub25jZQ=="), "s3pPLMBiTxaQ9kYGzzhZRbK+xOo=");
    }

    #[test]
    fn base64_round_trips_and_refuses_what_is_not_base64() {
        for (plain, encoded) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(base64_encode(plain.as_bytes()), encoded);
            assert_eq!(base64_decode(encoded).as_deref(), Some(plain.as_bytes()));
        }
        let all: Vec<u8> = (0..=255).collect();
        assert_eq!(base64_decode(&base64_encode(&all)), Some(all));
        assert_eq!(base64_decode("Zm9v!"), None);
        assert_eq!(base64_decode("Z"), None);
        assert_eq!(base64_decode("Zg==="), None);
    }

    fn server_frame(fin: bool, opcode: u8, payload: &[u8]) -> Vec<u8> {
        let mut frame = vec![if fin { 0x80 } else { 0 } | opcode];
        if payload.len() < 126 {
            frame.push(payload.len() as u8);
        } else if payload.len() <= 0xFFFF {
            frame.push(126);
            frame.extend_from_slice(&(payload.len() as u16).to_be_bytes());
        } else {
            frame.push(127);
            frame.extend_from_slice(&(payload.len() as u64).to_be_bytes());
        }
        frame.extend_from_slice(payload);
        frame
    }

    #[test]
    fn a_client_frame_is_masked_and_its_length_encoded() {
        let mask = [1, 2, 3, 4];
        for length in [0usize, 5, 125, 126, 65_535, 65_536] {
            let payload: Vec<u8> = (0..length).map(|i| i as u8).collect();
            let frame = encode_frame(OP_TEXT, &payload, mask);
            assert_eq!(frame[0], 0x81);
            assert_ne!(frame[1] & 0x80, 0, "masked");
            let (declared, header) = match frame[1] & 0x7F {
                126 => (usize::from(u16::from_be_bytes([frame[2], frame[3]])), 4),
                127 => {
                    let mut bytes = [0u8; 8];
                    bytes.copy_from_slice(&frame[2..10]);
                    (u64::from_be_bytes(bytes) as usize, 10)
                }
                short => (usize::from(short), 2),
            };
            assert_eq!(declared, length);
            assert_eq!(&frame[header..header + 4], &mask);
            let unmasked: Vec<u8> = frame[header + 4..]
                .iter()
                .enumerate()
                .map(|(i, b)| b ^ mask[i % 4])
                .collect();
            assert_eq!(unmasked, payload);
        }
    }

    #[test]
    fn server_frames_are_read_whole_or_not_at_all() {
        for length in [0usize, 7, 125, 126, 70_000] {
            let payload: Vec<u8> = (0..length).map(|i| (i % 251) as u8).collect();
            let bytes = server_frame(true, OP_BINARY, &payload);
            for cut in 0..bytes.len().min(12) {
                assert_eq!(parse_frame(&bytes[..cut]), Ok(None), "cut at {cut}");
            }
            assert_eq!(parse_frame(&bytes[..bytes.len() - 1]), Ok(None));
            let (frame, used) = parse_frame(&bytes).expect("valid").expect("whole");
            assert_eq!(used, bytes.len());
            assert_eq!(frame.payload, payload);
            assert!(frame.fin);
        }
    }

    #[test]
    fn broken_server_frames_are_refused() {
        // A reserved bit.
        assert_eq!(parse_frame(&[0xC1, 0]), Err(FrameError::Protocol));
        // An unknown opcode.
        assert_eq!(parse_frame(&[0x83, 0]), Err(FrameError::Protocol));
        // A masked server frame.
        assert_eq!(parse_frame(&[0x81, 0x80, 0, 0, 0, 0]), Err(FrameError::Protocol));
        // A fragmented control frame.
        assert_eq!(parse_frame(&[0x09, 0]), Err(FrameError::Protocol));
        // A control frame longer than 125 bytes.
        assert_eq!(parse_frame(&[0x89, 126, 0, 126]), Err(FrameError::Protocol));
        // Longer than the limit.
        let mut huge = vec![0x82, 127];
        huge.extend_from_slice(&((MAX_MESSAGE as u64) + 1).to_be_bytes());
        assert_eq!(parse_frame(&huge), Err(FrameError::TooLarge));
    }

    #[test]
    fn fragments_are_put_back_together() {
        let mut assembler = Assembler::default();
        let first = Frame { fin: false, opcode: OP_TEXT, payload: b"Hel".to_vec() };
        let middle = Frame { fin: false, opcode: OP_CONTINUATION, payload: b"lo, ".to_vec() };
        let last = Frame { fin: true, opcode: OP_CONTINUATION, payload: "P@x".as_bytes().to_vec() };
        assert_eq!(assembler.push(first), Ok(None));
        assert_eq!(assembler.push(middle), Ok(None));
        assert_eq!(assembler.push(last), Ok(Some(Incoming::Text("Hello, P@x".to_owned()))));
        // A continuation with nothing to continue.
        let stray = Frame { fin: true, opcode: OP_CONTINUATION, payload: Vec::new() };
        assert_eq!(assembler.push(stray), Err(FrameError::Protocol));
        // A new message inside an unfinished one.
        let open = Frame { fin: false, opcode: OP_BINARY, payload: vec![1] };
        let intruder = Frame { fin: true, opcode: OP_TEXT, payload: vec![b'x'] };
        assert_eq!(assembler.push(open), Ok(None));
        assert_eq!(assembler.push(intruder), Err(FrameError::Protocol));
        // Text that is not UTF-8.
        let mut fresh = Assembler::default();
        let bad = Frame { fin: true, opcode: OP_TEXT, payload: vec![0xFF, 0xFE] };
        assert_eq!(fresh.push(bad), Err(FrameError::Protocol));
    }

    #[test]
    fn addresses_are_wss_or_ws_to_this_computer_only() {
        let url = parse_url("wss://api.openai.com/v1/realtime?model=m").expect("valid");
        assert_eq!(
            url,
            WsUrl { secure: true, host: "api.openai.com".to_owned(), port: 443, target: "/v1/realtime?model=m".to_owned() }
        );
        let local = parse_url("ws://127.0.0.1:39281/v1/realtime").expect("valid");
        assert!(!local.secure);
        assert_eq!(local.port, 39281);
        assert!(parse_url("ws://localhost:8080").is_ok());
        assert!(parse_url("ws://[::1]:8080/x").is_ok());
        assert_eq!(parse_url("wss://example.org").expect("valid").target, "/");
        for refused in [
            "ws://api.openai.com/v1/realtime",
            "ws://10.0.0.5:8080/",
            "ws://127.example.org/",
            "https://api.openai.com/v1/realtime",
            "wss://user@api.openai.com/",
            "wss://api.openai.com/#x",
            "wss://:443/",
            "wss://host:notaport/",
            "wss://",
        ] {
            assert!(parse_url(refused).is_err(), "{refused}");
        }
    }

    #[test]
    fn a_refused_handshake_has_the_kind_of_its_status() {
        assert_eq!(refused_handshake(401).kind(), ErrorKind::AuthenticationFailed);
        assert_eq!(refused_handshake(429).kind(), ErrorKind::RateLimited);
        assert_eq!(refused_handshake(503).kind(), ErrorKind::Overloaded);
        assert_eq!(refused_handshake(400).kind(), ErrorKind::RequestRejected);
        assert_eq!(refused_handshake(401).http_status(), Some(401));
    }
}
