// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! `tur-diag report submit`, `show` and `fixed`, and what `prepare` and
//! `send` share with them.
//!
//! `submit` is the person's one command for a report
//! (problem-reports.md, section 2.3): it prepares the report, shows all of
//! it, and sends it through `send` only after the person types `yes` on a
//! terminal. It has no `--yes`. It is the command P@x hands over
//! ([`tur_diag::report::Handoff`]), the one a notice of the healing service
//! names, and the one Settings will run. Neither `submit` nor `send` runs in
//! P@x's sandbox.

use std::fs::{self, File};
use std::io::{self, BufRead, IsTerminal, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use tur_diag::bundle::ExtraDocument;
use tur_diag::heal::engine::DEFAULT_FIXES_FILE;
use tur_diag::heal::{Catalogue, Fixes, RealMachine, Running};
use tur_diag::redact::Format;
use tur_diag::report::{
    self, InspectedReport, PAX_ACTIVITY_PATH, PREPARED_DIR, ReportRequest, SentOrigin,
    SentRecord, activity::PAX_ACTIVITY_MAX_READ_BYTES,
};
use tur_diag::{DiagError, ErrorKind, SystemHost, gather_known_identifiers, write_bundle};

use crate::{Places, ReportFields, heal_cli, now, print_json, request_from_fields, send};

/// How `prepare` collects, saves and shows a report.
pub(crate) struct PrepareOptions {
    /// Leave out data from earlier boots.
    pub(crate) since_boot: bool,
    /// Save nothing.
    pub(crate) preview: bool,
    /// Save here instead of the reports directory.
    pub(crate) output: Option<PathBuf>,
    /// Print JSON.
    pub(crate) json: bool,
    /// Further documents for the bundle.
    pub(crate) more: Vec<ExtraDocument>,
}

/// Prepares a report, saves it unless `preview`, and shows what would be
/// sent. Nothing is sent. Returns where it was saved.
pub(crate) fn prepare_show_save(
    host: &SystemHost,
    request: &ReportRequest,
    options: &PrepareOptions,
    places: &Places,
) -> Result<Option<PathBuf>, DiagError> {
    // A configuration that cannot be read does not stop a report from
    // being prepared; it is reported when sending.
    let receiver = report::load_config(places.config.as_deref())
        .ok()
        .and_then(|config| config.receiver_url);
    let known = gather_known_identifiers(host, &crate::session_users());
    let prepared = report::prepare_with(
        host,
        request,
        options.since_boot,
        now(),
        known,
        &options.more,
    )?;
    let saved = if options.preview {
        None
    } else {
        let path = match &options.output {
            Some(path) => path.clone(),
            None => {
                let directory = crate::state_dir(places)?.join(PREPARED_DIR);
                report::ensure_private_dir(&directory)?;
                directory.join(report::prepared_file_name(&prepared))
            }
        };
        write_bundle(&path, &prepared.archive)?;
        Some(path)
    };
    let shown = report::preview(&prepared, receiver.as_deref(), saved.as_deref());
    if options.json {
        print_json(&shown)?;
    } else {
        print!("{}", report::render_preview(&shown));
    }
    Ok(saved)
}

/// P@x's audit trail: `$XDG_STATE_HOME/tur/pax/audit.jsonl`, or under
/// `~/.local/state`.
fn pax_audit_trail() -> Option<PathBuf> {
    let absolute = |name: &str| {
        std::env::var_os(name)
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
    };
    absolute("XDG_STATE_HOME")
        .or_else(|| absolute("HOME").map(|home| home.join(".local/state")))
        .map(|base| base.join("tur/pax/audit.jsonl"))
}

/// The last `max` bytes of `path`, from the first whole line in them.
fn read_tail(path: &Path, max: u64) -> io::Result<String> {
    let mut file = File::open(path)?;
    let length = file.metadata()?.len();
    let start = length.saturating_sub(max);
    file.seek(SeekFrom::Start(start))?;
    let mut data = Vec::new();
    file.take(max).read_to_end(&mut data)?;
    let mut text = String::from_utf8_lossy(&data).into_owned();
    if start > 0 {
        // The first line was cut; it is left out.
        text = text
            .split_once('\n')
            .map(|(_, rest)| rest.to_owned())
            .unwrap_or_default();
    }
    Ok(text)
}

/// The documents the fields ask for beyond the collected ones.
pub(crate) fn extra_documents(fields: &ReportFields) -> Result<Vec<ExtraDocument>, DiagError> {
    if !fields.include_pax_activity {
        return Ok(Vec::new());
    }
    let Some(path) = pax_audit_trail() else {
        eprintln!("P@x's activity is not added: neither XDG_STATE_HOME nor HOME is set.");
        return Ok(Vec::new());
    };
    match read_tail(&path, PAX_ACTIVITY_MAX_READ_BYTES) {
        Ok(text) => Ok(vec![ExtraDocument {
            path: PAX_ACTIVITY_PATH.to_owned(),
            format: Format::JsonLines,
            text: report::pax_activity(&text),
        }]),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            eprintln!("P@x's activity is not added: P@x has no audit trail in this account.");
            Ok(Vec::new())
        }
        Err(error) => Err(DiagError::new(
            ErrorKind::ReportInvalid,
            format!("P@x's audit trail {}: {error}", path.display()),
        )),
    }
}

