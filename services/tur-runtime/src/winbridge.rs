// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! P@x's bridge into a Windows application (docs/architecture/
//! pax-accessibility.md, section 11; the owner's decisions in
//! docs/reports/exp-winsee-1.md, section 11.1).
//!
//! `tur-winbridge.exe` (`services/tur-winbridge`) is a Windows program of
//! TUR's own that reads and operates a Windows application's windows from
//! inside the application's Wine prefix. It has to run **inside the
//! application's sandbox**: the Wine server's socket lives in the sandbox's
//! private `/tmp`, so a bridge started anywhere else would start a second
//! Wine server that sees none of the application's windows.
//!
//! How it is offered, without starting anything nobody asks for:
//!
//! 1. Before the sandbox starts, [`listen`] binds a socket for the
//!    application in the user's runtime directory
//!    (`$XDG_RUNTIME_DIR/tur/winbridge/<app-id>.sock`, the directory mode
//!    0700, the socket mode 0600) and the listening socket becomes the
//!    sandbox's standard input. Nothing in the sandbox sees the path.
//! 2. Inside the sandbox, the supervisor ([`Serving::start`]) gives the
//!    application an empty standard input and waits on that listening
//!    socket. When `tur-pax-a11y` connects, it starts the bridge under the
//!    application's own Wine, in its prefix, with the **connection as the
//!    bridge's standard input and output**: the pipe it inherits. One
//!    connection at a time; when it closes, the bridge reads the end of its
//!    input and exits. An application P@x never looks at pays nothing.
//! 3. When the application ends, the supervisor stops a bridge still
//!    running, before it waits for the Wine server, and the launcher
//!    removes the socket.
//!
//! The menu hook (`tur-winhook.dll`, MIT, the only code that runs inside the
//! application) is switched off with `TUR_WINBRIDGE_NO_HOOK=1` for every
//! application the catalogue calls a game and for every Proton launch
//! ([`menu_hook_allowed`]). Under Proton the bridge itself is not started
//! yet: Proton runs inside the Steam Linux Runtime's own container, where
//! the supervisor is not.
//!
//! The bridge holds no authority: it answers `tur-pax-a11y`, which asks
//! only for what the policy engine allowed. Nothing here talks to a model.

use std::fs::{self, DirBuilder};
use std::io::{self, Read};
use std::os::fd::{AsFd, OwnedFd};
use std::os::unix::fs::{DirBuilderExt, FileTypeExt, MetadataExt, PermissionsExt};
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::Duration;

use serde::Serialize;

use crate::metadata::RuntimeKind;
use crate::sandbox::SUPERVISOR_PATH;

/// Where the image installs the bridge (image/winbridge/winbridge.sh).
pub const BRIDGE_PROGRAM: &str = "/usr/lib/tur/winbridge/tur-winbridge.exe";

/// The directory of the sockets, under the user's runtime directory.
pub const SOCKET_DIRECTORY: &str = "tur/winbridge";

/// Where the image installs the application catalogue.
pub const CATALOGUE_DIRECTORY: &str = "/usr/share/tur/catalogue";

/// The variable that switches the menu hook off.
pub const NO_HOOK_VARIABLE: &str = "TUR_WINBRIDGE_NO_HOOK";

/// The longest path a Unix socket can be bound to, less a margin.
const MAX_SOCKET_PATH: usize = 100;

/// The largest catalogue entry read to tell whether it is a game.
const MAX_CATALOGUE_ENTRY: u64 = 256 * 1024;

/// How the bridge is offered for one launch, as a plan shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BridgeOffer {
    /// The socket `tur-pax-a11y` connects to.
    pub socket: PathBuf,
    /// The bridge, as Wine starts it inside the sandbox.
    pub program: String,
    /// Whether the menu hook may be loaded into the application.
    pub menu_hook: bool,
    /// Why the menu hook is or is not allowed.
    pub menu_hook_reason: String,
}

/// The socket of one application in a runtime directory. `app_id` must
/// already be a valid application identifier (it is a file name here).
pub fn socket_path(runtime_dir: &Path, app_id: &str) -> PathBuf {
    runtime_dir
        .join(SOCKET_DIRECTORY)
        .join(format!("{app_id}.sock"))
}

