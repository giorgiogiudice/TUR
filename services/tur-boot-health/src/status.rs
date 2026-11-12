// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! `status`, `acknowledge` and `export-evidence`: what a person, the updater
//! and `tur-diag` read (design sections 5.10, 5.11 and 8).

use crate::codes;
use crate::host::Host;
use crate::json::{self, Value};
use crate::model::{KernelArgs, VAR_COUNTER};
use crate::state::{self, HISTORY_FILE, NOTICE_FILE, REJECTED_FILE};
use crate::trial::{self, RUN_BOOT, RUN_VERDICT};

/// The most kept from the journal of one boot.
const EVIDENCE_LIMIT: u64 = 16 * 1024 * 1024;
/// How many fallback events keep their evidence.
const EVIDENCE_EVENTS: usize = 5;
const EVIDENCE_DIR: &str = "/var/lib/tur/boot-health/evidence";

fn parsed(text: Option<String>) -> Value {
    text.and_then(|text| json::parse(&text).ok())
        .unwrap_or(Value::Null)
}

/// Everything `status --json` reports, schema version 1.
pub(crate) fn status_json(host: &mut dyn Host) -> Value {
    let args = KernelArgs::parse(&host.cmdline());
    let env = host.env_list();
    let (counter, block) = match &env {
        Ok(block) => (
            Value::opt_str(block.get(VAR_COUNTER).map(String::as_str)),
            Value::from("valid"),
        ),
        Err(error) => (Value::Null, Value::from(format!("unreadable: {error}"))),
    };
    let layout = host.layout();
    let supported = matches!(&layout, Ok(layout) if layout.counting_supported());
    let rejected = state::rejected_parse(&host.read_state(REJECTED_FILE).unwrap_or_default());
    let history = state::history_events(&host.read_state(HISTORY_FILE).unwrap_or_default());
    let recent: Vec<Value> = history.iter().rev().take(10).rev().cloned().collect();
    Value::object(vec![
        ("schema_version", Value::Int(state::SCHEMA_VERSION)),
        ("boot_id", Value::from(host.boot_id())),
        ("build_id", Value::from(host.build_id())),
        ("boot", parsed(host.read_run(RUN_BOOT))),
        ("verdict", parsed(host.read_run(RUN_VERDICT))),
        (
            "counting",
            Value::object(vec![
                ("usable", Value::Bool(args.contract_ok() && supported)),
                ("contract", Value::opt_str(args.contract.as_deref())),
                (
                    "layout",
                    Value::from(match &layout {
                        Ok(layout) => layout.describe(),
                        Err(error) => format!("unknown: {error}"),
                    }),
                ),
                ("environment_block", block),
                ("boot_counter", counter),
            ]),
        ),
        ("watchdog", Value::opt_str(host.watchdog().as_deref())),
        (
            "trial",
            trial::load_record(host)
                .map(|record| record.to_json())
                .unwrap_or(Value::Null),
        ),
        ("rejected", state::rejected_to_json(&rejected)),
        ("notice", parsed(host.read_state(NOTICE_FILE))),
        ("recent_events", Value::Array(recent)),
    ])
}

fn text_of(value: &Value, keys: &[&str]) -> String {
    match value.path(keys) {
        Some(Value::Str(text)) => text.clone(),
        Some(Value::Null) | None => "none".to_owned(),
        Some(other) => other.to_compact(),
    }
}

/// `status` for a person.
pub(crate) fn status_text(status: &Value) -> String {
    let mut out = String::new();
    let mut line = |label: &str, text: String| {
        out.push_str(&format!("{label:<22}{text}\n"));
    };
    line("This boot:", text_of(status, &["boot", "classification"]));
    line("Health gate:", text_of(status, &["verdict", "result"]));
    if let Some(check) = status
        .path(&["verdict", "failed_check"])
        .and_then(Value::as_str)
    {
        line(
            "Failed check:",
            format!(
                "{check} ({}): {}",
                text_of(status, &["verdict", "error_code"]),
                text_of(status, &["verdict", "failed_detail"])
            ),
        );
    }
    line(
        "Attempt counting:",
        if status
            .path(&["counting", "usable"])
            .and_then(Value::as_bool)
            == Some(true)
        {
            format!("usable ({})", text_of(status, &["counting", "layout"]))
        } else {
            format!(
                "NOT usable: contract {}, {}",
                text_of(status, &["counting", "contract"]),
                text_of(status, &["counting", "layout"])
            )
        },
    );
    line(
        "Boot counter:",
        text_of(status, &["counting", "boot_counter"]),
    );
    line("Hardware watchdog:", text_of(status, &["watchdog"]));
    line(
        "Trial in progress:",
        match status.get("trial") {
            Some(Value::Null) | None => "no".to_owned(),
            Some(record) => format!(
                "yes, phase {}, event {}",
                record.str_or_empty("phase"),
                record.str_or_empty("event_id")
            ),
        },
    );
    let rejected = status
        .path(&["rejected", "images"])
        .and_then(Value::as_array)
        .unwrap_or(&[]);
    line("Rejected images:", rejected.len().to_string());
    for image in rejected {
        line(
            "",
            format!(
                "{} {} {}",
                image.str_or_empty("version"),
                image.str_or_empty("image"),
                image.str_or_empty("digest")
            ),
        );
    }
    match status.get("notice") {
        Some(Value::Null) | None => line("Notice:", "none".to_owned()),
        Some(notice) => {
            line(
                "Notice:",
                format!(
                    "{} ({})",
                    notice.str_or_empty("kind"),
                    notice.str_or_empty("error_code")
                ),
            );
            line("", notice.str_or_empty("text"));
            line(
                "",
                "Run `tur-boot-health acknowledge` to dismiss it.".to_owned(),
            );
        }
    }
    out
}