/// Refuses to go on inside P@x's read-only sandbox: a report is sent only
/// by the person, and P@x's commands run there.
pub(crate) fn refuse_in_pax_sandbox(host: &SystemHost) -> Result<(), DiagError> {
    if report::in_pax_sandbox(host) {
        return Err(DiagError::new(
            ErrorKind::ReportInPaxSandbox,
            format!(
                "the host name is {}, the name of P@x's read-only sandbox",
                report::PAX_SANDBOX_HOSTNAME
            ),
        ));
    }
    Ok(())
}

/// Keeps where a sent report came from (`origins/<report-id>.json`).
pub(crate) fn keep_origin(
    directory: &Path,
    record: &SentRecord,
    inspected: &InspectedReport,
) -> Result<PathBuf, DiagError> {
    let version = report::affected_version(&inspected.report, &inspected.manifest);
    let origin = SentOrigin {
        schema_version: 1,
        report_id: record.report_id.clone(),
        error_code: record.error_code.clone(),
        signature: report::signature(record.error_code.as_deref(), &record.component, &version),
        version: inspected.manifest.tur.version.clone(),
        build_id: inspected.manifest.tur.build_id.clone(),
    };
    report::write_origin(directory, &origin)
}

/// Asks the person one question on the terminal.
fn ask(question: &str) -> Result<String, DiagError> {
    eprint!("{question} ");
    let _ = io::stderr().flush();
    let mut answer = String::new();
    io::stdin()
        .lock()
        .read_line(&mut answer)
        .map_err(|_| DiagError::new(ErrorKind::ReportInvalid, "the answer could not be read"))?;
    Ok(answer.trim().to_owned())
}

/// `tur-diag report submit`: prepare, show everything, ask, send.
pub(crate) fn submit(
    host: &SystemHost,
    fields: &ReportFields,
    from_heal: Option<&str>,
    places: &Places,
    heal: &heal_cli::HealPlaces,
) -> Result<(), DiagError> {
    refuse_in_pax_sandbox(host)?;
    // Nothing is collected unless it can be sent, and asked about.
    let config = report::load_config(places.config.as_deref())?;
    let source = config.source.as_ref().map_or_else(
        || "no configuration file".to_owned(),
        |path| path.display().to_string(),
    );
    let Some(receiver) = config.receiver_url else {
        return Err(DiagError::new(
            ErrorKind::ReportNoReceiver,
            format!("RECEIVER_URL is empty ({source})"),
        ));
    };
    if !io::stdin().is_terminal() {
        return Err(DiagError::new(
            ErrorKind::ReportNotConfirmed,
            "submit asks on a terminal and there is none; prepare the report with \
             'tur-diag report prepare', read it, then send it with 'tur-diag report send'",
        ));
    }
    let base = match from_heal {
        Some(code) => Some(heal_cli::report_request(code, heal)?),
        None => None,
    };
    let mut fields = fields.clone();
    if base.is_none() && !fields.from_boot_health {
        if fields.title.is_none() {
            let title = ask("What went wrong, in one line?")?;
            fields.title = Some(title);
        }
        if fields.component.is_none() {
            let question = format!(
                "Which part of TUR? One of: {}. [other]",
                report::COMPONENTS.join(", ")
            );
            let component = ask(&question)?;
            fields.component = Some(if component.is_empty() {
                "other".to_owned()
            } else {
                component
            });
        }
    }
    let request = request_from_fields(host, &fields, base)?;
    let options = PrepareOptions {
        since_boot: fields.since_boot,
        preview: false,
        output: None,
        json: false,
        more: extra_documents(&fields)?,
    };
    let saved = prepare_show_save(host, &request, &options, places)?;
    let Some(path) = saved else {
        return Err(DiagError::new(
            ErrorKind::Internal,
            "the prepared report was not saved",
        ));
    };
    println!();
    send(host, &receiver, false, false, &path, places)
}

