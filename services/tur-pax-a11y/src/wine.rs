// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! Windows applications under Wine, through `tur-winbridge.exe` inside each
//! application's prefix (docs/architecture/pax-accessibility.md, section
//! 11).
//!
//! Wine puts nothing on the accessibility bus. For each Windows application
//! it starts under Wine, `tur-runtime` offers a socket in the user's runtime
//! directory (`$XDG_RUNTIME_DIR/tur/winbridge/<app-id>.sock`); connecting to
//! it starts the bridge inside the application's sandbox with the
//! connection as its standard input and output. This module is the other
//! end: it lists the applications that offer one, asks their bridges for
//! windows, reads a window's controls through the bridge's Win32 tier, and
//! asks the bridge to act on one control.
//!
//! What a model sees is the node model of the accessibility path: a window
//! has an identifier (`ww1`), a title, an application and a toolkit; an
//! element has an identifier (`we12`), an AT-SPI role, a name, states,
//! bounds in the window's coordinates and actions. The bridge reports
//! AT-SPI role numbers, so the policy engine's `UiRole::from_atspi` judges
//! a Windows button as it judges a GTK one. Who the application is comes
//! from `tur-runtime`'s socket (the application's identifier, which is the
//! catalogue's) and from the bridge (the program's file name); the window's
//! owner is the user, because only the user's own `tur-runtime` can make a
//! socket in the user's runtime directory, and it refuses to run as root.
//!
//! Nothing here decides: [`crate::serve`] does, in the same order as for the
//! accessibility tree.

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::fs::{FileTypeExt, MetadataExt};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{Map, Value, json};
use tur_pax::UiRole;

use crate::atspi::{role, role_name};
use crate::tree::{CONTENT_NOTE, MAX_LABEL, MAX_NODES, MAX_TEXT, Miss, cut, role_matches};

/// The directory of the sockets, under the user's runtime directory.
pub const SOCKET_DIRECTORY: &str = "tur/winbridge";

/// How a window of a Windows application is named to a model: `ww1`.
pub const WINDOW_PREFIX: &str = "ww";

/// How an element of a Windows application is named to a model: `we1`.
pub const ELEMENT_PREFIX: &str = "we";

/// The toolkit a Windows window is reported with.
pub const TOOLKIT: &str = "win32 (Wine)";

/// How long the bridge has to say hello: it may be starting under Wine.
const HELLO_TIMEOUT: Duration = Duration::from_secs(60);

/// How long the bridge has to answer any other request.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// The longest answer read from a bridge.
const MAX_ANSWER: usize = 8 * 1024 * 1024;

/// The most elements asked of the bridge for one window.
const BRIDGE_NODES: u32 = 1500;

/// Programs of Wine's own, and the bridge itself, whose windows are not the
/// application's. `conhost.exe` (Wine's console) is left out too: it is a
/// terminal, which P@x never operates.
const SYSTEM_PROGRAMS: [&str; 8] = [
    "explorer.exe",
    "winedevice.exe",
    "services.exe",
    "tur-winbridge.exe",
    "plugplay.exe",
    "svchost.exe",
    "rpcss.exe",
    "conhost.exe",
];

/// Roles whose elements are shown even when their window does not show
/// them: the items of a menu, which the bridge reads through the menu hook
/// and can send without opening the menu.
fn is_menu_role(role: u32) -> bool {
    matches!(role_name(role), "menu" | "menu item" | "check menu item")
}

/// A window of a Windows application.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WineWindow {
    /// The identifier a model names it by.
    pub id: String,
    /// The application, as `tur-runtime` named its socket.
    pub application: String,
    /// The bridge's own identifier of the window.
    pub handle: String,
    /// The window's title.
    pub title: String,
    /// The program's file name, in lower case (`7zfm.exe`).
    pub program: String,
    /// Whether it is the application's foreground window.
    pub active: bool,
    /// Its rectangle on the screen, as Windows sees it.
    pub screen: [i32; 4],
    /// Where the part the compositor shows as its content begins, on the
    /// screen; element bounds are given from there.
    pub content: [i32; 2],
}

impl WineWindow {
    /// The application's identifiers besides its main one: the program's
    /// file name and that name without `.exe`.
    pub fn also_known_as(&self) -> Vec<String> {
        let mut names = Vec::new();
        if !self.program.is_empty() {
            names.push(self.program.clone());
            if let Some(stem) = self.program.strip_suffix(".exe")
                && !stem.is_empty()
            {
                names.push(stem.to_owned());
            }
        }
        names.retain(|name| *name != self.application);
        names
    }

    /// The window's own rectangle in its own coordinates.
    pub fn frame(&self) -> [i32; 4] {
        [
            0,
            0,
            (self.screen[2] - (self.content[0] - self.screen[0]) * 2).max(1),
            (self.screen[3] - (self.content[1] - self.screen[1])).max(1),
        ]
    }
}

/// One control of a window, as the bridge read it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WineElement {
    /// The bridge's own identifier.
    pub handle: String,
    /// Its parent's identifier.
    pub parent: String,
    /// How deep it is; the window is 0.
    pub depth: u32,
    /// The AT-SPI role number.
    pub role: u32,
    /// Its name.
    pub name: String,
    /// Its description.
    pub description: String,
    /// Its states, by AT-SPI's names.
    pub states: Vec<String>,
    /// What can be done to it.
    pub actions: Vec<String>,
    /// Its rectangle on the screen.
    pub screen: Option<[i32; 4]>,
    /// The text it holds, unless withheld.
    pub text: Option<String>,
    /// Whether its text is withheld (a field for a password).
    pub text_withheld: bool,
}

