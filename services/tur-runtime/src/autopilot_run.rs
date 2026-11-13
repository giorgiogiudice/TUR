// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The part of the compatibility autopilot that touches the system: it
//! copies a prefix, sets remedies in the copy, runs the program in the
//! sandbox and watches for its window, and reads what is known about the
//! program and the machine (`docs/architecture/compat-autopilot.md`).
//!
//! Everything here works on copies. The real prefix is read: to copy it, to
//! fingerprint it before and after, and to look for names that differ only
//! in case. Nothing in this module writes to it; applying a result is a
//! separate, explicit operation of the command-line tool.
//!
//! Copies live under `$XDG_DATA_HOME/tur/runtime/autopilot/<app-id>/<time>/`
//! and are deleted through [`SystemExecutor`], which refuses anything
//! outside the runtime manager's own directories.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, DirBuilder, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use crate::autopilot::{
    CheckSpec, LogFile, RawRun, SnapshotInfo, Timings, Trial, TrialRunner, parse_result_id,
};
use crate::casefold::chattr_command;
use crate::exec::SystemExecutor;
use crate::failure::{Ended, StaticFacts, dll_base};
use crate::grant::GrantContext;
use crate::launch_host::LaunchHost;
use crate::metadata::{NtsyncDevice, PrefixMetadata, RuntimeKind};
use crate::paths::{AppPaths, RuntimeDirs};
use crate::plan::{CommandSpec, Operation, Plan, Step, run_plan};
use crate::profile::SyncBackend;
use crate::remedy::{RegistryValue, RemedyKind, Resolved, reg_file};
use crate::runtime_cmd::{
    DEFAULT_WINE_DEBUG, Invocation, PlanError, RUNTIME_PATH, Task, assemble, decide_sync,
};
use crate::sha256::{Sha256, hex, sha256_hex};
use crate::timefmt::Timestamp;

/// The most output of one attempt that is kept.
const MAX_TRIAL_OUTPUT: usize = 4 * 1024 * 1024;
/// The most entries a walk of a prefix looks at.
const MAX_WALK_ENTRIES: usize = 400_000;
/// The deepest a walk goes.
const MAX_WALK_DEPTH: usize = 32;
/// How long setting remedies in a copy may take.
const REMEDY_LIMIT_SECONDS: u64 = 180;
/// How often the display is asked for the program's window.
const WINDOW_POLL: Duration = Duration::from_millis(250);

/// Where Wine's own DLLs are, on the distributions TUR knows.
const WINE_DLL_DIRECTORIES: [&str; 8] = [
    "/usr/lib64/wine/x86_64-windows",
    "/usr/lib64/wine/i386-windows",
    "/usr/lib/wine/x86_64-windows",
    "/usr/lib/wine/i386-windows",
    "/usr/lib/x86_64-linux-gnu/wine/x86_64-windows",
    "/usr/lib/i386-linux-gnu/wine/i386-windows",
    "/usr/local/lib/wine/x86_64-windows",
    "/usr/local/lib/wine/i386-windows",
];

/// Where fonts are installed for every user.
const FONT_DIRECTORIES: [&str; 2] = ["/usr/share/fonts", "/usr/local/share/fonts"];

/// The directory that holds the autopilot's results for one application.
pub fn results_dir(dirs: &RuntimeDirs, app_id: &str) -> PathBuf {
    dirs.data_root().join("autopilot").join(app_id)
}

/// The directory of one result, when the identifier is one.
pub fn result_dir(dirs: &RuntimeDirs, result_id: &str) -> Option<PathBuf> {
    let (app_id, stamp) = parse_result_id(result_id)?;
    Some(results_dir(dirs, app_id).join(stamp))
}

/// The file that records the remedies applied to an application.
pub fn applied_file(app: &AppPaths) -> PathBuf {
    app.app_dir.join("remedies.json")
}

/// Creates a directory and its parents, readable by the user only.
///
/// # Errors
///
/// The I/O error as text.
pub fn create_private_dir(path: &Path) -> Result<(), String> {
    DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(path)
        .map_err(|error| format!("cannot create {}: {error}", path.display()))
}

/// Deletes a tree inside the runtime manager's own directories.
///
/// # Errors
///
/// What went wrong, as text.
pub fn remove_tree(dirs: &RuntimeDirs, path: &Path) -> Result<(), String> {
    let mut plan = Plan::new(Operation::Remove, "autopilot");
    plan.steps.push(Step::RemoveTree {
        path: path.to_path_buf(),
    });
    let report = run_plan(&plan, &mut SystemExecutor::new(dirs));
    match report.failure {
        Some(failure) => Err(failure.message),
        None => Ok(()),
    }
}