/// Whether the catalogue's entry for `app_id` says it is a game
/// (`category = "game"` before its first table). An application that is not
/// in the catalogue is not called a game.
pub fn is_catalogue_game(catalogue: &Path, app_id: &str) -> bool {
    let path = catalogue.join(format!("{app_id}.toml"));
    let mut text = String::new();
    let read = fs::File::open(&path)
        .and_then(|file| file.take(MAX_CATALOGUE_ENTRY).read_to_string(&mut text));
    if read.is_err() {
        return false;
    }
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            break;
        }
        let Some(rest) = line.strip_prefix("category") else {
            continue;
        };
        let Some(value) = rest.trim_start().strip_prefix('=') else {
            continue;
        };
        let value = value.trim_start();
        return value.starts_with("\"game\"") || value.starts_with("'game'");
    }
    false
}

/// Whether the menu hook may be loaded into the application: never under
/// Proton, never for a game (anti-cheat software treats a library loaded
/// into a game as tampering; the owner's decision of 14 November 2026).
pub fn menu_hook_allowed(runtime: RuntimeKind, game: bool) -> (bool, &'static str) {
    match (runtime, game) {
        (RuntimeKind::Proton, _) => (false, "a Proton launch: the menu hook is off"),
        (_, true) => (
            false,
            "the catalogue calls this application a game: the menu hook is off",
        ),
        (RuntimeKind::Wine, false) => (
            true,
            "the menu hook is read only while P@x reads a menu, for one message",
        ),
    }
}

/// The offer for one launch, or `None` when the bridge is not offered: not
/// under Wine, not installed, no runtime directory, or a socket path too
/// long to bind.
pub fn offer(
    runtime: RuntimeKind,
    bridge: Option<&Path>,
    runtime_dir: Option<&Path>,
    catalogue: &Path,
    app_id: &str,
) -> Option<BridgeOffer> {
    if runtime != RuntimeKind::Wine {
        return None;
    }
    let program = bridge?.to_str()?.to_owned();
    let socket = socket_path(runtime_dir?, app_id);
    if socket.as_os_str().len() > MAX_SOCKET_PATH {
        return None;
    }
    let (menu_hook, reason) = menu_hook_allowed(runtime, is_catalogue_game(catalogue, app_id));
    Some(BridgeOffer {
        socket,
        program,
        menu_hook,
        menu_hook_reason: reason.to_owned(),
    })
}

/// Puts the bridge's options on the supervisor's command line, right after
/// `supervise`. Returns false, changing nothing, when the command has no
/// supervisor (the bridge then cannot be started inside the sandbox).
pub fn add_to_supervisor(arguments: &mut Vec<String>, offer: &BridgeOffer) -> bool {
    let Some(at) = arguments
        .windows(2)
        .position(|pair| pair[0] == SUPERVISOR_PATH && pair[1] == "supervise")
    else {
        return false;
    };
    let mut options = vec!["--bridge".to_owned(), offer.program.clone()];
    if !offer.menu_hook {
        options.push("--bridge-no-hook".to_owned());
    }
    let insert_at = at + 2;
    arguments.splice(insert_at..insert_at, options);
    true
}