impl WineElement {
    /// It has this state.
    pub fn has(&self, state: &str) -> bool {
        self.states.iter().any(|named| named == state)
    }

    /// Whether a person can see it, or it is an item of a menu.
    pub fn shows(&self) -> bool {
        !self.has("offscreen") || is_menu_role(self.role)
    }

    /// It takes text.
    pub fn editable(&self) -> bool {
        self.has("editable")
    }

    /// Something can be done to it besides giving it the keyboard.
    pub fn usable(&self) -> bool {
        self.actions
            .iter()
            .any(|action| action != "focus" && action != "scroll")
    }

    /// Its rectangle in the coordinates of its window's content.
    pub fn bounds(&self, window: &WineWindow) -> Option<[i32; 4]> {
        let [x, y, w, h] = self.screen?;
        (w > 0 && h > 0).then(|| [x - window.content[0], y - window.content[1], w, h])
    }

    /// The states worth telling a model.
    pub fn reported(&self) -> Vec<String> {
        self.states
            .iter()
            .filter(|state| state.as_str() != "offscreen")
            .cloned()
            .collect()
    }

    fn from_value(value: &Value) -> Option<WineElement> {
        let text = |key: &str| {
            value
                .get(key)
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned()
        };
        let list = |key: &str| -> Vec<String> {
            value
                .get(key)
                .and_then(Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_owned)
                        .collect()
                })
                .unwrap_or_default()
        };
        let handle = text("id");
        if handle.is_empty() {
            return None;
        }
        let role = value
            .get("role_id")
            .and_then(Value::as_u64)
            .and_then(|n| u32::try_from(n).ok())
            .unwrap_or(role::UNKNOWN);
        let withheld = value
            .get("text_withheld")
            .and_then(Value::as_bool)
            .unwrap_or(false)
            || role == role::PASSWORD_TEXT
            || role == role::TERMINAL;
        Some(WineElement {
            parent: text("parent"),
            depth: value
                .get("depth")
                .and_then(Value::as_u64)
                .and_then(|n| u32::try_from(n).ok())
                .unwrap_or(0),
            role,
            name: text("name"),
            description: text("description"),
            states: list("states"),
            actions: list("actions"),
            screen: rectangle(value.get("screen")),
            // The text of a field for a secret or a terminal is never kept.
            text: if withheld {
                None
            } else {
                value.get("text").and_then(Value::as_str).map(str::to_owned)
            },
            text_withheld: withheld,
            handle,
        })
    }
}

fn rectangle(value: Option<&Value>) -> Option<[i32; 4]> {
    let items = value?.as_array()?;
    if items.len() != 4 {
        return None;
    }
    let mut rect = [0_i32; 4];
    for (slot, item) in rect.iter_mut().zip(items) {
        *slot = i32::try_from(item.as_i64()?).ok()?;
    }
    Some(rect)
}

/// The window list of one bridge's answer, without the windows that are
/// not the application's own.
pub fn windows_of(application: &str, answer: &Value) -> Vec<WineWindow> {
    let mut found = Vec::new();
    for window in answer
        .get("windows")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let text = |key: &str| {
            window
                .get(key)
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned()
        };
        let program = text("exe").to_lowercase();
        let Some(screen) = rectangle(window.get("screen")) else {
            continue;
        };
        if SYSTEM_PROGRAMS.contains(&program.as_str()) || screen[2] < 40 || screen[3] < 20 {
            continue;
        }
        let content = window
            .get("content")
            .and_then(Value::as_array)
            .and_then(|pair| {
                let x = i32::try_from(pair.first()?.as_i64()?).ok()?;
                let y = i32::try_from(pair.get(1)?.as_i64()?).ok()?;
                Some([x, y])
            })
            .unwrap_or([screen[0], screen[1]]);
        let handle = text("window");
        if handle.is_empty() {
            continue;
        }
        found.push(WineWindow {
            id: String::new(),
            application: application.to_owned(),
            handle,
            title: text("title"),
            program,
            active: window
                .get("active")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            screen,
            content,
        });
    }
    found
}

/// The controls of one read, in the bridge's order, without the window
/// itself.
pub fn elements_of(answer: &Value) -> Vec<WineElement> {
    answer
        .get("elements")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(WineElement::from_value)
        .filter(|element| element.depth > 0)
        .collect()
}

/// A plain application identifier: what `tur-runtime` names a socket.
fn is_application_id(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && !name.starts_with('.')
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
}

/// One connection to one application's bridge.
#[derive(Debug)]
struct Link {
    reader: BufReader<UnixStream>,
    writer: UnixStream,
    next: u64,
}

impl Link {
    fn open(socket: &Path) -> Result<Link, String> {
        let writer = UnixStream::connect(socket)
            .map_err(|error| format!("the application's bridge cannot be reached: {error}"))?;
        let reader = writer
            .try_clone()
            .map_err(|error| format!("the connection cannot be shared: {error}"))?;
        let mut link = Link {
            reader: BufReader::new(reader),
            writer,
            next: 1,
        };
        link.ask(&json!({ "op": "hello" }), HELLO_TIMEOUT)?;
        Ok(link)
    }