/// Visits every directory under `root`, without following links, with the
/// entries it holds sorted by name. Stops after [`MAX_WALK_ENTRIES`]
/// entries or below [`MAX_WALK_DEPTH`] levels. Returns whether the walk was
/// complete.
fn each_directory(
    root: &Path,
    visit: &mut dyn FnMut(&Path, &[(String, fs::Metadata)]),
) -> bool {
    let mut stack = vec![(PathBuf::new(), 0usize)];
    let mut seen = 0usize;
    let mut complete = true;
    while let Some((relative, depth)) = stack.pop() {
        let Ok(reader) = fs::read_dir(root.join(&relative)) else {
            continue;
        };
        let mut entries: Vec<(String, fs::Metadata)> = Vec::new();
        for entry in reader.flatten() {
            seen += 1;
            if seen > MAX_WALK_ENTRIES {
                complete = false;
                break;
            }
            let Ok(metadata) = fs::symlink_metadata(entry.path()) else {
                continue;
            };
            entries.push((entry.file_name().to_string_lossy().into_owned(), metadata));
        }
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        visit(&relative, &entries);
        if seen > MAX_WALK_ENTRIES {
            break;
        }
        for (name, metadata) in entries.iter().rev() {
            if metadata.file_type().is_dir() {
                if depth + 1 >= MAX_WALK_DEPTH {
                    complete = false;
                    continue;
                }
                stack.push((relative.join(name), depth + 1));
            }
        }
    }
    complete
}

/// A fingerprint of the trees at `roots`: the SHA-256 of every entry's
/// path, kind, size, modification time and mode, in a fixed order. Links
/// are fingerprinted as links, never followed. Two fingerprints are equal
/// when nothing was created, deleted, resized, rewritten or re-permitted.
pub fn fingerprint(roots: &[&Path]) -> String {
    let mut hasher = Sha256::new();
    for root in roots {
        hasher.update(root.to_string_lossy().as_bytes());
        hasher.update(b"\0root\n");
        if fs::symlink_metadata(root).is_err() {
            hasher.update(b"absent\n");
            continue;
        }
        let complete = each_directory(root, &mut |relative, entries| {
            for (name, metadata) in entries {
                let kind = if metadata.file_type().is_dir() {
                    'd'
                } else if metadata.file_type().is_symlink() {
                    'l'
                } else if metadata.file_type().is_file() {
                    'f'
                } else {
                    'o'
                };
                let line = format!(
                    "{}\0{kind}\0{}\0{}.{}\0{:o}\n",
                    relative.join(name).to_string_lossy(),
                    metadata.len(),
                    metadata.mtime(),
                    metadata.mtime_nsec(),
                    metadata.mode()
                );
                hasher.update(line.as_bytes());
            }
        });
        if !complete {
            hasher.update(b"incomplete\n");
        }
    }
    hex(&hasher.finish())
}

/// The bytes and the number of files under `roots`.
pub fn tree_size(roots: &[&Path]) -> (u64, u64) {
    let mut bytes = 0u64;
    let mut files = 0u64;
    for root in roots {
        each_directory(root, &mut |_, entries| {
            for (_, metadata) in entries {
                if metadata.file_type().is_file() {
                    bytes = bytes.saturating_add(metadata.len());
                    files += 1;
                }
            }
        });
    }
    (bytes, files)
}

/// Up to eight examples of names in one directory under `root` that differ
/// only in case, such as `Program Files/App: Plugins, plugins`.
pub fn case_collisions(root: &Path) -> Vec<String> {
    let mut found = Vec::new();
    each_directory(root, &mut |relative, entries| {
        if found.len() >= 8 {
            return;
        }
        let mut by_folded: BTreeMap<String, Vec<&str>> = BTreeMap::new();
        for (name, _) in entries {
            by_folded
                .entry(name.to_lowercase())
                .or_default()
                .push(name.as_str());
        }
        for names in by_folded.values() {
            if names.len() > 1 && found.len() < 8 {
                let place = relative.to_string_lossy();
                let place = if place.is_empty() { ".".to_owned() } else { place.into_owned() };
                let text: String = format!("{place}: {}", names.join(", "))
                    .chars()
                    .filter(|character| !character.is_control())
                    .take(300)
                    .collect();
                found.push(text);
            }
        }
    });
    found
}

/// The DLLs in the program's own directory, in lower case.
pub fn dlls_beside(program: &Path) -> Vec<String> {
    let Some(directory) = program.parent() else {
        return Vec::new();
    };
    let Ok(reader) = fs::read_dir(directory) else {
        return Vec::new();
    };
    let mut found: Vec<String> = reader
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
        .map(|entry| entry.file_name().to_string_lossy().to_lowercase())
        .filter(|name| name.ends_with(".dll") && !name.chars().any(char::is_control))
        .take(512)
        .collect();
    found.sort();
    found
}

/// What is known about the program at `program` before it is run.
pub fn static_facts(program: &Path, drive_c: &Path) -> StaticFacts {
    StaticFacts {
        exe_name: program
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default(),
        resource_languages: crate::pe_res::resource_languages(program),
        dlls_beside_program: dlls_beside(program),
        case_collisions: case_collisions(drive_c),
    }
}

/// The DLLs this Wine implements, by base name in lower case.
pub fn wine_builtin_dlls() -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    for directory in WINE_DLL_DIRECTORIES {
        let Ok(reader) = fs::read_dir(directory) else {
            continue;
        };
        for entry in reader.flatten().take(10_000) {
            let name = entry.file_name().to_string_lossy().to_lowercase();
            if name.ends_with(".dll") {
                found.insert(dll_base(&name));
            }
        }
    }
    found
}

