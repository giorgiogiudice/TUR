// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The policy store of P@x: where the person's policy and the standing
//! approvals of their schedules are kept, in a place P@x's own process cannot
//! write (AI control layer design, section 17 item 5, section 18 item 1,
//! section 22 item 2).
//!
//! # Where, and who can write it
//!
//! ```text
//! /var/lib/tur-pax-policy/            root:root 0755
//!     <uid>/                          root:<the person's group> 0750
//!         policy.json                 root:<the person's group> 0640
//!         approvals.json              root:<the person's group> 0640
//! ```
//!
//! The person, and so P@x, which runs as the person, can read their own
//! directory and nothing else of it; only root can write it. The program
//! `tur-pax-policy` writes it as root, started through `pkexec`, so every
//! change passes polkit: the action `org.tur.pax.policy.change` asks for an
//! administrator's password each time; `org.tur.pax.policy.tighten` (taking
//! standing approvals away) asks for nothing, because it only makes P@x
//! stricter. P@x's commands run in a sandbox with no bus and with
//! `no_new_privs`, so they can start neither.
//!
//! This is not yet the trusted screen of the design (section 19, item 2;
//! desktop task D-16), which is where loosening is to be confirmed: the
//! polkit prompt is drawn on the person's desktop. It does keep the store out
//! of reach of P@x's process and of every other program of the person
//! without the password. When the trusted screen exists it takes the place
//! of the polkit prompt; the store stays.
//!
//! # What is in it
//!
//! * `policy.json`: the person's policy, in the format of the policy file of
//!   `tur-pax` (`schemas/agent-policy.schema.json`). This program checks
//!   its outline; `tur-pax` reads it completely and, if it cannot use it,
//!   lets P@x observe only. P@x applies it together with the person's own
//!   file, which can only make it stricter.
//! * `approvals.json`: the standing approvals of schedules ([`Approvals`]).
//!   Each set is bound to a schedule's identifier **and** the exact text of
//!   its task, so an approval does not carry over to a schedule whose task
//!   was changed in the person's own store, which P@x's process can write.
//!   **They are not honoured yet**: the policy engine refuses every class 2
//!   action of an unattended task until a proven trusted session exists
//!   (rule 5), and the task loop does not read this file. Reading it into
//!   the engine is the reviewed step that comes with the trusted screen.
//!
//! Schedules that carry no approval stay in the person's own store
//! (`tur-pax-memory`): a schedule only says when a task runs, and the task
//! runs with classes 0 and 1 only.

use std::fmt;
use std::fs;
use std::io::{self, Read, Write};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::de::{self, Deserializer, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Number, Value};

/// The directory of the system's store.
pub const STORE_ROOT: &str = "/var/lib/tur-pax-policy";

/// The person's policy, in their directory.
pub const POLICY_FILE: &str = "policy.json";

/// The standing approvals, in their directory.
pub const APPROVALS_FILE: &str = "approvals.json";

/// The largest document the store accepts, in bytes.
pub const MAX_DOCUMENT_BYTES: usize = 64 * 1024;

/// The version of both documents.
pub const SCHEMA_VERSION: u64 = 1;

/// The most schedules with approvals one person may have.
pub const MAX_SCHEDULES: usize = 64;

/// The most approvals one schedule may carry.
pub const MAX_APPROVALS: usize = 32;

/// The longest task text, in bytes: that of `tur-pax-memory`.
pub const MAX_TASK_BYTES: usize = 4000;

/// The modes of the policy file.
const MODES: [&str; 3] = ["normal", "ask_every_action", "observe_only"];

/// The fields of the policy file.
const POLICY_FIELDS: [&str; 5] = [
    "schema_version",
    "mode",
    "excluded_applications",
    "sensitive_applications",
    "relaxations",
];

/// Why the store could not be read or written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreError {
    /// A document does not have the form the store accepts. Nothing was
    /// written.
    Invalid(String),
    /// The file system refused, or the store is not as it must be.
    Io(String),
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StoreError::Invalid(what) => write!(f, "the document is not accepted: {what}"),
            StoreError::Io(what) => write!(f, "the policy store cannot be used: {what}"),
        }
    }
}