/// The registry of error codes, one per line.
pub(crate) fn codes_text() -> String {
    let mut out = String::new();
    for (code, meaning) in codes::registry() {
        out.push_str(&format!("{code}  {meaning}\n"));
    }
    out
}

/// Dismisses the pending notice. Returns whether there was one.
pub(crate) fn acknowledge(host: &mut dyn Host) -> Result<bool, String> {
    let Some(text) = host.read_state(NOTICE_FILE) else {
        return Ok(false);
    };
    let notice = json::parse(&text).unwrap_or(Value::Null);
    host.remove_state(NOTICE_FILE)?;
    trial::history(
        host,
        "acknowledged",
        &notice.str_or_empty("event_id"),
        vec![("kind", Value::from(notice.str_or_empty("kind")))],
    );
    Ok(true)
}

/// Exports the journals of the newest fallback's attempts, once, and keeps
/// the evidence of the five most recent fallbacks. The files are raw:
/// `tur-diag` redacts them when it builds a bundle (design section 8).
pub(crate) fn export_evidence(host: &mut dyn Host) -> Result<String, String> {
    let history = state::history_events(&host.read_state(HISTORY_FILE).unwrap_or_default());
    let Some(event) = history
        .iter()
        .rev()
        .find(|event| event.str_or_empty("event") == "fallback")
    else {
        return Ok("no fallback has happened; nothing to export".to_owned());
    };
    let event_id = event.str_or_empty("event_id");
    if event_id.is_empty() || event_id.contains('/') {
        return Err("the fallback event has no usable identifier".to_owned());
    }
    let summary_name = format!("evidence/{event_id}/summary.json");
    if host.read_state(&summary_name).is_some() {
        return Ok(format!("evidence for {event_id} already exists"));
    }
    // The attempts that reached this program, and every other boot the
    // journal has between the arming boot and the fallback boot: an attempt
    // that stopped in emergency mode wrote a journal without reaching us.
    let mut wanted: Vec<String> = Vec::new();
    let known = host.journal_boots();
    let armed = event.str_or_empty("armed_boot_id");
    let fallback_boot = event.str_or_empty("boot_id");
    let start = known.iter().position(|boot| *boot == armed);
    let end = known.iter().position(|boot| *boot == fallback_boot);
    if let (Some(start), Some(end)) = (start, end)
        && start < end
    {
        wanted.extend(known[start + 1..end].iter().cloned());
    }
    for boot in event
        .get("attempt_boot_ids")
        .and_then(Value::as_array)
        .unwrap_or(&[])
    {
        if let Some(boot) = boot.as_str()
            && !wanted.iter().any(|known| known == boot)
        {
            wanted.push(boot.to_owned());
        }
    }
    let mut exported: Vec<Value> = Vec::new();
    for boot in &wanted {
        let name = format!("evidence/{event_id}/journal-{boot}.export");
        let outcome = host.export_journal(boot, &name, EVIDENCE_LIMIT);
        exported.push(Value::object(vec![
            ("boot_id", Value::from(boot.as_str())),
            (
                "file",
                Value::opt_str(outcome.as_ref().ok().map(String::as_str)),
            ),
            (
                "error",
                Value::opt_str(outcome.as_ref().err().map(String::as_str)),
            ),
        ]));
    }
    let pstore: Vec<Value> = host
        .list_dir("/var/lib/systemd/pstore")
        .into_iter()
        .map(Value::from)
        .collect();
    let summary = Value::object(vec![
        ("schema_version", Value::Int(state::SCHEMA_VERSION)),
        ("event_id", Value::from(event_id.as_str())),
        ("event", event.clone()),
        ("journals", Value::Array(exported)),
        (
            "boots_in_the_journal_during_the_trial",
            Value::Int(wanted.len() as i64),
        ),
        ("pstore_archive", Value::Array(pstore)),
        (
            "note",
            Value::from(
                "Raw journal exports, not redacted. An attempt that failed before the root \
                 filesystem was mounted wrote no journal; the only evidence of it is the count.",
            ),
        ),
    ]);
    host.write_state(&summary_name, &summary.to_pretty())?;
    // Keep the newest few. Event identifiers start with the time they were
    // made, so they sort by age.
    let mut events = host.list_dir(EVIDENCE_DIR);
    events.sort();
    let excess = events.len().saturating_sub(EVIDENCE_EVENTS);
    for old in events.into_iter().take(excess) {
        if old != event_id {
            let _ = host.remove_state_tree(&format!("evidence/{old}"));
        }
    }
    Ok(format!(
        "exported the journals of {} boots for {event_id}",
        wanted.len()
    ))
}
