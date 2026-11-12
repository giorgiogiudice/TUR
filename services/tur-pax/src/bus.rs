// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! A small D-Bus implementation: the wire format, a message, and a
//! connection to a message bus over a Unix socket.
//!
//! P@x needs little of D-Bus: it serves one interface whose arguments are
//! strings, booleans and numbers, it calls the desktop's notification
//! service and the Secret Service, and it listens for two signals. This
//! module is that much and no more. It is written from the D-Bus
//! specification (<https://dbus.freedesktop.org/doc/dbus-specification.html>),
//! it uses the standard library only, and it has no unsafe code.
//!
//! Why not a D-Bus crate: every component that ships tools for P@x builds
//! against this crate, so a new dependency here changes the lockfile of
//! each of them. `docs/architecture/pax-service.md`, section 10, records
//! what was looked at and why this was chosen.
//!
//! What is here:
//!
//! * [`Value`]: the types of the wire format, except file descriptors;
//! * [`Message`]: a method call, a reply, an error or a signal, encoded and
//!   decoded with every length and every alignment checked;
//! * [`Connection`]: the `EXTERNAL` authentication, a thread that reads
//!   what arrives, replies matched to calls, and everything else handed to
//!   the owner of the connection through a channel.
//!
//! What is not: file descriptor passing, any authentication but `EXTERNAL`,
//! any transport but a Unix socket, big-endian output (big-endian input is
//! read), and an object tree. The owner of a connection dispatches incoming
//! calls itself ([`crate::dbus`]).

use std::collections::HashMap;
use std::fmt;
use std::io::{Read, Write};
use std::os::linux::net::SocketAddrExt;
use std::os::unix::fs::MetadataExt;
use std::os::unix::net::{SocketAddr, UnixStream};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::Duration;

/// The largest message this module reads or writes, in bytes. The
/// specification allows 128 MiB; nothing P@x exchanges comes near this.
pub const MAX_MESSAGE_BYTES: usize = 16 * 1024 * 1024;

/// How deeply containers may be nested in a signature.
pub const MAX_DEPTH: usize = 32;

/// The name of the message bus itself.
pub const BUS_NAME: &str = "org.freedesktop.DBus";

/// The object path of the message bus itself.
pub const BUS_PATH: &str = "/org/freedesktop/DBus";

/// What went wrong on the bus.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BusError {
    /// The bus could not be reached: no address, no socket, or the bus did
    /// not accept the connection.
    Unavailable(String),
    /// Something that was received, or was to be sent, is not valid D-Bus.
    Protocol(String),
    /// The peer answered a call with an error.
    Remote {
        /// The error name, for example
        /// `org.freedesktop.DBus.Error.ServiceUnknown`.
        name: String,
        /// The text that came with it.
        message: String,
    },
    /// No reply arrived in time.
    TimedOut,
    /// The connection is closed.
    Closed,
}

impl BusError {
    /// Whether the error says that nobody owns the name that was called.
    pub fn is_no_such_service(&self) -> bool {
        matches!(
            self,
            BusError::Remote { name, .. }
                if name == "org.freedesktop.DBus.Error.ServiceUnknown"
                    || name == "org.freedesktop.DBus.Error.NameHasNoOwner"
        )
    }
}

impl fmt::Display for BusError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BusError::Unavailable(detail) => {
                write!(f, "the message bus is not available: {detail}")
            }
            BusError::Protocol(detail) => write!(f, "not valid D-Bus: {detail}"),
            BusError::Remote { name, message } => write!(f, "{name}: {message}"),
            BusError::TimedOut => f.write_str("no reply arrived in time"),
            BusError::Closed => f.write_str("the connection to the message bus is closed"),
        }
    }
}

impl std::error::Error for BusError {}

fn protocol(detail: impl Into<String>) -> BusError {
    BusError::Protocol(detail.into())
}

/// One value of the D-Bus type system.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// `y`: a byte.
    Byte(u8),
    /// `b`: true or false.
    Bool(bool),
    /// `n`: a signed 16-bit integer.
    I16(i16),
    /// `q`: an unsigned 16-bit integer.
    U16(u16),
    /// `i`: a signed 32-bit integer.
    I32(i32),
    /// `u`: an unsigned 32-bit integer.
    U32(u32),
    /// `x`: a signed 64-bit integer.
    I64(i64),
    /// `t`: an unsigned 64-bit integer.
    U64(u64),
    /// `d`: a double.
    Double(f64),
    /// `s`: a string.
    Str(String),
    /// `o`: an object path.
    Path(String),
    /// `g`: a signature.
    Signature(String),
    /// `a`: an array. The element signature is kept, because an empty array
    /// still has a type.
    Array {
        /// The signature of one element: a single complete type.
        element: String,
        /// The elements.
        items: Vec<Value>,
    },
    /// `(...)`: a struct.
    Struct(Vec<Value>),
    /// `{..}`: an entry of a dictionary. It occurs only as an array element.
    DictEntry(Box<Value>, Box<Value>),
    /// `v`: a value with its own signature.
    Variant(Box<Value>),
}

impl Value {
    /// A string value.
    pub fn string(text: impl Into<String>) -> Value {
        Value::Str(text.into())
    }

    /// An object path value.
    pub fn path(text: impl Into<String>) -> Value {
        Value::Path(text.into())
    }

    /// A variant holding `inner`.
    pub fn variant(inner: Value) -> Value {
        Value::Variant(Box::new(inner))
    }

