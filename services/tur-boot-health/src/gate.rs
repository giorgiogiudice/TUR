// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The health gate (design section 6): a fixed list of required checks, each
//! with a time limit, and a few that only mark the boot degraded.
//!
//! The gate observes the boot; it holds nothing back. It never looks at the
//! network, never waits for a person, and never asks whether *any* unit
//! failed: required units are a named list.

use crate::codes;
use crate::config::{self, ExtraCheck, GateConfig};
use crate::host::Host;
use crate::json::Value;
use crate::model::KernelArgs;
use crate::state;
use crate::trial::{self, MSG_STAGE, MSG_VERDICT, RUN_OSTREE_TARGET, RUN_VERDICT};

/// The core services of check R4.
const CORE_UNITS: [&str; 4] = [
    "dbus.service",
    "systemd-logind.service",
    "systemd-journald.service",
    "systemd-udevd.service",
];

/// How a check ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CheckState {
    Passed,
    Failed,
    Timeout,
    Skipped,
}

impl CheckState {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            CheckState::Passed => "succeeded",
            CheckState::Failed => "failed",
            CheckState::Timeout => "timeout",
            CheckState::Skipped => "skipped",
        }
    }

    fn bad(self) -> bool {
        matches!(self, CheckState::Failed | CheckState::Timeout)
    }
}

/// One check and how it ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CheckResult {
    /// `R1` to `R8`, `W…`, or `X:name` for a check from `checks.d`.
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) required: bool,
    pub(crate) state: CheckState,
    /// The error code, when the check failed or ran out of time.
    pub(crate) code: Option<String>,
    pub(crate) detail: String,
    pub(crate) duration_ms: u64,
}

/// The gate's decision about this boot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Verdict {
    /// `passed`, `degraded` or `failed`.
    pub(crate) result: &'static str,
    pub(crate) checks: Vec<CheckResult>,
}

impl Verdict {
    /// The first required check that failed or ran out of time.
    pub(crate) fn first_failure(&self) -> Option<&CheckResult> {
        self.checks
            .iter()
            .find(|check| check.required && check.state.bad())
    }

    pub(crate) fn passed(&self) -> bool {
        self.result != "failed"
    }

    pub(crate) fn to_json(&self, host: &dyn Host, classification: &str) -> Value {
        let failure = self.first_failure();
        Value::object(vec![
            ("schema_version", Value::Int(state::SCHEMA_VERSION)),
            ("boot_id", Value::from(host.boot_id())),
            ("classification", Value::from(classification)),
            ("result", Value::from(self.result)),
            (
                "failed_check",
                Value::opt_str(failure.map(|check| check.id.as_str())),
            ),
            (
                "error_code",
                Value::opt_str(failure.and_then(|check| check.code.as_deref())),
            ),
            (
                "failed_detail",
                Value::opt_str(failure.map(|check| check.detail.as_str())),
            ),
            ("finished_uptime_ms", Value::Int(host.uptime_ms() as i64)),
            (
                "checks",
                Value::Array(
                    self.checks
                        .iter()
                        .map(|check| {
                            Value::object(vec![
                                ("id", Value::from(check.id.as_str())),
                                ("name", Value::from(check.name.as_str())),
                                ("required", Value::Bool(check.required)),
                                ("state", Value::from(check.state.as_str())),
                                ("error_code", Value::opt_str(check.code.as_deref())),
                                ("detail", Value::from(check.detail.as_str())),
                                ("duration_ms", Value::Int(check.duration_ms as i64)),
                            ])
                        })
                        .collect(),
                ),
            ),
        ])
    }
}

/// What a probe saw.
enum Probe {
    /// The condition holds.
    Pass(String),
    /// The condition cannot come true in this boot.
    Fail(String),
    /// Not yet.
    Wait(String),
}

/// Polls a condition once a second until it holds, cannot hold, or the limit
/// has passed.
fn wait_for(
    host: &mut dyn Host,
    limit_s: u64,
    probe: &mut dyn FnMut(&mut dyn Host) -> Probe,
) -> (CheckState, String, u64) {
    let start = host.uptime_ms();
    loop {
        let seen = probe(host);
        let elapsed = host.uptime_ms().saturating_sub(start);
        match seen {
            Probe::Pass(detail) => return (CheckState::Passed, detail, elapsed),
            Probe::Fail(detail) => return (CheckState::Failed, detail, elapsed),
            Probe::Wait(detail) => {
                if elapsed >= limit_s * 1000 {
                    return (
                        CheckState::Timeout,
                        format!("not within {limit_s} s: {detail}"),
                        elapsed,
                    );
                }
            }
        }
        host.sleep_ms(1000);
    }
}

