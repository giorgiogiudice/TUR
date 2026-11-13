// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The accessibility bus, spoken directly over D-Bus.
//!
//! AT-SPI is a set of D-Bus interfaces on a bus of its own, the accessibility
//! bus. Its address is given by `org.a11y.Bus.GetAddress` on the session bus
//! (at-spi2-core's `at-spi-bus-launcher`), or by `AT_SPI_BUS_ADDRESS`. Every
//! application that exposes an accessibility tree is a connection on that
//! bus; each of its elements is an object, named by the connection's name and
//! an object path. The registry daemon (`org.a11y.atspi.Registry`) lists the
//! applications as the children of its root.
//!
//! The interfaces used, as at-spi2-core declares them in its XML (`xml/`,
//! checked against the main branch on 13 November 2026):
//!
//! | Interface | Members used |
//! |---|---|
//! | `org.a11y.atspi.Accessible` | `GetRole`, `GetState`, `GetInterfaces`, `GetChildren`, `GetChildAtIndex`, `GetIndexInParent`; properties `Name`, `Description`, `Parent`, `ChildCount` |
//! | `org.a11y.atspi.Application` | property `ToolkitName` |
//! | `org.a11y.atspi.Component` | `GetExtents`, `GrabFocus`, `ScrollTo` |
//! | `org.a11y.atspi.Action` | `GetActions`, `DoAction` |
//! | `org.a11y.atspi.Text` | `GetText`; properties `CharacterCount`, `CaretOffset` |
//! | `org.a11y.atspi.EditableText` | `InsertText`, `SetTextContents` |
//! | `org.a11y.atspi.Selection` | `SelectChild` |
//! | `org.a11y.atspi.Value` | property `CurrentValue` |
//!
//! Nothing here links libatspi or any other client library: the messages
//! are built with `tur_pax::bus`, which uses the standard library only.

use std::time::Duration;

use tur_pax::bus::{BUS_NAME, BUS_PATH, BusError, Connection, Message, Value};

/// The registry daemon's name on the accessibility bus.
pub const REGISTRY: &str = "org.a11y.atspi.Registry";

/// The object path of the root of every application, and of the registry.
pub const ROOT_PATH: &str = "/org/a11y/atspi/accessible/root";

/// The accessibility interfaces.
pub mod iface {
    /// Every element.
    pub const ACCESSIBLE: &str = "org.a11y.atspi.Accessible";
    /// The root of an application.
    pub const APPLICATION: &str = "org.a11y.atspi.Application";
    /// Position and keyboard focus.
    pub const COMPONENT: &str = "org.a11y.atspi.Component";
    /// What an element can be made to do.
    pub const ACTION: &str = "org.a11y.atspi.Action";
    /// Text that can be read.
    pub const TEXT: &str = "org.a11y.atspi.Text";
    /// Text that can be changed.
    pub const EDITABLE_TEXT: &str = "org.a11y.atspi.EditableText";
    /// A container whose children can be selected.
    pub const SELECTION: &str = "org.a11y.atspi.Selection";
    /// A number.
    pub const VALUE: &str = "org.a11y.atspi.Value";
}

/// `AtspiCoordType`: coordinates relative to the element's window.
pub const COORD_WINDOW: u32 = 1;

/// `AtspiScrollType`: scroll so that the element is anywhere in view.
pub const SCROLL_ANYWHERE: u32 = 6;

/// How long one call to an application may take. An application that
/// hangs does not hang the bridge for longer.
pub const CALL_TIMEOUT: Duration = Duration::from_millis(2500);