    /// An array of strings.
    pub fn strings<I, S>(items: I) -> Value
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Value::Array {
            element: "s".to_owned(),
            items: items
                .into_iter()
                .map(|item| Value::Str(item.into()))
                .collect(),
        }
    }

    /// A dictionary from strings to strings, `a{ss}`.
    pub fn string_map(entries: &[(&str, &str)]) -> Value {
        Value::Array {
            element: "{ss}".to_owned(),
            items: entries
                .iter()
                .map(|(key, value)| {
                    Value::DictEntry(
                        Box::new(Value::string(*key)),
                        Box::new(Value::string(*value)),
                    )
                })
                .collect(),
        }
    }

    /// A dictionary from strings to variants, `a{sv}`.
    pub fn variant_map(entries: Vec<(&str, Value)>) -> Value {
        Value::Array {
            element: "{sv}".to_owned(),
            items: entries
                .into_iter()
                .map(|(key, value)| {
                    Value::DictEntry(
                        Box::new(Value::string(key)),
                        Box::new(Value::variant(value)),
                    )
                })
                .collect(),
        }
    }

    /// The signature of the value.
    pub fn signature(&self) -> String {
        match self {
            Value::Byte(_) => "y".to_owned(),
            Value::Bool(_) => "b".to_owned(),
            Value::I16(_) => "n".to_owned(),
            Value::U16(_) => "q".to_owned(),
            Value::I32(_) => "i".to_owned(),
            Value::U32(_) => "u".to_owned(),
            Value::I64(_) => "x".to_owned(),
            Value::U64(_) => "t".to_owned(),
            Value::Double(_) => "d".to_owned(),
            Value::Str(_) => "s".to_owned(),
            Value::Path(_) => "o".to_owned(),
            Value::Signature(_) => "g".to_owned(),
            Value::Array { element, .. } => format!("a{element}"),
            Value::Struct(fields) => {
                let inner: String = fields.iter().map(Value::signature).collect();
                format!("({inner})")
            }
            Value::DictEntry(key, value) => format!("{{{}{}}}", key.signature(), value.signature()),
            Value::Variant(_) => "v".to_owned(),
        }
    }

    /// The text of a string or an object path.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(text) | Value::Path(text) => Some(text),
            _ => None,
        }
    }

    /// The value of a boolean.
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(value) => Some(*value),
            _ => None,
        }
    }

    /// The value of an unsigned 32-bit integer.
    pub fn as_u32(&self) -> Option<u32> {
        match self {
            Value::U32(value) => Some(*value),
            _ => None,
        }
    }

    /// The elements of an array.
    pub fn as_array(&self) -> Option<&[Value]> {
        match self {
            Value::Array { items, .. } => Some(items),
            _ => None,
        }
    }

    /// The fields of a struct.
    pub fn as_struct(&self) -> Option<&[Value]> {
        match self {
            Value::Struct(fields) => Some(fields),
            _ => None,
        }
    }

    /// The bytes of an array of bytes.
    pub fn as_bytes(&self) -> Option<Vec<u8>> {
        let items = self.as_array()?;
        items
            .iter()
            .map(|item| match item {
                Value::Byte(byte) => Some(*byte),
                _ => None,
            })
            .collect()
    }

    /// The value inside a variant; any other value is itself.
    pub fn unwrapped(&self) -> &Value {
        match self {
            Value::Variant(inner) => inner,
            other => other,
        }
    }

    /// An array of bytes.
    pub fn bytes(bytes: &[u8]) -> Value {
        Value::Array {
            element: "y".to_owned(),
            items: bytes.iter().map(|byte| Value::Byte(*byte)).collect(),
        }
    }
}

/// The boundary a type is aligned to, from the first character of its
/// signature.
const fn alignment(code: u8) -> usize {
    match code {
        b'y' | b'g' | b'v' => 1,
        b'n' | b'q' => 2,
        b'b' | b'i' | b'u' | b's' | b'o' | b'a' | b'h' => 4,
        _ => 8,
    }
}

const fn is_basic(code: u8) -> bool {
    matches!(
        code,
        b'y' | b'b' | b'n' | b'q' | b'i' | b'u' | b'x' | b't' | b'd' | b's' | b'o' | b'g' | b'h'
    )
}

/// The length of the single complete type at the start of `signature`.
///
/// A dictionary entry is a type only as the element of an array:
/// `entry_allowed` says whether this is that place.
fn complete_type_len(
    signature: &[u8],
    depth: usize,
    entry_allowed: bool,
) -> Result<usize, BusError> {
    if depth > MAX_DEPTH {
        return Err(protocol("a signature is nested too deeply"));
    }
    match signature.first().copied() {
        None => Err(protocol("a signature ends where a type is expected")),
        Some(code) if is_basic(code) || code == b'v' => Ok(1),
        Some(b'a') => Ok(1 + complete_type_len(&signature[1..], depth + 1, true)?),
        Some(b'(') => {
            let mut at = 1;
            loop {
                match signature.get(at).copied() {
                    None => return Err(protocol("a struct in a signature is not closed")),
                    Some(b')') if at == 1 => {
                        return Err(protocol("a struct in a signature is empty"));
                    }
                    Some(b')') => return Ok(at + 1),
                    Some(_) => at += complete_type_len(&signature[at..], depth + 1, false)?,
                }
            }
        }
        Some(b'{') if entry_allowed => {
            let key_is_basic = signature.get(1).copied().is_some_and(is_basic);
            if !key_is_basic {
                return Err(protocol(
                    "the key of a dictionary entry is not a basic type",
                ));
            }
            let value = complete_type_len(&signature[2..], depth + 1, false)?;
            if signature.get(2 + value).copied() != Some(b'}') {
                return Err(protocol("a dictionary entry in a signature is not closed"));
            }
            Ok(3 + value)
        }
        Some(b'{') => Err(protocol("a dictionary entry outside an array")),
        Some(_) => Err(protocol("a signature holds a character that is not a type")),
    }
}

/// Whether `element` is one type an array may hold: a single complete type
/// or a dictionary entry.
fn is_array_element(element: &str) -> bool {
    element.is_ascii()
        && element.len() < 255
        && complete_type_len(element.as_bytes(), 1, true)
            .is_ok_and(|length| length == element.len())
}

/// Split a signature into its single complete types.
///
/// # Errors
///
/// The text is not a valid signature, or is longer than 255 bytes.
pub fn split_signature(signature: &str) -> Result<Vec<&str>, BusError> {
    if signature.len() > 255 || !signature.is_ascii() {
        return Err(protocol("a signature is too long or is not ASCII"));
    }
    let bytes = signature.as_bytes();
    let mut types = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        let length = complete_type_len(&bytes[at..], 0, false)?;
        types.push(&signature[at..at + length]);
        at += length;
    }
    Ok(types)
}

/// Whether `text` is a valid object path.
pub fn is_object_path(text: &str) -> bool {
    if text == "/" {
        return true;
    }
    text.starts_with('/')
        && !text.ends_with('/')
        && text.len() <= 1024
        && text[1..].split('/').all(|element| {
            !element.is_empty()
                && element
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        })
}

/// Whether `text` is a valid interface or error name: two or more elements
/// of letters, digits and underscores, separated by dots, none starting
/// with a digit.
pub fn is_interface_name(text: &str) -> bool {
    text.len() <= 255
        && text.contains('.')
        && text.split('.').all(|element| {
            !element.is_empty()
                && !element.as_bytes()[0].is_ascii_digit()
                && element
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        })
}

