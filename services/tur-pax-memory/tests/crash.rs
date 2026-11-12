// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The file store survives a crash at any point, and damage to one file never
//! reaches another.
//!
//! The crash tests run each changing operation against an in-memory file
//! system that is told to fail at every operation in turn and, for a write,
//! after every possible number of bytes. After each failure the store is
//! opened again, as a restarted process would open it, and must hold either
//! the complete old state or the complete new one.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use serde_json::json;
use tur_pax_memory::fs::MAX_FILE_BYTES;
use tur_pax_memory::{
    ActionRecord, Code, Collection, CrashPoint, EntryId, FileStore, Filter, MemFs, Memory,
    MemoryEntry, MemorySwitch, MissedRunPolicy, NewEntry, NewRun, Problem, Proposal,
    ProposedAction, RecallBudget, RunOutcome, RunRecord, Schedule, ScheduleBook, ScheduleId, StdFs,
    Store, Timestamp, Trigger, UserConfirmation,
};

const ROOT: &str = "/store";

// The test stands in for the user.
struct TheUser(u64);

impl<T: ?Sized> UserConfirmation<T> for TheUser {
    fn covers(&self, _subject: &T) -> bool {
        true
    }

    fn confirmed_at(&self) -> Timestamp {
        Timestamp(self.0)
    }
}

fn memory(fs: &MemFs) -> Memory<FileStore<&MemFs>> {
    Memory::new(FileStore::new(ROOT, fs))
}

fn book(fs: &MemFs) -> ScheduleBook<FileStore<&MemFs>> {
    ScheduleBook::new(FileStore::new(ROOT, fs))
}

fn id(sequence: u64) -> EntryId {
    EntryId::parse(&format!("m-{sequence:08x}")).expect("a valid identifier")
}

// A store with memory on and three entries; the third waits for confirmation.
fn base() -> MemFs {
    let fs = MemFs::new();
    let mut memory = memory(&fs);
    memory
        .set_enabled(MemorySwitch { enabled: true }, &TheUser(1))
        .expect("turns memory on");
    memory
        .add(
            NewEntry::stated("alpha fact one about printers", "conversation"),
            Timestamp(10),
        )
        .expect("adds");
    memory
        .add(
            NewEntry::observed("beta fact two about browsers", "task"),
            Timestamp(11),
        )
        .expect("adds");
    memory
        .add(
            NewEntry::from_content("delta pending fact from a page", "web page"),
            Timestamp(12),
        )
        .expect("adds");
    fs
}

// The entries a freshly started process finds. Fails if any file is damaged.
fn entries(fs: &MemFs) -> Vec<MemoryEntry> {
    let listing = memory(fs).list(&Filter::default()).expect("lists");
    assert!(listing.damaged.is_empty(), "{:?}", listing.damaged);
    listing.entries
}

