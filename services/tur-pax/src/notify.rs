// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! Desktop notifications: what a reminder says when it fires, and what the
//! person is told after a scheduled task ran with nobody there.
//!
//! The notifications go to the desktop's own notification service,
//! `org.freedesktop.Notifications`, so the desktop shows them, keeps their
//! history and applies Do Not Disturb. P@x adds no surface of its own
//! (desktop design, section 7.4).
//!
//! The words of a notification are composed here, by plain functions, from
//! what the person confirmed when the schedule was created. Nothing a model
//! wrote during a run goes into a notification, except the count of what a
//! run did. The body is escaped, because a notification service may read
//! the body as markup.
//!
//! P@x sends a notification only for something the person asked to be told
//! about: a reminder they created, and the outcome of a schedule they
//! created. It never sends one to bring itself to mind.

use std::time::Duration;

use crate::bus::{Connection, Message, Value};

/// The bus name of the notification service.
pub const NOTIFICATIONS_NAME: &str = "org.freedesktop.Notifications";

/// The object path of the notification service.
pub const NOTIFICATIONS_PATH: &str = "/org/freedesktop/Notifications";

/// The name the notifications are sent under.
pub const APPLICATION_NAME: &str = "P@x";

/// The desktop entry the notifications name, so that the person can
/// silence P@x in the desktop's notification settings.
pub const DESKTOP_ENTRY: &str = "org.tur.Pax";

/// The longest summary, in characters.
pub const SUMMARY_MAX: usize = 160;

/// The longest body, in characters.
pub const BODY_MAX: usize = 1200;

/// One notification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notification {
    /// The title: what happened, in a few words.
    pub summary: String,
    /// The text under it, already escaped.
    pub body: String,
    /// Whether it stays until the person dismisses it.
    pub stays: bool,
}

/// Shows notifications.
pub trait Notifier: Send + Sync {
    /// Show a notification. Returns the identifier the service gave it.
    ///
    /// # Errors
    ///
    /// A sentence saying why it could not be shown.
    fn notify(&self, notification: &Notification) -> Result<u32, String>;
}

/// The text with the three characters a notification body may take for
/// markup written out, and control characters other than a line break
/// replaced.
pub fn escape_body(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '\n' => escaped.push('\n'),
            other if other.is_control() => escaped.push(' '),
            other => escaped.push(other),
        }
    }
    escaped
}

/// One line, of at most `most` characters, with control characters
/// replaced.
fn one_line(text: &str, most: usize) -> String {
    let cleaned: String = text
        .chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect();
    let cleaned = cleaned.trim();
    if cleaned.chars().count() <= most {
        cleaned.to_owned()
    } else {
        let mut cut: String = cleaned.chars().take(most.saturating_sub(1)).collect();
        cut.push('\u{2026}');
        cut
    }
}

fn limited(text: &str, most: usize) -> String {
    if text.chars().count() <= most {
        text.to_owned()
    } else {
        let mut cut: String = text.chars().take(most.saturating_sub(1)).collect();
        cut.push('\u{2026}');
        cut
    }
}

/// The notification of a reminder.
///
/// The title is the reminder's own message, because that is what the
/// person asked to be told. `late` is when it was due, as a date and a time
/// of day, if it is shown more than two minutes after that: the computer
/// was off, asleep or not signed in.
pub fn reminder(name: &str, message: &str, late: Option<(&str, &str)>) -> Notification {
    let mut body = format!("Reminder: {}", escape_body(&one_line(name, 80)));
    if let Some((date, time)) = late {
        body.push_str(&format!(
            "\nThis was due at {} on {}. The computer was off or asleep then.",
            escape_body(time),
            escape_body(date)
        ));
    }
    Notification {
        summary: one_line(message, SUMMARY_MAX),
        body: limited(&body, BODY_MAX),
        stays: true,
    }
}

/// What a scheduled task did while nobody was there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AwayOutcome {
    /// It finished, and nothing waits for the person.
    Done,
    /// It needed something it may not do alone; that waits for the person.
    Waiting,
    /// It did not finish.
    Failed,
}

/// The notification after a scheduled task ran.
///
/// `performed` and `waiting` are counts. `reason` is a sentence of the
/// system's own, such as the message of an error code; it is never text a
/// model wrote.
pub fn scheduled_task(
    name: &str,
    outcome: AwayOutcome,
    performed: usize,
    waiting: usize,
    reason: &str,
) -> Notification {
    let name = one_line(name, 80);
    let summary = match outcome {
        AwayOutcome::Done => format!("P@x ran \u{201c}{name}\u{201d}"),
        AwayOutcome::Waiting => format!("P@x ran \u{201c}{name}\u{201d} and waited for you"),
        AwayOutcome::Failed => format!("P@x could not finish \u{201c}{name}\u{201d}"),
    };
    let did = match performed {
        0 => "It did nothing on this computer.".to_owned(),
        1 => "It did one thing.".to_owned(),
        count => format!("It did {count} things."),
    };
    let mut body = did;
    match waiting {
        0 => {}
        1 => body.push_str("\nOne thing it may not do without you is waiting for you."),
        count => body.push_str(&format!(
            "\n{count} things it may not do without you are waiting for you."
        )),
    }
    if !reason.trim().is_empty() {
        body.push('\n');
        body.push_str(&escape_body(&one_line(reason, 300)));
    }
    body.push_str("\nSee Settings \u{25b8} P@x \u{25b8} Schedules.");
    Notification {
        summary: one_line(&summary, SUMMARY_MAX),
        body: limited(&body, BODY_MAX),
        stays: true,
    }
}