/// The names of the AT-SPI roles, by number: `AtspiRole` in at-spi2-core's
/// `atspi/atspi-constants.h`, written as libatspi's `atspi_role_get_name`
/// writes them (checked on 13 November 2026).
pub const ROLE_NAMES: [&str; 131] = [
    "invalid",
    "accelerator label",
    "alert",
    "animation",
    "arrow",
    "calendar",
    "canvas",
    "check box",
    "check menu item",
    "color chooser",
    "column header",
    "combo box",
    "date editor",
    "desktop icon",
    "desktop frame",
    "dial",
    "dialog",
    "directory pane",
    "drawing area",
    "file chooser",
    "filler",
    "focus traversable",
    "font chooser",
    "frame",
    "glass pane",
    "html container",
    "icon",
    "image",
    "internal frame",
    "label",
    "layered pane",
    "list",
    "list item",
    "menu",
    "menu bar",
    "menu item",
    "option pane",
    "page tab",
    "page tab list",
    "panel",
    "password text",
    "popup menu",
    "progress bar",
    "push button",
    "radio button",
    "radio menu item",
    "root pane",
    "row header",
    "scroll bar",
    "scroll pane",
    "separator",
    "slider",
    "spin button",
    "split pane",
    "status bar",
    "table",
    "table cell",
    "table column header",
    "table row header",
    "tear off menu item",
    "terminal",
    "text",
    "toggle button",
    "tool bar",
    "tool tip",
    "tree",
    "tree table",
    "unknown",
    "viewport",
    "window",
    "extended",
    "header",
    "footer",
    "paragraph",
    "ruler",
    "application",
    "autocomplete",
    "edit bar",
    "embedded",
    "entry",
    "chart",
    "caption",
    "document frame",
    "heading",
    "page",
    "section",
    "redundant object",
    "form",
    "link",
    "input method window",
    "table row",
    "tree item",
    "document spreadsheet",
    "document presentation",
    "document text",
    "document web",
    "document email",
    "comment",
    "list box",
    "grouping",
    "image map",
    "notification",
    "info bar",
    "level bar",
    "title bar",
    "block quote",
    "audio",
    "video",
    "definition",
    "article",
    "landmark",
    "log",
    "marquee",
    "math",
    "rating",
    "timer",
    "static",
    "math fraction",
    "math root",
    "subscript",
    "superscript",
    "description list",
    "description term",
    "description value",
    "footnote",
    "content deletion",
    "content insertion",
    "mark",
    "suggestion",
    "push button menu",
    "switch",
];

/// The name of an AT-SPI role number; `"unknown"` for one beyond the table.
pub fn role_name(role: u32) -> &'static str {
    ROLE_NAMES.get(role as usize).copied().unwrap_or("unknown")
}

/// Role numbers the bridge treats specially.
pub mod role {
    /// `ATSPI_ROLE_ALERT`
    pub const ALERT: u32 = 2;
    /// `ATSPI_ROLE_DIALOG`
    pub const DIALOG: u32 = 16;
    /// `ATSPI_ROLE_FILE_CHOOSER`
    pub const FILE_CHOOSER: u32 = 19;
    /// `ATSPI_ROLE_FILLER`
    pub const FILLER: u32 = 20;
    /// `ATSPI_ROLE_FRAME`
    pub const FRAME: u32 = 23;
    /// `ATSPI_ROLE_GLASS_PANE`
    pub const GLASS_PANE: u32 = 24;
    /// `ATSPI_ROLE_LAYERED_PANE`
    pub const LAYERED_PANE: u32 = 30;
    /// `ATSPI_ROLE_PANEL`
    pub const PANEL: u32 = 39;
    /// `ATSPI_ROLE_PASSWORD_TEXT`
    pub const PASSWORD_TEXT: u32 = 40;
    /// `ATSPI_ROLE_ROOT_PANE`
    pub const ROOT_PANE: u32 = 46;
    /// `ATSPI_ROLE_SCROLL_PANE`
    pub const SCROLL_PANE: u32 = 49;
    /// `ATSPI_ROLE_TERMINAL`
    pub const TERMINAL: u32 = 60;
    /// `ATSPI_ROLE_UNKNOWN`
    pub const UNKNOWN: u32 = 67;
    /// `ATSPI_ROLE_VIEWPORT`
    pub const VIEWPORT: u32 = 68;
    /// `ATSPI_ROLE_WINDOW`
    pub const WINDOW: u32 = 69;
    /// `ATSPI_ROLE_SECTION`
    pub const SECTION: u32 = 85;
    /// `ATSPI_ROLE_REDUNDANT_OBJECT`
    pub const REDUNDANT_OBJECT: u32 = 86;
    /// `ATSPI_ROLE_GROUPING`
    pub const GROUPING: u32 = 99;