fn unit_state(host: &mut dyn Host, unit: &str) -> (String, String) {
    match host.unit_show(unit, &["Id", "ActiveState"]) {
        Ok(found) => (
            found.get("Id").cloned().unwrap_or_default(),
            found.get("ActiveState").cloned().unwrap_or_default(),
        ),
        Err(error) => (String::new(), format!("unknown ({error})")),
    }
}

/// All of `units` active; fails at once if one is in the failed state.
fn units_active(host: &mut dyn Host, units: &[String]) -> Probe {
    let mut waiting = Vec::new();
    for unit in units {
        let (_, state) = unit_state(host, unit);
        match state.as_str() {
            "active" => {}
            "failed" => return Probe::Fail(format!("{unit} is failed")),
            other => waiting.push(format!("{unit} is {other}")),
        }
    }
    if waiting.is_empty() {
        Probe::Pass(format!("active: {}", units.join(", ")))
    } else {
        Probe::Wait(waiting.join("; "))
    }
}

fn probe_root(host: &mut dyn Host) -> Probe {
    let Some((fstype, source)) = host.root_mount() else {
        return Probe::Wait("no mount found at /".to_owned());
    };
    if fstype != "overlay" || source != "composefs" {
        return Probe::Fail(format!(
            "/ is {fstype} from {source}, not the composefs mount of a deployment"
        ));
    }
    let booted = match host.bootc_status() {
        Ok(status) => match status.booted {
            Some(booted) => booted,
            None => return Probe::Wait("bootc reports no booted deployment".to_owned()),
        },
        Err(error) => return Probe::Wait(error),
    };
    let args = KernelArgs::parse(&host.cmdline());
    let Some(ostree) = args.ostree else {
        return Probe::Fail("the kernel command line has no ostree= argument".to_owned());
    };
    // What `classify` resolved at the start of the boot, if it ran: a
    // fallback made permanent in this boot has since re-pointed the links.
    let recorded = host
        .read_run(RUN_OSTREE_TARGET)
        .map(|text| text.trim().to_owned())
        .filter(|text| !text.is_empty());
    let target = match recorded {
        Some(target) => target,
        None => match host.ostree_target(&ostree) {
            Some(target) => target,
            None => {
                return Probe::Fail(format!("ostree={ostree} does not resolve to a deployment"));
            }
        },
    };
    if target != booted.name() {
        return Probe::Fail(format!(
            "the kernel was told to start {target} but bootc reports {} as booted",
            booted.name()
        ));
    }
    Probe::Pass(format!("/ is the composefs mount of deployment {target}"))
}

fn probe_paths(host: &mut dyn Host) -> Probe {
    for path in [
        "/usr/lib/os-release",
        "/usr/lib/tur/release",
        "/boot/grub2/grubenv",
    ] {
        if !host.path_readable(path) {
            return Probe::Fail(format!("{path} is not readable"));
        }
    }
    if !host.dir_writable("/var/lib/tur") {
        return Probe::Fail("/var/lib/tur is not writable".to_owned());
    }
    Probe::Pass(
        "os-release, the TUR release file and the GRUB environment block are readable; \
         /var/lib/tur is writable"
            .to_owned(),
    )
}

/// What check R7 found, for R8 to compare with.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Seat {
    session: String,
    compositor: u32,
    restarts: String,
}

fn display_manager_restarts(host: &mut dyn Host) -> (String, String) {
    match host.unit_show("display-manager.service", &["ActiveState", "NRestarts"]) {
        Ok(found) => (
            found.get("ActiveState").cloned().unwrap_or_default(),
            found.get("NRestarts").cloned().unwrap_or_default(),
        ),
        Err(error) => (format!("unknown ({error})"), String::new()),
    }
}