/// Which of `wanted` are installed as font files for every user.
pub fn font_files(wanted: &BTreeSet<String>) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    if wanted.is_empty() {
        return found;
    }
    for directory in FONT_DIRECTORIES {
        each_directory(Path::new(directory), &mut |_, entries| {
            for (name, metadata) in entries {
                if !metadata.file_type().is_dir() && wanted.contains(name) {
                    found.insert(name.clone());
                }
            }
        });
    }
    found
}

/// Whether the file system holding `directory` can fold case: an empty
/// directory is created in it, `chattr +F` is asked of it, and it is
/// deleted again.
pub fn probe_case_folding(dirs: &RuntimeDirs, chattr: Option<&Path>, directory: &Path) -> bool {
    let Some(chattr) = chattr else {
        return false;
    };
    let probe = directory.join(format!("casefold-probe-{}", std::process::id()));
    if create_private_dir(&probe).is_err() {
        return false;
    }
    let folded = chattr_command(chattr, &probe).is_some_and(|spec| {
        spec_command(&spec)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    });
    let _ = remove_tree(dirs, &probe);
    folded
}

fn spec_command(spec: &CommandSpec) -> Command {
    let mut command = Command::new(&spec.program);
    command
        .args(&spec.arguments)
        .env_clear()
        .envs(&spec.environment)
        .stdin(Stdio::null());
    if let Some(directory) = &spec.working_directory {
        command.current_dir(directory);
    }
    command
}

fn printable(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes)
        .chars()
        .map(|character| {
            if character.is_control() && character != '\n' {
                '\u{fffd}'
            } else {
                character
            }
        })
        .collect()
}

fn exit_code(status: ExitStatus) -> i32 {
    status
        .code()
        .unwrap_or_else(|| 128 + status.signal().unwrap_or(0))
}

/// How a prefix is copied.
#[derive(Debug, Clone)]
pub struct Copier {
    /// The `cp` program.
    pub cp: PathBuf,
    /// The `chattr` program, for a case-insensitive drive C:.
    pub chattr: Option<PathBuf>,
    /// Whether copies are copy-on-write.
    pub reflink: bool,
}

impl Copier {
    fn cp(&self, from: &Path, to: &Path, reflink: bool) -> Result<(), String> {
        let mode = if reflink {
            "--reflink=always"
        } else {
            "--reflink=never"
        };
        let output = Command::new(&self.cp)
            .args(["-a", mode, "--"])
            .arg(from)
            .arg(to)
            .env_clear()
            .env("PATH", RUNTIME_PATH)
            .env("LC_ALL", "C")
            .stdin(Stdio::null())
            .output()
            .map_err(|error| format!("cannot start {}: {error}", self.cp.display()))?;
        if output.status.success() {
            Ok(())
        } else {
            let text = printable(&output.stderr);
            let tail: String = text
                .chars()
                .rev()
                .take(600)
                .collect::<Vec<char>>()
                .into_iter()
                .rev()
                .collect();
            Err(format!("cp exited with status {}: {}", exit_code(output.status), tail.trim()))
        }
    }

    /// Copies the prefix at `from` to `to`, which must not exist. With
    /// `case_folding`, `to/drive_c` is created empty and given the
    /// case-folding attribute first, and the prefix is copied into it, so
    /// that names that differ only in case become one.
    ///
    /// # Errors
    ///
    /// What went wrong, as text.
    pub fn copy_prefix(&self, from: &Path, to: &Path, case_folding: bool) -> Result<(), String> {
        if !case_folding {
            return self.cp(from, to, self.reflink);
        }
        let drive_c = to.join("drive_c");
        create_private_dir(&drive_c)?;
        let chattr = self
            .chattr
            .as_deref()
            .ok_or_else(|| "chattr is not installed".to_owned())?;
        let spec = chattr_command(chattr, &drive_c)
            .ok_or_else(|| "the path cannot be given to chattr".to_owned())?;
        let output = spec_command(&spec)
            .output()
            .map_err(|error| format!("cannot start chattr: {error}"))?;
        if !output.status.success() {
            let mut text = printable(&output.stdout);
            text.push_str(&printable(&output.stderr));
            return Err(format!(
                "drive C: could not be made case-insensitive: {}",
                text.trim()
            ));
        }
        let mut contents = from.as_os_str().to_owned();
        contents.push("/.");
        let mut target = to.as_os_str().to_owned();
        target.push("/");
        self.cp(Path::new(&contents), Path::new(&target), self.reflink)
    }

    /// Copies a directory that is not a prefix (the private home).
    ///
    /// # Errors
    ///
    /// What went wrong, as text.
    pub fn copy_tree(&self, from: &Path, to: &Path) -> Result<(), String> {
        self.cp(from, to, self.reflink)
    }
}