    /// Whether an element of this role is a window of its application.
    pub const fn is_window(role: u32) -> bool {
        matches!(role, ALERT | DIALOG | FILE_CHOOSER | FRAME | WINDOW)
    }

    /// Whether an element of this role only arranges others: when it has
    /// no name and nothing to do, it is left out of what is read and its
    /// children take its place.
    pub const fn is_structural(role: u32) -> bool {
        matches!(
            role,
            0 | FILLER
                | GLASS_PANE
                | LAYERED_PANE
                | PANEL
                | ROOT_PANE
                | SCROLL_PANE
                | UNKNOWN
                | VIEWPORT
                | SECTION
                | REDUNDANT_OBJECT
                | GROUPING
        )
    }
}

/// The names of the AT-SPI states, by number (`AtspiStateType`).
pub const STATE_NAMES: [&str; 44] = [
    "invalid",
    "active",
    "armed",
    "busy",
    "checked",
    "collapsed",
    "defunct",
    "editable",
    "enabled",
    "expandable",
    "expanded",
    "focusable",
    "focused",
    "has tooltip",
    "horizontal",
    "iconified",
    "modal",
    "multi line",
    "multiselectable",
    "opaque",
    "pressed",
    "resizable",
    "selectable",
    "selected",
    "sensitive",
    "showing",
    "single line",
    "stale",
    "transient",
    "vertical",
    "visible",
    "manages descendants",
    "indeterminate",
    "required",
    "truncated",
    "animated",
    "invalid entry",
    "supports autocompletion",
    "selectable text",
    "is default",
    "visited",
    "checkable",
    "has popup",
    "read only",
];

/// State numbers the bridge looks at.
pub mod state {
    /// The window is the active one.
    pub const ACTIVE: u32 = 1;
    /// Checked.
    pub const CHECKED: u32 = 4;
    /// Collapsed.
    pub const COLLAPSED: u32 = 5;
    /// The element no longer exists.
    pub const DEFUNCT: u32 = 6;
    /// Takes text.
    pub const EDITABLE: u32 = 7;
    /// Enabled.
    pub const ENABLED: u32 = 8;
    /// Can be expanded.
    pub const EXPANDABLE: u32 = 9;
    /// Expanded.
    pub const EXPANDED: u32 = 10;
    /// Can take the keyboard.
    pub const FOCUSABLE: u32 = 11;
    /// Has the keyboard.
    pub const FOCUSED: u32 = 12;
    /// Selected.
    pub const SELECTED: u32 = 23;
    /// Sensitive to input.
    pub const SENSITIVE: u32 = 24;
    /// On screen, as far as the toolkit knows.
    pub const SHOWING: u32 = 25;
    /// Visible, as far as the toolkit knows.
    pub const VISIBLE: u32 = 30;
    /// The default control of its window.
    pub const IS_DEFAULT: u32 = 39;
    /// Read only.
    pub const READ_ONLY: u32 = 43;

    /// The states worth telling a model, in the order they are listed.
    pub const REPORTED: [u32; 17] = [
        1, 4, 5, 7, 9, 10, 11, 12, 16, 17, 20, 22, 23, 32, 36, 39, 43,
    ];
}

/// The states of an element: a set of `AtspiStateType` numbers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct States(pub u64);

impl States {
    /// The set from the two words `GetState` answers with.
    pub fn from_words(words: &[u32]) -> Self {
        let low = u64::from(words.first().copied().unwrap_or(0));
        let high = u64::from(words.get(1).copied().unwrap_or(0));
        States(low | (high << 32))
    }

    /// Whether a state is in the set.
    pub const fn has(self, state: u32) -> bool {
        state < 64 && self.0 & (1 << state) != 0
    }