fn probe_seat(host: &mut dyn Host) -> Result<(Seat, String), String> {
    let session = match host.seat_session() {
        Ok(Some(session)) => session,
        Ok(None) => return Err("seat0 has no active session".to_owned()),
        Err(error) => return Err(error),
    };
    let describe = format!(
        "session {} type {} class {} state {}",
        session.id, session.kind, session.class, session.state
    );
    if session.kind != "wayland" && session.kind != "x11" {
        return Err(format!("{describe}: not a graphical session"));
    }
    if session.class != "greeter" && session.class != "user" {
        return Err(format!("{describe}: neither a greeter nor a user session"));
    }
    if session.state != "active" {
        return Err(format!("{describe}: not active"));
    }
    let Some(compositor) = host.compositor_pid(session.uid) else {
        return Err(format!(
            "{describe}: no compositor process owned by uid {}",
            session.uid
        ));
    };
    let (_, restarts) = display_manager_restarts(host);
    Ok((
        Seat {
            session: session.id.clone(),
            compositor,
            restarts,
        },
        format!("{describe}, compositor process {compositor}"),
    ))
}

fn stage(
    host: &mut dyn Host,
    classification: &str,
    stage: &str,
    what: &str,
    result: &CheckResult,
    sync: bool,
) {
    let tries = KernelArgs::parse(&host.cmdline())
        .tries_left
        .map(|tries| tries.to_string())
        .unwrap_or_default();
    let uptime_usec = host.uptime_ms() * 1000;
    let word = match result.state {
        CheckState::Passed => "completed",
        CheckState::Failed => "failed",
        CheckState::Timeout => "timed out",
        CheckState::Skipped => "skipped",
    };
    trial::log(
        host,
        MSG_STAGE,
        if result.state.bad() { 3 } else { 6 },
        format!("Boot stage {word}: {what}"),
        vec![
            ("TUR_BOOT_STAGE".to_owned(), stage.to_owned()),
            (
                "TUR_BOOT_STAGE_STATE".to_owned(),
                result.state.as_str().to_owned(),
            ),
            ("TUR_BOOT_MODE".to_owned(), classification.to_owned()),
            ("TUR_BOOT_TRIES_LEFT".to_owned(), tries),
            (
                "TUR_STAGE_MONOTONIC_USEC".to_owned(),
                uptime_usec.to_string(),
            ),
            ("TUR_DURATION_MS".to_owned(), result.duration_ms.to_string()),
            (
                "TUR_ERROR_CODE".to_owned(),
                result.code.clone().unwrap_or_default(),
            ),
            ("TUR_CHECK".to_owned(), result.id.clone()),
            ("TUR_CHECK_DETAIL".to_owned(), result.detail.clone()),
        ],
    );
    if sync {
        host.sync_journal();
    }
}

/// Builds the result of a required check from what `wait_for` returned.
fn required(
    id: &str,
    name: &str,
    code: &tur_core::ErrorCode,
    outcome: (CheckState, String, u64),
) -> CheckResult {
    let (state, detail, duration_ms) = outcome;
    let code = match state {
        CheckState::Failed => Some(code.to_string()),
        CheckState::Timeout => Some(codes::HEALTH_TIMEOUT.to_string()),
        CheckState::Passed | CheckState::Skipped => None,
    };
    CheckResult {
        id: id.to_owned(),
        name: name.to_owned(),
        required: true,
        state,
        code,
        detail,
        duration_ms,
    }
}

fn skipped(id: &str, name: &str, why: &str) -> CheckResult {
    CheckResult {
        id: id.to_owned(),
        name: name.to_owned(),
        required: true,
        state: CheckState::Skipped,
        code: None,
        detail: why.to_owned(),
        duration_ms: 0,
    }
}

fn wanted(id: &str, name: &str, ok: bool, detail: String) -> CheckResult {
    CheckResult {
        id: id.to_owned(),
        name: name.to_owned(),
        required: false,
        state: if ok {
            CheckState::Passed
        } else {
            CheckState::Failed
        },
        code: None,
        detail,
        duration_ms: 0,
    }
}

/// The units named in `required-units.d`, and the files that name none.
fn required_units(host: &mut dyn Host) -> (Vec<String>, Vec<String>) {
    let directory = format!("{}/required-units.d", config::CONFIG_DIR);
    let mut units = Vec::new();
    let mut broken = Vec::new();
    for name in host.list_dir(&directory) {
        if !name.ends_with(".conf") {
            continue;
        }
        let path = format!("{directory}/{name}");
        match host
            .read_file(&path)
            .and_then(|text| config::parse_required_unit(&text))
        {
            Some(unit) => units.push(unit),
            None => broken.push(path),
        }
    }
    (units, broken)
}

