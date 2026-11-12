// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! Serving a model: the engine as a child process, started on request,
//! stopped when it is idle or asked to.
//!
//! `tur-ai-local serve` is the supervisor. It checks that nothing is running
//! and that the port is free, writes a random access key to a file only the
//! person can read, starts the engine with it, waits until the engine
//! answers its health check, and writes the state file. Then it watches:
//!
//! * the engine's own counters (`/metrics`: tokens processed, requests in
//!   progress); when none changes for the idle time, the engine is stopped
//!   (`TUR-AI-LOCAL-020`);
//! * a stop request, written by `tur-ai-local stop` (`TUR-AI-LOCAL-021`);
//! * the engine itself; if it exits, that is a failure (`TUR-AI-LOCAL-015`).
//!
//! Everything lives in the session's runtime folder,
//! `$XDG_RUNTIME_DIR/tur-ai-local/`: `state.json`, `api-key`, `engine.log`
//! and the stop request. The folder is the person's own (mode 0700) and is
//! emptied at log-out.
//!
//! If the supervisor itself is killed, the engine is told to stop by the
//! kernel when `setpriv` is installed (`--pdeathsig`); in the planned
//! systemd user unit the unit's control group ends both. A state file left
//! behind by a killed supervisor is recognised as stale.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::codes::AiLocalCode;
use crate::engine::{ENGINE_ENVIRONMENT, Launch, Listen};
use crate::store::{create_private_dir, now_seconds};

/// The state file.
pub const STATE_FILE: &str = "state.json";
/// The stop request.
pub const STOP_FILE: &str = "stop-request";
/// The access key.
pub const KEY_FILE: &str = "api-key";
/// The engine's output.
pub const LOG_FILE: &str = "engine.log";
/// The program that makes the kernel stop the engine if the supervisor
/// dies. Used when it is installed.
pub const SETPRIV: &str = "/usr/bin/setpriv";

/// How often the engine's counters are read.
pub const POLL_INTERVAL: Duration = Duration::from_secs(2);

/// The idle time unless another is asked for.
pub const DEFAULT_IDLE_STOP: Duration = Duration::from_secs(600);

/// How long the engine may take to load a model.
pub const DEFAULT_START_TIMEOUT: Duration = Duration::from_secs(300);

/// The runtime folder of the service: `<runtime>/tur-ai-local`, with
/// `<runtime>` given or `$XDG_RUNTIME_DIR`.
pub fn runtime_dir(explicit: Option<&Path>) -> Option<PathBuf> {
    let base = match explicit {
        Some(path) => path.to_path_buf(),
        None => std::env::var_os("XDG_RUNTIME_DIR")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())?,
    };
    Some(base.join("tur-ai-local"))
}

/// What `status` reads while a model is served.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct State {
    /// The state format version.
    pub schema_version: u32,
    /// The supervisor's process.
    pub supervisor_pid: u32,
    /// The engine's process.
    pub engine_pid: u32,
    /// The catalogue id of the model.
    pub model: String,
    /// The engine program.
    pub engine: String,
    /// The address, for TCP.
    pub address: Option<String>,
    /// The port, for TCP.
    pub port: Option<u16>,
    /// The socket, for a Unix socket.
    pub socket: Option<String>,
    /// The base URL of the OpenAI-compatible API, for TCP.
    pub base_url: Option<String>,
    /// Listening on an address other devices can reach was allowed.
    pub network_allowed: bool,
    /// The file holding the access key.
    pub api_key_file: String,
    /// The engine's output.
    pub log_file: String,
    /// The context size.
    pub context: u32,
    /// When it became ready, seconds since 1970.
    pub started_at: u64,
    /// The idle time after which it stops, in seconds.
    pub idle_stop_seconds: u64,
    /// When activity was last seen, seconds since 1970.
    pub last_activity: u64,
}

/// Read the state file, if there is one that can be read.
pub fn read_state(runtime: &Path) -> Option<State> {
    let text = fs::read_to_string(runtime.join(STATE_FILE)).ok()?;
    serde_json::from_str(&text).ok()
}

