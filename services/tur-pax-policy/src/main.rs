// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! `tur-pax-policy`: read and change the policy store of P@x.
//!
//! ```text
//! tur-pax-policy show [--json]
//! tur-pax-policy approvals-for --schedule ID --task TEXT
//! tur-pax-policy set-policy            < policy.json      (polkit: org.tur.pax.policy.change)
//! tur-pax-policy set-approvals         < approvals.json   (polkit: org.tur.pax.policy.change)
//! tur-pax-policy clear-approvals --schedule ID            (polkit: org.tur.pax.policy.tighten)
//! ```
//!
//! Reading is the person's own act and needs nothing. A change is written by
//! this same program running as root: started by the person, it starts
//! itself again through `pkexec`, which asks polkit, and polkit asks for an
//! administrator's password for every change that could loosen P@x. Run as
//! root by `pkexec`, it writes the directory of the person `pkexec` names
//! (`PKEXEC_UID`) and of nobody else.
//!
//! `--root DIR` puts the store in another directory, written as the person,
//! for tests. It is refused when the program runs as root.

use std::env;
use std::io::{self, Read, Write};
use std::process::{Command, ExitCode};

use tur_pax_policy::{
    Approvals, MAX_DOCUMENT_BYTES, Owner, Store, StoreError, is_schedule_id, primary_group,
    process_uid,
};

/// Where the program is installed: the path polkit's actions name.
const INSTALLED: &str = "/usr/bin/tur-pax-policy";

/// The program that asks polkit and starts this one as root.
const PKEXEC: &str = "/usr/bin/pkexec";

const EXIT_OK: u8 = 0;
const EXIT_INVALID: u8 = 1;
const EXIT_USAGE: u8 = 2;
const EXIT_STORE: u8 = 3;
const EXIT_NOT_AUTHORISED: u8 = 4;

const USAGE: &str = "\
Usage: tur-pax-policy show [--json] [--root DIR]
       tur-pax-policy approvals-for --schedule ID --task TEXT [--root DIR]
       tur-pax-policy set-policy [--root DIR] < policy.json
       tur-pax-policy set-approvals [--root DIR] < approvals.json
       tur-pax-policy clear-approvals --schedule ID [--root DIR]

Reads or changes the policy store of P@x: the person's policy and the standing
approvals of their schedules, kept where P@x's own process cannot write them.
A change is made as root, through pkexec and polkit.
";

/// What the command line asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Request {
    command: String,
    json: bool,
    root: Option<String>,
    schedule: Option<String>,
    task: Option<String>,
}

fn parse(arguments: &[String]) -> Result<Request, String> {
    let mut iter = arguments.iter();
    let command = iter.next().ok_or("no command given")?.clone();
    let mut request = Request {
        command,
        json: false,
        root: None,
        schedule: None,
        task: None,
    };
    while let Some(argument) = iter.next() {
        let mut value = |name: &str| {
            iter.next()
                .cloned()
                .ok_or_else(|| format!("{name} needs a value"))
        };
        match argument.as_str() {
            "--json" => request.json = true,
            "--root" => request.root = Some(value("--root")?),
            "--schedule" => request.schedule = Some(value("--schedule")?),
            "--task" => request.task = Some(value("--task")?),
            other => return Err(format!("unknown option: {other}")),
        }
    }
    let needs_schedule = matches!(
        request.command.as_str(),
        "approvals-for" | "clear-approvals"
    );
    match request.command.as_str() {
        "show" | "set-policy" | "set-approvals" | "approvals-for" | "clear-approvals" => {}
        other => return Err(format!("unknown command: {other}")),
    }
    if needs_schedule != request.schedule.is_some() {
        return Err(if needs_schedule {
            "--schedule is needed".to_owned()
        } else {
            "--schedule is not an option of this command".to_owned()
        });
    }
    if (request.command == "approvals-for") != request.task.is_some() {
        return Err("--task goes with approvals-for, and only with it".to_owned());
    }
    if request.json && request.command != "show" {
        return Err("--json goes with show".to_owned());
    }
    if let Some(schedule) = &request.schedule
        && !is_schedule_id(schedule)
    {
        return Err("a schedule identifier is 1 to 64 letters, digits, - and _".to_owned());
    }
    Ok(request)
}

/// How a change is made, from who runs the program.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Writer {
    /// As root, for the person `pkexec` names, in the system's store.
    Root { person: u32 },
    /// As the person, in a store of their own (`--root`): tests.
    Person { uid: u32, root: String },
    /// Start again through `pkexec`.
    Escalate,
}