fn contains_bytes(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

// Whether any file of the store holds these bytes.
fn holds(fs: &MemFs, needle: &[u8]) -> bool {
    fs.files()
        .values()
        .any(|bytes| contains_bytes(bytes, needle))
}

fn is_temporary(path: &Path) -> bool {
    path.to_string_lossy().ends_with(".tmp")
}

type Operation = fn(&MemFs) -> Result<(), Problem>;

fn add(fs: &MemFs) -> Result<(), Problem> {
    let new = NewEntry::stated("charlie fact three, zebra-marker-two", "conversation");
    memory(fs).add(new, Timestamp(20)).map(|_| ())
}

fn correct(fs: &MemFs) -> Result<(), Problem> {
    let text = "A corrected fact with the marker quagga-marker-three.";
    memory(fs).correct(&id(1), text, &TheUser(21)).map(|_| ())
}

fn confirm(fs: &MemFs) -> Result<(), Problem> {
    memory(fs).confirm(&id(3), &TheUser(22)).map(|_| ())
}

fn forget(fs: &MemFs) -> Result<(), Problem> {
    memory(fs).forget(&id(2))
}

fn switch_off(fs: &MemFs) -> Result<(), Problem> {
    memory(fs).set_enabled(MemorySwitch { enabled: false }, &TheUser(23))
}

// Run `operation` with a crash at every point, and check what is found after
// each.
fn crash_everywhere(name: &str, operation: Operation) {
    let start = base();
    let before = entries(&start);

    // Without a crash: how many file-system operations it takes, and what
    // the store holds afterwards.
    let finished = start.clone();
    let first = finished.operations();
    operation(&finished).unwrap_or_else(|problem| panic!("{name} fails on its own: {problem}"));
    let after = entries(&finished);
    let steps = finished.operations() - first;
    assert!(steps > 0, "{name} changes nothing on disk");
    let longest = finished.files().values().map(Vec::len).max().unwrap_or(0);

    for step in 1..=steps {
        for bytes_written in 0..=longest {
            let fs = start.clone();
            fs.crash_at(CrashPoint {
                at_operation: first + step,
                bytes_written,
            });
            let result = operation(&fs);
            assert!(fs.has_crashed(), "{name}: step {step} was not reached");
            assert!(
                result.is_err(),
                "{name}: reported success although step {step} failed"
            );
            fs.restart();

            let place = format!("{name}: crash at step {step} after {bytes_written} bytes");
            let mut reopened = memory(&fs);
            let listing = reopened
                .list(&Filter::default())
                .unwrap_or_else(|problem| panic!("{place}: {problem}"));
            assert!(listing.damaged.is_empty(), "{place}: {:?}", listing.damaged);
            assert!(
                listing.entries == before || listing.entries == after,
                "{place}: the store holds neither the old nor the new state"
            );
            assert!(
                reopened.is_enabled().is_ok(),
                "{place}: the state file is torn"
            );

            // The store goes on working, and the next change removes whatever
            // the interrupted write left behind.
            reopened
                .forget(&id(1))
                .unwrap_or_else(|problem| panic!("{place}: {problem}"));
            let leftovers: Vec<_> = fs
                .files()
                .into_keys()
                .filter(|path| is_temporary(path))
                .collect();
            assert!(leftovers.is_empty(), "{place}: {leftovers:?}");
            assert!(entries(&fs).iter().all(|entry| entry.id() != &id(1)));
        }
    }
}

#[test]
fn a_crash_while_adding_leaves_the_old_or_the_new_state() {
    crash_everywhere("add", add);
}

#[test]
fn a_crash_while_correcting_leaves_the_old_or_the_new_state() {
    crash_everywhere("correct", correct);
}

#[test]
fn a_crash_while_confirming_leaves_the_old_or_the_new_state() {
    crash_everywhere("confirm", confirm);
}

#[test]
fn a_crash_while_forgetting_leaves_the_old_or_the_new_state() {
    crash_everywhere("forget", forget);
}

#[test]
fn a_crash_while_switching_memory_off_leaves_the_switch_readable() {
    crash_everywhere("switch off", switch_off);
}

#[test]
fn an_entry_file_truncated_at_any_byte_never_hides_the_others() {
    let start = base();
    let all = entries(&start);
    let store = FileStore::new(ROOT, &start);
    for victim in &all {
        let path = store.record_path(Collection::MemoryEntries, victim.id().as_str());
        let bytes = start.file(&path).expect("the entry file");
        let others: Vec<MemoryEntry> = all
            .iter()
            .filter(|entry| entry.id() != victim.id())
            .cloned()
            .collect();
        for length in 0..=bytes.len() {
            let fs = start.clone();
            fs.set_file(&path, &bytes[..length]);
            let mut memory = memory(&fs);
            let listing = memory.list(&Filter::default()).expect("lists");
            // The last byte is the line break after the document. Without it
            // the document is still complete.
            if length + 1 >= bytes.len() {
                assert_eq!(listing.entries, all);
                assert!(listing.damaged.is_empty());
                continue;
            }
            assert_eq!(listing.entries, others, "truncated to {length} bytes");
            assert_eq!(listing.damaged.len(), 1, "truncated to {length} bytes");
            assert_eq!(listing.damaged[0].id, victim.id().as_str());
            assert_eq!(
                memory.get(victim.id()).expect_err("damaged").code,
                Code::MemoryDamaged
            );

            // Recall and export go on without the damaged file.
            let notes = memory
                .recall("fact printers browsers page", RecallBudget::default())
                .expect("recalls");
            assert!(notes.notes().iter().all(|note| note.id() != victim.id()));
            let exported = memory.export().expect("exports");
            assert_eq!(
                exported["entries"].as_array().map(Vec::len),
                Some(others.len())
            );
            assert_eq!(exported["damaged"][0]["id"], victim.id().as_str());

            // The user can remove the damaged file, and only that.
            memory.forget(victim.id()).expect("forgets");
            let listing = memory.list(&Filter::default()).expect("lists");
            assert!(listing.damaged.is_empty());
            assert_eq!(listing.entries, others);
        }
    }
}

#[test]
fn a_truncated_state_file_fails_closed_and_loses_no_entry() {
    let start = base();
    let all = entries(&start);
    let path = FileStore::new(ROOT, &start).record_path(Collection::MemoryState, "state");
    let bytes = start.file(&path).expect("the state file");
    for length in 0..=bytes.len() {
        let fs = start.clone();
        fs.set_file(&path, &bytes[..length]);
        let mut memory = memory(&fs);
        // Reading the entries never depends on the state file.
        assert_eq!(memory.list(&Filter::default()).expect("lists").entries, all);
        let intact = length + 1 >= bytes.len();
        let added = memory.add(
            NewEntry::stated("written beside a damaged state file", "conversation"),
            Timestamp(40),
        );
        let recalled = memory.recall("fact", RecallBudget::default());
        if intact {
            assert_eq!(memory.is_enabled(), Ok(true));
            assert!(added.is_ok());
            assert!(recalled.is_ok());
            continue;
        }
        // It is not known whether the user turned memory off: nothing is
        // written and nothing is recalled.
        assert_eq!(
            memory.is_enabled().expect_err("damaged").code,
            Code::MemoryDamaged,
            "truncated to {length} bytes"
        );
        assert_eq!(added.expect_err("damaged").code, Code::MemoryDamaged);
        assert_eq!(recalled.expect_err("damaged").code, Code::MemoryDamaged);
        assert_eq!(memory.list(&Filter::default()).expect("lists").entries, all);
        // The user sets the switch again, and memory works.
        memory
            .set_enabled(MemorySwitch { enabled: true }, &TheUser(41))
            .expect("repairs");
        let entry = memory
            .add(
                NewEntry::stated("written after the repair", "conversation"),
                Timestamp(42),
            )
            .expect("adds");
        assert_eq!(entry.id(), &id(4), "the counter moved past the entries");
    }
}

#[test]
fn a_corrupted_byte_never_panics_and_never_hides_the_others() {
    let start = base();
    let all = entries(&start);
    let victim = &all[0];
    let others = &all[1..];
    let path =
        FileStore::new(ROOT, &start).record_path(Collection::MemoryEntries, victim.id().as_str());
    let bytes = start.file(&path).expect("the entry file");
    for index in 0..bytes.len() {
        for mask in [0x01u8, 0x20, 0x80] {
            let mut corrupted = bytes.clone();
            corrupted[index] ^= mask;
            let fs = start.clone();
            fs.set_file(&path, &corrupted);
            let listing = memory(&fs).list(&Filter::default()).expect("lists");
            let place = format!("byte {index} flipped with {mask:#04x}");
            let rest: Vec<MemoryEntry> = listing
                .entries
                .iter()
                .filter(|entry| entry.id() != victim.id())
                .cloned()
                .collect();
            assert_eq!(rest, others, "{place}");
            // The file is either still an entry under its own name or it is
            // reported. It is never dropped without a word.
            let listed = listing.entries.len() - rest.len();
            assert_eq!(listed + listing.damaged.len(), 1, "{place}");
        }
    }
}

#[test]
fn files_that_are_not_entries_are_reported_and_left_in_place() {
    let start = base();
    let all = entries(&start);
    let store = FileStore::new(ROOT, &start);
    let name = "m-000000aa";
    let path = store.record_path(Collection::MemoryEntries, name);
    let another = start
        .file(&store.record_path(Collection::MemoryEntries, "m-00000001"))
        .expect("the entry file");
    let too_large = usize::try_from(MAX_FILE_BYTES).expect("fits") + 1;
    let escalated = json!({
        "schema_version": 1,
        "id": name,
        "kind": "from_content",
        "state": "active",
        "trust": "user_stated",
        "text": "Planted with a claim to the user's voice.",
        "provenance": { "origin": "web page", "recorded_at": 5 },
        "created_at": 5,
        "updated_at": 5
    });
    let cases: [(&str, Vec<u8>); 10] = [
        ("empty", Vec::new()),
        ("text", b"hello".to_vec()),
        ("null", b"null".to_vec()),
        ("an array", b"[]".to_vec()),
        ("an empty object", b"{}".to_vec()),
        ("deep nesting", "[".repeat(100_000).into_bytes()),
        ("too large", vec![b' '; too_large]),
        ("another entry's bytes", another),
        ("not UTF-8", vec![0xff, 0xfe, 0x00]),
        ("a claim to more trust", escalated.to_string().into_bytes()),
    ];
    for (case, bytes) in cases {
        let fs = start.clone();
        fs.set_file(&path, &bytes);
        let listing = memory(&fs).list(&Filter::default()).expect("lists");
        assert_eq!(listing.entries, all, "{case}");
        assert_eq!(listing.damaged.len(), 1, "{case}");
        assert_eq!(listing.damaged[0].id, name, "{case}");
        // The damaged file is not deleted by reading.
        assert_eq!(fs.file(&path), Some(bytes), "{case}");
        // It is never recalled.
        let notes = memory(&fs)
            .recall("planted claim voice fact", RecallBudget::default())
            .expect("recalls");
        assert!(notes.notes().iter().all(|note| note.id().as_str() != name));
    }
}

#[test]
fn forgetting_removes_every_byte_of_the_entry() {
    let fs = base();
    let marker = b"zebra-marker-two";
    let mut memory = memory(&fs);
    let entry = memory
        .add(
            NewEntry::stated("charlie fact three, zebra-marker-two", "conversation")
                .with_tags(&["zebra-marker-two"]),
            Timestamp(20),
        )
        .expect("adds");
    assert!(holds(&fs, marker), "the entry is on disk");
    memory.forget(entry.id()).expect("forgets");
    assert!(!holds(&fs, marker), "the forgotten text is still in a file");
    assert_eq!(entries(&fs).len(), 3);
}

#[test]
fn correcting_removes_the_old_text() {
    let fs = base();
    let mut memory = memory(&fs);
    assert!(holds(&fs, b"alpha fact one"));
    memory
        .correct(
            &id(1),
            "A corrected fact, quagga-marker-three.",
            &TheUser(30),
        )
        .expect("corrects");
    assert!(
        !holds(&fs, b"alpha fact one"),
        "the old text is still in a file"
    );
    assert!(holds(&fs, b"quagga-marker-three"));
}

#[test]
fn what_an_interrupted_write_left_behind_is_removed_by_the_next_change() {
    let start = base();
    let marker = b"zebra-marker-two";
    let first = start.operations();
    let mut found = false;
    for step in 1..=16 {
        let fs = start.clone();
        fs.crash_at(CrashPoint {
            at_operation: first + step,
            bytes_written: usize::MAX,
        });
        let result = add(&fs);
        if !fs.has_crashed() {
            assert!(result.is_ok());
            break;
        }
        fs.restart();
        let leftover = fs
            .files()
            .iter()
            .any(|(path, bytes)| is_temporary(path) && contains_bytes(bytes, marker));
        if !leftover {
            continue;
        }
        found = true;
        // The entry was never completed, so it is not listed and the user
        // cannot forget it by name. Its text is on disk all the same.
        assert!(
            entries(&fs)
                .iter()
                .all(|entry| !entry.text().contains("zebra-marker-two"))
        );
        // The next change of any kind removes it.
        memory(&fs).forget(&id(1)).expect("forgets");
        assert!(!holds(&fs, marker), "the leftover survived a later change");
    }
    assert!(
        found,
        "no crash left a temporary file with the entry's text"
    );
}

fn proposal(name: &str) -> Proposal {
    Proposal {
        name: name.to_owned(),
        action: ProposedAction::Task {
            request: "Send the weekly report to my manager.".to_owned(),
            approvals: Vec::new(),
        },
        trigger: Trigger::Once {
            at: Timestamp(90_000),
        },
        missed_runs: MissedRunPolicy::RunOnce,
        proposed_by_task: None,
    }
}

fn schedule_id(sequence: u64) -> ScheduleId {
    ScheduleId::parse(&format!("s-{sequence:08x}")).expect("a valid identifier")
}

fn run_of(schedule: u64) -> NewRun {
    NewRun {
        schedule: schedule_id(schedule),
        scheduled_for: Some(Timestamp(90_000)),
        event: None,
        started_at: Timestamp(90_001),
        outcome: RunOutcome::Completed,
        performed: vec![ActionRecord {
            service: "org.uos.Mail1".to_owned(),
            tool: "mail_send".to_owned(),
            arguments: json!({ "to": "manager@example.org" })
                .as_object()
                .cloned()
                .unwrap_or_default(),
            arguments_withheld: false,
        }],
        deferred: Vec::new(),
        detail: String::new(),
    }
}

// A store with two schedules and one run record.
fn schedule_base() -> MemFs {
    let fs = MemFs::new();
    let mut book = book(&fs);
    book.create(proposal("First"), &TheUser(1_000))
        .expect("creates");
    book.create(proposal("Second"), &TheUser(1_000))
        .expect("creates");
    book.record_run(run_of(1), Timestamp(90_010))
        .expect("records");
    fs
}

fn schedule_state(fs: &MemFs) -> (Vec<Schedule>, Vec<RunRecord>) {
    let book = book(fs);
    let schedules = book.list().expect("lists");
    let runs = book.runs(None).expect("lists");
    assert!(schedules.damaged.is_empty(), "{:?}", schedules.damaged);
    assert!(runs.damaged.is_empty(), "{:?}", runs.damaged);
    (schedules.schedules, runs.runs)
}

fn create_schedule(fs: &MemFs) -> Result<(), Problem> {
    book(fs)
        .create(proposal("Third"), &TheUser(2_000))
        .map(|_| ())
}

fn record_run(fs: &MemFs) -> Result<(), Problem> {
    book(fs)
        .record_run(run_of(2), Timestamp(90_020))
        .map(|_| ())
}

fn mark_presented(fs: &MemFs) -> Result<(), Problem> {
    let run = tur_pax_memory::RunId::parse("r-00000001").expect("a valid identifier");
    book(fs).mark_presented(&run, Timestamp(95_000)).map(|_| ())
}

#[test]
fn a_crash_while_changing_schedules_leaves_the_old_or_the_new_state() {
    let operations: [(&str, Operation); 3] = [
        ("create", create_schedule),
        ("record a run", record_run),
        ("mark presented", mark_presented),
    ];
    for (name, operation) in operations {
        let start = schedule_base();
        let before = schedule_state(&start);
        let finished = start.clone();
        let first = finished.operations();
        operation(&finished).unwrap_or_else(|problem| panic!("{name} fails on its own: {problem}"));
        let after = schedule_state(&finished);
        assert_ne!(before, after, "{name} changes nothing");
        let steps = finished.operations() - first;
        let longest = finished.files().values().map(Vec::len).max().unwrap_or(0);
        for step in 1..=steps {
            for bytes_written in 0..=longest {
                let fs = start.clone();
                fs.crash_at(CrashPoint {
                    at_operation: first + step,
                    bytes_written,
                });
                assert!(operation(&fs).is_err(), "{name}: step {step}");
                fs.restart();
                let found = schedule_state(&fs);
                assert!(
                    found == before || found == after,
                    "{name}: crash at step {step} after {bytes_written} bytes"
                );
            }
        }
    }
}

#[test]
fn a_schedule_file_truncated_at_any_byte_never_hides_the_others() {
    let start = schedule_base();
    let (schedules, runs) = schedule_state(&start);
    let store = FileStore::new(ROOT, &start);
    let path = store.record_path(Collection::Schedules, "s-00000001");
    let bytes = start.file(&path).expect("the schedule file");
    for length in 0..=bytes.len() {
        let fs = start.clone();
        fs.set_file(&path, &bytes[..length]);
        let book = book(&fs);
        let listing = book.list().expect("lists");
        if length + 1 >= bytes.len() {
            assert_eq!(listing.schedules, schedules);
            assert!(listing.damaged.is_empty());
            continue;
        }
        assert_eq!(
            listing.schedules,
            schedules[1..],
            "truncated to {length} bytes"
        );
        assert_eq!(listing.damaged.len(), 1);
        assert_eq!(listing.damaged[0].id, "s-00000001");
        assert_eq!(
            book.get(&schedule_id(1)).expect_err("damaged").code,
            Code::ScheduleDamaged
        );
        // A damaged schedule is not run: nothing is due for it.
        assert_eq!(
            book.next_run(&schedule_id(1), Timestamp(95_000))
                .expect_err("damaged")
                .code,
            Code::ScheduleDamaged
        );
        assert_eq!(book.runs(None).expect("lists").runs, runs);
    }

    let path = store.record_path(Collection::Runs, "r-00000001");
    let bytes = start.file(&path).expect("the run file");
    for length in 0..bytes.len() - 1 {
        let fs = start.clone();
        fs.set_file(&path, &bytes[..length]);
        let book = book(&fs);
        let listing = book.runs(None).expect("lists");
        assert!(listing.runs.is_empty(), "truncated to {length} bytes");
        assert_eq!(listing.damaged.len(), 1);
        assert_eq!(book.list().expect("lists").schedules, schedules);
        // The run is not known to have happened, so the schedule is due
        // again rather than silently skipped.
        assert_eq!(book.last_run(&schedule_id(1)), Ok(None));
    }
}

#[test]
fn files_on_the_real_file_system_are_private_and_leave_no_temporaries() {
    let dir = std::env::temp_dir().join(format!("tur-pax-memory-store-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    let mut store = FileStore::new(dir, StdFs);
    store
        .put(Collection::MemoryEntries, "m-00000001", b"one")
        .expect("writes");
    store
        .put(Collection::MemoryEntries, "m-00000001", b"uno")
        .expect("replaces");
    store
        .put(Collection::MemoryEntries, "m-00000002", b"two")
        .expect("writes");

    let entries_dir = store.collection_dir(Collection::MemoryEntries);
    let mut names: Vec<String> = fs::read_dir(&entries_dir)
        .expect("the directory")
        .map(|entry| {
            entry
                .expect("an entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    names.sort();
    assert_eq!(names, ["m-00000001.json", "m-00000002.json"]);
    assert_eq!(
        store.get(Collection::MemoryEntries, "m-00000001"),
        Ok(Some(b"uno".to_vec()))
    );
    assert_eq!(
        store.ids(Collection::MemoryEntries),
        Ok(vec!["m-00000001".to_owned(), "m-00000002".to_owned()])
    );

    // Readable by the owner only.
    let file = store.record_path(Collection::MemoryEntries, "m-00000001");
    let file_mode = fs::metadata(&file).expect("the file").permissions().mode();
    assert_eq!(file_mode & 0o077, 0, "{file_mode:o}");
    let dir_mode = fs::metadata(entries_dir)
        .expect("the directory")
        .permissions()
        .mode();
    assert_eq!(dir_mode & 0o077, 0, "{dir_mode:o}");

    assert_eq!(
        store.remove(Collection::MemoryEntries, "m-00000001"),
        Ok(true)
    );
    assert_eq!(
        store.remove(Collection::MemoryEntries, "m-00000001"),
        Ok(false)
    );
    assert!(!file.exists());
    assert_eq!(store.get(Collection::MemoryEntries, "m-00000001"), Ok(None));
    let _ = fs::remove_dir_all(store.root());
}