/// The notification service of the session bus.
#[derive(Debug, Clone)]
pub struct BusNotifier {
    bus: Connection,
}

impl BusNotifier {
    /// A notifier that calls the notification service over `bus`.
    pub fn new(bus: Connection) -> Self {
        BusNotifier { bus }
    }
}

/// The arguments of `org.freedesktop.Notifications.Notify` for a
/// notification: `susssasa{sv}i`.
pub fn notify_arguments(notification: &Notification) -> Vec<Value> {
    vec![
        Value::string(APPLICATION_NAME),
        // Replaces nothing.
        Value::U32(0),
        // No icon of its own: the desktop entry names one.
        Value::string(""),
        Value::string(notification.summary.clone()),
        Value::string(notification.body.clone()),
        // No actions.
        Value::strings(Vec::<String>::new()),
        Value::variant_map(vec![
            // Normal urgency: 1.
            ("urgency", Value::Byte(1)),
            ("desktop-entry", Value::string(DESKTOP_ENTRY)),
        ]),
        // 0: it stays until dismissed. -1: the service decides.
        Value::I32(if notification.stays { 0 } else { -1 }),
    ]
}

impl Notifier for BusNotifier {
    fn notify(&self, notification: &Notification) -> Result<u32, String> {
        let reply = self
            .bus
            .call(
                Message::method_call(
                    NOTIFICATIONS_NAME,
                    NOTIFICATIONS_PATH,
                    NOTIFICATIONS_NAME,
                    "Notify",
                    notify_arguments(notification),
                ),
                Duration::from_secs(25),
            )
            .map_err(|error| error.to_string())?;
        reply
            .body
            .first()
            .and_then(Value::as_u32)
            .ok_or_else(|| "the notification service gave no identifier".to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reminder_says_what_the_person_asked_to_be_told() {
        let shown = reminder("Call Marta", "Call Marta about the report", None);
        assert_eq!(shown.summary, "Call Marta about the report");
        assert_eq!(shown.body, "Reminder: Call Marta");
        assert!(shown.stays);

        // Late: it says when it was due, and why it is late.
        let late = reminder(
            "Call Marta",
            "Call Marta about the report",
            Some(("2026-11-12", "16:00")),
        );
        assert_eq!(late.summary, shown.summary);
        assert!(late.body.contains("This was due at 16:00 on 2026-11-12."));
        assert!(late.body.contains("off or asleep"));
    }

    #[test]
    fn markup_and_control_characters_do_not_reach_the_notification_service() {
        let shown = reminder(
            "<b>bold</b> & more",
            "line one\nline two\u{1b}[2J <a href=\"https://example.net\">click</a>",
            Some(("<i>", "&")),
        );
        // The summary is plain text on one line.
        assert!(!shown.summary.contains('\n'));
        assert!(!shown.summary.contains('\u{1b}'));
        // The body has no markup left in it.
        assert!(!shown.body.contains('<'));
        assert!(!shown.body.contains('>'));
        assert!(shown.body.contains("&lt;b&gt;bold&lt;/b&gt; &amp; more"));
        assert!(shown.body.contains("&lt;i&gt;"));
        assert_eq!(escape_body("a&b<c>d\te\nf"), "a&amp;b&lt;c&gt;d e\nf");

        // Long texts are cut.
        let long = "x".repeat(5000);
        let shown = reminder(&long, &long, None);
        assert_eq!(shown.summary.chars().count(), SUMMARY_MAX);
        assert!(shown.summary.ends_with('\u{2026}'));
        assert!(shown.body.chars().count() <= BODY_MAX);
    }

    #[test]
    fn the_outcome_of_a_scheduled_task_is_said_in_the_systems_words() {
        let done = scheduled_task("Weekly report", AwayOutcome::Done, 3, 0, "");
        assert_eq!(done.summary, "P@x ran \u{201c}Weekly report\u{201d}");
        assert!(done.body.starts_with("It did 3 things."));
        assert!(!done.body.contains("waiting"));
        assert!(done.stays);

        let waiting = scheduled_task("Back up photos", AwayOutcome::Waiting, 1, 2, "");
        assert!(waiting.summary.ends_with("and waited for you"));
        assert!(waiting.body.contains("It did one thing."));
        assert!(waiting.body.contains("2 things it may not do without you"));

        let failed = scheduled_task(
            "Nightly <check>",
            AwayOutcome::Failed,
            0,
            1,
            "P@x stopped: today's spending limit was reached.",
        );
        assert!(failed.summary.starts_with("P@x could not finish"));
        assert!(failed.body.contains("It did nothing on this computer."));
        assert!(failed.body.contains("One thing it may not do without you"));
        assert!(failed.body.contains("today's spending limit was reached"));
        assert!(failed.body.contains("Schedules"));
    }

    #[test]
    fn the_arguments_are_those_of_the_notification_interface() {
        let shown = reminder("Stretch", "Time to stretch", None);
        let arguments = notify_arguments(&shown);
        let signature: String = arguments.iter().map(Value::signature).collect();
        assert_eq!(signature, "susssasa{sv}i");
        assert_eq!(arguments[0].as_str(), Some("P@x"));
        assert_eq!(arguments[3].as_str(), Some("Time to stretch"));
        assert_eq!(arguments[4].as_str(), Some("Reminder: Stretch"));
        assert_eq!(arguments[7], Value::I32(0));
        let call = Message::method_call(
            NOTIFICATIONS_NAME,
            NOTIFICATIONS_PATH,
            NOTIFICATIONS_NAME,
            "Notify",
            arguments,
        );
        assert!(call.encode(1).is_ok());
    }
}