fn write_state(runtime: &Path, state: &State) -> io::Result<()> {
    let text = serde_json::to_string_pretty(state)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let temporary = runtime.join(format!("{STATE_FILE}.part"));
    fs::write(&temporary, text)?;
    fs::rename(temporary, runtime.join(STATE_FILE))
}

/// The process exists and has not ended.
pub fn process_alive(pid: u32) -> bool {
    let Ok(stat) = fs::read_to_string(format!("/proc/{pid}/stat")) else {
        return false;
    };
    // The state is the first field after the command name, which is in
    // parentheses and may itself hold spaces and parentheses.
    let state = stat
        .rsplit_once(')')
        .and_then(|(_, rest)| rest.split_whitespace().next());
    !matches!(state, Some("Z") | Some("X") | None)
}

/// The resident and the peak resident memory of a process, in bytes.
pub fn process_memory(pid: u32) -> (Option<u64>, Option<u64>) {
    let Ok(status) = fs::read_to_string(format!("/proc/{pid}/status")) else {
        return (None, None);
    };
    let field = |name: &str| {
        status.lines().find_map(|line| {
            let rest = line.strip_prefix(name)?.strip_prefix(':')?;
            let kilobytes: u64 = rest.split_whitespace().next()?.parse().ok()?;
            kilobytes.checked_mul(1024)
        })
    };
    (field("VmRSS"), field("VmHWM"))
}

/// A plain HTTP/1.0 GET to the engine on its loopback address or socket.
/// Returns the status and the body.
pub fn local_get(
    listen: &Listen,
    path: &str,
    key: Option<&str>,
    timeout: Duration,
) -> io::Result<(u16, String)> {
    let host = match listen {
        Listen::Tcp { address, port } => SocketAddr::new(*address, *port).to_string(),
        Listen::Socket(_) => "localhost".to_owned(),
    };
    let mut request = format!("GET {path} HTTP/1.0\r\nHost: {host}\r\nAccept: */*\r\n");
    if let Some(key) = key {
        request.push_str(&format!("Authorization: Bearer {key}\r\n"));
    }
    request.push_str("\r\n");
    let mut response = Vec::new();
    match listen {
        Listen::Tcp { address, port } => {
            let mut stream =
                TcpStream::connect_timeout(&SocketAddr::new(*address, *port), timeout)?;
            stream.set_read_timeout(Some(timeout))?;
            stream.set_write_timeout(Some(timeout))?;
            stream.write_all(request.as_bytes())?;
            stream.take(4 << 20).read_to_end(&mut response)?;
        }
        Listen::Socket(socket) => {
            let mut stream = UnixStream::connect(socket)?;
            stream.set_read_timeout(Some(timeout))?;
            stream.set_write_timeout(Some(timeout))?;
            stream.write_all(request.as_bytes())?;
            stream.take(4 << 20).read_to_end(&mut response)?;
        }
    }
    let text = String::from_utf8_lossy(&response).into_owned();
    let (head, body) = text.split_once("\r\n\r\n").unwrap_or((text.as_str(), ""));
    let status = head
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|code| code.parse::<u16>().ok())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "no HTTP status line"))?;
    Ok((status, body.to_owned()))
}

/// The engine's activity counters, from its Prometheus metrics.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Activity {
    /// Prompt tokens processed, generated tokens and decode calls, added.
    pub work: f64,
    /// Requests being processed or waiting.
    pub in_progress: f64,
}

/// Read the counters from the text of `/metrics`.
pub fn parse_metrics(text: &str) -> Option<Activity> {
    let mut activity = Activity::default();
    let mut seen = false;
    for line in text.lines() {
        if line.starts_with('#') {
            continue;
        }
        let mut words = line.split_whitespace();
        let (Some(name), Some(value)) = (words.next(), words.next()) else {
            continue;
        };
        let Ok(value) = value.parse::<f64>() else {
            continue;
        };
        match name {
            "llamacpp:prompt_tokens_total"
            | "llamacpp:tokens_predicted_total"
            | "llamacpp:n_decode_total" => {
                activity.work += value;
                seen = true;
            }
            "llamacpp:requests_processing" | "llamacpp:requests_deferred" => {
                activity.in_progress += value;
                seen = true;
            }
            _ => {}
        }
    }
    seen.then_some(activity)
}

