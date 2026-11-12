// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! Where the provider key lives: the password store of the session, through
//! the freedesktop Secret Service interface.
//!
//! The key is the person's own, bought from a provider. It is kept in one
//! place only: an item of the default collection of whatever program
//! provides `org.freedesktop.secrets` on the session bus. On Plasma that is
//! KWallet. **It is never written to a file by this crate**: not to the
//! settings, not to the audit trail, not to a log. If no password store is
//! there, there is nowhere to keep a key, and the operation fails with
//! `TUR-AI-KEY-001`; nothing falls back to a file.
//!
//! [`KeyStore`] is what the rest of the service sees: is a key set, set
//! one, remove it, and take it for a request. There is no operation that
//! hands the key text to a caller: [`KeyStore::key`] returns the provider
//! layer's [`ApiKey`], which cannot be printed, cloned or serialised.
//!
//! # What this does and does not protect
//!
//! * The secret travels to and from the password store over the session
//!   bus in the store's `plain` transfer: not encrypted on the bus. The
//!   interface also offers an encrypted transfer. It would hide the key
//!   from a program that watches the bus and from nothing else: every
//!   program running as the person can ask the password store for the item
//!   directly, exactly as this module does. The store, not the transfer,
//!   decides who may read it, and on today's desktops it lets the person's
//!   programs read it.
//! * The buffers a message passes through on its way to the bus are not
//!   overwritten. The key as this module holds it is.
//! * The model has no way to this module. No tool calls it, the engine
//!   refuses the operation "read a keyring secret" as class 4, and the
//!   sandbox a command runs in has no session bus.
//!
//! The interface is the Secret Service API, draft 0.2
//! (<https://specifications.freedesktop.org/secret-service/latest/>). It
//! is tested in CI against gnome-keyring on a private bus; KWallet's
//! implementation has not been run against it.

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use tur_pax_providers::{ApiKey, ProviderId, Zeroizing, is_usable_key};

use crate::bus::{BusError, Connection, Kind, Message, Value};
use crate::consent::provider_name;
use crate::faults::{Fault, TaskCode};

/// The bus name of the password store.
pub const SECRETS_NAME: &str = "org.freedesktop.secrets";

/// The object path of the password store.
pub const SECRETS_PATH: &str = "/org/freedesktop/secrets";

/// The alias of the collection new items go to.
pub const DEFAULT_COLLECTION: &str = "default";

const SERVICE: &str = "org.freedesktop.Secret.Service";
const COLLECTION: &str = "org.freedesktop.Secret.Collection";
const ITEM: &str = "org.freedesktop.Secret.Item";
const PROMPT: &str = "org.freedesktop.Secret.Prompt";
const SESSION: &str = "org.freedesktop.Secret.Session";
const PROPERTIES: &str = "org.freedesktop.DBus.Properties";

/// The value of the `xdg:schema` attribute of the item: what kind of
/// secret it is, for tools that list the password store.
pub const ITEM_SCHEMA: &str = "org.tur.Pax.ProviderKey";

/// How long one call to the password store may take.
const CALL_TIMEOUT: Duration = Duration::from_secs(25);

/// How long the person has to answer a prompt of the password store.
const PROMPT_TIMEOUT: Duration = Duration::from_secs(120);

/// The key of each provider, kept where keys are kept.
pub trait KeyStore: Send + Sync {
    /// Whether a key is stored for the provider. The key is not read.
    ///
    /// # Errors
    ///
    /// `TUR-AI-KEY-001` if no password store is available; `TUR-AI-KEY-003`
    /// or `TUR-AI-KEY-005` if it is locked or failed.
    fn is_set(&self, provider: ProviderId) -> Result<bool, Fault>;

    /// Store the key for the provider, in place of any key already there.
    ///
    /// # Errors
    ///
    /// `TUR-AI-KEY-004` if the text cannot be a key, and then nothing is
    /// stored; otherwise as for [`KeyStore::is_set`].
    fn set(&self, provider: ProviderId, key: &str) -> Result<(), Fault>;

    /// Remove the key of the provider. `Ok(false)` if there was none.
    ///
    /// # Errors
    ///
    /// As for [`KeyStore::is_set`].
    fn remove(&self, provider: ProviderId) -> Result<bool, Fault>;