/// Whether `text` is a valid member name.
pub fn is_member_name(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= 255
        && !text.as_bytes()[0].is_ascii_digit()
        && text
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

/// Writes values in the wire format, little-endian.
struct Encoder {
    bytes: Vec<u8>,
}

impl Encoder {
    fn new() -> Self {
        Encoder { bytes: Vec::new() }
    }

    fn pad(&mut self, boundary: usize) {
        while !self.bytes.len().is_multiple_of(boundary) {
            self.bytes.push(0);
        }
    }

    fn u32(&mut self, value: u32) {
        self.pad(4);
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    fn text(&mut self, text: &str) -> Result<(), BusError> {
        if text.contains('\0') {
            return Err(protocol("a string holds a zero byte"));
        }
        let length = u32::try_from(text.len()).map_err(|_| protocol("a string is too long"))?;
        self.u32(length);
        self.bytes.extend_from_slice(text.as_bytes());
        self.bytes.push(0);
        Ok(())
    }

    fn signature(&mut self, text: &str) -> Result<(), BusError> {
        split_signature(text)?;
        let length = u8::try_from(text.len()).map_err(|_| protocol("a signature is too long"))?;
        self.bytes.push(length);
        self.bytes.extend_from_slice(text.as_bytes());
        self.bytes.push(0);
        Ok(())
    }

    fn value(&mut self, value: &Value) -> Result<(), BusError> {
        match value {
            Value::Byte(byte) => self.bytes.push(*byte),
            Value::Bool(flag) => self.u32(u32::from(*flag)),
            Value::I16(number) => {
                self.pad(2);
                self.bytes.extend_from_slice(&number.to_le_bytes());
            }
            Value::U16(number) => {
                self.pad(2);
                self.bytes.extend_from_slice(&number.to_le_bytes());
            }
            Value::I32(number) => {
                self.pad(4);
                self.bytes.extend_from_slice(&number.to_le_bytes());
            }
            Value::U32(number) => self.u32(*number),
            Value::I64(number) => {
                self.pad(8);
                self.bytes.extend_from_slice(&number.to_le_bytes());
            }
            Value::U64(number) => {
                self.pad(8);
                self.bytes.extend_from_slice(&number.to_le_bytes());
            }
            Value::Double(number) => {
                self.pad(8);
                self.bytes.extend_from_slice(&number.to_le_bytes());
            }
            Value::Str(text) => self.text(text)?,
            Value::Path(text) => {
                if !is_object_path(text) {
                    return Err(protocol("an object path is not valid"));
                }
                self.text(text)?;
            }
            Value::Signature(text) => self.signature(text)?,
            Value::Array { element, items } => {
                if !is_array_element(element) {
                    return Err(protocol("the element of an array is not one complete type"));
                }
                self.u32(0);
                let length_at = self.bytes.len() - 4;
                self.pad(alignment(element.as_bytes()[0]));
                let start = self.bytes.len();
                for item in items {
                    if item.signature() != *element {
                        return Err(protocol("an array holds an element of another type"));
                    }
                    self.value(item)?;
                }
                let length = u32::try_from(self.bytes.len() - start)
                    .map_err(|_| protocol("an array is too long"))?;
                self.bytes[length_at..length_at + 4].copy_from_slice(&length.to_le_bytes());
            }
            Value::Struct(fields) => {
                if fields.is_empty() {
                    return Err(protocol("a struct is empty"));
                }
                self.pad(8);
                for field in fields {
                    self.value(field)?;
                }
            }
            Value::DictEntry(key, entry) => {
                self.pad(8);
                self.value(key)?;
                self.value(entry)?;
            }
            Value::Variant(inner) => {
                self.signature(&inner.signature())?;
                self.value(inner)?;
            }
        }
        if self.bytes.len() > MAX_MESSAGE_BYTES {
            return Err(protocol("a message is too large"));
        }
        Ok(())
    }
}

/// Reads values in the wire format.
struct Decoder<'a> {
    bytes: &'a [u8],
    at: usize,
    big_endian: bool,
}

impl<'a> Decoder<'a> {
    fn new(bytes: &'a [u8], big_endian: bool) -> Self {
        Decoder {
            bytes,
            at: 0,
            big_endian,
        }
    }

    fn pad(&mut self, boundary: usize) -> Result<(), BusError> {
        let over = self.at % boundary;
        if over != 0 {
            let next = self.at + boundary - over;
            if next > self.bytes.len() {
                return Err(protocol("a message ends in its padding"));
            }
            self.at = next;
        }
        Ok(())
    }

    fn take(&mut self, count: usize) -> Result<&'a [u8], BusError> {
        let end = self
            .at
            .checked_add(count)
            .filter(|end| *end <= self.bytes.len())
            .ok_or_else(|| protocol("a message ends inside a value"))?;
        let slice = &self.bytes[self.at..end];
        self.at = end;
        Ok(slice)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], BusError> {
        self.pad(N)?;
        let slice = self.take(N)?;
        let mut bytes = [0u8; N];
        bytes.copy_from_slice(slice);
        if self.big_endian {
            bytes.reverse();
        }
        Ok(bytes)
    }

    fn u32(&mut self) -> Result<u32, BusError> {
        Ok(u32::from_le_bytes(self.array::<4>()?))
    }

    fn text(&mut self) -> Result<String, BusError> {
        let length = self.u32()? as usize;
        let bytes = self.take(length)?;
        let end = self.take(1)?;
        if end != [0] {
            return Err(protocol("a string does not end with a zero byte"));
        }
        let text = std::str::from_utf8(bytes).map_err(|_| protocol("a string is not UTF-8"))?;
        if text.contains('\0') {
            return Err(protocol("a string holds a zero byte"));
        }
        Ok(text.to_owned())
    }

    fn signature(&mut self) -> Result<String, BusError> {
        let length = usize::from(self.take(1)?[0]);
        let bytes = self.take(length)?;
        let end = self.take(1)?;
        if end != [0] {
            return Err(protocol("a signature does not end with a zero byte"));
        }
        let text = std::str::from_utf8(bytes).map_err(|_| protocol("a signature is not ASCII"))?;
        split_signature(text)?;
        Ok(text.to_owned())
    }

    fn value(&mut self, signature: &str, depth: usize) -> Result<Value, BusError> {
        if depth > 2 * MAX_DEPTH {
            return Err(protocol("a value is nested too deeply"));
        }
        let code = *signature
            .as_bytes()
            .first()
            .ok_or_else(|| protocol("a value has no type"))?;
        Ok(match code {
            b'y' => Value::Byte(self.take(1)?[0]),
            b'b' => match self.u32()? {
                0 => Value::Bool(false),
                1 => Value::Bool(true),
                _ => return Err(protocol("a boolean is neither 0 nor 1")),
            },
            b'n' => Value::I16(i16::from_le_bytes(self.array::<2>()?)),
            b'q' => Value::U16(u16::from_le_bytes(self.array::<2>()?)),
            b'i' => Value::I32(i32::from_le_bytes(self.array::<4>()?)),
            b'u' => Value::U32(self.u32()?),
            b'x' => Value::I64(i64::from_le_bytes(self.array::<8>()?)),
            b't' => Value::U64(u64::from_le_bytes(self.array::<8>()?)),
            b'd' => Value::Double(f64::from_le_bytes(self.array::<8>()?)),
            b's' => Value::Str(self.text()?),
            b'o' => {
                let text = self.text()?;
                if !is_object_path(&text) {
                    return Err(protocol("an object path is not valid"));
                }
                Value::Path(text)
            }
            b'g' => Value::Signature(self.signature()?),
            b'a' => {
                let element = &signature[1..];
                let length = self.u32()? as usize;
                if length > MAX_MESSAGE_BYTES {
                    return Err(protocol("an array is too long"));
                }
                self.pad(alignment(element.as_bytes()[0]))?;
                let end = self
                    .at
                    .checked_add(length)
                    .filter(|end| *end <= self.bytes.len())
                    .ok_or_else(|| protocol("a message ends inside an array"))?;
                let mut items = Vec::new();
                while self.at < end {
                    items.push(self.value(element, depth + 1)?);
                }
                if self.at != end {
                    return Err(protocol("an array does not end where it says"));
                }
                Value::Array {
                    element: element.to_owned(),
                    items,
                }
            }
            b'(' => {
                self.pad(8)?;
                let inner = &signature[1..signature.len() - 1];
                let mut fields = Vec::new();
                for field in split_signature(inner)? {
                    fields.push(self.value(field, depth + 1)?);
                }
                Value::Struct(fields)
            }
            b'{' => {
                self.pad(8)?;
                let inner = &signature[1..signature.len() - 1];
                let types = split_signature(inner)?;
                if types.len() != 2 {
                    return Err(protocol("a dictionary entry does not have two parts"));
                }
                let key = self.value(types[0], depth + 1)?;
                let entry = self.value(types[1], depth + 1)?;
                Value::DictEntry(Box::new(key), Box::new(entry))
            }
            b'v' => {
                let inner = self.signature()?;
                let types = split_signature(&inner)?;
                if types.len() != 1 {
                    return Err(protocol("a variant does not hold one complete type"));
                }
                Value::Variant(Box::new(self.value(types[0], depth + 1)?))
            }
            _ => return Err(protocol("a value of a type that is not supported")),
        })
    }
}