    /// The states worth telling a model, by name, with `disabled` for an
    /// element that is neither enabled nor sensitive.
    pub fn reported(self) -> Vec<&'static str> {
        let mut names: Vec<&'static str> = state::REPORTED
            .iter()
            .filter(|number| self.has(**number))
            .map(|number| STATE_NAMES[*number as usize])
            .collect();
        if self.0 != 0 && !self.has(state::ENABLED) && !self.has(state::SENSITIVE) {
            names.push("disabled");
        }
        names
    }

    /// Whether the toolkit says the element can be seen. A toolkit that
    /// reports no states at all is believed to show it.
    pub const fn shows(self) -> bool {
        self.0 == 0 || self.has(state::SHOWING) || self.has(state::VISIBLE)
    }
}

/// One element on the accessibility bus: the application's connection and
/// the object path.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Node {
    /// The connection's unique name on the accessibility bus.
    pub bus: String,
    /// The object path.
    pub path: String,
}

impl Node {
    /// The root of the application at `bus`.
    pub fn root(bus: &str) -> Self {
        Node {
            bus: bus.to_owned(),
            path: ROOT_PATH.to_owned(),
        }
    }

    /// The element a `(so)` value names. `None` for the null reference
    /// (`/org/a11y/atspi/null`) and for anything that is not a reference.
    pub fn from_value(value: &Value) -> Option<Self> {
        let fields = value.unwrapped().as_struct()?;
        let bus = fields.first()?.as_str()?;
        let path = fields.get(1)?.as_str()?;
        if bus.is_empty() || path.is_empty() || path == "/org/a11y/atspi/null" {
            return None;
        }
        Some(Node {
            bus: bus.to_owned(),
            path: path.to_owned(),
        })
    }
}

/// A whole number of any width a toolkit may answer with.
pub fn integer(value: &Value) -> Option<i64> {
    match value.unwrapped() {
        Value::Byte(n) => Some(i64::from(*n)),
        Value::I16(n) => Some(i64::from(*n)),
        Value::U16(n) => Some(i64::from(*n)),
        Value::I32(n) => Some(i64::from(*n)),
        Value::U32(n) => Some(i64::from(*n)),
        Value::I64(n) => Some(*n),
        Value::U64(n) => i64::try_from(*n).ok(),
        _ => None,
    }
}

/// A number, whole or not.
pub fn number(value: &Value) -> Option<f64> {
    match value.unwrapped() {
        Value::Double(n) => Some(*n),
        other => integer(other).map(|n| n as f64),
    }
}

fn protocol(detail: &str) -> BusError {
    BusError::Protocol(detail.to_owned())
}

/// The address of the session bus: `DBUS_SESSION_BUS_ADDRESS`, or the
/// socket in the session's runtime directory.
pub fn session_address() -> Option<String> {
    if let Ok(address) = std::env::var("DBUS_SESSION_BUS_ADDRESS")
        && !address.is_empty()
    {
        return Some(address);
    }
    std::env::var("XDG_RUNTIME_DIR")
        .ok()
        .filter(|directory| !directory.is_empty())
        .map(|directory| format!("unix:path={directory}/bus"))
}

/// A connection to the accessibility bus.
#[derive(Debug, Clone)]
pub struct Client {
    bus: Connection,
}

impl Client {
    /// Connect to the accessibility bus: `AT_SPI_BUS_ADDRESS`, or the
    /// address the session bus's `org.a11y.Bus` gives.
    ///
    /// # Errors
    ///
    /// Neither bus can be reached, or the session has no accessibility bus.
    pub fn connect(session: Option<&Connection>) -> Result<Self, BusError> {
        let address = match std::env::var("AT_SPI_BUS_ADDRESS") {
            Ok(address) if !address.is_empty() => address,
            _ => {
                let session = session.ok_or_else(|| {
                    BusError::Unavailable("there is no session bus to ask".to_owned())
                })?;
                let reply = session.call(
                    Message::method_call(
                        "org.a11y.Bus",
                        "/org/a11y/bus",
                        "org.a11y.Bus",
                        "GetAddress",
                        Vec::new(),
                    ),
                    Duration::from_secs(10),
                )?;
                reply
                    .first_string()
                    .ok_or_else(|| protocol("org.a11y.Bus gave no address"))?
                    .to_owned()
            }
        };
        let (bus, _incoming) = Connection::open(&address)?;
        Ok(Client { bus })
    }