impl std::error::Error for StoreError {}

fn invalid(what: impl Into<String>) -> StoreError {
    StoreError::Invalid(what.into())
}

fn io_error(what: &str, error: &io::Error) -> StoreError {
    StoreError::Io(format!("{what}: {:?}", error.kind()))
}

// ---- strict JSON ---------------------------------------------------------------

/// A JSON value with no repeated key in any of its objects.
///
/// The project's strict parser lives in the provider layer
/// (`tur_pax_providers::strict`). This program runs as root when it writes,
/// and is kept from linking an HTTP client and a TLS stack, so it carries
/// the same rule here: a document in which an object repeats a key, at any
/// depth, means two things to two readers and is refused.
struct Strict(Value);

impl<'de> Deserialize<'de> for Strict {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(StrictVisitor).map(Strict)
    }
}

struct StrictVisitor;

impl<'de> Visitor<'de> for StrictVisitor {
    type Value = Value;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a JSON value")
    }

    fn visit_bool<E: de::Error>(self, value: bool) -> Result<Value, E> {
        Ok(Value::Bool(value))
    }

    fn visit_i64<E: de::Error>(self, value: i64) -> Result<Value, E> {
        Ok(Value::Number(Number::from(value)))
    }

    fn visit_u64<E: de::Error>(self, value: u64) -> Result<Value, E> {
        Ok(Value::Number(Number::from(value)))
    }

    fn visit_f64<E: de::Error>(self, value: f64) -> Result<Value, E> {
        Ok(Number::from_f64(value).map_or(Value::Null, Value::Number))
    }

    fn visit_str<E: de::Error>(self, value: &str) -> Result<Value, E> {
        Ok(Value::String(value.to_owned()))
    }

    fn visit_string<E: de::Error>(self, value: String) -> Result<Value, E> {
        Ok(Value::String(value))
    }

    fn visit_unit<E: de::Error>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut access: A) -> Result<Value, A::Error> {
        let mut items = Vec::new();
        while let Some(Strict(item)) = access.next_element()? {
            items.push(item);
        }
        Ok(Value::Array(items))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut access: A) -> Result<Value, A::Error> {
        let mut object = Map::new();
        while let Some(key) = access.next_key::<String>()? {
            let Strict(value) = access.next_value()?;
            if object.insert(key, value).is_some() {
                return Err(de::Error::custom("an object repeats a key"));
            }
        }
        Ok(Value::Object(object))
    }
}

/// Parse a document, refusing a repeated key at any depth and a document
/// larger than [`MAX_DOCUMENT_BYTES`]. The error never quotes the document.
///
/// ```
/// use tur_pax_policy::strict_value;
///
/// assert!(strict_value(r#"{"a": [1, {"b": 2}]}"#).is_ok());
/// assert!(strict_value(r#"{"a": 1, "a": 2}"#).is_err());
/// assert!(strict_value(r#"[{"a": {"b": 1, "b": 1}}]"#).is_err());
/// ```
///
/// # Errors
///
/// [`StoreError::Invalid`].
pub fn strict_value(text: &str) -> Result<Value, StoreError> {
    if text.len() > MAX_DOCUMENT_BYTES {
        return Err(invalid(format!(
            "it is larger than {MAX_DOCUMENT_BYTES} bytes"
        )));
    }
    let mut deserializer = serde_json::Deserializer::from_str(text);
    let Strict(value) = Strict::deserialize(&mut deserializer).map_err(|error| {
        if error.to_string().contains("repeats a key") {
            invalid("an object in it repeats a key")
        } else {
            invalid(format!(
                "it is not valid JSON (line {}, column {})",
                error.line(),
                error.column()
            ))
        }
    })?;
    deserializer
        .end()
        .map_err(|_| invalid("there is more after the JSON document"))?;
    Ok(value)
}

// ---- the policy ----------------------------------------------------------------