/// The kind of a message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A method call.
    MethodCall,
    /// The reply to a method call.
    MethodReturn,
    /// The error reply to a method call.
    Error,
    /// A signal.
    Signal,
}

impl Kind {
    const fn code(self) -> u8 {
        match self {
            Kind::MethodCall => 1,
            Kind::MethodReturn => 2,
            Kind::Error => 3,
            Kind::Signal => 4,
        }
    }

    const fn from_code(code: u8) -> Option<Kind> {
        match code {
            1 => Some(Kind::MethodCall),
            2 => Some(Kind::MethodReturn),
            3 => Some(Kind::Error),
            4 => Some(Kind::Signal),
            _ => None,
        }
    }
}

/// The flag that says no reply is wanted.
pub const NO_REPLY_EXPECTED: u8 = 0x1;

/// One D-Bus message.
#[derive(Debug, Clone, PartialEq)]
pub struct Message {
    /// What kind of message it is.
    pub kind: Kind,
    /// The flags of the header.
    pub flags: u8,
    /// The serial number. Set by the connection when the message is sent;
    /// the sender's serial on a received message.
    pub serial: u32,
    /// The serial of the call this message replies to.
    pub reply_serial: Option<u32>,
    /// The object path.
    pub path: Option<String>,
    /// The interface.
    pub interface: Option<String>,
    /// The method or the signal.
    pub member: Option<String>,
    /// The error name, for an error.
    pub error_name: Option<String>,
    /// Who the message is for.
    pub destination: Option<String>,
    /// Who sent it: the unique name the bus put there.
    pub sender: Option<String>,
    /// The arguments.
    pub body: Vec<Value>,
}

impl Message {
    fn empty(kind: Kind) -> Message {
        Message {
            kind,
            flags: 0,
            serial: 0,
            reply_serial: None,
            path: None,
            interface: None,
            member: None,
            error_name: None,
            destination: None,
            sender: None,
            body: Vec::new(),
        }
    }

    /// A method call.
    pub fn method_call(
        destination: &str,
        path: &str,
        interface: &str,
        member: &str,
        body: Vec<Value>,
    ) -> Message {
        Message {
            path: Some(path.to_owned()),
            interface: Some(interface.to_owned()),
            member: Some(member.to_owned()),
            destination: Some(destination.to_owned()),
            body,
            ..Message::empty(Kind::MethodCall)
        }
    }

    /// A signal, sent to everybody who listens for it.
    pub fn signal(path: &str, interface: &str, member: &str, body: Vec<Value>) -> Message {
        Message {
            path: Some(path.to_owned()),
            interface: Some(interface.to_owned()),
            member: Some(member.to_owned()),
            body,
            ..Message::empty(Kind::Signal)
        }
    }

    /// The reply to this call.
    pub fn reply(&self, body: Vec<Value>) -> Message {
        Message {
            flags: NO_REPLY_EXPECTED,
            reply_serial: Some(self.serial),
            destination: self.sender.clone(),
            body,
            ..Message::empty(Kind::MethodReturn)
        }
    }

    /// The error reply to this call.
    pub fn error_reply(&self, name: &str, text: &str) -> Message {
        Message {
            flags: NO_REPLY_EXPECTED,
            reply_serial: Some(self.serial),
            destination: self.sender.clone(),
            error_name: Some(name.to_owned()),
            body: vec![Value::string(text)],
            ..Message::empty(Kind::Error)
        }
    }

    /// Whether the sender asked for no reply.
    pub fn wants_reply(&self) -> bool {
        self.kind == Kind::MethodCall && self.flags & NO_REPLY_EXPECTED == 0
    }

    /// The signature of the body.
    pub fn signature(&self) -> String {
        self.body.iter().map(Value::signature).collect()
    }

    /// The first argument, if it is a string.
    pub fn first_string(&self) -> Option<&str> {
        self.body.first().and_then(Value::as_str)
    }

    /// The message in the wire format, with the given serial number.
    ///
    /// # Errors
    ///
    /// A name in the header or a value in the body is not valid, or the
    /// message is larger than [`MAX_MESSAGE_BYTES`].
    pub fn encode(&self, serial: u32) -> Result<Vec<u8>, BusError> {
        if serial == 0 {
            return Err(protocol("a serial number is zero"));
        }
        if let Some(path) = &self.path
            && !is_object_path(path)
        {
            return Err(protocol("the object path of a message is not valid"));
        }
        for name in [&self.interface, &self.error_name].into_iter().flatten() {
            if !is_interface_name(name) {
                return Err(protocol(
                    "an interface or error name of a message is not valid",
                ));
            }
        }
        if let Some(member) = &self.member
            && !is_member_name(member)
        {
            return Err(protocol("the member of a message is not valid"));
        }
        let complete = match self.kind {
            Kind::MethodCall => self.path.is_some() && self.member.is_some(),
            Kind::Signal => {
                self.path.is_some() && self.interface.is_some() && self.member.is_some()
            }
            Kind::MethodReturn => self.reply_serial.is_some(),
            Kind::Error => self.reply_serial.is_some() && self.error_name.is_some(),
        };
        if !complete {
            return Err(protocol("a message lacks a field its kind requires"));
        }

        let mut body = Encoder::new();
        for value in &self.body {
            body.value(value)?;
        }
        let signature = self.signature();

        let field = |code: u8, value: Value| {
            Value::Struct(vec![Value::Byte(code), Value::Variant(Box::new(value))])
        };
        let mut fields = Vec::new();
        if let Some(path) = &self.path {
            fields.push(field(1, Value::Path(path.clone())));
        }
        if let Some(interface) = &self.interface {
            fields.push(field(2, Value::Str(interface.clone())));
        }
        if let Some(member) = &self.member {
            fields.push(field(3, Value::Str(member.clone())));
        }
        if let Some(name) = &self.error_name {
            fields.push(field(4, Value::Str(name.clone())));
        }
        if let Some(serial) = self.reply_serial {
            fields.push(field(5, Value::U32(serial)));
        }
        if let Some(destination) = &self.destination {
            fields.push(field(6, Value::Str(destination.clone())));
        }
        if !signature.is_empty() {
            fields.push(field(8, Value::Signature(signature)));
        }

        let body_length =
            u32::try_from(body.bytes.len()).map_err(|_| protocol("a message is too large"))?;
        let mut out = Encoder::new();
        out.bytes
            .extend_from_slice(&[b'l', self.kind.code(), self.flags, 1]);
        out.u32(body_length);
        out.u32(serial);
        out.value(&Value::Array {
            element: "(yv)".to_owned(),
            items: fields,
        })?;
        out.pad(8);
        out.bytes.extend_from_slice(&body.bytes);
        if out.bytes.len() > MAX_MESSAGE_BYTES {
            return Err(protocol("a message is too large"));
        }
        Ok(out.bytes)
    }