/// Makes the first copy of the real prefix and private home, under `base`,
/// copy-on-write when the file system can. Returns the copier to use for
/// every later copy and what the first copy cost.
///
/// # Errors
///
/// What went wrong, as text.
pub fn make_base(
    dirs: &RuntimeDirs,
    real: &AppPaths,
    base: &Path,
    cp: &Path,
    chattr: Option<&Path>,
    case_folded: bool,
) -> Result<(Copier, SnapshotInfo), String> {
    create_private_dir(base)?;
    let started = Instant::now();
    let mut copier = Copier {
        cp: cp.to_path_buf(),
        chattr: chattr.map(Path::to_path_buf),
        reflink: true,
    };
    let prefix = base.join("prefix");
    if copier
        .copy_prefix(&real.prefix, &prefix, case_folded)
        .is_err()
    {
        // No copy-on-write here: start again with an ordinary copy.
        remove_tree(dirs, &prefix)?;
        copier.reflink = false;
        copier.copy_prefix(&real.prefix, &prefix, case_folded)?;
    }
    copier.copy_tree(&real.home, &base.join("home"))?;
    let seconds = started.elapsed().as_secs_f64();
    let (bytes, files) = tree_size(&[&prefix, &base.join("home")]);
    let method = if copier.reflink { "reflink" } else { "copy" };
    let snapshot = SnapshotInfo {
        method: method.to_owned(),
        seconds,
        bytes,
        files,
    };
    Ok((copier, snapshot))
}

/// The paths of a copy, laid out as an application's own.
pub fn copy_paths(directory: &Path, app_id: &str) -> AppPaths {
    AppPaths {
        app_id: app_id.to_owned(),
        app_dir: directory.to_path_buf(),
        metadata: directory.join("metadata.json"),
        prefix: directory.join("prefix"),
        home: directory.join("home"),
        record: directory.join("application.json"),
        profile: directory.join("profile.json"),
        backups: directory.join("backups"),
        staging: directory.join("staging"),
        launchers: directory.join("launchers"),
        logs: directory.join("logs"),
    }
}

/// `path` as the prefix's `Z:` drive shows it.
pub fn z_drive(path: &Path) -> Option<String> {
    let text = crate::paths::plain_text(path)?;
    Some(format!("Z:{}", text.replace('/', "\\")))
}

/// The command that imports a `.reg` file into the prefix of `app` with
/// Wine's own `reg import`, inside the sandbox, without network and without
/// any granted directory. Only the file itself is shown, read-only.
///
/// # Errors
///
/// What [`assemble`] refuses.
pub fn reg_import_command(
    host: &LaunchHost,
    context: &GrantContext,
    app: &AppPaths,
    metadata: &PrefixMetadata,
    reg: &Path,
) -> Result<CommandSpec, PlanError> {
    let wine_path = z_drive(reg).ok_or(PlanError::Unsupported {
        reason: "the remedy file's path cannot be shown to Wine".to_owned(),
    })?;
    let arguments = vec!["reg".to_owned(), "import".to_owned(), wine_path];
    let assembled = assemble(
        &Invocation {
            host,
            app,
            context,
            runtime: metadata.runtime,
            runtime_version: &metadata.runtime_version,
            architecture: metadata.architecture,
            graphics: metadata.graphics_backend,
            sync: &metadata.sync,
            grants: &[],
            network: false,
            wine_debug: DEFAULT_WINE_DEBUG,
            extra_environment: &[],
        },
        Task::Tool {
            arguments: &arguments,
            shown: Some(reg),
        },
    )?;
    Ok(assembled.sandboxed)
}

/// Writes `values` into a copy's registry. Returns how long it took.
///
/// # Errors
///
/// What went wrong, as text.
pub fn import_registry(
    host: &LaunchHost,
    context: &GrantContext,
    dirs: &RuntimeDirs,
    app: &AppPaths,
    metadata: &PrefixMetadata,
    reg_text: &str,
    reg_path: &Path,
) -> Result<(), String> {
    let command = reg_import_command(host, context, app, metadata, reg_path)
        .map_err(|error| error.to_string())?;
    let mut plan = Plan::new(Operation::Configure, &app.app_id);
    plan.steps.push(Step::WriteFile {
        path: reg_path.to_path_buf(),
        contents: reg_text.to_owned(),
    });
    plan.steps.push(Step::RunBounded {
        label: "set the remedies in the copy's registry".to_owned(),
        command,
        limit_seconds: REMEDY_LIMIT_SECONDS,
    });
    let mut executor = SystemExecutor::new(dirs).with_command_log(app.app_dir.join("remedy.log"));
    let report = run_plan(&plan, &mut executor);
    match report.failure {
        None => Ok(()),
        Some(failure) => Err(format!(
            "{}: {}. {}",
            failure.description,
            failure.message,
            executor.last_output().trim()
        )),
    }
}

/// Asks the X server which windows are there.
#[derive(Debug, Clone)]
pub struct WindowProbe {
    /// The `xwininfo` program.
    pub xwininfo: PathBuf,
    /// `DISPLAY` and, when there is one, `XAUTHORITY`.
    pub environment: BTreeMap<String, String>,
}