/// Check the outline of a policy document and return it as it will be
/// stored. `tur-pax` reads the content completely; what is checked here is
/// what keeps the file a policy file: an object, the version, a known mode,
/// no field the format does not have, lists where lists go.
///
/// # Errors
///
/// [`StoreError::Invalid`].
pub fn check_policy(text: &str) -> Result<Value, StoreError> {
    let value = strict_value(text)?;
    let Value::Object(object) = &value else {
        return Err(invalid("a policy is a JSON object"));
    };
    if object.get("schema_version").and_then(Value::as_u64) != Some(SCHEMA_VERSION) {
        return Err(invalid("schema_version must be 1"));
    }
    match object.get("mode").and_then(Value::as_str) {
        Some(mode) if MODES.contains(&mode) => {}
        _ => {
            return Err(invalid(
                "mode must be normal, ask_every_action or observe_only",
            ));
        }
    }
    if let Some(field) = object
        .keys()
        .find(|key| !POLICY_FIELDS.contains(&key.as_str()))
    {
        return Err(invalid(format!("the policy has no field {field:?}")));
    }
    for list in [
        "excluded_applications",
        "sensitive_applications",
        "relaxations",
    ] {
        if object.get(list).is_some_and(|item| !item.is_array()) {
            return Err(invalid(format!("{list} must be a list")));
        }
    }
    Ok(value)
}

// ---- standing approvals ----------------------------------------------------------

/// The standing approvals one person has given, per schedule.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Approvals {
    /// [`SCHEMA_VERSION`].
    pub schema_version: u64,
    /// One entry per schedule that carries approvals.
    #[serde(default)]
    pub schedules: Vec<ScheduleApprovals>,
}

/// The standing approvals of one schedule.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScheduleApprovals {
    /// The schedule's identifier in the person's store, such as `s-00000001`.
    pub schedule: String,
    /// The exact text of the schedule's task, as the person confirmed it.
    /// The approvals apply only while the schedule still has this task.
    pub task: String,
    /// The calls the person approved in advance.
    pub approvals: Vec<Approval>,
}

/// One call approved in advance: a tool of a service with its complete
/// arguments. It matches a call only if all three are equal (design
/// section 5.6), as the policy engine compares them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Approval {
    /// The D-Bus name of the service that owns the tool.
    pub service: String,
    /// The tool's name.
    pub tool: String,
    /// The complete arguments.
    pub arguments: Map<String, Value>,
}