    /// Read a message from its wire format.
    ///
    /// # Errors
    ///
    /// The bytes are not one whole, valid message.
    pub fn decode(bytes: &[u8]) -> Result<Message, BusError> {
        if bytes.len() < 16 {
            return Err(protocol("a message is shorter than its header"));
        }
        let big_endian = match bytes[0] {
            b'l' => false,
            b'B' => true,
            _ => return Err(protocol("a message does not say its byte order")),
        };
        let kind =
            Kind::from_code(bytes[1]).ok_or_else(|| protocol("an unknown kind of message"))?;
        if bytes[3] != 1 {
            return Err(protocol("an unknown version of the protocol"));
        }
        let mut header = Decoder::new(bytes, big_endian);
        header.at = 4;
        let body_length = header.u32()? as usize;
        let serial = header.u32()?;
        if serial == 0 {
            return Err(protocol("a serial number is zero"));
        }
        let fields = header.value("a(yv)", 0)?;
        header.pad(8)?;
        let body_start = header.at;
        if bytes.len() - body_start != body_length {
            return Err(protocol("a message is not as long as it says"));
        }

        let mut message = Message {
            flags: bytes[2],
            serial,
            ..Message::empty(kind)
        };
        let mut signature = String::new();
        for field in fields.as_array().unwrap_or_default() {
            let Some([Value::Byte(code), Value::Variant(value)]) = field.as_struct() else {
                return Err(protocol("a header field is not a code and a value"));
            };
            let text = || value.as_str().map(str::to_owned);
            let wrong = || protocol("a header field has the wrong type");
            match (*code, &**value) {
                (1, Value::Path(_)) => message.path = text(),
                (2, Value::Str(_)) => message.interface = text(),
                (3, Value::Str(_)) => message.member = text(),
                (4, Value::Str(_)) => message.error_name = text(),
                (5, Value::U32(reply)) => message.reply_serial = Some(*reply),
                (6, Value::Str(_)) => message.destination = text(),
                (7, Value::Str(_)) => message.sender = text(),
                (8, Value::Signature(text)) => signature.clone_from(text),
                (1..=8, _) => return Err(wrong()),
                // A file descriptor count, or a field of a later protocol.
                _ => {}
            }
        }

        let mut body = Decoder::new(&bytes[body_start..], big_endian);
        for part in split_signature(&signature)? {
            message.body.push(body.value(part, 0)?);
        }
        if body.at != body_length {
            return Err(protocol(
                "the body of a message is longer than its signature says",
            ));
        }
        Ok(message)
    }
}

/// Read one whole message from a stream.
fn read_message(stream: &mut impl Read) -> Result<Message, BusError> {
    let closed = |_| BusError::Closed;
    let mut fixed = [0u8; 16];
    stream.read_exact(&mut fixed).map_err(closed)?;
    let word = |at: usize| {
        let bytes = [fixed[at], fixed[at + 1], fixed[at + 2], fixed[at + 3]];
        if fixed[0] == b'B' {
            u32::from_be_bytes(bytes) as usize
        } else {
            u32::from_le_bytes(bytes) as usize
        }
    };
    let body_length = word(4);
    let fields_length = word(12);
    let header_length = 16usize
        .checked_add(fields_length)
        .map(|length| length.div_ceil(8) * 8)
        .ok_or_else(|| protocol("a message is too large"))?;
    let total = header_length
        .checked_add(body_length)
        .filter(|total| *total <= MAX_MESSAGE_BYTES)
        .ok_or_else(|| protocol("a message is too large"))?;
    let mut bytes = vec![0u8; total];
    bytes[..16].copy_from_slice(&fixed);
    stream.read_exact(&mut bytes[16..]).map_err(closed)?;
    Message::decode(&bytes)
}

/// The value of one key of a bus address, with its escapes undone.
fn unescape(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] == b'%' {
            let hex = value.get(at + 1..at + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            at += 3;
        } else {
            out.push(bytes[at]);
            at += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// Connect to the first reachable Unix socket of a bus address.
fn connect(address: &str) -> Result<UnixStream, BusError> {
    let mut last = String::from("the address names no Unix socket");
    for entry in address.split(';') {
        let Some(rest) = entry.strip_prefix("unix:") else {
            continue;
        };
        for pair in rest.split(',') {
            let Some((key, value)) = pair.split_once('=') else {
                continue;
            };
            let Some(value) = unescape(value) else {
                continue;
            };
            let attempt = match key {
                "path" => UnixStream::connect(&value),
                "abstract" => SocketAddr::from_abstract_name(value.as_bytes())
                    .and_then(|socket| UnixStream::connect_addr(&socket)),
                _ => continue,
            };
            match attempt {
                Ok(stream) => return Ok(stream),
                Err(error) => last = error.to_string(),
            }
        }
    }
    Err(BusError::Unavailable(last))
}

/// The `EXTERNAL` authentication: the bus reads who we are from the socket
/// itself, and we say which user we claim to be.
fn authenticate(stream: &mut UnixStream) -> Result<(), BusError> {
    let refused = |detail: &str| BusError::Unavailable(detail.to_owned());
    let user = std::fs::metadata("/proc/self")
        .map(|metadata| metadata.uid())
        .map_err(|_| refused("the user of this process could not be read"))?;
    let hex: String = user
        .to_string()
        .bytes()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    stream
        .write_all(format!("\0AUTH EXTERNAL {hex}\r\n").as_bytes())
        .map_err(|_| refused("the bus closed the connection"))?;
    // The answer is one line. It is read a byte at a time so that nothing
    // after it is taken from the stream.
    let mut line = Vec::new();
    let mut byte = [0u8; 1];
    while !line.ends_with(b"\r\n") {
        if line.len() > 512 {
            return Err(refused("the bus did not answer the authentication"));
        }
        stream
            .read_exact(&mut byte)
            .map_err(|_| refused("the bus closed the connection"))?;
        line.push(byte[0]);
    }
    if !line.starts_with(b"OK ") {
        return Err(refused("the bus did not accept this user"));
    }
    stream
        .write_all(b"BEGIN\r\n")
        .map_err(|_| refused("the bus closed the connection"))
}

fn locked<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

struct Inner {
    writer: Mutex<UnixStream>,
    next_serial: AtomicU32,
    pending: Mutex<HashMap<u32, Sender<Message>>>,
    closed: AtomicBool,
    name: Mutex<String>,
}

/// A connection to a message bus. Clones share the connection.
#[derive(Clone)]
pub struct Connection {
    inner: Arc<Inner>,
}

impl fmt::Debug for Connection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Connection")
            .field("name", &self.unique_name())
            .finish_non_exhaustive()
    }
}