/// Binds the application's socket: its directory is created mode 0700, a
/// stale socket from an earlier launch is replaced, the socket is made mode
/// 0600. Anything at the path that is not a socket is left alone and is an
/// error.
///
/// # Errors
///
/// The directory cannot be made private, or the socket cannot be bound.
pub fn listen(socket: &Path) -> io::Result<UnixListener> {
    let directory = socket
        .parent()
        .ok_or_else(|| io::Error::other("the socket has no directory"))?;
    DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(directory)?;
    let metadata = fs::symlink_metadata(directory)?;
    if !metadata.is_dir() {
        return Err(io::Error::other(format!(
            "{} is not a directory",
            directory.display()
        )));
    }
    if metadata.mode() & 0o077 != 0 {
        fs::set_permissions(directory, fs::Permissions::from_mode(0o700))?;
    }
    match fs::symlink_metadata(socket) {
        Ok(existing) if existing.file_type().is_socket() => fs::remove_file(socket)?,
        Ok(_) => {
            return Err(io::Error::other(format!(
                "{} exists and is not a socket",
                socket.display()
            )));
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let listener = UnixListener::bind(socket)?;
    fs::set_permissions(socket, fs::Permissions::from_mode(0o600))?;
    Ok(listener)
}

/// Removes the application's socket after the sandbox has ended. Only a
/// socket is removed.
pub fn remove_socket(socket: &Path) {
    if fs::symlink_metadata(socket).is_ok_and(|metadata| metadata.file_type().is_socket()) {
        let _ = fs::remove_file(socket);
    }
}

/// What the supervisor needs to start the bridge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BridgeCommand {
    /// The application's Wine, the program the supervisor starts.
    pub wine: String,
    /// The bridge's file.
    pub program: String,
    /// Whether the menu hook is switched off.
    pub no_hook: bool,
}

fn locked<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// The bridge being served inside the sandbox, from the listening socket
/// the supervisor was given as its standard input.
#[derive(Debug)]
pub struct Serving {
    closing: Arc<AtomicBool>,
    child: Arc<Mutex<Option<Child>>>,
}

impl Serving {
    /// Serves connections on `listener`: for each, one bridge, with the
    /// connection as its standard input and output; the next connection is
    /// taken when that bridge has ended.
    pub fn start(listener: UnixListener, command: BridgeCommand) -> Serving {
        let closing = Arc::new(AtomicBool::new(false));
        let child: Arc<Mutex<Option<Child>>> = Arc::new(Mutex::new(None));
        let (thread_closing, thread_child) = (Arc::clone(&closing), Arc::clone(&child));
        let spawned = thread::Builder::new()
            .name("tur-winbridge".to_owned())
            .spawn(move || serve(&listener, &command, &thread_closing, &thread_child));
        if spawned.is_err() {
            closing.store(true, Ordering::SeqCst);
        }
        Serving { closing, child }
    }

    /// Serves from this process's standard input, when it is a listening
    /// socket. `None` when it is not one.
    pub fn from_stdin(command: BridgeCommand) -> Option<Serving> {
        let fd: OwnedFd = io::stdin().as_fd().try_clone_to_owned().ok()?;
        let listener = UnixListener::from(fd);
        // A standard input that is not a listening socket has no address.
        listener.local_addr().ok()?;
        Some(Serving::start(listener, command))
    }

    /// Stops serving: no bridge is started any more, and a running one is
    /// killed and waited for.
    pub fn close(&self) {
        self.closing.store(true, Ordering::SeqCst);
        let mut slot = locked(&self.child);
        if let Some(mut running) = slot.take() {
            let _ = running.kill();
            let _ = running.wait();
        }
    }
}

fn serve(
    listener: &UnixListener,
    command: &BridgeCommand,
    closing: &AtomicBool,
    child: &Mutex<Option<Child>>,
) {
    let mut failures = 0;
    for connection in listener.incoming() {
        if closing.load(Ordering::SeqCst) {
            break;
        }
        let stream = match connection {
            Ok(stream) => stream,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => {
                // A standard input that is not a listening socket fails at
                // once and every time.
                failures += 1;
                if failures > 8 {
                    break;
                }
                thread::sleep(Duration::from_millis(50));
                continue;
            }
        };
        let Ok(input) = stream.try_clone() else {
            continue;
        };
        {
            let mut slot = locked(child);
            if closing.load(Ordering::SeqCst) {
                break;
            }
            let mut bridge = Command::new(&command.wine);
            bridge
                .arg(&command.program)
                .stdin(Stdio::from(OwnedFd::from(input)))
                .stdout(Stdio::from(OwnedFd::from(stream)))
                .env("WINEDEBUG", "-all");
            if command.no_hook {
                bridge.env(NO_HOOK_VARIABLE, "1");
            } else {
                bridge.env_remove(NO_HOOK_VARIABLE);
            }
            match bridge.spawn() {
                Ok(started) => *slot = Some(started),
                Err(_) => continue,
            }
            // The command, and the connection's ends it held, go here: the
            // bridge holds the only ones.
        }
        // Wait for this bridge before taking the next connection.
        loop {
            {
                let mut slot = locked(child);
                match slot.as_mut() {
                    None => break,
                    Some(running) => {
                        if !matches!(running.try_wait(), Ok(None)) {
                            *slot = None;
                            break;
                        }
                    }
                }
            }
            thread::sleep(Duration::from_millis(100));
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixStream;
    use std::time::Instant;

    use super::*;
    use crate::testdata::TempDir;

    #[test]
    fn the_socket_is_in_the_runtime_directory_named_after_the_application() {
        assert_eq!(
            socket_path(Path::new("/run/user/1000"), "org.7-zip.7zip"),
            PathBuf::from("/run/user/1000/tur/winbridge/org.7-zip.7zip.sock")
        );
    }

    #[test]
    fn a_game_is_what_the_catalogue_calls_one() {
        let temp = TempDir::new("winbridge-catalogue");
        let catalogue = temp.path();
        let write = |id: &str, text: &str| fs::write(catalogue.join(format!("{id}.toml")), text).unwrap();
        write("com.example.game", "schema_version = 1\nid = \"com.example.game\"\ncategory = \"game\"\n");
        write("com.example.tool", "id = \"com.example.tool\"\ncategory = \"utility\"\n");
        write("com.example.spaced", "category   =   \"game\" # a comment\n");
        // A key of the same name in a table is not the application's.
        write("com.example.table", "id = \"x\"\n[editions]\ncategory = \"game\"\n");
        write("com.example.other", "categoryx = \"game\"\n");
        assert!(is_catalogue_game(catalogue, "com.example.game"));
        assert!(is_catalogue_game(catalogue, "com.example.spaced"));
        assert!(!is_catalogue_game(catalogue, "com.example.tool"));
        assert!(!is_catalogue_game(catalogue, "com.example.table"));
        assert!(!is_catalogue_game(catalogue, "com.example.other"));
        assert!(!is_catalogue_game(catalogue, "com.example.missing"));
    }

    #[test]
    fn the_menu_hook_is_off_for_games_and_for_proton() {
        assert!(menu_hook_allowed(RuntimeKind::Wine, false).0);
        assert!(!menu_hook_allowed(RuntimeKind::Wine, true).0);
        assert!(!menu_hook_allowed(RuntimeKind::Proton, false).0);
        assert!(!menu_hook_allowed(RuntimeKind::Proton, true).0);
    }

    #[test]
    fn the_bridge_is_offered_under_wine_only_when_it_can_be() {
        let temp = TempDir::new("winbridge-offer");
        let runtime = temp.path().join("run");
        let catalogue = temp.path().join("catalogue");
        fs::create_dir_all(&catalogue).unwrap();
        fs::write(catalogue.join("com.example.game.toml"), "category = \"game\"\n").unwrap();
        let bridge = Path::new(BRIDGE_PROGRAM);

        let offered = offer(RuntimeKind::Wine, Some(bridge), Some(&runtime), &catalogue, "com.example.app").unwrap();
        assert_eq!(offered.socket, runtime.join("tur/winbridge/com.example.app.sock"));
        assert_eq!(offered.program, BRIDGE_PROGRAM);
        assert!(offered.menu_hook);
        let game = offer(RuntimeKind::Wine, Some(bridge), Some(&runtime), &catalogue, "com.example.game").unwrap();
        assert!(!game.menu_hook);
        assert!(game.menu_hook_reason.contains("game"));

        assert_eq!(offer(RuntimeKind::Proton, Some(bridge), Some(&runtime), &catalogue, "com.example.app"), None);
        assert_eq!(offer(RuntimeKind::Wine, None, Some(&runtime), &catalogue, "com.example.app"), None);
        assert_eq!(offer(RuntimeKind::Wine, Some(bridge), None, &catalogue, "com.example.app"), None);
        let deep = PathBuf::from(format!("/{}", "d".repeat(120)));
        assert_eq!(offer(RuntimeKind::Wine, Some(bridge), Some(&deep), &catalogue, "com.example.app"), None);
    }

    #[test]
    fn the_supervisor_is_told_about_the_bridge_and_nothing_else_changes() {
        let offered = BridgeOffer {
            socket: PathBuf::from("/run/user/1000/tur/winbridge/a.sock"),
            program: BRIDGE_PROGRAM.to_owned(),
            menu_hook: false,
            menu_hook_reason: String::new(),
        };
        let mut arguments: Vec<String> = [
            "--unshare-pid",
            "--",
            SUPERVISOR_PATH,
            "supervise",
            "--settle",
            "/usr/bin/wineserver",
            "--",
            "/usr/bin/wine",
            "app.exe",
        ]
        .iter()
        .map(|argument| (*argument).to_owned())
        .collect();
        assert!(add_to_supervisor(&mut arguments, &offered));
        assert_eq!(
            arguments,
            [
                "--unshare-pid",
                "--",
                SUPERVISOR_PATH,
                "supervise",
                "--bridge",
                BRIDGE_PROGRAM,
                "--bridge-no-hook",
                "--settle",
                "/usr/bin/wineserver",
                "--",
                "/usr/bin/wine",
                "app.exe",
            ]
        );
        // A command without the supervisor is left as it was.
        let mut proton = vec!["--".to_owned(), "/usr/bin/umu-run".to_owned(), "app.exe".to_owned()];
        let before = proton.clone();
        assert!(!add_to_supervisor(&mut proton, &offered));
        assert_eq!(proton, before);
    }

    #[test]
    fn the_socket_is_private_and_a_stale_one_is_replaced() {
        let temp = TempDir::new("winbridge-listen");
        let socket = temp.path().join("run/tur/winbridge/com.example.app.sock");
        let first = listen(&socket).unwrap();
        drop(first);
        // The socket of an earlier launch is still there; it is replaced.
        let listener = listen(&socket).unwrap();
        let mode = |path: &Path| fs::metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&socket), 0o600);
        assert_eq!(mode(socket.parent().unwrap()), 0o700);
        drop(listener);
        remove_socket(&socket);
        assert!(!socket.exists());
        // Something that is not a socket is never removed or replaced.
        fs::write(&socket, b"keep").unwrap();
        assert!(listen(&socket).is_err());
        remove_socket(&socket);
        assert_eq!(fs::read(&socket).unwrap(), b"keep");
    }

    /// A stand-in for Wine: `sh BRIDGE` where BRIDGE answers each line with
    /// what it was started with.
    fn stand_in(temp: &TempDir) -> BridgeCommand {
        let script = temp.path().join("bridge.sh");
        fs::write(
            &script,
            "while IFS= read -r line; do printf '%s hook-off=%s debug=%s\\n' \"$line\" \"${TUR_WINBRIDGE_NO_HOOK:-}\" \"${WINEDEBUG:-}\"; done\n",
        )
        .unwrap();
        BridgeCommand {
            wine: "/bin/sh".to_owned(),
            program: script.to_str().unwrap().to_owned(),
            no_hook: true,
        }
    }

    fn ask(socket: &Path, line: &str) -> String {
        let mut stream = UnixStream::connect(socket).unwrap();
        stream.set_read_timeout(Some(Duration::from_secs(20))).unwrap();
        writeln!(stream, "{line}").unwrap();
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut answer = String::new();
        reader.read_line(&mut answer).unwrap();
        answer.trim_end().to_owned()
    }

    #[test]
    fn each_connection_gets_a_bridge_with_the_connection_as_its_pipe() {
        let temp = TempDir::new("winbridge-serve");
        let socket = temp.path().join("tur/winbridge/com.example.app.sock");
        let serving = Serving::start(listen(&socket).unwrap(), stand_in(&temp));
        assert_eq!(ask(&socket, "hello"), "hello hook-off=1 debug=-all");
        // The first bridge ended with its connection; the next connection
        // gets a new one.
        assert_eq!(ask(&socket, "again"), "again hook-off=1 debug=-all");

        // A bridge still running when the application ends is stopped.
        let mut held = UnixStream::connect(&socket).unwrap();
        writeln!(held, "held").unwrap();
        let mut reader = BufReader::new(held.try_clone().unwrap());
        let mut answer = String::new();
        reader.read_line(&mut answer).unwrap();
        let started = Instant::now();
        serving.close();
        assert!(started.elapsed() < Duration::from_secs(10));
        held.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
        let mut rest = String::new();
        // The bridge is gone: its end of the connection is closed.
        assert_eq!(reader.read_line(&mut rest).unwrap_or(0), 0);
    }

    #[test]
    fn the_hook_variable_is_not_passed_on_when_the_hook_is_allowed() {
        let temp = TempDir::new("winbridge-hook");
        let socket = temp.path().join("tur/winbridge/com.example.app.sock");
        let mut command = stand_in(&temp);
        command.no_hook = false;
        let serving = Serving::start(listen(&socket).unwrap(), command);
        assert_eq!(ask(&socket, "x"), "x hook-off= debug=-all");
        serving.close();
    }
}
