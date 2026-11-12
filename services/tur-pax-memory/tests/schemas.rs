// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The published JSON schemas agree with the serde types.
//!
//! Three kinds of check:
//!
//! 1. Every document the library writes is valid against its schema.
//! 2. A set of invalid documents is rejected by the schema **and** by the
//!    loader.
//! 3. The enumerations and limits the schema states are the ones the code
//!    uses.
//!
//! The validator below implements only the keywords the two schemas use, and
//! fails on any other, so a keyword cannot be added to a schema without being
//! checked here. `pattern` is the exception: it is accepted and not evaluated
//! (this crate has no regular-expression engine). The loaders are
//! authoritative.

use serde_json::{Map, Value, json};
use tur_pax_memory::approval::MAX_STANDING_APPROVALS;
use tur_pax_memory::entry::{
    ENTRY_SCHEMA_VERSION, MAX_APPLICATION_BYTES, MAX_ORIGIN_BYTES, MAX_PROCEDURE_NAME_BYTES,
    MAX_PROCEDURE_STEPS, MAX_TAG_BYTES, MAX_TAGS, MAX_TASK_BYTES, MAX_TEXT_BYTES,
};
use tur_pax_memory::schedule::{MAX_NAME_BYTES, MAX_REQUEST_BYTES, SCHEDULE_SCHEMA_VERSION};
use tur_pax_memory::{
    ApprovalRequest, Collection, EntryKind, EntryState, IntervalUnit, LocalTime, Memory,
    MemoryEntry, MemoryStore, MemorySwitch, MissedRunPolicy, NewEntry, Procedure, ProcedureStep,
    Proposal, ProposedAction, ScheduleBook, ScheduleId, ShortMonth, Store, SystemEvent, Timestamp,
    Trigger, UserConfirmation, Weekday,
};

const ENTRY_SCHEMA: &str = include_str!("../../../schemas/pax-memory-entry.schema.json");
const SCHEDULE_SCHEMA: &str = include_str!("../../../schemas/pax-schedule.schema.json");

fn parse(text: &str) -> Value {
    serde_json::from_str(text).expect("the schema is valid JSON")
}

// ------------------------------------------------------------------ validator

// Keywords that describe and do not constrain, and `pattern`, which this
// validator does not evaluate.
const IGNORED: [&str; 6] = ["$schema", "$id", "title", "description", "$defs", "pattern"];

fn type_matches(name: &str, instance: &Value) -> bool {
    match name {
        "object" => instance.is_object(),
        "array" => instance.is_array(),
        "string" => instance.is_string(),
        "integer" => instance.is_u64() || instance.is_i64(),
        "number" => instance.is_number(),
        "boolean" => instance.is_boolean(),
        "null" => instance.is_null(),
        other => panic!("unsupported type {other}"),
    }
}

fn number(value: &Value) -> f64 {
    value.as_f64().expect("a number")
}

fn length(value: &Value) -> usize {
    usize::try_from(value.as_u64().expect("a length")).expect("fits")
}