/// Why a served model stopped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stopped {
    /// `TUR-AI-LOCAL-020` (idle) or `TUR-AI-LOCAL-021` (asked).
    pub code: AiLocalCode,
    /// The state it was served with.
    pub state: State,
    /// The engine's peak resident memory, read just before it was stopped.
    pub peak_memory_bytes: Option<u64>,
}

/// Why a model could not be served.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServeError {
    /// The code.
    pub code: AiLocalCode,
    /// What exactly went wrong.
    pub detail: String,
}

impl ServeError {
    fn new(code: AiLocalCode, detail: impl Into<String>) -> ServeError {
        ServeError {
            code,
            detail: detail.into(),
        }
    }
}

/// How to serve.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServeOptions {
    /// The engine and its command line.
    pub launch: Launch,
    /// Listening on an address other devices can reach was allowed.
    pub network_allowed: bool,
    /// Stop after this long without activity.
    pub idle_stop: Duration,
    /// Give up if the engine is not ready after this long.
    pub start_timeout: Duration,
    /// The service's runtime folder.
    pub runtime: PathBuf,
}

/// The last lines of the engine's output.
pub fn log_tail(path: &Path, lines: usize) -> String {
    let text = fs::read_to_string(path).unwrap_or_default();
    let all: Vec<&str> = text.lines().collect();
    all[all.len().saturating_sub(lines)..].join("\n")
}

fn random_key() -> io::Result<String> {
    let mut bytes = [0u8; 32];
    File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    Ok(crate::download::hex(&bytes))
}

fn write_private(path: &Path, text: &str) -> io::Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(text.as_bytes())?;
    file.write_all(b"\n")
}

/// The supervisor is running for this state.
pub fn supervisor_running(state: &State) -> bool {
    process_alive(state.supervisor_pid)
}

fn cleanup(runtime: &Path, listen: &Listen) {
    for name in [STATE_FILE, KEY_FILE, STOP_FILE] {
        let _ = fs::remove_file(runtime.join(name));
    }
    if let Listen::Socket(path) = listen {
        let _ = fs::remove_file(path);
    }
}