    /// The key of the provider, to send requests with.
    ///
    /// # Errors
    ///
    /// `TUR-AI-KEY-002` if none is stored; otherwise as for
    /// [`KeyStore::is_set`].
    fn key(&self, provider: ProviderId) -> Result<ApiKey, Fault>;
}

fn fault(code: TaskCode, detail: impl Into<String>) -> Fault {
    Fault::new(code, detail)
}

/// The attributes that identify the item of a provider.
fn attributes(provider: ProviderId) -> Value {
    Value::string_map(&[
        ("xdg:schema", ITEM_SCHEMA),
        ("application", "org.tur.Pax"),
        ("provider", provider.as_str()),
    ])
}

/// Turn an error of the bus into the fault the person is shown. The detail
/// names the operation and the error's name, never its text: the text is
/// the password store's, and nothing of it is needed.
fn store_fault(operation: &str, error: &BusError) -> Fault {
    match error {
        BusError::Unavailable(_) | BusError::Closed => fault(
            TaskCode::KeyStoreUnavailable,
            format!("{operation}: the session bus is not available"),
        ),
        error if error.is_no_such_service() => fault(
            TaskCode::KeyStoreUnavailable,
            format!("{operation}: nothing provides {SECRETS_NAME} on the session bus"),
        ),
        BusError::Remote { name, .. } if name == "org.freedesktop.Secret.Error.IsLocked" => fault(
            TaskCode::KeyStoreLocked,
            format!("{operation}: the password store is locked"),
        ),
        BusError::Remote { name, .. } => fault(
            TaskCode::KeyStoreFailed,
            format!("{operation}: the password store answered {name}"),
        ),
        BusError::TimedOut => fault(
            TaskCode::KeyStoreFailed,
            format!("{operation}: the password store did not answer in time"),
        ),
        BusError::Protocol(_) => fault(
            TaskCode::KeyStoreFailed,
            format!("{operation}: the answer of the password store could not be read"),
        ),
    }
}

/// One conversation with the password store.
struct Session<'a> {
    bus: &'a Connection,
    signals: &'a Receiver<Message>,
    session: String,
    collection: String,
}

impl Session<'_> {
    fn call(
        &self,
        operation: &str,
        path: &str,
        interface: &str,
        member: &str,
        body: Vec<Value>,
    ) -> Result<Message, Fault> {
        self.bus
            .call(
                Message::method_call(SECRETS_NAME, path, interface, member, body),
                CALL_TIMEOUT,
            )
            .map_err(|error| store_fault(operation, &error))
    }

    /// Let the password store ask the person, and wait for the answer.
    /// `Ok(())` if the person went through with it.
    fn prompt(&self, operation: &str, prompt: &str) -> Result<(), Fault> {
        if prompt == "/" {
            return Ok(());
        }
        self.call(operation, prompt, PROMPT, "Prompt", vec![Value::string("")])?;
        let deadline = Instant::now() + PROMPT_TIMEOUT;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let Ok(message) = self.signals.recv_timeout(left) else {
                return Err(fault(
                    TaskCode::KeyStoreLocked,
                    format!("{operation}: the prompt of the password store was not answered"),
                ));
            };
            let is_answer = message.kind == Kind::Signal
                && message.path.as_deref() == Some(prompt)
                && message.interface.as_deref() == Some(PROMPT)
                && message.member.as_deref() == Some("Completed");
            if !is_answer {
                continue;
            }
            return match message.body.first().and_then(Value::as_bool) {
                Some(false) => Ok(()),
                _ => Err(fault(
                    TaskCode::KeyStoreLocked,
                    format!("{operation}: the prompt of the password store was dismissed"),
                )),
            };
        }
    }

    fn is_locked(&self, operation: &str, path: &str, interface: &str) -> Result<bool, Fault> {
        let reply = self.call(
            operation,
            path,
            PROPERTIES,
            "Get",
            vec![Value::string(interface), Value::string("Locked")],
        )?;
        Ok(reply
            .body
            .first()
            .map(Value::unwrapped)
            .and_then(Value::as_bool)
            .unwrap_or(true))
    }

    /// Unlock a collection or an item if it is locked.
    fn unlock(&self, operation: &str, path: &str, interface: &str) -> Result<(), Fault> {
        if !self.is_locked(operation, path, interface)? {
            return Ok(());
        }
        let reply = self.call(
            operation,
            SECRETS_PATH,
            SERVICE,
            "Unlock",
            vec![Value::Array {
                element: "o".to_owned(),
                items: vec![Value::path(path)],
            }],
        )?;
        let prompt = reply.body.get(1).and_then(Value::as_str).unwrap_or("/");
        self.prompt(operation, prompt)?;
        if self.is_locked(operation, path, interface)? {
            return Err(fault(
                TaskCode::KeyStoreLocked,
                format!("{operation}: the password store stayed locked"),
            ));
        }
        Ok(())
    }

    /// The items that hold the key of a provider.
    fn items(&self, operation: &str, provider: ProviderId) -> Result<Vec<String>, Fault> {
        let reply = self.call(
            operation,
            &self.collection,
            COLLECTION,
            "SearchItems",
            vec![attributes(provider)],
        )?;
        Ok(reply
            .body
            .first()
            .and_then(Value::as_array)
            .unwrap_or_default()
            .iter()
            .filter_map(|item| item.as_str().map(str::to_owned))
            .collect())
    }
}