    /// The connection, for calls this module does not wrap.
    pub fn connection(&self) -> &Connection {
        &self.bus
    }

    fn call(
        &self,
        node: &Node,
        interface: &str,
        member: &str,
        body: Vec<Value>,
    ) -> Result<Vec<Value>, BusError> {
        self.bus
            .call(
                Message::method_call(&node.bus, &node.path, interface, member, body),
                CALL_TIMEOUT,
            )
            .map(|reply| reply.body)
    }

    fn first(
        &self,
        node: &Node,
        interface: &str,
        member: &str,
        body: Vec<Value>,
    ) -> Result<Value, BusError> {
        self.call(node, interface, member, body)?
            .into_iter()
            .next()
            .ok_or_else(|| protocol("an empty answer"))
    }

    /// A property of an element.
    ///
    /// # Errors
    ///
    /// The application did not answer, or answered with an error.
    pub fn property(&self, node: &Node, interface: &str, name: &str) -> Result<Value, BusError> {
        self.first(
            node,
            "org.freedesktop.DBus.Properties",
            "Get",
            vec![Value::string(interface), Value::string(name)],
        )
        .map(|value| value.unwrapped().clone())
    }

    fn boolean(
        &self,
        node: &Node,
        interface: &str,
        member: &str,
        body: Vec<Value>,
    ) -> Result<bool, BusError> {
        self.first(node, interface, member, body)?
            .unwrapped()
            .as_bool()
            .ok_or_else(|| protocol("not a boolean"))
    }

    /// The applications the registry knows.
    ///
    /// # Errors
    ///
    /// The registry did not answer.
    pub fn applications(&self) -> Result<Vec<Node>, BusError> {
        self.children(&Node {
            bus: REGISTRY.to_owned(),
            path: ROOT_PATH.to_owned(),
        })
    }

    /// The role number.
    ///
    /// # Errors
    ///
    /// The application did not answer.
    pub fn role(&self, node: &Node) -> Result<u32, BusError> {
        let value = self.first(node, iface::ACCESSIBLE, "GetRole", Vec::new())?;
        integer(&value)
            .and_then(|n| u32::try_from(n).ok())
            .ok_or_else(|| protocol("a role that is not a number"))
    }

    /// The accessible name.
    ///
    /// # Errors
    ///
    /// The application did not answer.
    pub fn name(&self, node: &Node) -> Result<String, BusError> {
        Ok(self
            .property(node, iface::ACCESSIBLE, "Name")?
            .as_str()
            .unwrap_or_default()
            .to_owned())
    }

    /// The accessible description.
    ///
    /// # Errors
    ///
    /// The application did not answer.
    pub fn description(&self, node: &Node) -> Result<String, BusError> {
        Ok(self
            .property(node, iface::ACCESSIBLE, "Description")?
            .as_str()
            .unwrap_or_default()
            .to_owned())
    }

    /// The states.
    ///
    /// # Errors
    ///
    /// The application did not answer.
    pub fn states(&self, node: &Node) -> Result<States, BusError> {
        let value = self.first(node, iface::ACCESSIBLE, "GetState", Vec::new())?;
        let words: Vec<u32> = value
            .as_array()
            .unwrap_or_default()
            .iter()
            .filter_map(Value::as_u32)
            .collect();
        Ok(States::from_words(&words))
    }

    /// The interfaces the element offers.
    ///
    /// # Errors
    ///
    /// The application did not answer.
    pub fn interfaces(&self, node: &Node) -> Result<Vec<String>, BusError> {
        let value = self.first(node, iface::ACCESSIBLE, "GetInterfaces", Vec::new())?;
        Ok(value
            .as_array()
            .unwrap_or_default()
            .iter()
            .filter_map(|item| item.as_str().map(str::to_owned))
            .collect())
    }