impl WindowProbe {
    /// The probe for the session's X11 display, when there is one.
    pub fn for_host(host: &LaunchHost, xwininfo: Option<PathBuf>) -> Option<WindowProbe> {
        let xwininfo = xwininfo?;
        let x11 = host.sockets.x11.as_ref()?;
        let mut environment = BTreeMap::new();
        environment.insert("DISPLAY".to_owned(), x11.display.clone());
        environment.insert("PATH".to_owned(), RUNTIME_PATH.to_owned());
        if let Some(authority) = &host.sockets.xauthority {
            environment.insert(
                "XAUTHORITY".to_owned(),
                authority.to_string_lossy().into_owned(),
            );
        }
        Some(WindowProbe {
            xwininfo,
            environment,
        })
    }

    fn query(&self, arguments: &[&str]) -> String {
        Command::new(&self.xwininfo)
            .args(arguments)
            .env_clear()
            .envs(&self.environment)
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .map(|output| printable(&output.stdout))
            .unwrap_or_default()
    }

    /// The windows whose X11 instance or class is one of `classes`, by
    /// identifier.
    pub fn windows(&self, classes: &[String]) -> Vec<String> {
        parse_window_tree(&self.query(&["-root", "-tree"]), classes)
    }

    /// Whether the window is mapped and viewable.
    pub fn is_viewable(&self, id: &str) -> bool {
        self.query(&["-id", id]).contains("IsViewable")
    }
}

/// The identifiers of the windows in `xwininfo -root -tree` output whose
/// instance or class is one of `classes`, ignoring case. A line reads
/// `0x1a00004 "Title": ("app.exe" "app.exe")  640x480+0+0  +0+0`.
pub fn parse_window_tree(tree: &str, classes: &[String]) -> Vec<String> {
    let mut found = Vec::new();
    for line in tree.lines() {
        let line = line.trim_start();
        if !line.starts_with("0x") {
            continue;
        }
        let Some(id) = line.split_whitespace().next() else {
            continue;
        };
        let Some(at) = line.rfind(": (\"") else {
            continue;
        };
        let rest = line.get(at + 4..).unwrap_or_default();
        let Some((instance, rest)) = rest.split_once('"') else {
            continue;
        };
        let class = rest
            .trim_start()
            .strip_prefix('"')
            .and_then(|rest| rest.split_once('"'))
            .map_or("", |(class, _)| class);
        let matches = classes.iter().any(|wanted| {
            instance.eq_ignore_ascii_case(wanted) || class.eq_ignore_ascii_case(wanted)
        });
        if matches {
            found.push(id.to_owned());
        }
    }
    found
}

/// Collects a stream, up to [`MAX_TRIAL_OUTPUT`] bytes, and reads it to the
/// end either way.
fn collect(mut stream: impl Read, captured: &Mutex<Vec<u8>>) {
    let mut buffer = [0u8; 8192];
    loop {
        let read = match stream.read(&mut buffer) {
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Ok(0) | Err(_) => break,
            Ok(read) => read,
        };
        if let Ok(mut captured) = captured.lock() {
            let room = MAX_TRIAL_OUTPUT.saturating_sub(captured.len());
            captured.extend_from_slice(buffer.get(..read.min(room)).unwrap_or_default());
        }
    }
}

/// Runs a sandboxed command as `check` says: until it exits, until its
/// window has been up for the settle time, or until its time is up. Returns
/// what it did and what it printed.
pub fn watch(
    spec: &CommandSpec,
    check: &CheckSpec,
    classes: &[String],
    probe: Option<&WindowProbe>,
) -> Result<(RawRun, Vec<u8>), String> {
    let before: BTreeSet<String> = probe
        .filter(|_| check.window)
        .map(|probe| probe.windows(classes).into_iter().collect())
        .unwrap_or_default();
    let mut command = spec_command(spec);
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command
        .spawn()
        .map_err(|error| format!("cannot start {}: {error}", spec.program))?;
    let captured = Arc::new(Mutex::new(Vec::new()));
    let readers = [
        child.stdout.take().map(|stream| {
            let captured = Arc::clone(&captured);
            thread::spawn(move || collect(stream, &captured))
        }),
        child.stderr.take().map(|stream| {
            let captured = Arc::clone(&captured);
            thread::spawn(move || collect(stream, &captured))
        }),
    ];
    let started = Instant::now();
    let deadline = started + Duration::from_secs(check.timeout_seconds);
    let settle = Duration::from_secs(check.settle_seconds);
    let mut window_at: Option<Instant> = None;
    let mut last_probe: Option<Instant> = None;
    let (exit_status, ended) = loop {
        match child.try_wait() {
            Ok(Some(status)) => break (Some(exit_code(status)), Ended::Exited),
            Ok(None) => {}
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("cannot wait for the program: {error}"));
            }
        }
        let now = Instant::now();
        if check.window
            && window_at.is_none()
            && let Some(probe) = probe
            && last_probe.is_none_or(|at| now.duration_since(at) >= WINDOW_POLL)
        {
            last_probe = Some(now);
            let appeared = probe
                .windows(classes)
                .into_iter()
                .any(|id| !before.contains(&id) && probe.is_viewable(&id));
            if appeared {
                window_at = Some(now);
            }
        }
        if let Some(at) = window_at
            && now.duration_since(at) >= settle
        {
            let _ = child.kill();
            let _ = child.wait();
            break (None, Ended::Stopped);
        }
        if now >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            break (None, Ended::TimedOut);
        }
        thread::sleep(Duration::from_millis(100));
    };
    for reader in readers.into_iter().flatten() {
        let _ = reader.join();
    }
    let output = captured
        .lock()
        .map(|buffer| buffer.clone())
        .unwrap_or_default();
    Ok((
        RawRun {
            exit_status,
            ended,
            window_seen: window_at.is_some(),
        },
        output,
    ))
}