/// The password store of the session, reached over a connection of its own.
///
/// The connection is separate from the one the service answers its callers
/// on, so that a prompt of the password store can be waited for without
/// anything else being held up or mixed in.
pub struct SecretService {
    address: String,
    link: Mutex<Option<(Connection, Receiver<Message>)>>,
}

impl std::fmt::Debug for SecretService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SecretService").finish_non_exhaustive()
    }
}

impl SecretService {
    /// The password store on the bus at `address`. Nothing is opened until
    /// the first operation.
    pub fn new(address: impl Into<String>) -> Self {
        SecretService {
            address: address.into(),
            link: Mutex::new(None),
        }
    }

    /// The connection to the bus, opened if it is not open.
    fn connected<'a>(
        &self,
        operation: &str,
        link: &'a mut Option<(Connection, Receiver<Message>)>,
    ) -> Result<&'a (Connection, Receiver<Message>), Fault> {
        if link.as_ref().is_none_or(|(bus, _)| bus.is_closed()) {
            let (bus, signals) =
                Connection::open(&self.address).map_err(|error| store_fault(operation, &error))?;
            // The answers to the store's prompts arrive as signals.
            bus.add_match(&format!(
                "type='signal',interface='{PROMPT}',member='Completed'"
            ))
            .map_err(|error| store_fault(operation, &error))?;
            *link = Some((bus, signals));
        }
        link.as_ref().ok_or_else(|| {
            fault(
                TaskCode::KeyStoreUnavailable,
                format!("{operation}: the session bus is not available"),
            )
        })
    }

    /// Run one operation in a session with the password store, with its
    /// collection unlocked.
    fn with<T>(
        &self,
        operation: &str,
        work: impl FnOnce(&Session<'_>) -> Result<T, Fault>,
    ) -> Result<T, Fault> {
        let mut link = self
            .link
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let (bus, signals) = self.connected(operation, &mut link)?;
        let call = |path: &str, member: &str, body: Vec<Value>| {
            bus.call(
                Message::method_call(SECRETS_NAME, path, SERVICE, member, body),
                CALL_TIMEOUT,
            )
            .map_err(|error| store_fault(operation, &error))
        };

        let opened = call(
            SECRETS_PATH,
            "OpenSession",
            vec![Value::string("plain"), Value::variant(Value::string(""))],
        )?;
        let session = opened
            .body
            .get(1)
            .and_then(Value::as_str)
            .ok_or_else(|| {
                fault(
                    TaskCode::KeyStoreFailed,
                    format!("{operation}: the password store opened no session"),
                )
            })?
            .to_owned();
        let mut collection = String::from("/");
        // The collection the `default` alias names. A store that has not
        // set the alias, as gnome-keyring may not have in a new account,
        // keeps its first collection under the alias `login`.
        for alias in [DEFAULT_COLLECTION, "login"] {
            let found = call(SECRETS_PATH, "ReadAlias", vec![Value::string(alias)])?;
            collection = found.first_string().unwrap_or("/").to_owned();
            if collection != "/" {
                break;
            }
        }
        let result = if collection == "/" {
            Err(fault(
                TaskCode::KeyStoreUnavailable,
                format!("{operation}: the password store has no default collection"),
            ))
        } else {
            let session = Session {
                bus,
                signals,
                session: session.clone(),
                collection,
            };
            session
                .unlock(operation, &session.collection, COLLECTION)
                .and_then(|()| work(&session))
        };
        // The session is closed whatever happened in it.
        let _ = bus.call(
            Message::method_call(SECRETS_NAME, &session, SESSION, "Close", Vec::new()),
            CALL_TIMEOUT,
        );
        result
    }
}