// Validate `instance` against `schema`. Returns the first problem found.
fn validate(root: &Value, schema: &Value, instance: &Value) -> Result<(), String> {
    let schema = match schema {
        Value::Bool(true) => return Ok(()),
        Value::Bool(false) => return Err("the schema forbids any value here".to_owned()),
        Value::Object(schema) => schema,
        other => panic!("a schema is an object or a boolean, not {other}"),
    };
    for (keyword, rule) in schema {
        match keyword.as_str() {
            "$ref" => {
                let name = rule
                    .as_str()
                    .and_then(|reference| reference.strip_prefix("#/$defs/"))
                    .expect("a reference to a definition in the same schema");
                validate(root, &root["$defs"][name], instance)?;
            }
            "type" => {
                let name = rule.as_str().expect("a type name");
                if !type_matches(name, instance) {
                    return Err(format!("{instance} is not of type {name}"));
                }
            }
            "const" => {
                if instance != rule {
                    return Err(format!("{instance} is not {rule}"));
                }
            }
            "enum" => {
                if !rule.as_array().expect("a list").contains(instance) {
                    return Err(format!("{instance} is not one of {rule}"));
                }
            }
            "required" => {
                if let Some(object) = instance.as_object() {
                    for name in rule.as_array().expect("a list") {
                        let name = name.as_str().expect("a property name");
                        if !object.contains_key(name) {
                            return Err(format!("the property {name} is missing"));
                        }
                    }
                }
            }
            "properties" => {
                if let Some(object) = instance.as_object() {
                    for (name, subschema) in rule.as_object().expect("a map of schemas") {
                        if let Some(value) = object.get(name) {
                            validate(root, subschema, value)
                                .map_err(|problem| format!("{name}: {problem}"))?;
                        }
                    }
                }
            }
            "additionalProperties" => {
                assert_eq!(rule, &Value::Bool(false), "only `false` is supported");
                if let Some(object) = instance.as_object() {
                    let known = schema
                        .get("properties")
                        .and_then(Value::as_object)
                        .expect("additionalProperties needs properties beside it");
                    for name in object.keys() {
                        if !known.contains_key(name) {
                            return Err(format!("the property {name} is not allowed"));
                        }
                    }
                }
            }
            "items" => {
                if let Some(items) = instance.as_array() {
                    for item in items {
                        validate(root, rule, item)?;
                    }
                }
            }
            "minItems" => {
                if instance
                    .as_array()
                    .is_some_and(|items| items.len() < length(rule))
                {
                    return Err(format!("fewer than {rule} items"));
                }
            }
            "maxItems" => {
                if instance
                    .as_array()
                    .is_some_and(|items| items.len() > length(rule))
                {
                    return Err(format!("more than {rule} items"));
                }
            }
            "minLength" => {
                if instance
                    .as_str()
                    .is_some_and(|text| text.chars().count() < length(rule))
                {
                    return Err(format!("shorter than {rule} characters"));
                }
            }
            "maxLength" => {
                if instance
                    .as_str()
                    .is_some_and(|text| text.chars().count() > length(rule))
                {
                    return Err(format!("longer than {rule} characters"));
                }
            }
            "minimum" => {
                if instance.is_number() && number(instance) < number(rule) {
                    return Err(format!("{instance} is below {rule}"));
                }
            }
            "maximum" => {
                if instance.is_number() && number(instance) > number(rule) {
                    return Err(format!("{instance} is above {rule}"));
                }
            }
            "oneOf" => {
                let matching = rule
                    .as_array()
                    .expect("a list of schemas")
                    .iter()
                    .filter(|option| validate(root, option, instance).is_ok())
                    .count();
                if matching != 1 {
                    return Err(format!("{matching} of the alternatives match, not one"));
                }
            }
            other => {
                assert!(IGNORED.contains(&other), "unsupported keyword {other}");
            }
        }
    }
    Ok(())
}

fn is_valid(schema: &Value, instance: &Value) -> bool {
    validate(schema, schema, instance).is_ok()
}

// ------------------------------------------------------------------- the user

struct TheUser(u64);

impl<T: ?Sized> UserConfirmation<T> for TheUser {
    fn covers(&self, _subject: &T) -> bool {
        true
    }

    fn confirmed_at(&self) -> Timestamp {
        Timestamp(self.0)
    }
}

fn object(value: Value) -> Map<String, Value> {
    value.as_object().cloned().unwrap_or_default()
}

fn strings(values: &Value) -> Vec<String> {
    let mut found: Vec<String> = values
        .as_array()
        .expect("a list")
        .iter()
        .map(|value| value.as_str().expect("a string").to_owned())
        .collect();
    found.sort();
    found
}

fn sorted(mut names: Vec<String>) -> Vec<String> {
    names.sort();
    names
}

// --------------------------------------------------------------------- entries