/// Whether a schedule identifier has the form the store accepts: 1 to 64
/// ASCII letters, digits, `-` and `_`.
pub fn is_schedule_id(id: &str) -> bool {
    (1..=64).contains(&id.len())
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

fn is_name(text: &str) -> bool {
    (1..=255).contains(&text.len()) && text.chars().all(|c| c.is_ascii_graphic())
}

impl Approvals {
    /// No approvals.
    pub fn empty() -> Self {
        Approvals {
            schema_version: SCHEMA_VERSION,
            schedules: Vec::new(),
        }
    }

    /// Read a document of approvals.
    ///
    /// # Errors
    ///
    /// [`StoreError::Invalid`] for a document that repeats a key, has a
    /// field the format does not have, or breaks a limit.
    pub fn from_json(text: &str) -> Result<Self, StoreError> {
        let value = strict_value(text)?;
        let approvals: Approvals = serde_json::from_value(value)
            .map_err(|_| invalid("it does not have the fields of a list of approvals"))?;
        approvals.check()?;
        Ok(approvals)
    }

    fn check(&self) -> Result<(), StoreError> {
        if self.schema_version != SCHEMA_VERSION {
            return Err(invalid("schema_version must be 1"));
        }
        if self.schedules.len() > MAX_SCHEDULES {
            return Err(invalid(format!(
                "at most {MAX_SCHEDULES} schedules may carry approvals"
            )));
        }
        for (index, entry) in self.schedules.iter().enumerate() {
            if !is_schedule_id(&entry.schedule) {
                return Err(invalid(
                    "a schedule identifier is 1 to 64 letters, digits, - and _",
                ));
            }
            if self.schedules[..index]
                .iter()
                .any(|earlier| earlier.schedule == entry.schedule)
            {
                return Err(invalid(format!(
                    "the schedule {} is listed twice",
                    entry.schedule
                )));
            }
            if entry.task.trim().is_empty() || entry.task.len() > MAX_TASK_BYTES {
                return Err(invalid(format!(
                    "the task of {} must have 1 to {MAX_TASK_BYTES} bytes",
                    entry.schedule
                )));
            }
            if entry.approvals.is_empty() || entry.approvals.len() > MAX_APPROVALS {
                return Err(invalid(format!(
                    "{} must carry 1 to {MAX_APPROVALS} approvals",
                    entry.schedule
                )));
            }
            if entry
                .approvals
                .iter()
                .any(|approval| !is_name(&approval.service) || !is_name(&approval.tool))
            {
                return Err(invalid(format!(
                    "an approval of {} has no service or no tool",
                    entry.schedule
                )));
            }
        }
        Ok(())
    }

    /// The approvals that apply to a run of `schedule` whose task is
    /// `task`: those stored for that schedule, if its stored task is exactly
    /// `task`. Nothing otherwise.
    pub fn for_schedule(&self, schedule: &str, task: &str) -> Vec<Approval> {
        self.schedules
            .iter()
            .find(|entry| entry.schedule == schedule)
            .filter(|entry| entry.task == task)
            .map(|entry| entry.approvals.clone())
            .unwrap_or_default()
    }

    /// The same approvals without those of `schedule`.
    #[must_use]
    pub fn without(&self, schedule: &str) -> Self {
        Approvals {
            schema_version: SCHEMA_VERSION,
            schedules: self
                .schedules
                .iter()
                .filter(|entry| entry.schedule != schedule)
                .cloned()
                .collect(),
        }
    }

    /// The document as it is stored.
    pub fn to_document(&self) -> Vec<u8> {
        let mut bytes = serde_json::to_vec_pretty(self).unwrap_or_default();
        bytes.push(b'\n');
        bytes
    }
}

// ---- the store ---------------------------------------------------------------------

/// Whose directory a write goes to, and who owns what is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Owner {
    /// The person.
    pub uid: u32,
    /// The person's primary group, which may read the directory.
    pub gid: u32,
    /// The owner of the directory and the files: root (`Some(0)`) in the
    /// system's store. `None` for a store written by the person themself,
    /// which only tests do (`--root`).
    pub file_uid: Option<u32>,
}

/// The policy store under one root directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Store {
    root: PathBuf,
}

impl Store {
    /// The system's store, [`STORE_ROOT`].
    pub fn system() -> Self {
        Store::at(STORE_ROOT)
    }

    /// A store under another directory, for tests.
    pub fn at(root: impl Into<PathBuf>) -> Self {
        Store { root: root.into() }
    }

    /// The root directory.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// A person's directory.
    pub fn person_dir(&self, uid: u32) -> PathBuf {
        self.root.join(uid.to_string())
    }

    /// A person's policy file.
    pub fn policy_path(&self, uid: u32) -> PathBuf {
        self.person_dir(uid).join(POLICY_FILE)
    }

    /// A person's approvals file.
    pub fn approvals_path(&self, uid: u32) -> PathBuf {
        self.person_dir(uid).join(APPROVALS_FILE)
    }

    /// A person's policy as stored, or `None` if they have none.
    ///
    /// # Errors
    ///
    /// [`StoreError::Io`] if the file is there and cannot be read, or is too
    /// large. A caller then lets P@x observe only.
    pub fn read_policy(&self, uid: u32) -> Result<Option<String>, StoreError> {
        read_document(&self.policy_path(uid))
    }

    /// A person's approvals, or none if they have given none.
    ///
    /// # Errors
    ///
    /// [`StoreError`] if the file is there and cannot be read or used.
    pub fn read_approvals(&self, uid: u32) -> Result<Approvals, StoreError> {
        match read_document(&self.approvals_path(uid))? {
            Some(text) => Approvals::from_json(&text),
            None => Ok(Approvals::empty()),
        }
    }