impl KeyStore for SecretService {
    fn is_set(&self, provider: ProviderId) -> Result<bool, Fault> {
        // Asked of the store as a whole, which lists locked items too: the
        // question never makes the store ask the person to unlock it.
        let operation = "looking for the key";
        let mut link = self
            .link
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let (bus, _) = self.connected(operation, &mut link)?;
        let reply = bus
            .call(
                Message::method_call(
                    SECRETS_NAME,
                    SECRETS_PATH,
                    SERVICE,
                    "SearchItems",
                    vec![attributes(provider)],
                ),
                CALL_TIMEOUT,
            )
            .map_err(|error| store_fault(operation, &error))?;
        Ok(reply
            .body
            .iter()
            .take(2)
            .any(|found| found.as_array().is_some_and(|items| !items.is_empty())))
    }

    fn set(&self, provider: ProviderId, key: &str) -> Result<(), Fault> {
        if !is_usable_key(key) {
            return Err(fault(
                TaskCode::KeyInvalid,
                "the text is empty, too long, or holds a space or a character a key cannot have",
            ));
        }
        let operation = "storing the key";
        self.with(operation, |session| {
            let properties = Value::variant_map(vec![
                (
                    "org.freedesktop.Secret.Item.Label",
                    Value::string(format!("P@x: key for {}", provider_name(provider))),
                ),
                (
                    "org.freedesktop.Secret.Item.Attributes",
                    attributes(provider),
                ),
            ]);
            let secret = Value::Struct(vec![
                Value::path(session.session.clone()),
                Value::bytes(&[]),
                Value::bytes(key.trim().as_bytes()),
                Value::string("text/plain"),
            ]);
            let reply = session.call(
                operation,
                &session.collection,
                COLLECTION,
                "CreateItem",
                vec![properties, secret, Value::Bool(true)],
            )?;
            let prompt = reply.body.get(1).and_then(Value::as_str).unwrap_or("/");
            session.prompt(operation, prompt)
        })
    }

    fn remove(&self, provider: ProviderId) -> Result<bool, Fault> {
        let operation = "removing the key";
        self.with(operation, |session| {
            let items = session.items(operation, provider)?;
            for item in &items {
                let reply = session.call(operation, item, ITEM, "Delete", Vec::new())?;
                let prompt = reply.first_string().unwrap_or("/");
                session.prompt(operation, prompt)?;
            }
            Ok(!items.is_empty())
        })
    }

    fn key(&self, provider: ProviderId) -> Result<ApiKey, Fault> {
        let operation = "reading the key";
        self.with(operation, |session| {
            let items = session.items(operation, provider)?;
            let Some(item) = items.first() else {
                return Err(fault(
                    TaskCode::KeyNotSet,
                    format!("no key is stored for {}", provider_name(provider)),
                ));
            };
            session.unlock(operation, item, ITEM)?;
            let reply = session.call(
                operation,
                item,
                ITEM,
                "GetSecret",
                vec![Value::path(session.session.clone())],
            )?;
            let unreadable = || {
                fault(
                    TaskCode::KeyStoreFailed,
                    format!("{operation}: the item of the password store holds no usable key"),
                )
            };
            let bytes = reply
                .body
                .first()
                .and_then(Value::as_struct)
                .and_then(|fields| fields.get(2))
                .and_then(Value::as_bytes)
                .map(Zeroizing::new)
                .ok_or_else(unreadable)?;
            let text = String::from_utf8(bytes.to_vec()).map_err(|_| unreadable())?;
            // `ApiKey::new` overwrites the string it is given.
            ApiKey::new(text).map_err(|_| unreadable())
        })
    }
}