    /// The children, in order. Asked for all at once; one by one where the
    /// toolkit does not answer that.
    ///
    /// # Errors
    ///
    /// The application did not answer either way.
    pub fn children(&self, node: &Node) -> Result<Vec<Node>, BusError> {
        match self.first(node, iface::ACCESSIBLE, "GetChildren", Vec::new()) {
            Ok(value) => Ok(value
                .as_array()
                .unwrap_or_default()
                .iter()
                .filter_map(Node::from_value)
                .collect()),
            Err(BusError::TimedOut) => Err(BusError::TimedOut),
            Err(_) => {
                let count = integer(&self.property(node, iface::ACCESSIBLE, "ChildCount")?)
                    .unwrap_or(0)
                    .clamp(0, 2000);
                let mut children = Vec::new();
                for index in 0..count {
                    let index = i32::try_from(index).unwrap_or(i32::MAX);
                    let child = self.first(
                        node,
                        iface::ACCESSIBLE,
                        "GetChildAtIndex",
                        vec![Value::I32(index)],
                    )?;
                    if let Some(child) = Node::from_value(&child) {
                        children.push(child);
                    }
                }
                Ok(children)
            }
        }
    }

    /// The parent, if it has one.
    ///
    /// # Errors
    ///
    /// The application did not answer.
    pub fn parent(&self, node: &Node) -> Result<Option<Node>, BusError> {
        Ok(Node::from_value(
            &self.property(node, iface::ACCESSIBLE, "Parent")?,
        ))
    }

    /// The element's place among its parent's children.
    ///
    /// # Errors
    ///
    /// The application did not answer.
    pub fn index_in_parent(&self, node: &Node) -> Result<i32, BusError> {
        let value = self.first(node, iface::ACCESSIBLE, "GetIndexInParent", Vec::new())?;
        integer(&value)
            .and_then(|n| i32::try_from(n).ok())
            .ok_or_else(|| protocol("an index that is not a number"))
    }

    /// The application's toolkit, as it names itself.
    ///
    /// # Errors
    ///
    /// The application did not answer.
    pub fn toolkit(&self, application: &Node) -> Result<String, BusError> {
        Ok(self
            .property(application, iface::APPLICATION, "ToolkitName")?
            .as_str()
            .unwrap_or_default()
            .to_owned())
    }

    /// Where the element is, relative to its window: x, y, width, height.
    ///
    /// # Errors
    ///
    /// The application did not answer, or offers no position.
    pub fn extents(&self, node: &Node) -> Result<[i32; 4], BusError> {
        let value = self.first(
            node,
            iface::COMPONENT,
            "GetExtents",
            vec![Value::U32(COORD_WINDOW)],
        )?;
        let fields = value
            .as_struct()
            .ok_or_else(|| protocol("extents that are not a rectangle"))?;
        let mut rect = [0_i32; 4];
        for (slot, field) in rect.iter_mut().zip(fields) {
            *slot = integer(field)
                .and_then(|n| i32::try_from(n).ok())
                .unwrap_or(0);
        }
        Ok(rect)
    }

    /// The names of the element's actions, in order.
    ///
    /// # Errors
    ///
    /// The application did not answer.
    pub fn actions(&self, node: &Node) -> Result<Vec<String>, BusError> {
        let value = self.first(node, iface::ACTION, "GetActions", Vec::new())?;
        Ok(value
            .as_array()
            .unwrap_or_default()
            .iter()
            .map(|action| {
                action
                    .as_struct()
                    .and_then(|fields| fields.first())
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned()
            })
            .collect())
    }

    /// Do the element's action number `index`.
    ///
    /// # Errors
    ///
    /// The application did not answer.
    pub fn do_action(&self, node: &Node, index: i32) -> Result<bool, BusError> {
        self.boolean(node, iface::ACTION, "DoAction", vec![Value::I32(index)])
    }

    /// Give the element the keyboard.
    ///
    /// # Errors
    ///
    /// The application did not answer.
    pub fn grab_focus(&self, node: &Node) -> Result<bool, BusError> {
        self.boolean(node, iface::COMPONENT, "GrabFocus", Vec::new())
    }