fn writer(running_as: Option<u32>, root: Option<&str>, pkexec_uid: Option<&str>) -> Result<Writer, String> {
    match (running_as, root) {
        (None, _) => Err("the user this program runs as cannot be read".to_owned()),
        (Some(0), Some(_)) => Err("--root is for tests and is refused when run as root".to_owned()),
        (Some(0), None) => {
            let person = pkexec_uid
                .and_then(|text| text.parse::<u32>().ok())
                .ok_or("run as root, a change is made only through pkexec, for the person who started it")?;
            if person == 0 {
                return Err("root has no P@x policy to change".to_owned());
            }
            Ok(Writer::Root { person })
        }
        (Some(uid), Some(root)) => Ok(Writer::Person {
            uid,
            root: root.to_owned(),
        }),
        (Some(_), None) => Ok(Writer::Escalate),
    }
}

fn read_stdin() -> Result<String, String> {
    let mut text = String::new();
    io::stdin()
        .take(MAX_DOCUMENT_BYTES as u64 + 1)
        .read_to_string(&mut text)
        .map_err(|_| "standard input could not be read as text".to_owned())?;
    if text.len() > MAX_DOCUMENT_BYTES {
        return Err(format!("the document is larger than {MAX_DOCUMENT_BYTES} bytes"));
    }
    Ok(text)
}

fn store_error(error: &StoreError) -> ExitCode {
    eprintln!("tur-pax-policy: {error}");
    ExitCode::from(match error {
        StoreError::Invalid(_) => EXIT_INVALID,
        StoreError::Io(_) => EXIT_STORE,
    })
}

fn show(request: &Request) -> ExitCode {
    let Some(uid) = process_uid() else {
        eprintln!("tur-pax-policy: the user this program runs as cannot be read");
        return ExitCode::from(EXIT_STORE);
    };
    let store = request.root.as_deref().map_or_else(Store::system, Store::at);
    let policy = match store.read_policy(uid) {
        Ok(policy) => policy,
        Err(error) => return store_error(&error),
    };
    let approvals = match store.read_approvals(uid) {
        Ok(approvals) => approvals,
        Err(error) => return store_error(&error),
    };
    if request.command == "approvals-for" {
        let found = approvals.for_schedule(
            request.schedule.as_deref().unwrap_or_default(),
            request.task.as_deref().unwrap_or_default(),
        );
        println!("{}", serde_json::to_string(&found).unwrap_or_default());
        return ExitCode::from(EXIT_OK);
    }
    if request.json {
        let policy_value = policy
            .as_deref()
            .and_then(|text| serde_json::from_str::<serde_json::Value>(text).ok());
        let document = serde_json::json!({
            "store": store.person_dir(uid),
            "policy": policy_value,
            "approvals": approvals,
        });
        println!("{}", serde_json::to_string_pretty(&document).unwrap_or_default());
    } else {
        println!("store: {}", store.person_dir(uid).display());
        match &policy {
            Some(text) => println!("policy:\n{}", text.trim_end()),
            None => println!("policy: none (the default policy applies)"),
        }
        if approvals.schedules.is_empty() {
            println!("standing approvals: none");
        }
        for entry in &approvals.schedules {
            println!(
                "standing approvals of {} ({} calls), for the task {:?}",
                entry.schedule,
                entry.approvals.len(),
                entry.task
            );
        }
    }
    ExitCode::from(EXIT_OK)
}