/// A key store kept in memory, for tests. Nothing is written anywhere.
#[derive(Debug, Default)]
pub struct MemoryKeyStore {
    keys: Mutex<HashMap<ProviderId, String>>,
    unavailable: std::sync::atomic::AtomicBool,
}

impl MemoryKeyStore {
    /// An empty key store.
    pub fn new() -> Self {
        MemoryKeyStore::default()
    }

    /// Make the store behave as if no password store were there.
    pub fn set_unavailable(&self, unavailable: bool) {
        self.unavailable
            .store(unavailable, std::sync::atomic::Ordering::SeqCst);
    }

    fn keys(&self) -> Result<std::sync::MutexGuard<'_, HashMap<ProviderId, String>>, Fault> {
        if self.unavailable.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(fault(
                TaskCode::KeyStoreUnavailable,
                "the test key store is switched off",
            ));
        }
        Ok(self
            .keys
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner))
    }
}

impl KeyStore for MemoryKeyStore {
    fn is_set(&self, provider: ProviderId) -> Result<bool, Fault> {
        Ok(self.keys()?.contains_key(&provider))
    }

    fn set(&self, provider: ProviderId, key: &str) -> Result<(), Fault> {
        if !is_usable_key(key) {
            return Err(fault(
                TaskCode::KeyInvalid,
                "the text is empty, too long, or holds a space or a character a key cannot have",
            ));
        }
        self.keys()?.insert(provider, key.trim().to_owned());
        Ok(())
    }

    fn remove(&self, provider: ProviderId) -> Result<bool, Fault> {
        Ok(self.keys()?.remove(&provider).is_some())
    }