    fn ask(&mut self, request: &Value, timeout: Duration) -> Result<Value, String> {
        let id = self.next;
        self.next += 1;
        let mut request = request.clone();
        if let Some(object) = request.as_object_mut() {
            object.insert("id".to_owned(), json!(id));
        }
        let line = serde_json::to_string(&request).map_err(|error| error.to_string())?;
        self.writer
            .write_all(format!("{line}\n").as_bytes())
            .map_err(|error| format!("the bridge's input is closed: {error}"))?;
        self.reader
            .get_ref()
            .set_read_timeout(Some(timeout))
            .map_err(|error| error.to_string())?;
        loop {
            let mut answer = Vec::new();
            let read = (&mut self.reader)
                .take(MAX_ANSWER as u64 + 1)
                .read_until(b'\n', &mut answer)
                .map_err(|error| format!("the bridge did not answer: {error}"))?;
            if read == 0 {
                return Err("the bridge ended".to_owned());
            }
            if answer.len() > MAX_ANSWER {
                return Err("the bridge's answer is too long".to_owned());
            }
            // Wine may write lines of its own on the bridge's output.
            let Ok(reply) = serde_json::from_slice::<Value>(&answer) else {
                continue;
            };
            if reply.get("id").and_then(Value::as_u64) != Some(id) {
                continue;
            }
            if reply.get("ok").and_then(Value::as_bool) != Some(true) {
                let message = reply
                    .pointer("/error/message")
                    .and_then(Value::as_str)
                    .unwrap_or("no reason given");
                return Err(format!("the bridge refused: {}", cut(message, 200)));
            }
            return Ok(reply.get("result").cloned().unwrap_or(Value::Null));
        }
    }
}

/// The Windows applications that offer a bridge, and what was read of
/// them.
#[derive(Debug, Default)]
pub struct Wine {
    directory: Option<PathBuf>,
    own_user: Option<u32>,
    links: BTreeMap<String, Link>,
    windows: Vec<(String, String)>,
    window_ids: HashMap<(String, String), String>,
    elements: Vec<(String, String)>,
    element_ids: HashMap<(String, String), String>,
}

impl Wine {
    /// The sockets of this session: `$XDG_RUNTIME_DIR/tur/winbridge`.
    pub fn from_environment() -> Wine {
        let directory = std::env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .map(|path| path.join(SOCKET_DIRECTORY));
        Wine::with_directory(directory, tur_pax::bus::own_user())
    }

    /// The sockets of `directory`, which must belong to `own_user`.
    pub fn with_directory(directory: Option<PathBuf>, own_user: Option<u32>) -> Wine {
        Wine {
            directory,
            own_user,
            ..Wine::default()
        }
    }

    /// The applications that offer a bridge: a socket of the user's own,
    /// named after the application, in a directory only the user can enter.
    pub fn applications(&self) -> Vec<String> {
        let (Some(directory), Some(own)) = (&self.directory, self.own_user) else {
            return Vec::new();
        };
        let Ok(metadata) = fs::symlink_metadata(directory) else {
            return Vec::new();
        };
        if !metadata.is_dir() || metadata.uid() != own || metadata.mode() & 0o077 != 0 {
            return Vec::new();
        }
        let Ok(entries) = fs::read_dir(directory) else {
            return Vec::new();
        };
        let mut found: Vec<String> = entries
            .filter_map(Result::ok)
            .filter_map(|entry| {
                let name = entry.file_name().into_string().ok()?;
                let application = name.strip_suffix(".sock")?.to_owned();
                let metadata = entry.metadata().ok()?;
                (is_application_id(&application)
                    && metadata.file_type().is_socket()
                    && metadata.uid() == own)
                    .then_some(application)
            })
            .collect();
        found.sort();
        found.truncate(64);
        found
    }

    fn ask(&mut self, application: &str, request: &Value) -> Result<Value, String> {
        let Some(directory) = &self.directory else {
            return Err("no runtime directory is known".to_owned());
        };
        if !self.links.contains_key(application) {
            let link = Link::open(&directory.join(format!("{application}.sock")))?;
            self.links.insert(application.to_owned(), link);
        }
        let Some(link) = self.links.get_mut(application) else {
            return Err("no connection".to_owned());
        };
        let answer = link.ask(request, REQUEST_TIMEOUT);
        if answer.is_err() {
            // A bridge that failed is started afresh on the next request.
            self.links.remove(application);
        }
        answer
    }

    fn window_id(&mut self, application: &str, handle: &str) -> String {
        let key = (application.to_owned(), handle.to_owned());
        if let Some(id) = self.window_ids.get(&key) {
            return id.clone();
        }
        self.windows.push(key.clone());
        let id = format!("{WINDOW_PREFIX}{}", self.windows.len());
        self.window_ids.insert(key, id.clone());
        id
    }

    /// The identifier of an element of a window.
    pub fn element_id(&mut self, window: &str, handle: &str) -> String {
        let key = (window.to_owned(), handle.to_owned());
        if let Some(id) = self.element_ids.get(&key) {
            return id.clone();
        }
        self.elements.push(key.clone());
        let id = format!("{ELEMENT_PREFIX}{}", self.elements.len());
        self.element_ids.insert(key, id.clone());
        id
    }