    /// Replace a person's policy.
    ///
    /// # Errors
    ///
    /// [`StoreError::Invalid`] for a document [`check_policy`] refuses;
    /// [`StoreError::Io`] if it cannot be written. Nothing is changed then.
    pub fn write_policy(&self, owner: &Owner, text: &str) -> Result<(), StoreError> {
        let value = check_policy(text)?;
        let mut bytes = serde_json::to_vec_pretty(&value)
            .map_err(|_| invalid("it cannot be written back"))?;
        bytes.push(b'\n');
        self.write_document(owner, POLICY_FILE, &bytes)
    }

    /// Replace a person's approvals.
    ///
    /// # Errors
    ///
    /// As for [`Store::write_policy`].
    pub fn write_approvals(&self, owner: &Owner, approvals: &Approvals) -> Result<(), StoreError> {
        approvals.check()?;
        self.write_document(owner, APPROVALS_FILE, &approvals.to_document())
    }

    fn write_document(&self, owner: &Owner, name: &str, bytes: &[u8]) -> Result<(), StoreError> {
        let root = &self.root;
        match fs::symlink_metadata(root) {
            Ok(metadata) => check_directory(root, &metadata, owner.file_uid)?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                fs::DirBuilder::new()
                    .mode(0o755)
                    .create(root)
                    .map_err(|error| io_error("the store's directory cannot be made", &error))?;
            }
            Err(error) => return Err(io_error("the store's directory cannot be read", &error)),
        }
        let dir = self.person_dir(owner.uid);
        match fs::symlink_metadata(&dir) {
            Ok(metadata) => check_directory(&dir, &metadata, owner.file_uid)?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                fs::DirBuilder::new()
                    .mode(0o750)
                    .create(&dir)
                    .map_err(|error| io_error("the person's directory cannot be made", &error))?;
                std::os::unix::fs::chown(&dir, owner.file_uid, Some(owner.gid))
                    .map_err(|error| io_error("the person's directory cannot be given", &error))?;
                fs::set_permissions(&dir, fs::Permissions::from_mode(0o750))
                    .map_err(|error| io_error("the person's directory cannot be closed", &error))?;
            }
            Err(error) => {
                return Err(io_error("the person's directory cannot be read", &error));
            }
        }

        let temporary = dir.join(format!(".{name}.{}.tmp", std::process::id()));
        if fs::symlink_metadata(&temporary).is_ok() {
            fs::remove_file(&temporary)
                .map_err(|error| io_error("a leftover file cannot be removed", &error))?;
        }
        let result = put_in_place(&temporary, &dir.join(name), &dir, owner, bytes);
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result
    }
}

/// Write `bytes` to the new file `temporary`, give it to the owner, close
/// it to everyone else and move it to `target` in `dir`.
fn put_in_place(
    temporary: &Path,
    target: &Path,
    dir: &Path,
    owner: &Owner,
    bytes: &[u8],
) -> Result<(), StoreError> {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o640)
        .open(temporary)
        .map_err(|error| io_error("the new file cannot be made", &error))?;
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(|error| io_error("the new file cannot be written", &error))?;
    std::os::unix::fs::chown(temporary, owner.file_uid, Some(owner.gid))
        .map_err(|error| io_error("the new file cannot be given", &error))?;
    fs::set_permissions(temporary, fs::Permissions::from_mode(0o640))
        .map_err(|error| io_error("the new file cannot be closed", &error))?;
    fs::rename(temporary, target)
        .map_err(|error| io_error("the new file cannot be put in place", &error))?;
    fs::File::open(dir)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| io_error("the directory cannot be synchronised", &error))
}

