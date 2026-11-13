// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! Carrying out one operation on one element through its accessibility
//! interfaces.
//!
//! Every operation uses what the element itself offers on the bus: its
//! focus and scrolling (`Component`), its named actions (`Action`), its
//! container's selection (`Selection`) and its text (`EditableText`). No
//! keyboard or pointer input is emulated: on Wayland the accessibility bus
//! cannot inject input, and input through the compositor is the third way
//! of acting, not this one. An element that offers no way to do what was
//! asked is reported as such; nothing else is tried in its place.

use std::sync::atomic::{AtomicBool, Ordering};

use tur_pax::UiOperation;
use tur_pax::bus::BusError;

use crate::atspi::{Client, Node, iface, state};
use crate::tree::Facts;

/// Why an operation was not carried out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotDone {
    /// `not_offered`, `failed` or `stopped`.
    pub code: &'static str,
    /// What happened, for a log and the model.
    pub message: String,
}

impl NotDone {
    fn not_offered(message: impl Into<String>) -> Self {
        NotDone {
            code: "not_offered",
            message: message.into(),
        }
    }

    fn failed(message: impl Into<String>) -> Self {
        NotDone {
            code: "failed",
            message: message.into(),
        }
    }

    fn bus(what: &str, error: &BusError) -> Self {
        NotDone::failed(format!("{what}: the application answered {error}"))
    }
}

/// The action names, in order of preference, that mean an operation. They
/// are compared without regard to letter case. Toolkits name the same thing
/// differently: Qt "Press", GTK "click", Firefox "press" and "jump" for a
/// link, "activate" for a menu item.
pub fn action_names(operation: UiOperation) -> &'static [&'static str] {
    match operation {
        UiOperation::Press => &[
            "press", "click", "activate", "jump", "toggle", "push", "open",
        ],
        UiOperation::Expand => &["expand", "expand or contract", "toggle", "open", "activate"],
        UiOperation::Collapse => &[
            "collapse",
            "expand or contract",
            "toggle",
            "close",
            "activate",
        ],
        UiOperation::Select => &["select", "switch", "activate", "click", "press", "jump"],
        UiOperation::Focus => &["setfocus", "set focus", "focus"],
        _ => &[],
    }
}

/// The index of the first of the element's actions that means
/// `operation`, by the order of [`action_names`].
pub fn choose_action(operation: UiOperation, offered: &[String]) -> Option<usize> {
    for wanted in action_names(operation) {
        if let Some(index) = offered
            .iter()
            .position(|name| name.trim().eq_ignore_ascii_case(wanted))
        {
            return Some(index);
        }
    }
    None
}

/// How many characters are inserted at a time when typing, so that a stop
/// takes effect between two pieces.
pub const TYPE_PIECE: usize = 64;

/// The pieces `text` is typed in.
pub fn pieces(text: &str) -> Vec<String> {
    let characters: Vec<char> = text.chars().collect();
    characters
        .chunks(TYPE_PIECE)
        .map(|chunk| chunk.iter().collect())
        .collect()
}

fn by_action(
    client: &Client,
    node: &Node,
    facts: &Facts,
    operation: UiOperation,
) -> Result<String, NotDone> {
    let offered = if facts.offers(iface::ACTION) {
        client
            .actions(node)
            .map_err(|error| NotDone::bus("its actions could not be read", &error))?
    } else {
        Vec::new()
    };
    let Some(index) = choose_action(operation, &offered) else {
        return Err(NotDone::not_offered(format!(
            "the element offers no action to {} through the accessibility tree; it offers: {}",
            operation.as_str(),
            if offered.is_empty() {
                "nothing".to_owned()
            } else {
                offered.join(", ")
            }
        )));
    };
    let index_i32 = i32::try_from(index).unwrap_or(i32::MAX);
    match client.do_action(node, index_i32) {
        Ok(true) => Ok(offered[index].clone()),
        Ok(false) => Err(NotDone::failed(format!(
            "the application did not do the action {:?}",
            offered[index]
        ))),
        Err(error) => Err(NotDone::bus("the action", &error)),
    }
}