// One entry of every kind and state, as the library stores them.
fn sample_entries() -> Vec<Value> {
    let mut memory = Memory::new(MemoryStore::new());
    memory
        .set_enabled(MemorySwitch { enabled: true }, &TheUser(1))
        .expect("turns memory on");
    let now = Timestamp(1_794_477_600);
    memory
        .add(
            NewEntry::stated("My printer is the one in the study.", "conversation")
                .with_task("task-7")
                .with_tags(&["printer", "home"])
                .with_application("org.kde.print"),
            now,
        )
        .expect("adds");
    memory
        .add(
            NewEntry::observed("Firefox is the default browser.", "task"),
            now,
        )
        .expect("adds");
    let procedure = Procedure {
        name: "Reset the wireless profile".to_owned(),
        steps: vec![ProcedureStep {
            service: "org.tur.Network1".to_owned(),
            tool: "network_reset_profile".to_owned(),
            arguments: object(json!({ "profile": "{{profile}}", "retries": 2 })),
        }],
    };
    memory
        .add(
            NewEntry::procedure("Fixes wifi after suspend.", procedure, "task"),
            now,
        )
        .expect("adds");
    memory
        .add(NewEntry::from_content("Seen on a page.", "web page"), now)
        .expect("adds");
    let confirmed = memory
        .add(NewEntry::from_content("Seen in a file.", "file"), now)
        .expect("adds");
    memory
        .confirm(confirmed.id(), &TheUser(1_794_477_700))
        .expect("confirms");
    let exported = memory.export().expect("exports");
    exported["entries"].as_array().expect("entries").clone()
}

#[test]
fn every_stored_entry_is_valid_against_the_schema() {
    let schema = parse(ENTRY_SCHEMA);
    let entries = sample_entries();
    assert_eq!(entries.len(), 5);
    let mut shapes = Vec::new();
    for entry in &entries {
        validate(&schema, &schema, entry).unwrap_or_else(|problem| panic!("{problem}: {entry}"));
        // And the loader reads back what the schema accepted.
        let loaded: MemoryEntry = serde_json::from_value(entry.clone()).expect("loads");
        assert_eq!(&serde_json::to_value(&loaded).expect("serialises"), entry);
        shapes.push((loaded.kind(), loaded.state()));
    }
    assert_eq!(
        shapes,
        [
            (EntryKind::Stated, EntryState::Active),
            (EntryKind::Observed, EntryState::Active),
            (EntryKind::Procedure, EntryState::Active),
            (EntryKind::FromContent, EntryState::Pending),
            (EntryKind::FromContent, EntryState::Active),
        ]
    );
}