fn extra_checks(host: &mut dyn Host) -> Vec<Result<ExtraCheck, (String, String)>> {
    let directory = format!("{}/checks.d", config::CONFIG_DIR);
    let mut checks = Vec::new();
    for name in host.list_dir(&directory) {
        let Some(stem) = name.strip_suffix(".toml") else {
            continue;
        };
        let path = format!("{directory}/{name}");
        let parsed = match host.read_file(&path) {
            Some(text) => ExtraCheck::parse(stem, &text),
            None => Err("the file cannot be read".to_owned()),
        };
        checks.push(parsed.map_err(|error| (stem.to_owned(), format!("{path}: {error}"))));
    }
    checks
}

/// Runs the gate and returns its verdict. Stops at the first required check
/// that fails; the checks after it could only wait for nothing.
pub(crate) fn run_gate(host: &mut dyn Host, config: &GateConfig, classification: &str) -> Verdict {
    let limits = &config.limits;
    let sync = classification != "normal";
    let mut checks: Vec<CheckResult> = Vec::new();
    let mut failed = false;
    let mut default_target = String::new();

    // R1 and R2: the root filesystem and the paths everything else needs.
    let result = required(
        "R1",
        "root filesystem",
        &codes::HEALTH_ROOT,
        wait_for(host, limits.root, &mut probe_root),
    );
    failed |= result.state.bad();
    checks.push(result);
    if !failed {
        let result = required(
            "R2",
            "critical paths",
            &codes::HEALTH_PATHS,
            wait_for(host, limits.paths, &mut probe_paths),
        );
        failed |= result.state.bad();
        checks.push(result);
    }

    // R3: the default target.
    if !failed {
        let outcome = wait_for(host, limits.default_target, &mut |host: &mut dyn Host| {
            let (id, state) = unit_state(host, "default.target");
            default_target = id.clone();
            if state == "active" {
                Probe::Pass(format!("{id} is active"))
            } else {
                Probe::Wait(format!("{id} is {state}"))
            }
        });
        let result = required(
            "R3",
            "default target",
            &codes::HEALTH_DEFAULT_TARGET,
            outcome,
        );
        failed |= result.state.bad();
        checks.push(result);
    }

    // R4: core services. This is stage BOOT-080.
    if !failed {
        let units: Vec<String> = CORE_UNITS.iter().map(|unit| (*unit).to_owned()).collect();
        let outcome = wait_for(host, limits.core_units, &mut |host: &mut dyn Host| {
            units_active(host, &units)
        });
        let result = required("R4", "core units", &codes::HEALTH_CORE_UNITS, outcome);
        stage(
            host,
            classification,
            "BOOT-080",
            "core system services",
            &result,
            sync,
        );
        failed |= result.state.bad();
        checks.push(result);
    }

    // R5: TUR's own required units. Stage BOOT-130.
    if !failed {
        let (units, broken) = required_units(host);
        let result = if !broken.is_empty() {
            required(
                "R5",
                "TUR units",
                &codes::HEALTH_TUR_UNITS,
                (
                    CheckState::Failed,
                    format!("names no unit: {}", broken.join(", ")),
                    0,
                ),
            )
        } else if units.is_empty() {
            skipped("R5", "TUR units", "no unit is listed in required-units.d")
        } else {
            let outcome = wait_for(host, limits.tur_units, &mut |host: &mut dyn Host| {
                units_active(host, &units)
            });
            required("R5", "TUR units", &codes::HEALTH_TUR_UNITS, outcome)
        };
        stage(
            host,
            classification,
            "BOOT-130",
            "TUR session services",
            &result,
            sync,
        );
        failed |= result.state.bad();
        checks.push(result);
    }

    // R6 to R8: the display manager and the seat's session. Skipped when the
    // machine is configured headless.
    let headless = default_target != "graphical.target";
    if !failed && headless {
        let why = format!("the default target is {default_target}, not graphical.target");
        checks.push(skipped("R6", "display manager", &why));
        checks.push(skipped("R7", "seat session", &why));
        checks.push(skipped("R8", "stability", &why));
    }
    let mut seat: Option<Seat> = None;
    if !failed && !headless {
        let outcome = wait_for(host, limits.display_manager, &mut |host: &mut dyn Host| {
            let (state, _) = display_manager_restarts(host);
            if state == "active" {
                Probe::Pass("display-manager.service is active".to_owned())
            } else {
                Probe::Wait(format!("display-manager.service is {state}"))
            }
        });
        let result = required(
            "R6",
            "display manager",
            &codes::HEALTH_DISPLAY_MANAGER,
            outcome,
        );
        stage(
            host,
            classification,
            "BOOT-110",
            "display manager",
            &result,
            sync,
        );
        failed |= result.state.bad();
        checks.push(result);
    }
    if !failed && !headless {
        let outcome =
            wait_for(
                host,
                limits.seat_session,
                &mut |host: &mut dyn Host| match probe_seat(host) {
                    Ok((found, detail)) => {
                        seat = Some(found);
                        Probe::Pass(detail)
                    }
                    Err(detail) => Probe::Wait(detail),
                },
            );
        let result = required("R7", "seat session", &codes::HEALTH_SEAT_SESSION, outcome);
        stage(
            host,
            classification,
            "BOOT-120",
            "Wayland compositor",
            &result,
            sync,
        );
        failed |= result.state.bad();
        checks.push(result);
    }
    if !failed
        && !headless
        && let Some(before) = seat.clone()
    {
        let start = host.uptime_ms();
        host.sleep_ms(limits.hold * 1000);
        let (state, detail) = match probe_seat(host) {
            Ok((after, detail)) if after == before => {
                let (active, _) = display_manager_restarts(host);
                if active == "active" {
                    (
                        CheckState::Passed,
                        format!("unchanged after {} s: {detail}", limits.hold),
                    )
                } else {
                    (
                        CheckState::Failed,
                        format!("display-manager.service is {active}"),
                    )
                }
            }
            Ok((after, _)) => (
                CheckState::Failed,
                format!(
                    "the seat changed within {} s: session {} compositor {} restarts {} became \
                     session {} compositor {} restarts {}",
                    limits.hold,
                    before.session,
                    before.compositor,
                    before.restarts,
                    after.session,
                    after.compositor,
                    after.restarts
                ),
            ),
            Err(detail) => (
                CheckState::Failed,
                format!(
                    "the session did not stay up for {} s: {detail}",
                    limits.hold
                ),
            ),
        };
        let elapsed = host.uptime_ms().saturating_sub(start);
        let state = if state == CheckState::Passed && elapsed > limits.stability * 1000 {
            CheckState::Timeout
        } else {
            state
        };
        let result = required(
            "R8",
            "stability",
            &codes::HEALTH_STABILITY,
            (state, detail, elapsed),
        );
        stage(
            host,
            classification,
            "BOOT-140",
            "seat session healthy",
            &result,
            sync,
        );
        failed |= result.state.bad();
        checks.push(result);
    }

    // Additional checks from the image.
    if !failed {
        for check in extra_checks(host) {
            let result = match check {
                Err((name, error)) => CheckResult {
                    id: format!("X:{name}"),
                    name: format!("additional check {name}"),
                    required: true,
                    state: CheckState::Failed,
                    code: Some(codes::HEALTH_EXTRA_CHECK.to_string()),
                    detail: error,
                    duration_ms: 0,
                },
                Ok(check) => {
                    let start = host.uptime_ms();
                    let ran = host.run_check(&check.exec, &check.args, check.timeout);
                    let duration_ms = host.uptime_ms().saturating_sub(start);
                    let (state, detail) = match ran {
                        Ok(0) => (CheckState::Passed, format!("{} exited 0", check.exec)),
                        Ok(status) => (
                            CheckState::Failed,
                            format!("{} exited {status}", check.exec),
                        ),
                        Err(error) => (CheckState::Failed, error),
                    };
                    CheckResult {
                        id: format!("X:{}", check.name),
                        name: format!("additional check {}", check.name),
                        required: check.required,
                        state,
                        code: if state.bad() && check.required {
                            Some(codes::HEALTH_EXTRA_CHECK.to_string())
                        } else {
                            None
                        },
                        detail,
                        duration_ms,
                    }
                }
            };
            let stop = result.required && result.state.bad();
            checks.push(result);
            if stop {
                failed = true;
                break;
            }
        }
    }

    // Checks that never fail the gate (design section 6.3). Only some of the
    // twelve are implemented; `status` says which.
    let failed_units = host.failed_units();
    checks.push(wanted(
        "W1",
        "failed units",
        failed_units.is_empty(),
        if failed_units.is_empty() {
            "none".to_owned()
        } else {
            failed_units.join(", ")
        },
    ));
    let (_, finalisation) = unit_state(host, "ostree-boot-complete.service");
    checks.push(wanted(
        "W8",
        "finalisation of the previous update",
        finalisation != "failed",
        format!("ostree-boot-complete.service is {finalisation}"),
    ));
    let contract = KernelArgs::parse(&host.cmdline()).contract_ok();
    let layout = host.layout();
    let layout_ok = matches!(&layout, Ok(layout) if layout.counting_supported());
    checks.push(wanted(
        "W10",
        "boot attempts can be counted",
        contract && layout_ok,
        format!(
            "contract version on the kernel command line: {}; /boot/grub2 is on {}",
            if contract { "1" } else { "absent" },
            match &layout {
                Ok(layout) => layout.describe(),
                Err(error) => format!("an unknown filesystem ({error})"),
            }
        ),
    ));
    let watchdog = host.watchdog();
    checks.push(CheckResult {
        id: "W12".to_owned(),
        name: "hardware watchdog".to_owned(),
        required: false,
        state: if watchdog.is_some() {
            CheckState::Passed
        } else {
            CheckState::Skipped
        },
        code: None,
        detail: watchdog
            .unwrap_or_else(|| "this machine has no hardware watchdog device".to_owned()),
        duration_ms: 0,
    });

    let degraded = checks
        .iter()
        .any(|check| !check.required && check.state.bad());
    Verdict {
        result: if failed {
            "failed"
        } else if degraded {
            "degraded"
        } else {
            "passed"
        },
        checks,
    }
}