    /// The window and the bridge's identifier an element identifier names.
    pub fn element(&self, id: &str) -> Option<(String, String)> {
        let index: usize = id.strip_prefix(ELEMENT_PREFIX)?.parse().ok()?;
        self.elements.get(index.checked_sub(1)?).cloned()
    }

    /// Whether an identifier names a window of this module.
    pub fn is_window_id(id: &str) -> bool {
        id.strip_prefix(WINDOW_PREFIX)
            .is_some_and(|rest| !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit()))
    }

    /// Every window of every Windows application that offers a bridge. An
    /// application whose bridge does not answer is left out.
    pub fn windows(&mut self) -> Vec<WineWindow> {
        let mut found = Vec::new();
        for application in self.applications() {
            let Ok(answer) = self.ask(&application, &json!({ "op": "windows" })) else {
                continue;
            };
            for mut window in windows_of(&application, &answer) {
                window.id = self.window_id(&application, &window.handle);
                found.push(window);
            }
        }
        found
    }

    /// The window an identifier names, as it is now.
    pub fn window(&mut self, id: &str) -> Option<WineWindow> {
        let index: usize = id.strip_prefix(WINDOW_PREFIX)?.parse().ok()?;
        let (application, handle) = self.windows.get(index.checked_sub(1)?)?.clone();
        let answer = self.ask(&application, &json!({ "op": "windows" })).ok()?;
        windows_of(&application, &answer)
            .into_iter()
            .find(|window| window.handle == handle)
            .map(|mut window| {
                window.id = id.to_owned();
                window
            })
    }

    /// A window's controls, read now through the bridge's Win32 tier.
    ///
    /// # Errors
    ///
    /// The bridge did not answer, or the window is gone.
    pub fn read(&mut self, window: &WineWindow) -> Result<(Vec<WineElement>, bool), Miss> {
        let answer = self
            .ask(
                &window.application,
                &json!({ "op": "read", "window": window.handle, "max_nodes": BRIDGE_NODES }),
            )
            .map_err(|message| Miss {
                code: "unavailable",
                message,
            })?;
        if answer.get("error").is_some() {
            return Err(Miss {
                code: "not_found",
                message: "the window is gone".to_owned(),
            });
        }
        let truncated = answer
            .get("truncated")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        Ok((elements_of(&answer), truncated))
    }

    /// Ask the bridge to do something to a control.
    ///
    /// # Errors
    ///
    /// The bridge did not answer, or the application did not do it: the
    /// protocol's code and a sentence.
    pub fn act(
        &mut self,
        window: &WineWindow,
        handle: &str,
        action: &str,
        text: &str,
    ) -> Result<Value, (&'static str, String)> {
        let mut request = json!({ "op": "act", "element": handle, "action": action });
        if action == "type"
            && let Some(object) = request.as_object_mut()
        {
            object.insert("text".to_owned(), json!(text));
        }
        let answer = self
            .ask(&window.application, &request)
            .map_err(|message| ("unavailable", message))?;
        if answer.get("done").and_then(Value::as_bool) == Some(true) {
            return Ok(answer);
        }
        let code = match answer.get("code").and_then(Value::as_str) {
            Some("not_offered") => "not_offered",
            Some("not_found") => "changed",
            _ => "failed",
        };
        let why = answer
            .get("why")
            .and_then(Value::as_str)
            .unwrap_or("the application did not do it");
        Err((code, cut(why, 200)))
    }

    /// What the bridge says is under a point of a window's content, for the
    /// rule a position proposed from a picture is held to
    /// ([`crate::vision`]).
    ///
    /// # Errors
    ///
    /// The bridge did not answer.
    pub fn at_point(
        &mut self,
        window: &WineWindow,
        x: i32,
        y: i32,
    ) -> Result<crate::vision::UnderPoint, String> {
        let answer = self.ask(
            &window.application,
            &json!({
                "op": "at_point",
                "window": window.handle,
                "x": window.content[0].saturating_add(x),
                "y": window.content[1].saturating_add(y),
            }),
        )?;
        let element = answer
            .get("element")
            .filter(|_| answer.get("found").and_then(Value::as_bool) == Some(true))
            .and_then(WineElement::from_value)
            .map(|element| crate::vision::Seen {
                id: self.element_id(&window.id, &element.handle),
                name: element.name,
                role: element.role,
            });
        let covered = answer
            .pointer("/topmost/same_window")
            .and_then(Value::as_bool)
            .map(|same| !same);
        Ok(crate::vision::UnderPoint {
            element,
            covered: covered.or(Some(true)),
        })
    }

    /// One element as a model is told of it, without its children.
    pub fn describe(
        &mut self,
        window: &WineWindow,
        element: &WineElement,
    ) -> Map<String, Value> {
        let mut out = Map::new();
        out.insert(
            "id".to_owned(),
            json!(self.element_id(&window.id, &element.handle)),
        );
        out.insert("role".to_owned(), json!(role_name(element.role)));
        if !element.name.is_empty() {
            out.insert("name".to_owned(), json!(cut(&element.name, MAX_LABEL)));
        }
        if !element.description.is_empty() && element.description != element.name {
            out.insert(
                "description".to_owned(),
                json!(cut(&element.description, MAX_LABEL)),
            );
        }
        let states = element.reported();
        if !states.is_empty() {
            out.insert("states".to_owned(), json!(states));
        }
        if let Some(bounds) = element.bounds(window) {
            out.insert("bounds".to_owned(), json!(bounds));
        }
        if !element.actions.is_empty() {
            let actions: Vec<String> = element
                .actions
                .iter()
                .take(8)
                .map(|action| cut(action, 40))
                .collect();
            out.insert("actions".to_owned(), json!(actions));
        }
        if element.text_withheld {
            out.insert("text_withheld".to_owned(), json!(true));
        } else if let Some(text) = &element.text
            && !text.is_empty()
            && *text != element.name
        {
            out.insert("text".to_owned(), json!(cut(text, MAX_TEXT)));
        }
        out
    }

    /// A window's controls as a tree, pruned as the accessibility tree is:
    /// what is not shown is left out with what is below it (menu items
    /// excepted), and containers with no name and nothing to do give way to
    /// their children.
    pub fn read_answer(
        &mut self,
        window: &WineWindow,
        elements: &[WineElement],
        truncated: bool,
        max_nodes: usize,
    ) -> Value {
        let most = max_nodes.clamp(1, MAX_NODES);
        // Each element under the nearest ancestor that is kept.
        let mut kept: HashMap<&str, bool> = HashMap::new();
        let mut keeper: HashMap<&str, Option<&str>> = HashMap::new();
        let mut children: BTreeMap<Option<&str>, Vec<usize>> = BTreeMap::new();
        let mut returned = 0;
        let mut cut_short = truncated;
        for (index, element) in elements.iter().enumerate() {
            let parent_hidden = kept.get(element.parent.as_str()) == Some(&false)
                && !is_menu_role(element.role);
            let hidden = parent_hidden || !element.shows();
            let above = keeper.get(element.parent.as_str()).copied().flatten();
            if hidden {
                kept.insert(&element.handle, false);
                keeper.insert(&element.handle, above);
                continue;
            }
            let structural = role::is_structural(element.role)
                && element.name.is_empty()
                && !element.usable()
                && element.text.is_none();
            if structural {
                kept.insert(&element.handle, true);
                keeper.insert(&element.handle, above);
                continue;
            }
            if returned >= most {
                cut_short = true;
                kept.insert(&element.handle, false);
                keeper.insert(&element.handle, above);
                continue;
            }
            returned += 1;
            kept.insert(&element.handle, true);
            keeper.insert(&element.handle, Some(&element.handle));
            children.entry(above).or_default().push(index);
        }
        let tree = self.nest(window, elements, &children, None, 0);
        json!({
            "note": CONTENT_NOTE,
            "window": window.id,
            "title": cut(&window.title, MAX_LABEL),
            "application": window.application,
            "toolkit": TOOLKIT,
            "elements": tree,
            "truncated": cut_short,
            "visited": elements.len(),
        })
    }

    fn nest<'e>(
        &mut self,
        window: &WineWindow,
        elements: &'e [WineElement],
        children: &BTreeMap<Option<&'e str>, Vec<usize>>,
        parent: Option<&'e str>,
        depth: usize,
    ) -> Vec<Value> {
        let Some(below) = children.get(&parent) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for index in below {
            let element = &elements[*index];
            let mut described = self.describe(window, element);
            if depth < 40 {
                let nested = self.nest(window, elements, children, Some(&element.handle), depth + 1);
                if !nested.is_empty() {
                    described.insert("children".to_owned(), Value::Array(nested));
                }
            }
            out.push(Value::Object(described));
        }
        out
    }

    /// The controls of a window whose name contains `name` (any case) and
    /// whose role matches `role`.
    pub fn find_answer(
        &mut self,
        window: &WineWindow,
        elements: &[WineElement],
        name: Option<&str>,
        role_filter: Option<&str>,
    ) -> Value {
        let wanted = name.map(str::to_lowercase);
        let mut matches = Vec::new();
        let mut truncated = false;
        for element in elements.iter().filter(|element| element.shows()) {
            let named = wanted
                .as_deref()
                .is_none_or(|wanted| element.name.to_lowercase().contains(wanted));
            let typed = role_filter.is_none_or(|filter| role_matches(filter, element.role));
            if named && typed {
                if matches.len() >= 50 {
                    truncated = true;
                    break;
                }
                matches.push(Value::Object(self.describe(window, element)));
            }
        }
        json!({
            "note": CONTENT_NOTE,
            "window": window.id,
            "title": cut(&window.title, MAX_LABEL),
            "matches": matches,
            "truncated": truncated,
        })
    }

    /// The control an action names in a window: by identifier, or by exact
    /// name (then the same name in another letter case) and role; for
    /// typing with neither, the field that has the keyboard or the window's
    /// only field. The rules of [`crate::tree::Desktop::resolve_element`].
    ///
    /// # Errors
    ///
    /// None or several match.
    pub fn choose<'a>(
        &mut self,
        window: &WineWindow,
        elements: &'a [WineElement],
        element: Option<&str>,
        name: Option<&str>,
        role_filter: Option<&str>,
        typing: bool,
    ) -> Result<&'a WineElement, Miss> {
        let shown: Vec<&WineElement> = elements.iter().filter(|e| e.shows()).collect();
        if let Some(id) = element {
            let (in_window, handle) = self.element(id).ok_or_else(|| Miss {
                code: "not_found",
                message: format!("no element {id} was read"),
            })?;
            if in_window != window.id {
                return Err(Miss {
                    code: "not_found",
                    message: format!("{id} is not in window {}", window.id),
                });
            }
            return shown
                .into_iter()
                .find(|candidate| candidate.handle == handle)
                .ok_or_else(|| Miss {
                    code: "not_found",
                    message: format!("{id} no longer exists"),
                });
        }
        let typed =
            |e: &WineElement| role_filter.is_none_or(|filter| role_matches(filter, e.role));
        let chosen: Vec<&WineElement> = match name {
            Some(name) => {
                let exact: Vec<&WineElement> = shown
                    .iter()
                    .copied()
                    .filter(|e| e.name == name && typed(*e))
                    .collect();
                if exact.is_empty() {
                    let lower = name.to_lowercase();
                    shown
                        .iter()
                        .copied()
                        .filter(|e| e.name.to_lowercase() == lower && typed(*e))
                        .collect()
                } else {
                    exact
                }
            }
            None if typing => {
                let fields: Vec<&WineElement> = shown
                    .iter()
                    .copied()
                    .filter(|e| {
                        e.editable() && UiRole::from_atspi(e.role) == UiRole::Text && typed(*e)
                    })
                    .collect();
                let focused: Vec<&WineElement> =
                    fields.iter().copied().filter(|e| e.has("focused")).collect();
                if focused.len() == 1 { focused } else { fields }
            }
            None => {
                return Err(Miss {
                    code: "not_found",
                    message: "name the element: its identifier from a read, or its name"
                        .to_owned(),
                });
            }
        };
        let chosen = if chosen.len() > 1 {
            let kind: Vec<&WineElement> = chosen
                .iter()
                .copied()
                .filter(|e| {
                    let category = UiRole::from_atspi(e.role);
                    if typing {
                        category == UiRole::Text && e.editable()
                    } else {
                        category.is_pressable() || category == UiRole::Text
                    }
                })
                .collect();
            if kind.is_empty() { chosen } else { kind }
        } else {
            chosen
        };
        let chosen = if chosen.len() > 1 {
            let usable: Vec<&WineElement> = chosen.iter().copied().filter(|e| e.usable()).collect();
            if usable.is_empty() { chosen } else { usable }
        } else {
            chosen
        };
        match chosen.as_slice() {
            [one] => Ok(*one),
            [] => Err(Miss {
                code: "not_found",
                message: format!("no element of window {} matches", window.id),
            }),
            several => {
                let names: Vec<String> = several
                    .iter()
                    .take(10)
                    .map(|e| {
                        format!(
                            "{} {} {:?}",
                            self.element_id(&window.id, &e.handle),
                            role_name(e.role),
                            cut(&e.name, 60)
                        )
                    })
                    .collect();
                Err(Miss {
                    code: "ambiguous",
                    message: format!("several elements match: {}", names.join(", ")),
                })
            }
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::net::UnixListener;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::thread;

    use super::*;

    /// A scratch directory.
    pub(crate) struct Scratch(pub(crate) PathBuf);

    impl Scratch {
        pub(crate) fn new(label: &str) -> Scratch {
            static COUNTER: AtomicU32 = AtomicU32::new(0);
            let path = std::env::temp_dir().join(format!(
                "tur-a11y-{}-{}-{label}",
                std::process::id(),
                COUNTER.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&path).unwrap();
            Scratch(path)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// The window the stand-in bridge has: a dialog of 7-Zip with a button,
    /// a field, a password field, a hidden button, a menu item that is not
    /// shown, and a button named Delete.
    pub(crate) fn dialog() -> Value {
        json!({
            "window": "h10", "title": "Add to Archive", "elements": [
                { "id": "h10", "parent": "", "depth": 0, "role": "dialog", "role_id": 16, "name": "Add to Archive",
                  "states": [], "actions": [], "screen": [100, 100, 400, 300] },
                { "id": "h11", "parent": "h10", "depth": 1, "role": "push button", "role_id": 43, "name": "Help",
                  "states": [], "actions": ["press", "focus"], "screen": [120, 360, 80, 24] },
                { "id": "h12", "parent": "h10", "depth": 1, "role": "text", "role_id": 61, "name": "Archive:",
                  "states": ["editable", "focused"], "actions": ["type", "focus"], "screen": [120, 140, 200, 20],
                  "text": "archive.7z" },
                { "id": "h13", "parent": "h10", "depth": 1, "role": "password text", "role_id": 40, "name": "Password",
                  "states": ["editable"], "actions": ["type", "focus"], "screen": [120, 180, 200, 20],
                  "text": "hunter2" },
                { "id": "h14", "parent": "h10", "depth": 1, "role": "push button", "role_id": 43, "name": "Hidden",
                  "states": ["offscreen"], "actions": ["press"], "screen": [0, 0, 10, 10] },
                { "id": "h10/m1", "parent": "h10", "depth": 1, "role": "menu item", "role_id": 35, "name": "New",
                  "states": ["offscreen"], "actions": ["press"] },
                { "id": "h15", "parent": "h10", "depth": 1, "role": "panel", "role_id": 39,
                  "states": [], "actions": [], "screen": [110, 200, 300, 100] },
                { "id": "h16", "parent": "h15", "depth": 2, "role": "push button", "role_id": 43, "name": "Delete",
                  "states": [], "actions": ["press", "focus"], "screen": [130, 220, 80, 24] },
                { "id": "h17", "parent": "h15", "depth": 2, "role": "push button", "role_id": 43, "name": "OK",
                  "states": ["is default"], "actions": ["press", "focus"], "screen": [220, 220, 80, 24] }
            ], "visited": 9, "truncated": false
        })
    }

    pub(crate) fn listing() -> Value {
        json!({ "windows": [
            { "window": "h10", "title": "Add to Archive", "class": "#32770", "exe": "7zFM.exe", "pid": 32,
              "active": true, "screen": [100, 100, 400, 300], "content": [103, 126] },
            { "window": "h20", "title": "", "class": "Shell_TrayWnd", "exe": "explorer.exe", "pid": 8,
              "active": false, "screen": [0, 0, 1280, 30] },
            { "window": "h30", "title": "tiny", "class": "x", "exe": "7zFM.exe", "pid": 32,
              "active": false, "screen": [0, 0, 10, 10] }
        ]})
    }

    /// A stand-in for tur-winbridge.exe on a socket, as tur-runtime offers
    /// it: answers hello, windows, read, act and at_point; records each
    /// request in `log`.
    pub(crate) fn stand_in(directory: &Path, application: &str) -> PathBuf {
        fs::create_dir_all(directory).unwrap();
        fs::set_permissions(directory, fs::Permissions::from_mode(0o700)).unwrap();
        let socket = directory.join(format!("{application}.sock"));
        let listener = UnixListener::bind(&socket).unwrap();
        let log = directory.join(format!("{application}.log"));
        let log_path = log.clone();
        thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { break };
                let mut writer = stream.try_clone().unwrap();
                let reader = BufReader::new(stream);
                for line in reader.lines() {
                    let Ok(line) = line else { break };
                    let request: Value = serde_json::from_str(&line).unwrap();
                    let mut record = fs::OpenOptions::new().create(true).append(true).open(&log_path).unwrap();
                    writeln!(record, "{line}").unwrap();
                    let result = match request["op"].as_str().unwrap_or_default() {
                        "hello" => json!({ "bridge": "tur-winbridge", "protocol": 1 }),
                        "windows" => listing(),
                        "read" if request["window"] == "h10" => dialog(),
                        "read" => json!({ "error": "no such window" }),
                        "act" => json!({ "done": true, "how": "stand-in", "text_now_contains_typed_text": true }),
                        "at_point" => {
                            let (x, y) = (request["x"].as_i64().unwrap(), request["y"].as_i64().unwrap());
                            if (130..210).contains(&x) && (220..244).contains(&y) {
                                json!({ "found": true, "element": dialog()["elements"][7].clone(),
                                        "topmost": { "same_window": true } })
                            } else if (220..300).contains(&x) && (220..244).contains(&y) {
                                json!({ "found": true, "element": dialog()["elements"][8].clone(),
                                        "topmost": { "same_window": false } })
                            } else {
                                json!({ "found": false, "topmost": { "same_window": true } })
                            }
                        }
                        _ => json!({}),
                    };
                    // A line of Wine's own comes first now and then.
                    writeln!(writer, "wine: a message of its own").unwrap();
                    writeln!(writer, "{}", json!({ "id": request["id"], "ok": true, "result": result })).unwrap();
                }
            }
        });
        log
    }

    pub(crate) fn wine_with_stand_in(scratch: &Scratch) -> (Wine, PathBuf) {
        let directory = scratch.0.join("run/tur/winbridge");
        let log = stand_in(&directory, "org.7-zip.7zip");
        let own = fs::metadata(&scratch.0).unwrap().uid();
        (Wine::with_directory(Some(directory), Some(own)), log)
    }

    #[test]
    fn only_the_user_s_own_sockets_named_after_applications_are_used() {
        let scratch = Scratch::new("wine-apps");
        let (wine, _) = wine_with_stand_in(&scratch);
        assert_eq!(wine.applications(), ["org.7-zip.7zip"]);
        // Anything that is not a socket is not an application.
        fs::write(scratch.0.join("run/tur/winbridge/fake.app.sock"), b"").unwrap();
        assert_eq!(wine.applications(), ["org.7-zip.7zip"]);
        // A directory others can enter is not trusted at all.
        fs::set_permissions(scratch.0.join("run/tur/winbridge"), fs::Permissions::from_mode(0o755)).unwrap();
        assert!(wine.applications().is_empty());
        // Nor is one that belongs to somebody else.
        let other = Wine::with_directory(Some(scratch.0.join("run/tur/winbridge")), Some(u32::MAX));
        assert!(other.applications().is_empty());
        assert!(Wine::with_directory(None, Some(0)).applications().is_empty());
        assert!(!is_application_id("../x"));
        assert!(!is_application_id(".hidden"));
        assert!(is_application_id("org.notepad-plus-plus.notepad-plus-plus"));
    }

    #[test]
    fn windows_are_the_application_s_own_with_stable_identifiers() {
        let scratch = Scratch::new("wine-windows");
        let (mut wine, _) = wine_with_stand_in(&scratch);
        let windows = wine.windows();
        assert_eq!(windows.len(), 1, "{windows:?}");
        let window = &windows[0];
        assert_eq!(window.id, "ww1");
        assert_eq!(window.application, "org.7-zip.7zip");
        assert_eq!(window.program, "7zfm.exe");
        assert_eq!(window.also_known_as(), ["7zfm.exe", "7zfm"]);
        assert!(window.active);
        // The same window keeps its identifier.
        assert_eq!(wine.windows()[0].id, "ww1");
        assert_eq!(wine.window("ww1").map(|w| w.handle), Some("h10".to_owned()));
        assert_eq!(wine.window("ww9"), None);
        assert!(Wine::is_window_id("ww12"));
        assert!(!Wine::is_window_id("w12"));
        assert!(!Wine::is_window_id("ww"));
    }

    #[test]
    fn a_read_is_the_node_model_of_the_accessibility_path() {
        let scratch = Scratch::new("wine-read");
        let (mut wine, _) = wine_with_stand_in(&scratch);
        let window = wine.windows().remove(0);
        let (elements, truncated) = wine.read(&window).unwrap();
        assert!(!truncated);
        assert_eq!(elements.len(), 8);
        let read = wine.read_answer(&window, &elements, truncated, 150);
        assert_eq!(read["toolkit"], TOOLKIT);
        assert_eq!(read["note"], CONTENT_NOTE);
        let tree = read["elements"].as_array().unwrap();
        let names: Vec<&str> = tree.iter().filter_map(|e| e["name"].as_str()).collect();
        // The hidden button is left out; the menu item is kept; the unnamed
        // panel gives way to its buttons.
        assert_eq!(names, ["Help", "Archive:", "Password", "New", "Delete", "OK"]);
        let help = &tree[0];
        assert_eq!(help["role"], "push button");
        assert_eq!(help["bounds"], json!([17, 234, 80, 24]));
        // The text of a field is read; the password's never is.
        assert_eq!(tree[1]["text"], "archive.7z");
        assert_eq!(tree[2]["text_withheld"], true);
        assert!(tree[2].get("text").is_none());
        assert!(!serde_json::to_string(&read).unwrap().contains("hunter2"));
        // Within a budget.
        let short = wine.read_answer(&window, &elements, false, 2);
        assert_eq!(short["elements"].as_array().unwrap().len(), 2);
        assert_eq!(short["truncated"], true);
        let found = wine.find_answer(&window, &elements, Some("e"), Some("push button"));
        let found: Vec<&str> = found["matches"].as_array().unwrap().iter().filter_map(|e| e["name"].as_str()).collect();
        assert_eq!(found, ["Help", "Delete"]);
    }

    #[test]
    fn an_element_is_chosen_by_the_rules_of_the_accessibility_path() {
        let scratch = Scratch::new("wine-choose");
        let (mut wine, _) = wine_with_stand_in(&scratch);
        let window = wine.windows().remove(0);
        let (elements, _) = wine.read(&window).unwrap();
        let name = |wine: &mut Wine, name: &str| {
            wine.choose(&window, &elements, None, Some(name), None, false).map(|e| e.handle.clone())
        };
        assert_eq!(name(&mut wine, "Delete"), Ok("h16".to_owned()));
        assert_eq!(name(&mut wine, "delete"), Ok("h16".to_owned()));
        assert_eq!(name(&mut wine, "New"), Ok("h10/m1".to_owned()));
        assert_eq!(name(&mut wine, "Hidden").map_err(|miss| miss.code), Err("not_found"));
        // Typing with no name: the field that has the keyboard.
        let typed = wine.choose(&window, &elements, None, None, None, true).unwrap();
        assert_eq!(typed.handle, "h12");
        // By identifier, only in its own window.
        let id = wine.element_id("ww1", "h16");
        assert_eq!(wine.choose(&window, &elements, Some(&id), None, None, false).unwrap().handle, "h16");
        let elsewhere = wine.element_id("ww7", "h16");
        assert!(wine.choose(&window, &elements, Some(&elsewhere), None, None, false).is_err());
        assert!(wine.choose(&window, &elements, Some("we999"), None, None, false).is_err());
    }

    #[test]
    fn acting_asks_the_bridge_and_says_why_not() {
        let scratch = Scratch::new("wine-act");
        let (mut wine, log) = wine_with_stand_in(&scratch);
        let window = wine.windows().remove(0);
        let done = wine.act(&window, "h12", "type", "hello").unwrap();
        assert_eq!(done["done"], true);
        let requests = fs::read_to_string(&log).unwrap();
        assert!(requests.contains(r#""op":"act""#));
        assert!(requests.contains(r#""text":"hello""#));
        // A press sends no text.
        wine.act(&window, "h11", "press", "ignored").unwrap();
        let last = fs::read_to_string(&log).unwrap().lines().last().unwrap().to_owned();
        assert!(!last.contains("ignored"), "{last}");
    }

    #[test]
    fn a_bridge_that_is_gone_is_left_out() {
        let scratch = Scratch::new("wine-gone");
        let directory = scratch.0.join("run/tur/winbridge");
        fs::create_dir_all(&directory).unwrap();
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).unwrap();
        // A socket left behind by a launcher that died: nobody listens.
        drop(UnixListener::bind(directory.join("com.example.gone.sock")).unwrap());
        let own = fs::metadata(&scratch.0).unwrap().uid();
        let mut wine = Wine::with_directory(Some(directory), Some(own));
        assert_eq!(wine.applications(), ["com.example.gone"]);
        assert!(wine.windows().is_empty());
    }
}