fn stop_child(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

/// Serve until the model is idle, a stop is requested or the engine fails.
/// `on_ready` is called once, when the engine answers and the state file is
/// written.
pub fn serve(options: &ServeOptions, on_ready: &mut dyn FnMut(&State)) -> Result<Stopped, ServeError> {
    let runtime = &options.runtime;
    let launch = &options.launch;
    create_private_dir(runtime).map_err(|error| {
        ServeError::new(
            AiLocalCode::RuntimeDirMissing,
            format!("{}: {error}", runtime.display()),
        )
    })?;
    if let Some(state) = read_state(runtime)
        && supervisor_running(&state)
    {
        return Err(ServeError::new(
            AiLocalCode::AlreadyRunning,
            format!(
                "{} is served by process {} on {}",
                state.model,
                state.supervisor_pid,
                state.base_url.or(state.socket).unwrap_or_default()
            ),
        ));
    }
    let _ = fs::remove_file(runtime.join(STOP_FILE));
    if !launch.program.is_file() {
        return Err(ServeError::new(
            AiLocalCode::EngineMissing,
            format!("{} does not exist", launch.program.display()),
        ));
    }
    match &launch.listen {
        Listen::Tcp { address, port } => {
            if let Err(error) = TcpListener::bind(SocketAddr::new(*address, *port)) {
                let code = if error.kind() == io::ErrorKind::AddrInUse {
                    AiLocalCode::PortInUse
                } else {
                    AiLocalCode::EngineFailed
                };
                return Err(ServeError::new(code, format!("{address} port {port}: {error}")));
            }
        }
        Listen::Socket(path) => {
            // The state said nothing runs, so a socket left here is stale.
            let _ = fs::remove_file(path);
        }
    }

    let key = random_key()
        .map_err(|error| ServeError::new(AiLocalCode::EngineFailed, format!("/dev/urandom: {error}")))?;
    write_private(&launch.api_key_file, &key).map_err(|error| {
        ServeError::new(
            AiLocalCode::RuntimeDirMissing,
            format!("{}: {error}", launch.api_key_file.display()),
        )
    })?;
    let log_path = runtime.join(LOG_FILE);
    let log = File::create(&log_path).map_err(|error| {
        ServeError::new(AiLocalCode::RuntimeDirMissing, format!("{}: {error}", log_path.display()))
    })?;
    let log_for_errors = log.try_clone().map_err(|error| {
        ServeError::new(AiLocalCode::RuntimeDirMissing, format!("{}: {error}", log_path.display()))
    })?;

    let mut command = if Path::new(SETPRIV).is_file() {
        let mut command = Command::new(SETPRIV);
        command.args(["--pdeathsig", "TERM", "--"]).arg(&launch.program);
        command
    } else {
        Command::new(&launch.program)
    };
    command
        .args(launch.arguments())
        .env_clear()
        .envs(ENGINE_ENVIRONMENT)
        .env("HOME", runtime)
        .current_dir(runtime)
        .stdin(Stdio::null())
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(log_for_errors));
    let mut child = command.spawn().map_err(|error| {
        cleanup(runtime, &launch.listen);
        ServeError::new(
            AiLocalCode::EngineFailed,
            format!("{}: {error}", launch.program.display()),
        )
    })?;

    // Wait until the engine answers its health check.
    let started = Instant::now();
    loop {
        if let Ok(Some(status)) = child.try_wait() {
            cleanup(runtime, &launch.listen);
            return Err(ServeError::new(
                AiLocalCode::EngineFailed,
                format!(
                    "the engine exited with {status} before it was ready:\n{}",
                    log_tail(&log_path, 20)
                ),
            ));
        }
        if let Ok((200, _)) = local_get(&launch.listen, "/health", None, Duration::from_secs(2)) {
            break;
        }
        if started.elapsed() > options.start_timeout {
            stop_child(&mut child);
            cleanup(runtime, &launch.listen);
            return Err(ServeError::new(
                AiLocalCode::EngineFailed,
                format!(
                    "the engine was not ready after {} seconds:\n{}",
                    options.start_timeout.as_secs(),
                    log_tail(&log_path, 20)
                ),
            ));
        }
        std::thread::sleep(Duration::from_millis(250));
    }

    let (address, port, socket) = match &launch.listen {
        Listen::Tcp { address, port } => (Some(address.to_string()), Some(*port), None),
        Listen::Socket(path) => (None, None, Some(path.to_string_lossy().into_owned())),
    };
    let now = now_seconds();
    let mut state = State {
        schema_version: 1,
        supervisor_pid: std::process::id(),
        engine_pid: child.id(),
        model: launch.model_id.clone(),
        engine: launch.program.to_string_lossy().into_owned(),
        address,
        port,
        socket,
        base_url: launch.listen.base_url(),
        network_allowed: options.network_allowed,
        api_key_file: launch.api_key_file.to_string_lossy().into_owned(),
        log_file: log_path.to_string_lossy().into_owned(),
        context: launch.context,
        started_at: now,
        idle_stop_seconds: options.idle_stop.as_secs(),
        last_activity: now,
    };
    if let Err(error) = write_state(runtime, &state) {
        stop_child(&mut child);
        cleanup(runtime, &launch.listen);
        return Err(ServeError::new(
            AiLocalCode::RuntimeDirMissing,
            format!("{}: {error}", runtime.join(STATE_FILE).display()),
        ));
    }
    on_ready(&state);

    // Watch.
    let mut last_activity = Instant::now();
    let mut last_poll = Instant::now()
        .checked_sub(POLL_INTERVAL)
        .unwrap_or_else(Instant::now);
    let mut last_written = Instant::now();
    let mut previous: Option<Activity> = None;
    loop {
        std::thread::sleep(Duration::from_millis(500));
        if runtime.join(STOP_FILE).exists() {
            let (_, peak) = process_memory(child.id());
            stop_child(&mut child);
            cleanup(runtime, &launch.listen);
            return Ok(Stopped {
                code: AiLocalCode::Stopped,
                state,
                peak_memory_bytes: peak,
            });
        }
        if let Ok(Some(status)) = child.try_wait() {
            cleanup(runtime, &launch.listen);
            return Err(ServeError::new(
                AiLocalCode::EngineFailed,
                format!(
                    "the engine exited with {status} while serving:\n{}",
                    log_tail(&log_path, 20)
                ),
            ));
        }
        if last_poll.elapsed() < POLL_INTERVAL {
            continue;
        }
        last_poll = Instant::now();
        let current = local_get(&launch.listen, "/metrics", Some(&key), Duration::from_secs(10))
            .ok()
            .filter(|(status, _)| *status == 200)
            .and_then(|(_, body)| parse_metrics(&body));
        let active = match (current, previous) {
            // Counters that cannot be read count as activity: a busy
            // engine is never stopped for being slow to answer.
            (None, _) => true,
            (Some(now), Some(before)) => now.in_progress > 0.0 || now.work != before.work,
            (Some(now), None) => now.in_progress > 0.0,
        };
        if current.is_some() {
            previous = current;
        }
        if active {
            last_activity = Instant::now();
            if last_written.elapsed() >= Duration::from_secs(5) {
                state.last_activity = now_seconds();
                let _ = write_state(runtime, &state);
                last_written = Instant::now();
            }
        }
        if last_activity.elapsed() >= options.idle_stop {
            let (_, peak) = process_memory(child.id());
            stop_child(&mut child);
            cleanup(runtime, &launch.listen);
            return Ok(Stopped {
                code: AiLocalCode::StoppedIdle,
                state,
                peak_memory_bytes: peak,
            });
        }
    }
}