    fn key(&self, provider: ProviderId) -> Result<ApiKey, Fault> {
        let text = self.keys()?.get(&provider).cloned().ok_or_else(|| {
            fault(
                TaskCode::KeyNotSet,
                format!("no key is stored for {}", provider_name(provider)),
            )
        })?;
        ApiKey::new(text).map_err(|_| fault(TaskCode::KeyInvalid, "the stored key is not usable"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: &str = "sk-test-KEY-keyring-0123456789";

    #[test]
    fn a_key_is_set_replaced_and_removed() {
        let store = MemoryKeyStore::new();
        assert!(!store.is_set(ProviderId::Anthropic).expect("answered"));
        assert_eq!(
            store
                .key(ProviderId::Anthropic)
                .expect_err("no key")
                .code
                .as_str(),
            "TUR-AI-KEY-002"
        );
        store.set(ProviderId::Anthropic, KEY).expect("stored");
        assert!(store.is_set(ProviderId::Anthropic).expect("answered"));
        // A key is kept per provider.
        assert!(!store.is_set(ProviderId::OpenAi).expect("answered"));
        // The key comes back as the type that cannot be printed.
        let key = store.key(ProviderId::Anthropic).expect("a key");
        assert!(!format!("{key:?}{key}").contains(KEY));
        // Replaced, then removed.
        store
            .set(ProviderId::Anthropic, "  another-key-0123\n")
            .expect("replaced");
        assert!(store.is_set(ProviderId::Anthropic).expect("answered"));
        assert!(store.remove(ProviderId::Anthropic).expect("removed"));
        assert!(!store.remove(ProviderId::Anthropic).expect("nothing left"));
        assert!(!store.is_set(ProviderId::Anthropic).expect("answered"));
    }

    #[test]
    fn what_cannot_be_a_key_is_not_stored_and_not_repeated() {
        let store = MemoryKeyStore::new();
        for text in ["", "   ", "two words", "line\nbreak", "k\u{e9}y"] {
            let refused = store.set(ProviderId::Google, text).expect_err("refused");
            assert_eq!(refused.code.as_str(), "TUR-AI-KEY-004", "{text:?}");
            if !text.trim().is_empty() {
                assert!(!refused.to_string().contains(text.trim()), "{text:?}");
            }
        }
        assert!(!store.is_set(ProviderId::Google).expect("answered"));
        // The real store refuses the same text before it opens anything: the
        // address below leads nowhere, and the refusal is about the key.
        let real = SecretService::new("unix:path=/nonexistent/tur-pax-test/bus");
        assert_eq!(
            real.set(ProviderId::Google, "two words")
                .expect_err("refused")
                .code
                .as_str(),
            "TUR-AI-KEY-004"
        );
    }

    #[test]
    fn without_a_password_store_there_is_a_clear_error_and_no_other_place() {
        let store = MemoryKeyStore::new();
        store.set(ProviderId::Anthropic, KEY).expect("stored");
        store.set_unavailable(true);
        for code in [
            store.is_set(ProviderId::Anthropic).map(|_| ()),
            store.set(ProviderId::Anthropic, KEY),
            store.remove(ProviderId::Anthropic).map(|_| ()),
            store.key(ProviderId::Anthropic).map(|_| ()),
        ] {
            assert_eq!(
                code.expect_err("unavailable").code.as_str(),
                "TUR-AI-KEY-001"
            );
        }

        // The real store with no bus to reach: every operation says that no
        // password store is available, and none of them invents one.
        let real = SecretService::new("unix:path=/nonexistent/tur-pax-test/bus");
        for code in [
            real.is_set(ProviderId::Anthropic).map(|_| ()),
            real.set(ProviderId::Anthropic, KEY),
            real.remove(ProviderId::Anthropic).map(|_| ()),
            real.key(ProviderId::Anthropic).map(|_| ()),
        ] {
            let refused = code.expect_err("unavailable");
            assert_eq!(refused.code.as_str(), "TUR-AI-KEY-001");
            assert!(!refused.to_string().contains(KEY));
        }
    }

    #[test]
    fn errors_of_the_store_are_named_without_their_text() {
        let remote = |name: &str| BusError::Remote {
            name: name.to_owned(),
            message: format!("something about {KEY}"),
        };
        let cases = [
            (
                remote("org.freedesktop.DBus.Error.ServiceUnknown"),
                "TUR-AI-KEY-001",
            ),
            (
                remote("org.freedesktop.DBus.Error.NameHasNoOwner"),
                "TUR-AI-KEY-001",
            ),
            (
                remote("org.freedesktop.Secret.Error.IsLocked"),
                "TUR-AI-KEY-003",
            ),
            (
                remote("org.freedesktop.Secret.Error.NoSuchObject"),
                "TUR-AI-KEY-005",
            ),
            (BusError::TimedOut, "TUR-AI-KEY-005"),
            (BusError::Closed, "TUR-AI-KEY-001"),
            (BusError::Protocol("x".to_owned()), "TUR-AI-KEY-005"),
            (BusError::Unavailable("x".to_owned()), "TUR-AI-KEY-001"),
        ];
        for (error, code) in cases {
            let shown = store_fault("reading the key", &error);
            assert_eq!(shown.code.as_str(), code, "{error:?}");
            assert!(!shown.detail.contains(KEY), "{error:?}");
            assert!(shown.detail.starts_with("reading the key: "));
        }
    }

    #[test]
    fn the_item_of_a_provider_is_found_by_its_attributes() {
        let anthropic = attributes(ProviderId::Anthropic);
        assert_eq!(anthropic.signature(), "a{ss}");
        let pairs: Vec<(String, String)> = anthropic
            .as_array()
            .expect("an array")
            .iter()
            .filter_map(|entry| match entry {
                Value::DictEntry(key, value) => {
                    Some((key.as_str()?.to_owned(), value.as_str()?.to_owned()))
                }
                _ => None,
            })
            .collect();
        assert!(pairs.contains(&("provider".to_owned(), "anthropic".to_owned())));
        assert!(pairs.contains(&("xdg:schema".to_owned(), ITEM_SCHEMA.to_owned())));
        assert_ne!(anthropic, attributes(ProviderId::OpenAi));
    }
}