/// Writes an attempt's log beside the result, readable by the user only.
fn write_log(directory: &Path, number: usize, text: &[u8]) -> Option<LogFile> {
    let file = format!("attempt-{number}.log");
    let path = directory.join(&file);
    let mut handle = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&path)
        .ok()?;
    handle.write_all(text).ok()?;
    Some(LogFile {
        file,
        sha256: sha256_hex(text),
        bytes: u64::try_from(text.len()).unwrap_or(u64::MAX),
    })
}

/// The trial runner that works on real copies of the prefix.
pub struct CopyTrials<'a> {
    /// The machine and session.
    pub host: &'a LaunchHost,
    /// The grant rules; no directory is granted to a copy.
    pub context: &'a GrantContext,
    /// The runtime manager's directories.
    pub dirs: &'a RuntimeDirs,
    /// The application's metadata, for the runtime and its settings.
    pub metadata: &'a PrefixMetadata,
    /// The application identifier.
    pub app_id: &'a str,
    /// The first copy every attempt starts from.
    pub base: PathBuf,
    /// Whether the first copy's drive C: is case-insensitive.
    pub base_case_folded: bool,
    /// The result's directory, where the logs go.
    pub result_dir: PathBuf,
    /// The program, relative to the prefix.
    pub program: PathBuf,
    /// What "works" means.
    pub check: &'a CheckSpec,
    /// The program's window class.
    pub class: String,
    /// How copies are made.
    pub copier: Copier,
    /// The display probe.
    pub probe: Option<WindowProbe>,
    /// Whether copies share the host's network.
    pub network: bool,
    /// The variables of remedies the person already applied: every launch
    /// of the application gets them, so every attempt does too.
    pub base_environment: Vec<(String, String)>,
    /// Whether an applied remedy leaves `/dev/ntsync` out.
    pub base_hide_ntsync: bool,
    /// The time the run began.
    pub now: Timestamp,
}

impl CopyTrials<'_> {
    fn attempt(
        &self,
        number: usize,
        remedies: &[Resolved],
        timings: &mut Timings,
        header: &mut String,
    ) -> Result<Result<(RawRun, Vec<u8>), String>, String> {
        let directory = self.result_dir.join("trial");
        remove_tree(self.dirs, &directory)?;
        create_private_dir(&directory)?;
        let copy = copy_paths(&directory, self.app_id);

        // A fresh copy.
        let started = Instant::now();
        let case_folding = self.base_case_folded
            || remedies
                .iter()
                .any(|remedy| remedy.kind == RemedyKind::DriveCCaseFolding);
        if let Err(problem) = self
            .copier
            .copy_prefix(&self.base.join("prefix"), &copy.prefix, case_folding)
        {
            return Ok(Err(problem));
        }
        self.copier
            .copy_tree(&self.base.join("home"), &copy.home)?;
        create_private_dir(&copy.logs)?;
        timings.snapshot_seconds = started.elapsed().as_secs_f64();

        // The remedies.
        let started = Instant::now();
        let registry: Vec<&Resolved> = remedies
            .iter()
            .filter(|remedy| !remedy.registry.is_empty())
            .collect();
        if !registry.is_empty() {
            let text = reg_file(&[], &registry);
            header.push_str("# registry:\n");
            for line in text.lines() {
                header.push_str("#   ");
                header.push_str(line);
                header.push('\n');
            }
            if let Err(problem) = import_registry(
                self.host,
                self.context,
                self.dirs,
                &copy,
                self.metadata,
                &text,
                &directory.join("remedy.reg"),
            ) {
                return Ok(Err(problem));
            }
        }
        timings.remedy_seconds = started.elapsed().as_secs_f64();

        // The run.
        let mut environment = self.base_environment.clone();
        for (name, value) in remedies
            .iter()
            .flat_map(|remedy| remedy.environment.iter().cloned())
        {
            environment.retain(|(given, _)| *given != name);
            environment.push((name, value));
        }
        let mut host = self.host.clone();
        if self.base_hide_ntsync
            || remedies
                .iter()
                .any(|remedy| remedy.kind == RemedyKind::HideNtsync)
        {
            host.ntsync = NtsyncDevice::Absent;
        }
        let sync = decide_sync(
            SyncBackend::Auto,
            RuntimeKind::Wine,
            &self.metadata.runtime_version,
            host.ntsync,
            self.now,
        );
        let program = copy.prefix.join(&self.program);
        let assembled = assemble(
            &Invocation {
                host: &host,
                app: &copy,
                context: self.context,
                runtime: RuntimeKind::Wine,
                runtime_version: &self.metadata.runtime_version,
                architecture: self.metadata.architecture,
                graphics: self.metadata.graphics_backend,
                sync: &sync,
                grants: &[],
                network: self.network,
                wine_debug: DEFAULT_WINE_DEBUG,
                extra_environment: &environment,
            },
            Task::Application {
                program: &program,
                outside_prefix: false,
                arguments: &self.check.arguments,
            },
        )
        .map_err(|error| error.to_string())?;
        let mut classes = vec![self.class.clone()];
        if remedies
            .iter()
            .any(|remedy| remedy.group == "virtual-desktop")
        {
            // In a virtual desktop the program's windows are inside the
            // desktop's own, whose class is explorer's.
            classes.push("explorer.exe".to_owned());
        }
        let names: Vec<&str> = assembled
            .application
            .environment
            .keys()
            .map(String::as_str)
            .collect();
        header.push_str(&format!(
            "# command: {}\n# environment: {}\n",
            assembled.application.display_line(),
            names.join(" ")
        ));
        for (name, value) in &environment {
            header.push_str(&format!("# remedy sets {name}={value}\n"));
        }
        let started = Instant::now();
        let watched = watch(&assembled.sandboxed, self.check, &classes, self.probe.as_ref());
        timings.run_seconds = started.elapsed().as_secs_f64();
        Ok(Ok(watched?))
    }
}