/// A directory of the store must be a directory, not a link, owned by the
/// expected owner, and writable by nobody else.
fn check_directory(
    path: &Path,
    metadata: &fs::Metadata,
    file_uid: Option<u32>,
) -> Result<(), StoreError> {
    if !metadata.file_type().is_dir() {
        return Err(StoreError::Io(format!(
            "{} is not a directory",
            path.display()
        )));
    }
    if let Some(uid) = file_uid
        && metadata.uid() != uid
    {
        return Err(StoreError::Io(format!(
            "{} does not belong to the store's owner",
            path.display()
        )));
    }
    if metadata.mode() & 0o022 != 0 {
        return Err(StoreError::Io(format!(
            "{} can be written by others",
            path.display()
        )));
    }
    Ok(())
}

fn read_document(path: &Path) -> Result<Option<String>, StoreError> {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(io_error("a file of the store cannot be read", &error)),
    };
    let mut text = String::new();
    file.take(MAX_DOCUMENT_BYTES as u64 + 1)
        .read_to_string(&mut text)
        .map_err(|error| io_error("a file of the store cannot be read", &error))?;
    if text.len() > MAX_DOCUMENT_BYTES {
        return Err(StoreError::Io(format!(
            "a file of the store is larger than {MAX_DOCUMENT_BYTES} bytes"
        )));
    }
    Ok(Some(text))
}

/// The user this process runs as, from the owner of `/proc/self`.
pub fn process_uid() -> Option<u32> {
    fs::metadata("/proc/self").ok().map(|metadata| metadata.uid())
}

/// A person's primary group, from the system's account database
/// (`getent passwd`, so that accounts from every source are found).
///
/// # Errors
///
/// [`StoreError::Io`] if the account is not known.
pub fn primary_group(uid: u32) -> Result<u32, StoreError> {
    let output = Command::new("/usr/bin/getent")
        .args(["passwd", &uid.to_string()])
        .env_clear()
        .output()
        .map_err(|error| io_error("the account database cannot be asked", &error))?;
    let text = String::from_utf8_lossy(&output.stdout);
    parse_group(&text, uid).ok_or_else(|| StoreError::Io(format!("there is no account {uid}")))
}