    /// Scroll the element into view.
    ///
    /// # Errors
    ///
    /// The application did not answer.
    pub fn scroll_to(&self, node: &Node) -> Result<bool, BusError> {
        self.boolean(
            node,
            iface::COMPONENT,
            "ScrollTo",
            vec![Value::U32(SCROLL_ANYWHERE)],
        )
    }

    /// Select a child of a container.
    ///
    /// # Errors
    ///
    /// The application did not answer.
    pub fn select_child(&self, container: &Node, index: i32) -> Result<bool, BusError> {
        self.boolean(
            container,
            iface::SELECTION,
            "SelectChild",
            vec![Value::I32(index)],
        )
    }

    /// The number of characters of the element's text.
    ///
    /// # Errors
    ///
    /// The application did not answer.
    pub fn character_count(&self, node: &Node) -> Result<i32, BusError> {
        integer(&self.property(node, iface::TEXT, "CharacterCount")?)
            .and_then(|n| i32::try_from(n).ok())
            .ok_or_else(|| protocol("a count that is not a number"))
    }

    /// Where the caret is, in characters; negative if the element has none.
    ///
    /// # Errors
    ///
    /// The application did not answer.
    pub fn caret_offset(&self, node: &Node) -> Result<i32, BusError> {
        integer(&self.property(node, iface::TEXT, "CaretOffset")?)
            .and_then(|n| i32::try_from(n).ok())
            .ok_or_else(|| protocol("an offset that is not a number"))
    }

    /// The first `limit` characters of the element's text.
    ///
    /// # Errors
    ///
    /// The application did not answer.
    pub fn text(&self, node: &Node, limit: i32) -> Result<String, BusError> {
        let count = self.character_count(node)?.clamp(0, limit);
        let value = self.first(
            node,
            iface::TEXT,
            "GetText",
            vec![Value::I32(0), Value::I32(count)],
        )?;
        Ok(value.as_str().unwrap_or_default().to_owned())
    }

    /// Insert text at a position.
    ///
    /// # Errors
    ///
    /// The application did not answer.
    pub fn insert_text(&self, node: &Node, position: i32, text: &str) -> Result<bool, BusError> {
        let length = i32::try_from(text.chars().count()).unwrap_or(i32::MAX);
        self.boolean(
            node,
            iface::EDITABLE_TEXT,
            "InsertText",
            vec![Value::I32(position), Value::string(text), Value::I32(length)],
        )
    }

    /// Replace the whole text.
    ///
    /// # Errors
    ///
    /// The application did not answer.
    pub fn set_text_contents(&self, node: &Node, text: &str) -> Result<bool, BusError> {
        self.boolean(
            node,
            iface::EDITABLE_TEXT,
            "SetTextContents",
            vec![Value::string(text)],
        )
    }

    /// The element's number, for a slider or a progress bar.
    ///
    /// # Errors
    ///
    /// The application did not answer.
    pub fn current_value(&self, node: &Node) -> Result<f64, BusError> {
        number(&self.property(node, iface::VALUE, "CurrentValue")?)
            .ok_or_else(|| protocol("a value that is not a number"))
    }

    /// The process of an application, as the accessibility bus itself
    /// records it for the connection.
    ///
    /// # Errors
    ///
    /// The bus does not know the connection.
    pub fn process_of(&self, bus_name: &str) -> Result<u32, BusError> {
        let reply = self.bus.call(
            Message::method_call(
                BUS_NAME,
                BUS_PATH,
                BUS_NAME,
                "GetConnectionUnixProcessID",
                vec![Value::string(bus_name)],
            ),
            CALL_TIMEOUT,
        )?;
        reply
            .body
            .first()
            .and_then(Value::as_u32)
            .ok_or_else(|| protocol("no process identifier"))
    }