/// Runs the gate, writes the verdict where `mark-good` and `record-failure`
/// find it, and says whether the boot passed.
pub(crate) fn gate(host: &mut dyn Host, config: Result<GateConfig, String>) -> bool {
    let classification =
        trial::recorded_classification(host).unwrap_or_else(|| "unknown".to_owned());
    let verdict = match config {
        _ if classification == "rejected" => Verdict {
            result: "failed",
            checks: vec![CheckResult {
                id: "rejected".to_owned(),
                name: "rejected version".to_owned(),
                required: true,
                state: CheckState::Failed,
                code: Some(codes::TRIAL_FALLBACK.to_string()),
                detail: "this version failed its trial and is being replaced; it cannot be \
                         marked good"
                    .to_owned(),
                duration_ms: 0,
            }],
        },
        Ok(config) => run_gate(host, &config, &classification),
        Err(error) => Verdict {
            result: "failed",
            checks: vec![CheckResult {
                id: "config".to_owned(),
                name: "gate configuration".to_owned(),
                required: true,
                state: CheckState::Failed,
                code: Some(codes::HEALTH_EXTRA_CHECK.to_string()),
                detail: format!("{}/gate.toml: {error}", config::CONFIG_DIR),
                duration_ms: 0,
            }],
        },
    };
    let text = verdict.to_json(host, &classification).to_pretty();
    if let Err(error) = host.write_run(RUN_VERDICT, &text) {
        host.console(&format!("gate: the verdict could not be written: {error}"));
    }
    let failure = verdict.first_failure().cloned();
    let summary = match &failure {
        Some(check) => format!(
            "Boot health gate failed: {} ({}): {}",
            check.id,
            check.code.clone().unwrap_or_default(),
            check.detail
        ),
        None => format!("Boot health gate {}", verdict.result),
    };
    host.console(&format!("gate: {summary}"));
    trial::log(
        host,
        MSG_VERDICT,
        if verdict.passed() { 6 } else { 3 },
        summary,
        vec![
            ("TUR_VERDICT".to_owned(), verdict.result.to_owned()),
            ("TUR_BOOT_MODE".to_owned(), classification.clone()),
            (
                "TUR_FAILED_CHECK".to_owned(),
                failure
                    .as_ref()
                    .map(|check| check.id.clone())
                    .unwrap_or_default(),
            ),
            (
                "TUR_ERROR_CODE".to_owned(),
                failure
                    .as_ref()
                    .and_then(|check| check.code.clone())
                    .unwrap_or_default(),
            ),
        ],
    );
    if classification != "normal" || !verdict.passed() {
        host.sync_journal();
    }
    verdict.passed()
}