impl TrialRunner for CopyTrials<'_> {
    fn run(&mut self, number: usize, remedies: &[Resolved]) -> Trial {
        let mut timings = Timings::default();
        let labels: Vec<&str> = remedies
            .iter()
            .map(|remedy| remedy.instance.label.as_str())
            .collect();
        let mut header = format!(
            "# tur-runtime compatibility autopilot, attempt {number}, {}\n# remedies: {}\n",
            Timestamp::now().rfc3339(),
            if labels.is_empty() {
                "none (the application unchanged)".to_owned()
            } else {
                labels.join(" + ")
            }
        );
        let outcome = self.attempt(number, remedies, &mut timings, &mut header);
        let _ = remove_tree(self.dirs, &self.result_dir.join("trial"));
        let (not_applicable, run, output) = match outcome {
            Ok(Ok((run, output))) => (None, run, output),
            Ok(Err(problem)) | Err(problem) => (
                Some(problem),
                RawRun {
                    exit_status: None,
                    ended: Ended::Exited,
                    window_seen: false,
                },
                Vec::new(),
            ),
        };
        let mut log = header.into_bytes();
        if let Some(problem) = &not_applicable {
            log.extend_from_slice(format!("# not run: {problem}\n").as_bytes());
        }
        log.extend_from_slice(&output);
        if !output.ends_with(b"\n") && !output.is_empty() {
            log.push(b'\n');
        }
        log.extend_from_slice(
            format!(
                "# ended: {:?}, exit status {}, window seen: {}\n",
                run.ended,
                run.exit_status
                    .map_or_else(|| "none".to_owned(), |status| status.to_string()),
                run.window_seen
            )
            .as_bytes(),
        );
        Trial {
            not_applicable,
            run,
            output: String::from_utf8_lossy(&output).into_owned(),
            timings,
            log: write_log(&self.result_dir, number, &log),
        }
    }
}

