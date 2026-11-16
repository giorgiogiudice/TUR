// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! What P@x did recently, for a report about P@x.
//!
//! P@x's audit trail (`$XDG_STATE_HOME/tur/pax/audit.jsonl`, written by
//! `tur-pax`) holds one JSON object per decision and event. It never holds an
//! argument value, a command line or any output (pax-orchestrator.md,
//! section 7), but a few fields are free text: `MESSAGE` and the statement
//! of a privileged request (`TUR_AI_REQUEST`). A report takes only the
//! fields below, from the newest entries, and they are redacted with the
//! rest of the bundle. Only the person's own `tur-diag report prepare` or
//! `submit` can read the trail, and only when asked to
//! (`--include-pax-activity`): P@x's sandbox hides the home directory.

use serde_json::{Map, Value};

/// The member of the bundle that holds the summary.
pub const PAX_ACTIVITY_PATH: &str = "pax/activity.jsonl";

/// The newest entries kept.
pub const PAX_ACTIVITY_MAX_ENTRIES: usize = 200;

/// The most of the trail read, from its end, in bytes.
pub const PAX_ACTIVITY_MAX_READ_BYTES: u64 = 4 * 1024 * 1024;

/// The fields kept from each entry: when, which task, what kind of action,
/// its class, tier and sandbox, the decision, the outcome and the codes.
pub const PAX_ACTIVITY_FIELDS: [&str; 17] = [
    "__REALTIME_TIMESTAMP",
    "TUR_AI_EVENT",
    "TUR_AI_TASK",
    "TUR_AI_PROVIDER",
    "TUR_AI_ACTION",
    "TUR_AI_CLASS",
    "TUR_AI_TIER",
    "TUR_AI_SANDBOX",
    "TUR_AI_DECISION",
    "TUR_AI_ATTENDED",
    "TUR_AI_RESULT",
    "TUR_AI_EXIT",
    "TUR_AI_OUTPUT_BYTES",
    "TUR_AI_TRUNCATED",
    "TUR_AI_WITHHELD",
    "TUR_AI_WITHHELD_KINDS",
    "TUR_ERROR_CODE",
];

/// The summary of the trail `text`: its newest entries, each with only the
/// fields of [`PAX_ACTIVITY_FIELDS`] whose values are strings, numbers or
/// booleans, one JSON object per line, oldest first. A line that is not a
/// JSON object is left out, and so is an entry with none of the fields.
pub fn pax_activity(text: &str) -> String {
    let mut kept: Vec<String> = Vec::new();
    for line in text.lines().rev() {
        if kept.len() == PAX_ACTIVITY_MAX_ENTRIES {
            break;
        }
        let Ok(Value::Object(entry)) = serde_json::from_str::<Value>(line.trim()) else {
            continue;
        };
        let mut summary = Map::new();
        for field in PAX_ACTIVITY_FIELDS {
            if let Some(value) = entry.get(field)
                && (value.is_string() || value.is_number() || value.is_boolean())
            {
                summary.insert(field.to_owned(), value.clone());
            }
        }
        if !summary.is_empty() {
            kept.push(Value::Object(summary).to_string());
        }
    }
    kept.reverse();
    let mut out = kept.join("\n");
    if !out.is_empty() {
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn only_the_listed_fields_of_the_newest_entries_are_kept() {
        let trail = [
            json!({
                "MESSAGE": "tur-pax: allowed tool:x (class 0, tier 1): ok; path=string",
                "TUR_AI_TASK": "task-1",
                "TUR_AI_ACTION": "org.tur.Diagnostics1:diag_get_status",
                "TUR_AI_CLASS": "0",
                "TUR_AI_DECISION": "allowed",
                "TUR_AI_REQUEST": "rm -rf /home/alice/secret",
                "__REALTIME_TIMESTAMP": "1793000000000000",
                "TUR_AI_PROVIDER": "anthropic",
            })
            .to_string(),
            "not json".to_owned(),
            json!({ "MESSAGE": "only a message" }).to_string(),
            json!({ "TUR_AI_EVENT": "task_finished", "TUR_AI_TASK": { "nested": 1 } }).to_string(),
        ]
        .join("\n");
        let summary = pax_activity(&trail);
        let lines: Vec<Value> = summary
            .lines()
            .map(|line| serde_json::from_str(line).expect("JSON"))
            .collect();
        assert_eq!(lines.len(), 2, "{summary}");
        assert_eq!(lines[0]["TUR_AI_DECISION"], "allowed");
        assert_eq!(lines[0]["TUR_AI_PROVIDER"], "anthropic");
        assert_eq!(lines[1], json!({ "TUR_AI_EVENT": "task_finished" }));
        assert!(!summary.contains("MESSAGE"));
        assert!(!summary.contains("alice"));
        assert!(!summary.contains("TUR_AI_REQUEST"));
    }

    #[test]
    fn at_most_the_newest_entries_are_kept_oldest_first() {
        let trail: String = (0..PAX_ACTIVITY_MAX_ENTRIES + 50)
            .map(|n| format!("{}\n", json!({ "TUR_AI_TASK": format!("task-{n}") })))
            .collect();
        let summary = pax_activity(&trail);
        let lines: Vec<&str> = summary.lines().collect();
        assert_eq!(lines.len(), PAX_ACTIVITY_MAX_ENTRIES);
        assert!(lines[0].contains("task-50\""), "{}", lines[0]);
        assert!(lines[lines.len() - 1].contains(&format!(
            "task-{}\"",
            PAX_ACTIVITY_MAX_ENTRIES + 49
        )));
        assert_eq!(pax_activity(""), "");
    }
}