type Mutation = (&'static str, usize, fn(&mut Value));

#[test]
fn what_the_entry_schema_rejects_the_loader_rejects() {
    let schema = parse(ENTRY_SCHEMA);
    let entries = sample_entries();
    // The description, the index of the sample to start from, the change.
    let mutations: [Mutation; 19] = [
        ("content claiming the user's voice", 3, |entry| {
            entry["trust"] = json!("user_stated");
        }),
        ("a stated entry downgraded to untrusted", 0, |entry| {
            entry["trust"] = json!("untrusted");
        }),
        ("a pending stated entry", 0, |entry| {
            entry["state"] = json!("pending");
        }),
        ("content made active without a confirmation", 3, |entry| {
            entry["state"] = json!("active");
        }),
        ("a confirmation time on a stated entry", 0, |entry| {
            entry["confirmed_at"] = json!(5);
        }),
        ("a pending entry with a confirmation time", 3, |entry| {
            entry["confirmed_at"] = json!(5);
        }),
        ("an unknown property", 0, |entry| {
            entry["standing_approval"] = json!(true);
        }),
        ("no provenance", 0, |entry| {
            entry
                .as_object_mut()
                .expect("an object")
                .remove("provenance");
        }),
        ("another schema version", 0, |entry| {
            entry["schema_version"] = json!(2);
        }),
        ("an empty text", 0, |entry| {
            entry["text"] = json!("");
        }),
        ("a text that is not a string", 0, |entry| {
            entry["text"] = json!(["a", "list"]);
        }),
        ("too many tags", 0, |entry| {
            entry["tags"] = json!(vec!["t"; MAX_TAGS + 1]);
        }),
        ("a negative time", 0, |entry| {
            entry["created_at"] = json!(-1);
        }),
        ("a time beyond the supported range", 0, |entry| {
            entry["updated_at"] = json!(253_402_300_800u64);
        }),
        ("steps on a stated entry", 0, |entry| {
            entry["procedure"] = json!({
                "name": "x",
                "steps": [{ "service": "a.b", "tool": "t", "arguments": {} }]
            });
        }),
        ("a procedure without steps", 2, |entry| {
            entry
                .as_object_mut()
                .expect("an object")
                .remove("procedure");
        }),
        ("a procedure with an empty list of steps", 2, |entry| {
            entry["procedure"]["steps"] = json!([]);
        }),
        ("an empty origin", 0, |entry| {
            entry["provenance"]["origin"] = json!("");
        }),
        ("an unknown property in the provenance", 0, |entry| {
            entry["provenance"]["verified"] = json!(true);
        }),
    ];
    for (name, index, mutate) in mutations {
        let mut entry = entries[index].clone();
        mutate(&mut entry);
        assert!(!is_valid(&schema, &entry), "the schema accepts {name}");
        assert!(
            serde_json::from_value::<MemoryEntry>(entry).is_err(),
            "the loader accepts {name}"
        );
    }
}

#[test]
fn the_entry_schema_states_the_limits_of_the_code() {
    let schema = parse(ENTRY_SCHEMA);
    assert_eq!(
        schema["$id"],
        "https://github.com/giorgiogiudice/TUR/schemas/pax-memory-entry.schema.json"
    );
    assert_eq!(schema["additionalProperties"], false);
    let properties = &schema["properties"];
    assert_eq!(properties["schema_version"]["const"], ENTRY_SCHEMA_VERSION);

    let kinds: Vec<String> = EntryKind::ALL
        .iter()
        .map(|kind| kind.as_str().to_owned())
        .collect();
    assert_eq!(strings(&properties["kind"]["enum"]), sorted(kinds));
    assert_eq!(strings(&properties["state"]["enum"]), ["active", "pending"]);
    let trusts: Vec<String> = EntryKind::ALL
        .iter()
        .map(|kind| kind.trust().as_str().to_owned())
        .collect();
    let mut trusts = sorted(trusts);
    trusts.dedup();
    assert_eq!(strings(&properties["trust"]["enum"]), trusts);

    // Each alternative ties one kind to the trust the code derives from it.
    let alternatives = schema["oneOf"].as_array().expect("alternatives");
    for alternative in alternatives {
        let kind = alternative["properties"]["kind"]["const"]
            .as_str()
            .and_then(EntryKind::parse)
            .expect("a kind");
        assert_eq!(
            alternative["properties"]["trust"]["const"],
            kind.trust().as_str()
        );
        let state = alternative["properties"]["state"]["const"]
            .as_str()
            .and_then(EntryState::parse)
            .expect("a state");
        if kind != EntryKind::FromContent {
            assert_eq!(state, kind.initial_state());
        }
    }

    assert_eq!(properties["text"]["maxLength"], MAX_TEXT_BYTES);
    assert_eq!(properties["tags"]["maxItems"], MAX_TAGS);
    assert_eq!(properties["tags"]["items"]["maxLength"], MAX_TAG_BYTES);
    assert_eq!(
        properties["application"]["maxLength"],
        MAX_APPLICATION_BYTES
    );
    let defs = &schema["$defs"];
    assert_eq!(defs["timestamp"]["maximum"], Timestamp::MAX.0);
    assert_eq!(
        defs["provenance"]["properties"]["origin"]["maxLength"],
        MAX_ORIGIN_BYTES
    );
    assert_eq!(
        defs["provenance"]["properties"]["task"]["maxLength"],
        MAX_TASK_BYTES
    );
    assert_eq!(
        defs["procedure"]["properties"]["name"]["maxLength"],
        MAX_PROCEDURE_NAME_BYTES
    );
    assert_eq!(
        defs["procedure"]["properties"]["steps"]["maxItems"],
        MAX_PROCEDURE_STEPS
    );
}

// ------------------------------------------------------------------- schedules

fn time(text: &str) -> LocalTime {
    LocalTime::parse(text).expect("a valid time")
}

fn sample_triggers() -> Vec<Trigger> {
    let zone = "Europe/Rome".to_owned();
    vec![
        Trigger::Once {
            at: Timestamp(1_800_000_000),
        },
        Trigger::Every {
            every: 15,
            unit: IntervalUnit::Minutes,
            anchor: Timestamp(1_794_477_600),
        },
        Trigger::Daily {
            time: time("07:30"),
            zone: zone.clone(),
        },
        Trigger::Weekly {
            weekdays: vec![Weekday::Monday, Weekday::Friday],
            time: time("09:00"),
            zone: zone.clone(),
        },
        Trigger::Monthly {
            day: 31,
            time: time("08:00"),
            zone,
            if_day_missing: ShortMonth::Skip,
        },
        Trigger::Event {
            event: SystemEvent::DeviceConnected,
        },
    ]
}

// A schedule for every trigger, alternating reminders and tasks, as stored.
fn sample_schedules() -> (ScheduleBook<MemoryStore>, Vec<(ScheduleId, Value)>) {
    let mut book = ScheduleBook::new(MemoryStore::new());
    let mut stored = Vec::new();
    for (index, trigger) in sample_triggers().into_iter().enumerate() {
        let even = index.is_multiple_of(2);
        let action = if even {
            ProposedAction::Task {
                request: "Send the weekly report to my manager.".to_owned(),
                approvals: vec![ApprovalRequest {
                    service: "org.tur.Mail1".to_owned(),
                    tool: "mail_send".to_owned(),
                    arguments: object(json!({ "to": "manager@example.org", "copies": 1 })),
                }],
            }
        } else {
            ProposedAction::Reminder {
                message: "Take the bins out.".to_owned(),
            }
        };
        let proposal = Proposal {
            name: format!("Schedule {index}"),
            action,
            trigger,
            missed_runs: if even {
                MissedRunPolicy::RunOnce
            } else {
                MissedRunPolicy::Skip
            },
            proposed_by_task: (index == 0).then(|| "task-12".to_owned()),
        };
        let schedule = book
            .create(proposal, &TheUser(1_794_477_600))
            .expect("creates");
        let value = serde_json::to_value(&schedule).expect("serialises");
        stored.push((schedule.id().clone(), value));
    }
    (book, stored)
}

// Whether the loader accepts a document as the schedule file of `id`.
fn loads(id: &ScheduleId, document: &Value) -> bool {
    let mut store = MemoryStore::new();
    let bytes = serde_json::to_vec(document).expect("serialises");
    store
        .put(Collection::Schedules, id.as_str(), &bytes)
        .expect("writes");
    ScheduleBook::new(store).get(id).is_ok()
}

#[test]
fn every_stored_schedule_is_valid_against_the_schema() {
    let schema = parse(SCHEDULE_SCHEMA);
    let (book, stored) = sample_schedules();
    assert_eq!(stored.len(), 6);
    let mut kinds = Vec::new();
    for (id, document) in &stored {
        validate(&schema, &schema, document)
            .unwrap_or_else(|problem| panic!("{problem}: {document}"));
        // The file in the store is the same document, and it loads.
        let bytes = book
            .store()
            .get(Collection::Schedules, id.as_str())
            .expect("reads")
            .expect("the schedule file");
        let on_disk: Value = serde_json::from_slice(&bytes).expect("valid JSON");
        assert_eq!(&on_disk, document);
        assert!(loads(id, document));
        kinds.push(
            document["trigger"]["type"]
                .as_str()
                .expect("a type")
                .to_owned(),
        );
    }
    assert_eq!(
        kinds,
        ["once", "every", "daily", "weekly", "monthly", "event"]
    );
    let from_code: Vec<&str> = sample_triggers().iter().map(Trigger::kind).collect();
    assert_eq!(kinds, from_code);
}

#[test]
fn what_the_schedule_schema_rejects_the_loader_rejects() {
    let schema = parse(SCHEDULE_SCHEMA);
    let (_, stored) = sample_schedules();
    // Samples: 0 once (task), 1 every (reminder), 2 daily (task), 3 weekly
    // (reminder), 4 monthly (task), 5 event (reminder).
    let mutations: [Mutation; 24] = [
        ("a reminder with standing approvals", 1, |schedule| {
            schedule["action"]["standing_approvals"] = json!([
                { "service": "org.tur.Mail1", "tool": "mail_send", "arguments": {} }
            ]);
        }),
        ("a reminder with a request", 1, |schedule| {
            schedule["action"]["request"] = json!("Send mail.");
        }),
        ("an unknown property", 0, |schedule| {
            schedule["approved_by"] = json!("the assistant");
        }),
        ("no confirmation time", 0, |schedule| {
            schedule
                .as_object_mut()
                .expect("an object")
                .remove("confirmed_at");
        }),
        ("another schema version", 0, |schedule| {
            schedule["schema_version"] = json!(2);
        }),
        ("an empty name", 0, |schedule| {
            schedule["name"] = json!("");
        }),
        ("an unknown kind of action", 0, |schedule| {
            schedule["action"]["type"] = json!("command");
        }),
        ("an unknown kind of trigger", 0, |schedule| {
            schedule["trigger"]["type"] = json!("hourly");
        }),
        ("a trigger with a property of another kind", 2, |schedule| {
            schedule["trigger"]["every"] = json!(1);
        }),
        ("an interval of zero", 1, |schedule| {
            schedule["trigger"]["every"] = json!(0);
        }),
        ("an unknown unit", 1, |schedule| {
            schedule["trigger"]["unit"] = json!("seconds");
        }),
        ("a weekly trigger without weekdays", 3, |schedule| {
            schedule["trigger"]["weekdays"] = json!([]);
        }),
        ("an unknown weekday", 3, |schedule| {
            schedule["trigger"]["weekdays"] = json!(["funday"]);
        }),
        ("day zero of the month", 4, |schedule| {
            schedule["trigger"]["day"] = json!(0);
        }),
        ("day thirty-two of the month", 4, |schedule| {
            schedule["trigger"]["day"] = json!(32);
        }),
        ("an unknown rule for short months", 4, |schedule| {
            schedule["trigger"]["if_day_missing"] = json!("first_day");
        }),
        ("an unknown event", 5, |schedule| {
            schedule["trigger"]["event"] = json!("meteor_strike");
        }),
        ("an unknown missed-run policy", 0, |schedule| {
            schedule["missed_runs"] = json!("always");
        }),
        ("a time written as text", 0, |schedule| {
            schedule["created_at"] = json!("yesterday");
        }),
        ("an approval without arguments", 0, |schedule| {
            schedule["action"]["standing_approvals"][0]
                .as_object_mut()
                .expect("an object")
                .remove("arguments");
        }),
        ("an approval with a property of its own", 0, |schedule| {
            schedule["action"]["standing_approvals"][0]["class"] = json!(0);
        }),
        ("an approval whose arguments are a list", 0, |schedule| {
            schedule["action"]["standing_approvals"][0]["arguments"] = json!(["to"]);
        }),
        ("too many approvals", 0, |schedule| {
            let one = schedule["action"]["standing_approvals"][0].clone();
            schedule["action"]["standing_approvals"] = json!(vec![one; MAX_STANDING_APPROVALS + 1]);
        }),
        ("a once trigger without its time", 0, |schedule| {
            schedule["trigger"]
                .as_object_mut()
                .expect("an object")
                .remove("at");
        }),
    ];
    for (name, index, mutate) in mutations {
        let (id, original) = &stored[index];
        let mut schedule = original.clone();
        mutate(&mut schedule);
        assert!(!is_valid(&schema, &schedule), "the schema accepts {name}");
        assert!(!loads(id, &schedule), "the loader accepts {name}");
    }
}

#[test]
fn the_schedule_schema_states_the_limits_of_the_code() {
    let schema = parse(SCHEDULE_SCHEMA);
    assert_eq!(
        schema["$id"],
        "https://github.com/giorgiogiudice/TUR/schemas/pax-schedule.schema.json"
    );
    assert_eq!(schema["additionalProperties"], false);
    let properties = &schema["properties"];
    let defs = &schema["$defs"];
    assert_eq!(
        properties["schema_version"]["const"],
        SCHEDULE_SCHEMA_VERSION
    );
    assert_eq!(properties["name"]["maxLength"], MAX_NAME_BYTES);
    assert_eq!(defs["timestamp"]["maximum"], Timestamp::MAX.0);
    assert_eq!(
        defs["reminder"]["properties"]["message"]["maxLength"],
        MAX_REQUEST_BYTES
    );
    assert_eq!(
        defs["task"]["properties"]["request"]["maxLength"],
        MAX_REQUEST_BYTES
    );
    assert_eq!(
        defs["task"]["properties"]["standing_approvals"]["maxItems"],
        MAX_STANDING_APPROVALS
    );
    // A reminder has no place for an approval.
    assert_eq!(defs["reminder"]["additionalProperties"], false);
    assert!(defs["reminder"]["properties"]["standing_approvals"].is_null());
    // A standing approval has exactly the three properties the policy engine
    // reads.
    assert_eq!(defs["standing_approval"]["additionalProperties"], false);
    assert_eq!(
        strings(&defs["standing_approval"]["required"]),
        ["arguments", "service", "tool"]
    );

    let weekdays: Vec<String> = Weekday::ALL
        .iter()
        .map(|weekday| weekday.as_str().to_owned())
        .collect();
    assert_eq!(
        strings(&defs["trigger_weekly"]["properties"]["weekdays"]["items"]["enum"]),
        sorted(weekdays)
    );
    let events: Vec<String> = SystemEvent::ALL
        .iter()
        .map(|event| event.as_str().to_owned())
        .collect();
    assert_eq!(
        strings(&defs["trigger_event"]["properties"]["event"]["enum"]),
        sorted(events)
    );
    let units = [
        IntervalUnit::Days,
        IntervalUnit::Hours,
        IntervalUnit::Minutes,
    ];
    assert_eq!(
        strings(&defs["trigger_every"]["properties"]["unit"]["enum"]),
        units.map(IntervalUnit::as_str)
    );
    assert_eq!(
        strings(&properties["missed_runs"]["enum"]),
        [
            MissedRunPolicy::RunOnce.as_str(),
            MissedRunPolicy::Skip.as_str()
        ]
    );
    assert_eq!(
        strings(&defs["trigger_monthly"]["properties"]["if_day_missing"]["enum"]),
        ["last_day", "skip"]
    );
    // One alternative for each kind of trigger the code has.
    let alternatives: Vec<String> = properties["trigger"]["oneOf"]
        .as_array()
        .expect("alternatives")
        .iter()
        .map(|alternative| {
            let name = alternative["$ref"]
                .as_str()
                .and_then(|reference| reference.strip_prefix("#/$defs/"))
                .expect("a reference");
            defs[name]["properties"]["type"]["const"]
                .as_str()
                .expect("a type")
                .to_owned()
        })
        .collect();
    let from_code: Vec<&str> = sample_triggers().iter().map(Trigger::kind).collect();
    assert_eq!(alternatives, from_code);
}