/// How long a call to the bus itself may take.
const BUS_CALL_TIMEOUT: Duration = Duration::from_secs(25);

impl Connection {
    /// Connect to the bus at `address`, for example
    /// `unix:path=/run/user/1000/bus`, and say hello.
    ///
    /// Returns the connection and the channel on which everything arrives
    /// that is not the reply to a call made through the connection: method
    /// calls and signals. The channel ends when the connection does.
    ///
    /// # Errors
    ///
    /// The bus cannot be reached or does not accept this user.
    pub fn open(address: &str) -> Result<(Connection, Receiver<Message>), BusError> {
        let mut stream = connect(address)?;
        authenticate(&mut stream)?;
        let mut reader = stream
            .try_clone()
            .map_err(|error| BusError::Unavailable(error.to_string()))?;
        let connection = Connection {
            inner: Arc::new(Inner {
                writer: Mutex::new(stream),
                next_serial: AtomicU32::new(1),
                pending: Mutex::new(HashMap::new()),
                closed: AtomicBool::new(false),
                name: Mutex::new(String::new()),
            }),
        };
        let (incoming, received) = mpsc::channel();
        let inner = Arc::clone(&connection.inner);
        thread::Builder::new()
            .name("tur-pax-bus".to_owned())
            .spawn(move || {
                while let Ok(message) = read_message(&mut reader) {
                    let reply_to = match message.kind {
                        Kind::MethodReturn | Kind::Error => message.reply_serial,
                        Kind::MethodCall | Kind::Signal => None,
                    };
                    match reply_to {
                        Some(serial) => {
                            if let Some(waiting) = locked(&inner.pending).remove(&serial) {
                                let _ = waiting.send(message);
                            }
                        }
                        // Nobody may be listening: a client that only calls.
                        None => {
                            let _ = incoming.send(message);
                        }
                    }
                }
                inner.closed.store(true, Ordering::SeqCst);
                // Whoever waits for a reply is told that none will come.
                locked(&inner.pending).clear();
            })
            .map_err(|error| BusError::Unavailable(error.to_string()))?;

        let hello = connection.call(
            Message::method_call(BUS_NAME, BUS_PATH, BUS_NAME, "Hello", Vec::new()),
            BUS_CALL_TIMEOUT,
        )?;
        let name = hello
            .first_string()
            .ok_or_else(|| protocol("the bus did not give this connection a name"))?
            .to_owned();
        *locked(&connection.inner.name) = name;
        Ok((connection, received))
    }

    /// The unique name the bus gave this connection.
    pub fn unique_name(&self) -> String {
        locked(&self.inner.name).clone()
    }

    /// Whether the connection has ended.
    pub fn is_closed(&self) -> bool {
        self.inner.closed.load(Ordering::SeqCst)
    }

    fn write(&self, message: &Message) -> Result<u32, BusError> {
        if self.is_closed() {
            return Err(BusError::Closed);
        }
        // The serial is taken and the bytes are written under one lock, so
        // that messages leave in the order of their serials.
        let mut writer = locked(&self.inner.writer);
        let mut serial = self.inner.next_serial.fetch_add(1, Ordering::SeqCst);
        if serial == 0 {
            serial = self.inner.next_serial.fetch_add(1, Ordering::SeqCst);
        }
        let bytes = message.encode(serial)?;
        writer
            .write_all(&bytes)
            .and_then(|()| writer.flush())
            .map_err(|_| BusError::Closed)?;
        Ok(serial)
    }

    /// Send a message that expects no reply: a signal, a reply or an error.
    ///
    /// # Errors
    ///
    /// The message is not valid or the connection is closed.
    pub fn send(&self, message: &Message) -> Result<(), BusError> {
        self.write(message).map(|_| ())
    }

    /// Call a method and wait for its reply.
    ///
    /// Must not be called from the thread that reads the connection; no code
    /// outside this module runs on it.
    ///
    /// # Errors
    ///
    /// [`BusError::Remote`] if the peer answered with an error,
    /// [`BusError::TimedOut`] if nothing came back in time.
    pub fn call(&self, message: Message, timeout: Duration) -> Result<Message, BusError> {
        let (sender, receiver) = mpsc::channel();
        let serial = {
            // Registered before the bytes leave, so that a quick reply
            // finds its waiter.
            let mut pending = locked(&self.inner.pending);
            let mut writer = locked(&self.inner.writer);
            if self.is_closed() {
                return Err(BusError::Closed);
            }
            let mut serial = self.inner.next_serial.fetch_add(1, Ordering::SeqCst);
            if serial == 0 {
                serial = self.inner.next_serial.fetch_add(1, Ordering::SeqCst);
            }
            let bytes = message.encode(serial)?;
            pending.insert(serial, sender);
            if writer
                .write_all(&bytes)
                .and_then(|()| writer.flush())
                .is_err()
            {
                pending.remove(&serial);
                return Err(BusError::Closed);
            }
            serial
        };
        match receiver.recv_timeout(timeout) {
            Ok(reply) if reply.kind == Kind::Error => Err(BusError::Remote {
                name: reply.error_name.clone().unwrap_or_default(),
                message: reply.first_string().unwrap_or_default().to_owned(),
            }),
            Ok(reply) => Ok(reply),
            Err(RecvTimeoutError::Timeout) => {
                locked(&self.inner.pending).remove(&serial);
                Err(BusError::TimedOut)
            }
            Err(RecvTimeoutError::Disconnected) => Err(BusError::Closed),
        }
    }

    /// Ask the bus for a well-known name. `Ok(true)` if this connection now
    /// owns it; `Ok(false)` if another connection does.
    ///
    /// # Errors
    ///
    /// The bus refused or did not answer.
    pub fn request_name(&self, name: &str) -> Result<bool, BusError> {
        // Flag 4: do not wait in the queue for the name.
        let reply = self.call(
            Message::method_call(
                BUS_NAME,
                BUS_PATH,
                BUS_NAME,
                "RequestName",
                vec![Value::string(name), Value::U32(4)],
            ),
            BUS_CALL_TIMEOUT,
        )?;
        // 1: now the primary owner. 4: already the owner.
        Ok(matches!(
            reply.body.first().and_then(Value::as_u32),
            Some(1 | 4)
        ))
    }

    /// Ask the bus to deliver the messages a match rule describes.
    ///
    /// # Errors
    ///
    /// The bus refused the rule or did not answer.
    pub fn add_match(&self, rule: &str) -> Result<(), BusError> {
        self.call(
            Message::method_call(
                BUS_NAME,
                BUS_PATH,
                BUS_NAME,
                "AddMatch",
                vec![Value::string(rule)],
            ),
            BUS_CALL_TIMEOUT,
        )
        .map(|_| ())
    }

    /// The Unix user of the connection that owns a bus name, as the bus
    /// itself reports it.
    ///
    /// # Errors
    ///
    /// The bus does not know the name or did not answer.
    pub fn unix_user_of(&self, name: &str) -> Result<u32, BusError> {
        let reply = self.call(
            Message::method_call(
                BUS_NAME,
                BUS_PATH,
                BUS_NAME,
                "GetConnectionUnixUser",
                vec![Value::string(name)],
            ),
            BUS_CALL_TIMEOUT,
        )?;
        reply
            .body
            .first()
            .and_then(Value::as_u32)
            .ok_or_else(|| protocol("the bus did not say which user a connection belongs to"))
    }
}