fn parse_group(passwd_line: &str, uid: u32) -> Option<u32> {
    let line = passwd_line.lines().next()?;
    let fields: Vec<&str> = line.split(':').collect();
    if fields.len() < 7 || fields[2].parse::<u32>().ok()? != uid {
        return None;
    }
    fields[3].parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approvals_text(arguments: &str) -> String {
        format!(
            r#"{{"schema_version": 1, "schedules": [{{"schedule": "s-00000001", "task": "Send the weekly report", "approvals": [{{"service": "org.tur.Mail1", "tool": "mail_send", "arguments": {arguments}}}]}}]}}"#
        )
    }

    #[test]
    fn a_policy_must_have_the_outline_of_a_policy_file() {
        for good in [
            r#"{"schema_version": 1, "mode": "normal"}"#,
            r#"{"schema_version": 1, "mode": "observe_only", "excluded_applications": ["org.example.Bank"]}"#,
            r#"{"schema_version": 1, "mode": "ask_every_action", "sensitive_applications": [], "relaxations": []}"#,
        ] {
            assert!(check_policy(good).is_ok(), "{good}");
        }
        for bad in [
            "[]",
            "1",
            "",
            r#"{"mode": "normal"}"#,
            r#"{"schema_version": 2, "mode": "normal"}"#,
            r#"{"schema_version": 1, "mode": "lenient"}"#,
            r#"{"schema_version": 1}"#,
            r#"{"schema_version": 1, "mode": "normal", "trusted": true}"#,
            r#"{"schema_version": 1, "mode": "normal", "relaxations": {}}"#,
            r#"{"schema_version": 1, "mode": "normal", "mode": "observe_only"}"#,
            r#"{"schema_version": 1, "mode": "normal"} {}"#,
        ] {
            assert!(
                matches!(check_policy(bad), Err(StoreError::Invalid(_))),
                "{bad}"
            );
        }
        let large = format!(
            r#"{{"schema_version": 1, "mode": "normal", "excluded_applications": ["{}"]}}"#,
            "a".repeat(MAX_DOCUMENT_BYTES)
        );
        assert!(check_policy(&large).is_err());
    }

    #[test]
    fn approvals_are_read_strictly() {
        let good = Approvals::from_json(&approvals_text(r#"{"to": "a@example.org"}"#))
            .expect("approvals");
        assert_eq!(good.schedules.len(), 1);
        for bad in [
            approvals_text(r#"{"to": "a@example.org", "to": "b@example.org"}"#),
            approvals_text(r#"{"to": {"x": 1, "x": 2}}"#),
            approvals_text(r#"[]"#),
            r#"{"schema_version": 2, "schedules": []}"#.to_owned(),
            r#"{"schema_version": 1, "schedules": [], "extra": 1}"#.to_owned(),
            r#"{"schema_version": 1, "schedules": [{"schedule": "s 1", "task": "t", "approvals": [{"service": "a", "tool": "b", "arguments": {}}]}]}"#.to_owned(),
            r#"{"schema_version": 1, "schedules": [{"schedule": "s-1", "task": " ", "approvals": [{"service": "a", "tool": "b", "arguments": {}}]}]}"#.to_owned(),
            r#"{"schema_version": 1, "schedules": [{"schedule": "s-1", "task": "t", "approvals": []}]}"#.to_owned(),
            r#"{"schema_version": 1, "schedules": [{"schedule": "s-1", "task": "t", "approvals": [{"service": "", "tool": "b", "arguments": {}}]}]}"#.to_owned(),
            r#"{"schema_version": 1, "schedules": [{"schedule": "s-1", "task": "t", "approvals": [{"service": "a", "tool": "b", "arguments": {}}]}, {"schedule": "s-1", "task": "u", "approvals": [{"service": "a", "tool": "b", "arguments": {}}]}]}"#.to_owned(),
        ] {
            assert!(
                matches!(Approvals::from_json(&bad), Err(StoreError::Invalid(_))),
                "{bad}"
            );
        }
    }

    #[test]
    fn approvals_apply_to_their_schedule_with_its_task_only() {
        let approvals = Approvals::from_json(&approvals_text(r#"{"to": "a@example.org"}"#))
            .expect("approvals");
        assert_eq!(
            approvals
                .for_schedule("s-00000001", "Send the weekly report")
                .len(),
            1
        );
        // The task was changed in the person's own store: nothing applies.
        assert!(
            approvals
                .for_schedule("s-00000001", "Send the weekly report to everyone")
                .is_empty()
        );
        assert!(
            approvals
                .for_schedule("s-00000001", "send the weekly report")
                .is_empty()
        );
        assert!(
            approvals
                .for_schedule("s-00000002", "Send the weekly report")
                .is_empty()
        );
        assert!(approvals.without("s-00000001").schedules.is_empty());
        assert_eq!(approvals.without("s-00000002"), approvals);
        // What is stored reads back the same.
        let again = Approvals::from_json(&String::from_utf8_lossy(&approvals.to_document()))
            .expect("approvals");
        assert_eq!(again, approvals);
    }

    #[test]
    fn a_primary_group_is_read_from_the_account_line() {
        assert_eq!(
            parse_group("ada:x:1000:1000:Ada:/var/home/ada:/bin/bash\n", 1000),
            Some(1000)
        );
        assert_eq!(
            parse_group("ada:x:1000:1001:Ada:/var/home/ada:/bin/bash", 1000),
            Some(1001)
        );
        assert_eq!(parse_group("ada:x:1000:1000:Ada:/home/ada:/bin/bash", 1001), None);
        assert_eq!(parse_group("", 1000), None);
        assert_eq!(parse_group("broken", 1000), None);
    }

    #[test]
    fn schedule_identifiers_are_plain() {
        for good in ["s-00000001", "a", "A_b-9"] {
            assert!(is_schedule_id(good), "{good}");
        }
        for bad in ["", "s 1", "s/1", "../x", "é", "a".repeat(65).as_str()] {
            assert!(!is_schedule_id(bad), "{bad}");
        }
    }
}