fn change(request: &Request, arguments: &[String]) -> ExitCode {
    let pkexec_uid = env::var("PKEXEC_UID").ok();
    let how = match writer(process_uid(), request.root.as_deref(), pkexec_uid.as_deref()) {
        Ok(how) => how,
        Err(problem) => {
            eprintln!("tur-pax-policy: {problem}");
            return ExitCode::from(EXIT_USAGE);
        }
    };
    let (store, owner) = match how {
        Writer::Escalate => {
            // polkit decides; this process stays the person's.
            return match Command::new(PKEXEC).arg(INSTALLED).args(arguments).status() {
                Ok(status) if status.success() => ExitCode::from(EXIT_OK),
                Ok(status) => match status.code() {
                    Some(code @ 1..=3) => ExitCode::from(code as u8),
                    _ => {
                        eprintln!("tur-pax-policy: the change was not authorised; nothing was changed");
                        ExitCode::from(EXIT_NOT_AUTHORISED)
                    }
                },
                Err(_) => {
                    eprintln!("tur-pax-policy: {PKEXEC} cannot be started; nothing was changed");
                    ExitCode::from(EXIT_NOT_AUTHORISED)
                }
            };
        }
        Writer::Root { person } => {
            let gid = match primary_group(person) {
                Ok(gid) => gid,
                Err(error) => return store_error(&error),
            };
            (
                Store::system(),
                Owner {
                    uid: person,
                    gid,
                    file_uid: Some(0),
                },
            )
        }
        Writer::Person { uid, root } => {
            let gid = match primary_group(uid) {
                Ok(gid) => gid,
                Err(error) => return store_error(&error),
            };
            (
                Store::at(root),
                Owner {
                    uid,
                    gid,
                    file_uid: None,
                },
            )
        }
    };

    let result = match request.command.as_str() {
        "set-policy" => match read_stdin() {
            Ok(text) => store.write_policy(&owner, &text),
            Err(problem) => Err(StoreError::Invalid(problem)),
        },
        "set-approvals" => match read_stdin() {
            Ok(text) => Approvals::from_json(&text).and_then(|approvals| store.write_approvals(&owner, &approvals)),
            Err(problem) => Err(StoreError::Invalid(problem)),
        },
        _ => {
            let schedule = request.schedule.as_deref().unwrap_or_default();
            store
                .read_approvals(owner.uid)
                .and_then(|approvals| store.write_approvals(&owner, &approvals.without(schedule)))
        }
    };
    match result {
        Ok(()) => {
            let _ = writeln!(io::stdout(), "changed: {}", store.person_dir(owner.uid).display());
            ExitCode::from(EXIT_OK)
        }
        Err(error) => store_error(&error),
    }
}

fn main() -> ExitCode {
    let arguments: Vec<String> = env::args().skip(1).collect();
    if arguments.iter().any(|argument| argument == "--help" || argument == "-h") {
        print!("{USAGE}");
        return ExitCode::from(EXIT_OK);
    }
    if arguments.iter().any(|argument| argument == "--version" || argument == "-V") {
        println!("tur-pax-policy {}", env!("CARGO_PKG_VERSION"));
        return ExitCode::from(EXIT_OK);
    }
    let request = match parse(&arguments) {
        Ok(request) => request,
        Err(problem) => {
            eprintln!("tur-pax-policy: {problem}\n\n{USAGE}");
            return ExitCode::from(EXIT_USAGE);
        }
    };
    match request.command.as_str() {
        "show" | "approvals-for" => show(&request),
        _ => change(&request, &arguments),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|item| (*item).to_owned()).collect()
    }

    #[test]
    fn the_command_line_is_read_strictly() {
        for good in [
            &["show"][..],
            &["show", "--json"],
            &["set-policy"],
            &["set-approvals", "--root", "/tmp/x"],
            &["clear-approvals", "--schedule", "s-00000001"],
            &["approvals-for", "--schedule", "s-1", "--task", "a task"],
        ] {
            assert!(parse(&args(good)).is_ok(), "{good:?}");
        }
        for bad in [
            &[][..],
            &["approve"],
            &["show", "--verbose"],
            &["set-policy", "--json"],
            &["clear-approvals"],
            &["clear-approvals", "--schedule", "../etc"],
            &["set-policy", "--schedule", "s-1"],
            &["approvals-for", "--schedule", "s-1"],
            &["show", "--task", "x"],
            &["show", "--root"],
        ] {
            assert!(parse(&args(bad)).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn root_writes_only_for_the_person_pkexec_names_and_never_elsewhere() {
        assert_eq!(
            writer(Some(0), None, Some("1000")),
            Ok(Writer::Root { person: 1000 })
        );
        // Run as root but not through pkexec: refused.
        assert!(writer(Some(0), None, None).is_err());
        assert!(writer(Some(0), None, Some("ada")).is_err());
        assert!(writer(Some(0), None, Some("0")).is_err());
        // A store elsewhere is never written as root.
        assert!(writer(Some(0), Some("/tmp/x"), Some("1000")).is_err());
        assert!(writer(None, None, None).is_err());
        // The person: their own store for tests, else through pkexec.
        assert_eq!(
            writer(Some(1000), Some("/tmp/x"), None),
            Ok(Writer::Person {
                uid: 1000,
                root: "/tmp/x".to_owned()
            })
        );
        assert_eq!(writer(Some(1000), None, Some("0")), Ok(Writer::Escalate));
        assert_eq!(writer(Some(1000), None, None), Ok(Writer::Escalate));
    }
}