    /// The Unix user of an application's connection, as the accessibility
    /// bus records it.
    ///
    /// # Errors
    ///
    /// The bus does not know the connection.
    pub fn user_of(&self, bus_name: &str) -> Result<u32, BusError> {
        self.bus.unix_user_of(bus_name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_role_table_has_every_role_of_at_spi_in_order() {
        assert_eq!(ROLE_NAMES.len(), 131);
        for (number, name) in [
            (0, "invalid"),
            (7, "check box"),
            (16, "dialog"),
            (23, "frame"),
            (29, "label"),
            (32, "list item"),
            (37, "page tab"),
            (40, "password text"),
            (43, "push button"),
            (60, "terminal"),
            (61, "text"),
            (67, "unknown"),
            (69, "window"),
            (75, "application"),
            (79, "entry"),
            (88, "link"),
            (91, "tree item"),
            (95, "document web"),
            (129, "push button menu"),
            (130, "switch"),
        ] {
            assert_eq!(role_name(number), name, "{number}");
        }
        assert_eq!(role_name(131), "unknown");
        assert_eq!(role_name(u32::MAX), "unknown");
    }

    #[test]
    fn the_role_categories_of_the_engine_agree_with_the_names() {
        use tur_pax::UiRole;
        for (number, name) in ROLE_NAMES.iter().enumerate() {
            let number = u32::try_from(number).expect("a small number");
            let category = UiRole::from_atspi(number);
            match *name {
                "push button" | "push button menu" => assert_eq!(category, UiRole::Button),
                "password text" => assert_eq!(category, UiRole::PasswordText),
                "terminal" => assert_eq!(category, UiRole::Terminal),
                "text" | "entry" | "paragraph" | "document text" | "document web" => {
                    assert_eq!(category, UiRole::Text, "{name}");
                }
                "frame" | "window" | "dialog" | "alert" => {
                    assert_eq!(category, UiRole::Window, "{name}");
                }
                "link" => assert_eq!(category, UiRole::Link),
                "page tab" => assert_eq!(category, UiRole::PageTab),
                "list item" => assert_eq!(category, UiRole::ListItem),
                _ => {}
            }
        }
    }

    #[test]
    fn states_are_read_from_two_words() {
        let states = States::from_words(&[
            (1 << state::FOCUSED) | (1 << state::SHOWING),
            1 << (state::IS_DEFAULT - 32),
        ]);
        assert!(states.has(state::FOCUSED));
        assert!(states.has(state::IS_DEFAULT));
        assert!(!states.has(state::CHECKED));
        assert!(states.shows());
        assert_eq!(states.reported(), ["focused", "is default", "disabled"]);
        assert!(!States::from_words(&[1 << state::ENABLED]).shows());
        assert!(States::default().shows());
        assert!(!States(u64::MAX).has(64));
    }

    #[test]
    fn a_reference_is_read_and_the_null_one_is_none() {
        let reference = Value::Struct(vec![
            Value::string(":1.42"),
            Value::path("/org/a11y/atspi/accessible/7"),
        ]);
        assert_eq!(
            Node::from_value(&reference),
            Some(Node {
                bus: ":1.42".to_owned(),
                path: "/org/a11y/atspi/accessible/7".to_owned()
            })
        );
        let null = Value::Struct(vec![Value::string(""), Value::path("/org/a11y/atspi/null")]);
        assert_eq!(Node::from_value(&null), None);
        assert!(Node::from_value(&Value::variant(reference.clone())).is_some());
        assert_eq!(Node::from_value(&Value::string("x")), None);
    }

    #[test]
    fn numbers_of_any_width_are_read() {
        assert_eq!(integer(&Value::I32(-3)), Some(-3));
        assert_eq!(integer(&Value::variant(Value::U32(7))), Some(7));
        assert_eq!(integer(&Value::U64(u64::MAX)), None);
        assert_eq!(number(&Value::Double(0.5)), Some(0.5));
        assert_eq!(number(&Value::string("1")), None);
    }

    #[test]
    fn windows_and_structural_roles_are_told_apart() {
        for window in [2, 16, 19, 23, 69] {
            assert!(role::is_window(window));
            assert!(!role::is_structural(window));
        }
        assert!(role::is_structural(role::PANEL));
        assert!(!role::is_structural(43));
    }
}