/// What `stop` found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StopOutcome {
    /// Nothing was running.
    NotRunning,
    /// The served model stopped.
    Stopped(State),
}

/// Ask the supervisor to stop and wait for it, up to `timeout`.
pub fn request_stop(runtime: &Path, timeout: Duration) -> Result<StopOutcome, ServeError> {
    let Some(state) = read_state(runtime) else {
        return Ok(StopOutcome::NotRunning);
    };
    if !supervisor_running(&state) {
        return Ok(StopOutcome::NotRunning);
    }
    fs::write(runtime.join(STOP_FILE), b"stop\n").map_err(|error| {
        ServeError::new(
            AiLocalCode::RuntimeDirMissing,
            format!("{}: {error}", runtime.join(STOP_FILE).display()),
        )
    })?;
    let started = Instant::now();
    while started.elapsed() < timeout {
        if !runtime.join(STATE_FILE).exists() || !supervisor_running(&state) {
            return Ok(StopOutcome::Stopped(state));
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    Err(ServeError::new(
        AiLocalCode::EngineFailed,
        format!(
            "process {} did not stop within {} seconds",
            state.supervisor_pid,
            timeout.as_secs()
        ),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metrics_give_the_activity() {
        let text = "# HELP llamacpp:prompt_tokens_total Number of prompt tokens processed.\n\
                    # TYPE llamacpp:prompt_tokens_total counter\n\
                    llamacpp:prompt_tokens_total 120\n\
                    llamacpp:tokens_predicted_total 30\n\
                    llamacpp:n_decode_total 31\n\
                    llamacpp:requests_processing 1\n\
                    llamacpp:requests_deferred 0\n";
        let activity = parse_metrics(text).expect("counters");
        assert_eq!(activity.work, 181.0);
        assert_eq!(activity.in_progress, 1.0);
        assert_eq!(parse_metrics("# nothing\n"), None);
    }

    #[test]
    fn this_process_is_alive_and_has_memory() {
        assert!(process_alive(std::process::id()));
        assert!(!process_alive(u32::MAX));
        let (rss, peak) = process_memory(std::process::id());
        assert!(rss.is_some_and(|bytes| bytes > 0));
        assert!(peak.is_some_and(|bytes| bytes > 0));
    }

    #[test]
    fn the_runtime_folder_is_under_the_session_folder() {
        assert_eq!(
            runtime_dir(Some(Path::new("/run/user/1000"))),
            Some(PathBuf::from("/run/user/1000/tur-ai-local"))
        );
    }
}