/// Carry out `operation` on the element. Returns a sentence saying what
/// was done.
///
/// # Errors
///
/// The element offers no way to do it, the application refused, or the
/// bridge was stopped while typing.
pub fn perform(
    client: &Client,
    node: &Node,
    facts: &Facts,
    operation: UiOperation,
    text: &str,
    stopped: &AtomicBool,
) -> Result<String, NotDone> {
    match operation {
        UiOperation::Focus => {
            if facts.offers(iface::COMPONENT)
                && let Ok(true) = client.grab_focus(node)
            {
                return Ok("the element has the keyboard".to_owned());
            }
            by_action(client, node, facts, operation).map(|name| format!("did {name:?}"))
        }
        UiOperation::Scroll => {
            if !facts.offers(iface::COMPONENT) {
                return Err(NotDone::not_offered(
                    "the element has no position to scroll to",
                ));
            }
            match client.scroll_to(node) {
                Ok(true) => Ok("scrolled into view".to_owned()),
                Ok(false) => Err(NotDone::failed("the application did not scroll")),
                Err(error) => Err(NotDone::not_offered(format!(
                    "the application does not scroll to an element: {error}"
                ))),
            }
        }
        UiOperation::Expand | UiOperation::Collapse => {
            let expanded = facts.states.has(state::EXPANDED);
            let want = operation == UiOperation::Expand;
            if facts.states.has(state::EXPANDABLE) && expanded == want {
                return Ok(format!(
                    "it was already {}",
                    if want { "expanded" } else { "collapsed" }
                ));
            }
            by_action(client, node, facts, operation).map(|name| format!("did {name:?}"))
        }
        UiOperation::Select => {
            if facts.states.has(state::SELECTED) {
                return Ok("it was already selected".to_owned());
            }
            // Through the container's selection, where it has one.
            if let Ok(Some(parent)) = client.parent(node)
                && client
                    .interfaces(&parent)
                    .unwrap_or_default()
                    .iter()
                    .any(|offered| offered == iface::SELECTION)
                && let Ok(index) = client.index_in_parent(node)
                && let Ok(true) = client.select_child(&parent, index)
            {
                return Ok("selected through its container".to_owned());
            }
            by_action(client, node, facts, operation).map(|name| format!("did {name:?}"))
        }
        UiOperation::Press => {
            by_action(client, node, facts, operation).map(|name| format!("did {name:?}"))
        }
        UiOperation::Type => type_text(client, node, facts, text, stopped),
        UiOperation::Unspecified | UiOperation::Read => {
            Err(NotDone::not_offered("nothing to carry out"))
        }
    }
}