/// The registry values every copy gets before anything is tried, as a
/// `.reg` text: see [`crate::remedy::trial_settings`].
pub fn trial_settings_text() -> String {
    let settings: Vec<RegistryValue> = crate::remedy::trial_settings();
    reg_file(&settings, &[])
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::symlink;

    use super::*;
    use crate::testdata::TempDir;

    #[test]
    fn window_trees_are_read_by_class() {
        let tree = "\nxwininfo: Window id: 0x1e9 (the root window) (has no name)\n\n  Root window id: 0x1e9 (the root window) (has no name)\n  Parent window id: 0x0 (none)\n     3 children:\n     0x1a00004 \"7-Zip\": (\"7zfm.exe\" \"7zfm.exe\")  640x480+0+0  +0+0\n        1 child:\n        0x1a00005 (has no name): ()  1x1+-1+-1  +-1+-1\n     0x200001 \"a (b): (\\\"c\\\")\": (\"other.exe\" \"Other.exe\")  10x10+0+0  +0+0\n     0x400001 \"Desktop\": (\"explorer.exe\" \"explorer.exe\")  1024x768+0+0  +0+0\n";
        assert_eq!(
            parse_window_tree(tree, &["7zfm.exe".to_owned()]),
            ["0x1a00004"]
        );
        assert_eq!(
            parse_window_tree(tree, &["OTHER.EXE".to_owned()]),
            ["0x200001"]
        );
        assert_eq!(
            parse_window_tree(tree, &["x.exe".to_owned(), "explorer.exe".to_owned()]),
            ["0x400001"]
        );
        assert!(parse_window_tree(tree, &["missing.exe".to_owned()]).is_empty());
    }

    #[test]
    fn a_fingerprint_sees_every_change_and_follows_no_link() {
        let temp = TempDir::new("fingerprint");
        let root = temp.path().join("prefix");
        fs::create_dir_all(root.join("drive_c/App")).unwrap();
        fs::write(root.join("drive_c/App/app.exe"), b"program").unwrap();
        let outside = temp.path().join("outside");
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("secret"), b"x").unwrap();
        symlink(&outside, root.join("drive_c/link")).unwrap();
        let first = fingerprint(&[&root]);
        assert_eq!(first, fingerprint(&[&root]));
        // A change behind the link is not part of the prefix.
        fs::write(outside.join("secret"), b"changed").unwrap();
        assert_eq!(first, fingerprint(&[&root]));
        fs::write(root.join("drive_c/App/app.exe"), b"program, changed").unwrap();
        let second = fingerprint(&[&root]);
        assert_ne!(first, second);
        fs::write(root.join("drive_c/App/new.txt"), b"").unwrap();
        assert_ne!(second, fingerprint(&[&root]));
        assert_ne!(fingerprint(&[&temp.path().join("missing")]), first);
        let (bytes, files) = tree_size(&[&root]);
        assert_eq!(files, 2);
        assert_eq!(bytes, 16);
    }

    #[test]
    fn names_that_differ_only_in_case_are_found() {
        let temp = TempDir::new("collisions");
        let drive_c = temp.path().join("drive_c");
        fs::create_dir_all(drive_c.join("App/Plugins")).unwrap();
        fs::create_dir_all(drive_c.join("App/plugins")).unwrap();
        fs::write(drive_c.join("App/readme.txt"), b"").unwrap();
        let found = case_collisions(&drive_c);
        assert_eq!(found, ["App: Plugins, plugins"]);
        fs::write(drive_c.join("App/x.DLL"), b"").unwrap();
        fs::write(drive_c.join("App/app.exe"), b"").unwrap();
        assert_eq!(dlls_beside(&drive_c.join("App/app.exe")), ["x.dll"]);
        let facts = static_facts(&drive_c.join("App/app.exe"), &drive_c);
        assert_eq!(facts.exe_name, "app.exe");
        assert_eq!(facts.case_collisions.len(), 1);
    }

    #[test]
    fn copies_are_laid_out_as_an_application_s_own() {
        let paths = copy_paths(Path::new("/d/tur/runtime/autopilot/a.b/x/trial"), "a.b");
        assert_eq!(paths.prefix, Path::new("/d/tur/runtime/autopilot/a.b/x/trial/prefix"));
        assert_eq!(paths.home, Path::new("/d/tur/runtime/autopilot/a.b/x/trial/home"));
        assert_eq!(
            z_drive(Path::new("/d/x/remedy.reg")).as_deref(),
            Some("Z:\\d\\x\\remedy.reg")
        );
        assert!(trial_settings_text().contains("ShowCrashDialog"));
        let dirs = RuntimeDirs::new(Path::new("/d"), Path::new("/s"));
        assert_eq!(
            result_dir(&dirs, "a.b@20261113T101500Z").unwrap(),
            Path::new("/d/tur/runtime/autopilot/a.b/20261113T101500Z")
        );
        assert!(result_dir(&dirs, "a.b@x").is_none());
    }

    #[test]
    fn a_copy_never_reaches_back_into_the_original() {
        let temp = TempDir::new("copy");
        let dirs = RuntimeDirs::new(temp.path(), temp.path());
        let from = temp.path().join("tur/runtime/prefixes/a.b/prefix");
        fs::create_dir_all(from.join("drive_c")).unwrap();
        fs::write(from.join("drive_c/file.txt"), b"original").unwrap();
        let Some(cp) = crate::launch_host::find_program(&["/usr/bin", "/bin"], &["cp"]) else {
            return;
        };
        let copier = Copier {
            cp,
            chattr: None,
            reflink: false,
        };
        let to = temp.path().join("tur/runtime/autopilot/a.b/x/trial/prefix");
        create_private_dir(to.parent().unwrap()).unwrap();
        copier.copy_prefix(&from, &to, false).unwrap();
        fs::write(to.join("drive_c/file.txt"), b"changed in the copy").unwrap();
        assert_eq!(fs::read(from.join("drive_c/file.txt")).unwrap(), b"original");
        // Without chattr a case-insensitive copy is refused, not faked.
        let other = temp.path().join("tur/runtime/autopilot/a.b/x/other/prefix");
        assert!(copier.copy_prefix(&from, &other, true).is_err());
        remove_tree(&dirs, &temp.path().join("tur/runtime/autopilot/a.b/x")).unwrap();
        assert!(!to.exists());
        // Outside the runtime manager's directories nothing is deleted.
        assert!(remove_tree(&dirs, temp.path()).is_err());
    }
}