/// `text` with every control character other than a line break or a tab
/// shown as an escape, so that a log line cannot drive the terminal.
fn printable(data: &[u8]) -> String {
    String::from_utf8_lossy(data)
        .chars()
        .map(|c| {
            if c.is_control() && c != '\n' && c != '\t' {
                c.escape_unicode().to_string()
            } else {
                c.to_string()
            }
        })
        .collect()
}

/// `tur-diag report show`.
pub(crate) fn show(
    report_file: &Path,
    file: Option<&str>,
    json: bool,
    places: &Places,
) -> Result<(), DiagError> {
    let archive = fs::read(report_file).map_err(|error| {
        DiagError::new(
            ErrorKind::BundleRead,
            format!("{}: {error}", report_file.display()),
        )
    })?;
    if let Some(path) = file {
        let inspected = report::inspect_archive(&archive)?;
        let Some(data) = report::member(&inspected, path) else {
            return Err(DiagError::new(
                ErrorKind::BundleRead,
                format!(
                    "the report has no file {path}; 'tur-diag report show {}' lists its files",
                    report_file.display()
                ),
            ));
        };
        print!("{}", printable(data));
        return Ok(());
    }
    let receiver = report::load_config(places.config.as_deref())
        .ok()
        .and_then(|config| config.receiver_url);
    let shown = report::preview_of_archive(&archive, receiver.as_deref(), report_file)?;
    if json {
        print_json(&shown)
    } else {
        print!("{}", report::render_preview(&shown));
        println!("Read any file of it with:  tur-diag report show --file PATH <report>");
        Ok(())
    }
}

/// `tur-diag report fixed`.
pub(crate) fn fixed(json: bool, fixes: Option<&Path>, places: &Places) -> Result<(), DiagError> {
    let directory = places
        .state_dir
        .clone()
        .or_else(report::default_state_dir)
        .filter(|directory| directory.is_dir());
    let records = match &directory {
        Some(directory) => report::read_records(directory)?.0,
        None => Vec::new(),
    };
    let origins: Vec<Option<SentOrigin>> = records
        .iter()
        .map(|record| {
            directory
                .as_deref()
                .and_then(|directory| report::read_origin(directory, &record.report_id))
        })
        .collect();
    let fixes = Fixes::load(fixes.unwrap_or(Path::new(DEFAULT_FIXES_FILE)));
    let running = Running::read(&RealMachine::new());
    let catalogue_codes: Vec<String> = Catalogue::built_in()
        .map(|catalogue| {
            catalogue
                .faults
                .iter()
                .map(|fault| fault.code.clone())
                .collect()
        })
        .unwrap_or_default();
    let list = report::fixed_reports(&records, &origins, &fixes, &running, &catalogue_codes);
    if json {
        print_json(&list)
    } else {
        print!("{}", report::render_fixed(&list));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_characters_are_shown_not_obeyed() {
        assert_eq!(printable(b"a\tb\nc"), "a\tb\nc");
        assert_eq!(printable(b"\x1b[2J"), "\\u{1b}[2J");
    }

    #[test]
    fn the_tail_of_a_long_trail_starts_at_a_whole_line() {
        let path = std::env::temp_dir().join(format!("tur-diag-tail-{}", std::process::id()));
        fs::write(&path, "first line\nsecond\nthird\n").expect("written");
        assert_eq!(read_tail(&path, 1000).expect("read"), "first line\nsecond\nthird\n");
        assert_eq!(read_tail(&path, 10).expect("read"), "third\n");
        let _ = fs::remove_file(&path);
    }
}