fn type_text(
    client: &Client,
    node: &Node,
    facts: &Facts,
    text: &str,
    stopped: &AtomicBool,
) -> Result<String, NotDone> {
    if !facts.offers(iface::TEXT) && !facts.offers(iface::EDITABLE_TEXT) {
        return Err(NotDone::not_offered(
            "the element offers no text to type into",
        ));
    }
    // The field takes the keyboard first, as it would for a person: some
    // toolkits (Firefox) take text only into the field that has it.
    if facts.offers(iface::COMPONENT) && !facts.states.has(state::FOCUSED) {
        let _ = client.grab_focus(node);
    }
    if facts.offers(iface::EDITABLE_TEXT) {
        let before = client.text(node, 4000).unwrap_or_default();
        let count = client.character_count(node).unwrap_or(0).max(0);
        let caret = client.caret_offset(node).unwrap_or(-1);
        let mut position = if (0..=count).contains(&caret) {
            caret
        } else {
            count
        };
        let parts = pieces(text);
        let total = parts.len();
        for (done, piece) in parts.into_iter().enumerate() {
            if stopped.load(Ordering::SeqCst) {
                return Err(NotDone {
                    code: "stopped",
                    message: format!("stopped after {done} of {total} pieces of text"),
                });
            }
            match client.insert_text(node, position, &piece) {
                Ok(true) => {}
                Ok(false) => {
                    return Err(NotDone::failed(format!(
                        "the application did not take the text (after {done} of {total} pieces)"
                    )));
                }
                Err(error) => return Err(NotDone::bus("inserting text", &error)),
            }
            position += i32::try_from(piece.chars().count()).unwrap_or(0);
        }
        // An application may take the text after it has answered (Firefox
        // passes it to the page's process). If it is not there after a
        // short wait, the whole text is set instead: what it was, with the
        // new text at the caret.
        if arrived(client, node, text) {
            return Ok(format!(
                "typed {} characters at the caret",
                text.chars().count()
            ));
        }
        let at = usize::try_from(position)
            .unwrap_or(0)
            .saturating_sub(text.chars().count())
            .min(before.chars().count());
        let head: String = before.chars().take(at).collect();
        let tail: String = before.chars().skip(at).collect();
        return match client.set_text_contents(node, &format!("{head}{text}{tail}")) {
            Ok(true) if arrived(client, node, text) => Ok(format!(
                "set the field's text with {} characters inserted at the caret",
                text.chars().count()
            )),
            Ok(_) => Err(NotDone::failed(
                "the application answered that it took the text, and the text is not in the field",
            )),
            Err(error) => Err(NotDone::bus("setting the text", &error)),
        };
    }
    // A toolkit that does not say it offers editable text may still take
    // the whole text (Qt Quick does, through its fallback): the text is
    // set to what it was with the new text at its end.
    let before = client
        .text(node, 4000)
        .map_err(|error| NotDone::bus("reading the field", &error))?;
    match client.set_text_contents(node, &format!("{before}{text}")) {
        Ok(true) => Ok(format!(
            "set the field's text to what it was with {} characters at its end",
            text.chars().count()
        )),
        Ok(false) => Err(NotDone::failed("the application did not take the text")),
        Err(error) => Err(NotDone::not_offered(format!(
            "the field takes no text through the accessibility tree: {error}"
        ))),
    }
}

/// Whether `text` is in the field now, or within half a second.
fn arrived(client: &Client, node: &Node, text: &str) -> bool {
    for _ in 0..6 {
        if client.text(node, 4000).is_ok_and(|now| now.contains(text)) {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn offered(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| (*name).to_owned()).collect()
    }

    #[test]
    fn each_toolkit_s_name_for_pressing_is_found() {
        assert_eq!(
            choose_action(UiOperation::Press, &offered(&["Press", "SetFocus"])),
            Some(0)
        );
        assert_eq!(
            choose_action(UiOperation::Press, &offered(&["click"])),
            Some(0)
        );
        assert_eq!(
            choose_action(UiOperation::Press, &offered(&["SetFocus", "jump"])),
            Some(1)
        );
        assert_eq!(
            choose_action(UiOperation::Press, &offered(&["Toggle", "Press"])),
            Some(1)
        );
        // A list row of Qt Quick offers only to take the keyboard.
        assert_eq!(
            choose_action(UiOperation::Press, &offered(&["SetFocus"])),
            None
        );
        assert_eq!(choose_action(UiOperation::Press, &[]), None);
        assert_eq!(
            choose_action(UiOperation::Focus, &offered(&["Press", "SetFocus"])),
            Some(1)
        );
        assert_eq!(
            choose_action(
                UiOperation::Expand,
                &offered(&["activate", "expand or contract"])
            ),
            Some(1)
        );
        assert_eq!(choose_action(UiOperation::Type, &offered(&["press"])), None);
    }

    #[test]
    fn text_is_typed_in_pieces_at_a_character() {
        assert_eq!(pieces("hello"), ["hello"]);
        let long: String = "é".repeat(TYPE_PIECE + 1);
        let parts = pieces(&long);
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0].chars().count(), TYPE_PIECE);
        assert_eq!(parts[1], "é");
        assert!(pieces("").is_empty());
    }
}
