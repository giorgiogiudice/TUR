// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! `tur-pax-a11y survey`: what the accessibility bus exposes of each
//! application on it, measured, for the coverage table of the design
//! (`docs/architecture/pax-accessibility.md`, section 7).
//!
//! It only reads, and it reports counts, role names, interface names and
//! action names: never an element's name, description, value or text. It
//! needs no indicator, because it is not P@x and sends nothing anywhere;
//! applications on the built-in excluded list are counted and not read.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

use serde_json::{Value, json};
use tur_pax::UserPolicy;
use tur_pax::bus::Connection;

use crate::atspi::{Client, iface, role_name, session_address};
use crate::tree::{Budget, Desktop, MAX_NODES};

/// The interfaces whose presence is counted.
pub const COUNTED: [&str; 7] = [
    iface::ACTION,
    iface::COMPONENT,
    iface::TEXT,
    iface::EDITABLE_TEXT,
    iface::SELECTION,
    iface::VALUE,
    iface::APPLICATION,
];

/// Survey every application on the accessibility bus.
///
/// # Errors
///
/// A bus cannot be reached.
pub fn survey() -> Result<Value, String> {
    let started = Instant::now();
    let session = session_address()
        .and_then(|address| Connection::open(&address).ok())
        .map(|(connection, _)| connection);
    let client = Client::connect(session.as_ref())
        .map_err(|error| format!("the accessibility bus cannot be reached: {error}"))?;
    let stopped = Arc::new(AtomicBool::new(false));
    let mut desktop = Desktop::new(client, Arc::clone(&stopped));
    let policy = UserPolicy::default();
    let windows = desktop
        .windows()
        .map_err(|error| format!("the registry did not answer: {error}"))?;

    let mut applications: BTreeMap<String, Value> = BTreeMap::new();
    for window in windows {
        let application = window.application.clone();
        let mut identities = vec![application.application.clone()];
        identities.extend(application.also_known_as.iter().cloned());
        let key = format!("{} ({})", application.application, application.bus);
        if identities
            .iter()
            .any(|identity| policy.is_excluded(identity))
        {
            applications.insert(key, json!({ "excluded": true }));
            continue;
        }
        let begun = Instant::now();
        let mut budget = Budget::new(MAX_NODES, Arc::clone(&stopped));
        let elements = desktop.elements(&window, &mut budget);
        let mut roles: BTreeMap<&'static str, usize> = BTreeMap::new();
        let mut interfaces: BTreeMap<&'static str, usize> = BTreeMap::new();
        let mut actions: BTreeMap<String, usize> = BTreeMap::new();
        let mut named = 0;
        let mut positioned = 0;
        let mut editable = 0;
        for (node, facts) in &elements {
            *roles.entry(role_name(facts.role)).or_default() += 1;
            if !facts.name.is_empty() {
                named += 1;
            }
            if facts.editable() {
                editable += 1;
            }
            for counted in COUNTED {
                if facts.offers(counted) {
                    *interfaces
                        .entry(counted.trim_start_matches("org.a11y.atspi."))
                        .or_default() += 1;
                }
            }
            if facts.offers(iface::COMPONENT)
                && let Ok(rect) = desktop.client().extents(node)
                && rect[2] > 0
                && rect[3] > 0
            {
                positioned += 1;
            }
            if facts.offers(iface::ACTION)
                && let Ok(names) = desktop.client().actions(node)
            {
                for name in names {
                    *actions.entry(name.to_lowercase()).or_default() += 1;
                }
            }
        }
        let entry = applications.entry(key).or_insert_with(|| {
            json!({
                "application": application.application,
                "also_known_as": application.also_known_as,
                "toolkit": application.toolkit,
                "pid": application.pid,
                "owner": application.owner,
                "windows": [],
            })
        });
        if let Some(list) = entry.get_mut("windows").and_then(Value::as_array_mut) {
            list.push(json!({
                "window": window.id,
                "role": role_name(window.role),
                "titled": !window.title.is_empty(),
                "elements": elements.len(),
                "named": named,
                "positioned": positioned,
                "editable": editable,
                "roles": roles,
                "interfaces": interfaces,
                "actions": actions,
                "truncated": budget.truncated,
                "visited": budget.visited(),
                "milliseconds": u64::try_from(begun.elapsed().as_millis()).unwrap_or(u64::MAX),
            }));
        }
    }
    // Applications on the bus that showed no window: counted too, so that a
    // runtime that registers and exposes nothing is seen as such.
    if let Ok(all) = desktop.client().applications() {
        for node in all {
            if applications
                .keys()
                .any(|key| key.ends_with(&format!("({})", node.bus)))
            {
                continue;
            }
            let application = desktop.application(&node.bus);
            let key = format!("{} ({})", application.application, application.bus);
            let excluded = std::iter::once(&application.application)
                .chain(application.also_known_as.iter())
                .any(|identity| policy.is_excluded(identity));
            if excluded {
                applications.insert(key, json!({ "excluded": true }));
                continue;
            }
            applications.insert(
                key,
                json!({
                    "application": application.application,
                    "also_known_as": application.also_known_as,
                    "toolkit": application.toolkit,
                    "pid": application.pid,
                    "owner": application.owner,
                    "windows": [],
                }),
            );
        }
    }
    Ok(json!({
        "applications": applications.into_values().collect::<Vec<Value>>(),
        "milliseconds": u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
    }))
}