/// The user this process runs as, read from the kernel's view of the
/// process. `None` if it cannot be read.
pub fn own_user() -> Option<u32> {
    std::fs::metadata("/proc/self")
        .map(|metadata| metadata.uid())
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signatures_are_split_into_complete_types() {
        assert_eq!(split_signature("").expect("valid"), Vec::<&str>::new());
        assert_eq!(split_signature("s").expect("valid"), ["s"]);
        assert_eq!(
            split_signature("susssasa{sv}i").expect("valid"),
            ["s", "u", "s", "s", "s", "as", "a{sv}", "i"]
        );
        assert_eq!(
            split_signature("a{sv}(oayays)b").expect("valid"),
            ["a{sv}", "(oayays)", "b"]
        );
        assert_eq!(
            split_signature("aa{ss}(a(yv))").expect("valid"),
            ["aa{ss}", "(a(yv))"]
        );
        for bad in [
            "a", "(", ")", "()", "{ss}", "a{vs}", "a{s}", "a{sss}", "(s", "z", "s s", "a{ss",
            "a{(s)s}", "({ss})",
        ] {
            assert!(split_signature(bad).is_err(), "{bad:?}");
        }
        let deep = format!("{}s", "a".repeat(MAX_DEPTH + 2));
        assert!(split_signature(&deep).is_err());
        let long = "s".repeat(256);
        assert!(split_signature(&long).is_err());
    }

    #[test]
    fn names_are_checked() {
        for good in ["/", "/org/tur/Agent1", "/a/b_c/d1"] {
            assert!(is_object_path(good), "{good}");
        }
        for bad in ["", "org", "/org/", "//", "/a//b", "/a-b", "/a b", "/ä"] {
            assert!(!is_object_path(bad), "{bad:?}");
        }
        for good in ["org.tur.Agent1", "a.b", "org.freedesktop.DBus.Error.Failed"] {
            assert!(is_interface_name(good), "{good}");
        }
        for bad in [
            "",
            "org",
            "org.",
            ".org",
            "org..tur",
            "org.1tur",
            "org.tur-x",
            "a b.c",
        ] {
            assert!(!is_interface_name(bad), "{bad:?}");
        }
        for good in ["StartTask", "a", "Get_1"] {
            assert!(is_member_name(good), "{good}");
        }
        for bad in ["", "1a", "a.b", "a-b", "a b"] {
            assert!(!is_member_name(bad), "{bad:?}");
        }
    }

    #[test]
    fn the_hello_call_is_the_bytes_the_specification_gives() {
        let hello = Message::method_call(BUS_NAME, BUS_PATH, BUS_NAME, "Hello", Vec::new());
        let bytes = hello.encode(1).expect("encodes");
        let mut expected: Vec<u8> = Vec::new();
        // Little-endian, method call, no flags, version 1; body 0 bytes;
        // serial 1; 109 bytes of header fields.
        expected.extend_from_slice(&[b'l', 1, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0x6d, 0, 0, 0]);
        // Field 1, the path: an object path of 21 bytes.
        expected.extend_from_slice(&[1, 1, b'o', 0, 21, 0, 0, 0]);
        expected.extend_from_slice(b"/org/freedesktop/DBus\0\0\0");
        // Field 2, the interface: a string of 20 bytes.
        expected.extend_from_slice(&[2, 1, b's', 0, 20, 0, 0, 0]);
        expected.extend_from_slice(b"org.freedesktop.DBus\0\0\0\0");
        // Field 3, the member: a string of 5 bytes.
        expected.extend_from_slice(&[3, 1, b's', 0, 5, 0, 0, 0]);
        expected.extend_from_slice(b"Hello\0\0\0");
        // Field 6, the destination.
        expected.extend_from_slice(&[6, 1, b's', 0, 20, 0, 0, 0]);
        expected.extend_from_slice(b"org.freedesktop.DBus\0");
        // Padding to the 8-byte boundary where the (empty) body starts.
        expected.extend_from_slice(&[0, 0, 0]);
        assert_eq!(bytes.len(), 128);
        assert_eq!(bytes, expected);

        let back = Message::decode(&bytes).expect("decodes");
        assert_eq!(back.kind, Kind::MethodCall);
        assert_eq!(back.serial, 1);
        assert_eq!(back.path.as_deref(), Some(BUS_PATH));
        assert_eq!(back.member.as_deref(), Some("Hello"));
        assert_eq!(back.destination.as_deref(), Some(BUS_NAME));
        assert!(back.body.is_empty());
        assert!(back.wants_reply());
    }

    fn every_kind_of_value() -> Vec<Value> {
        vec![
            Value::Byte(7),
            Value::Bool(true),
            Value::I16(-2),
            Value::U16(65_535),
            Value::I32(-1),
            Value::U32(4_000_000_000),
            Value::I64(-9_000_000_000),
            Value::U64(18_000_000_000_000_000_000),
            Value::Double(1.5),
            Value::string("P@x and \u{1f525}"),
            Value::path("/org/tur/Agent1"),
            Value::Signature("a{sv}".to_owned()),
            Value::strings(["one", "two", ""]),
            Value::strings(Vec::<String>::new()),
            Value::bytes(&[1, 2, 3]),
            Value::string_map(&[("service", "tur-pax"), ("provider", "anthropic")]),
            Value::variant_map(vec![
                ("urgency", Value::Byte(1)),
                ("desktop-entry", Value::string("org.tur.Pax")),
                ("nested", Value::strings(["a"])),
            ]),
            Value::Struct(vec![
                Value::path("/"),
                Value::bytes(&[]),
                Value::bytes(b"secret"),
                Value::string("text/plain"),
            ]),
            Value::variant(Value::variant(Value::U64(1))),
            Value::Array {
                element: "(yv)".to_owned(),
                items: vec![Value::Struct(vec![
                    Value::Byte(1),
                    Value::variant(Value::Bool(false)),
                ])],
            },
        ]
    }

    #[test]
    fn every_kind_of_value_survives_the_wire_at_every_alignment() {
        let values = every_kind_of_value();
        // Each value alone, and after 0 to 7 bytes, so that its padding is
        // exercised at every offset.
        for value in &values {
            for lead in 0..8usize {
                let mut body: Vec<Value> = (0..lead).map(|_| Value::Byte(0xaa)).collect();
                body.push(value.clone());
                let message = Message::signal("/org/tur/Agent1", "org.tur.Agent1", "Test", body);
                let bytes = message.encode(9).expect("encodes");
                let back = Message::decode(&bytes).expect("decodes");
                assert_eq!(
                    back.body.last(),
                    Some(value),
                    "{value:?} after {lead} bytes"
                );
                assert_eq!(back.body.len(), lead + 1);
                assert_eq!(back.signature(), message.signature());
            }
        }
        // All of them in one body.
        let message = Message::signal("/org/tur/Agent1", "org.tur.Agent1", "Test", values.clone());
        let back = Message::decode(&message.encode(2).expect("encodes")).expect("decodes");
        assert_eq!(back.body, values);
        assert_eq!(back.kind, Kind::Signal);
        assert_eq!(back.interface.as_deref(), Some("org.tur.Agent1"));
    }

    #[test]
    fn a_big_endian_message_is_read() {
        // The reply the bus would give to Hello, written big-endian by hand:
        // one string, ":1.5".
        let mut bytes: Vec<u8> = Vec::new();
        bytes.extend_from_slice(&[b'B', 2, 1, 1]);
        bytes.extend_from_slice(&9u32.to_be_bytes()); // body length
        bytes.extend_from_slice(&3u32.to_be_bytes()); // serial
        bytes.extend_from_slice(&15u32.to_be_bytes()); // header fields
        // Field 5, the reply serial 1.
        bytes.extend_from_slice(&[5, 1, b'u', 0]);
        bytes.extend_from_slice(&1u32.to_be_bytes());
        // Field 8, the signature "s".
        bytes.extend_from_slice(&[8, 1, b'g', 0, 1, b's', 0]);
        bytes.push(0); // padding to 8
        bytes.extend_from_slice(&4u32.to_be_bytes());
        bytes.extend_from_slice(b":1.5\0");
        let message = Message::decode(&bytes).expect("decodes");
        assert_eq!(message.kind, Kind::MethodReturn);
        assert_eq!(message.reply_serial, Some(1));
        assert_eq!(message.first_string(), Some(":1.5"));
        assert!(!message.wants_reply());
    }

    #[test]
    fn a_damaged_message_is_refused_and_never_panics() {
        let message = Message::method_call(
            "org.freedesktop.Notifications",
            "/org/freedesktop/Notifications",
            "org.freedesktop.Notifications",
            "Notify",
            every_kind_of_value(),
        );
        let bytes = message.encode(77).expect("encodes");
        assert_eq!(Message::decode(&bytes).expect("decodes").body, message.body);
        // Cut short at every length.
        for length in 0..bytes.len() {
            assert!(
                Message::decode(&bytes[..length]).is_err(),
                "cut at {length}"
            );
        }
        // One byte too long.
        let mut longer = bytes.clone();
        longer.push(0);
        assert!(Message::decode(&longer).is_err());
        // Every byte changed in three ways: the result is an error or a
        // message, never a panic.
        for index in 0..bytes.len() {
            for change in [0x00u8, 0xff, 0x80] {
                let mut damaged = bytes.clone();
                damaged[index] ^= change | 1;
                let _ = Message::decode(&damaged);
            }
        }
        // An array that claims more than there is.
        let mut claims = Message::signal("/a", "a.b", "C", vec![Value::strings(["x"])])
            .encode(1)
            .expect("encodes");
        // The body is the array's length, then one string of one byte:
        // ten bytes in all.
        let body = claims.len() - 10;
        claims[body..body + 4].copy_from_slice(&0x00ff_ffffu32.to_le_bytes());
        assert!(Message::decode(&claims).is_err());
    }

    #[test]
    fn a_message_that_is_not_valid_is_not_sent() {
        let call = |path: &str, interface: &str, member: &str| {
            Message::method_call("a.b", path, interface, member, Vec::new()).encode(1)
        };
        assert!(call("/a", "a.b", "C").is_ok());
        assert!(call("a", "a.b", "C").is_err());
        assert!(call("/a", "ab", "C").is_err());
        assert!(call("/a", "a.b", "C.D").is_err());
        assert!(call("/a", "a.b", "C").is_ok());
        // Serial zero, a zero byte in a string, a wrong array element, an
        // invalid path value, an empty struct.
        assert!(
            Message::signal("/a", "a.b", "C", Vec::new())
                .encode(0)
                .is_err()
        );
        for body in [
            vec![Value::string("a\0b")],
            vec![Value::Array {
                element: "s".to_owned(),
                items: vec![Value::U32(1)],
            }],
            vec![Value::path("not/a/path")],
            vec![Value::Struct(Vec::new())],
            vec![Value::Signature("a".to_owned())],
        ] {
            assert!(Message::signal("/a", "a.b", "C", body).encode(1).is_err());
        }
        // A reply needs the serial it answers; an error needs its name too.
        let mut orphan = Message::empty(Kind::MethodReturn);
        assert!(orphan.encode(1).is_err());
        orphan.reply_serial = Some(4);
        assert!(orphan.encode(1).is_ok());
    }

    #[test]
    fn replies_are_addressed_to_the_caller() {
        let mut call = Message::method_call(
            "org.tur.Agent1",
            "/org/tur/Agent1",
            "org.tur.Agent1",
            "GetStatus",
            Vec::new(),
        );
        call.serial = 12;
        call.sender = Some(":1.42".to_owned());
        let reply = call.reply(vec![Value::string("{}")]);
        assert_eq!(reply.kind, Kind::MethodReturn);
        assert_eq!(reply.reply_serial, Some(12));
        assert_eq!(reply.destination.as_deref(), Some(":1.42"));
        let error = call.error_reply("org.tur.Agent1.Error.Disabled", "off");
        assert_eq!(error.kind, Kind::Error);
        assert_eq!(
            error.error_name.as_deref(),
            Some("org.tur.Agent1.Error.Disabled")
        );
        assert_eq!(error.first_string(), Some("off"));
        let back = Message::decode(&error.encode(3).expect("encodes")).expect("decodes");
        assert_eq!(back.error_name, error.error_name);
        assert_eq!(back.reply_serial, Some(12));
    }

    #[test]
    fn a_bus_address_is_read_with_its_escapes() {
        assert_eq!(
            unescape("/run/user/1000/bus").as_deref(),
            Some("/run/user/1000/bus")
        );
        assert_eq!(unescape("/tmp/a%20b").as_deref(), Some("/tmp/a b"));
        assert_eq!(unescape("%2Ftmp%2fx").as_deref(), Some("/tmp/x"));
        assert_eq!(unescape("%"), None);
        assert_eq!(unescape("%zz"), None);
        // No Unix socket in the address, and a socket that is not there.
        assert!(matches!(
            connect("tcp:host=localhost,port=1"),
            Err(BusError::Unavailable(_))
        ));
        assert!(matches!(
            connect("unix:path=/nonexistent/tur-pax-test/bus"),
            Err(BusError::Unavailable(_))
        ));
        assert!(matches!(
            Connection::open("unix:path=/nonexistent/tur-pax-test/bus;unix:tmpdir=/tmp"),
            Err(BusError::Unavailable(_))
        ));
        assert!(own_user().is_some());
    }

    #[test]
    fn the_errors_say_what_happened() {
        let unknown = BusError::Remote {
            name: "org.freedesktop.DBus.Error.ServiceUnknown".to_owned(),
            message: "The name is not activatable".to_owned(),
        };
        assert!(unknown.is_no_such_service());
        assert!(!BusError::TimedOut.is_no_such_service());
        assert!(unknown.to_string().contains("ServiceUnknown"));
        assert!(BusError::Closed.to_string().contains("closed"));
        assert!(
            BusError::Unavailable("x".to_owned())
                .to_string()
                .contains("not available")
        );
    }
}
