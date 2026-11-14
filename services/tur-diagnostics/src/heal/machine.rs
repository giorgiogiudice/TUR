// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The only door between self-healing and the computer it repairs.
//!
//! Checks and remedies reach the machine through [`Machine`]: run a command
//! (no shell, a time limit, an output limit), read a file or a link, and
//! replace one file. [`RealMachine`] does it for real, optionally below
//! another root directory (for tests on a CI runner); [`FakeMachine`]
//! answers from tables and records what was run, so that the engine's tests
//! never depend on the machine that runs them.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::host::{CommandOutput, Host, Limits, ReadError, RunError, SystemHost};

/// The longest a check's or a remedy's command may run.
pub const COMMAND_TIMEOUT: Duration = Duration::from_secs(120);

/// The most output kept of one command.
pub const COMMAND_OUTPUT_BYTES: usize = 4 * 1024 * 1024;

/// The most read of one file.
pub const FILE_BYTES: usize = 1024 * 1024;

/// What self-healing may do on a computer.
pub trait Machine {
    /// Runs `program` with `args`, without a shell.
    fn run(&self, program: &str, args: &[&str]) -> Result<CommandOutput, RunError>;

    /// Reads at most [`FILE_BYTES`] of the file at the absolute `path`.
    fn read_file(&self, path: &str) -> Result<Vec<u8>, ReadError>;

    /// The target of the symbolic link at `path`, as written. An error of
    /// kind [`io::ErrorKind::InvalidInput`] means the path exists and is not
    /// a link.
    fn read_link(&self, path: &str) -> io::Result<String>;

    /// The permission bits of the file at `path`.
    fn mode(&self, path: &str) -> io::Result<u32>;

    /// Replaces whatever is at `path` with a symbolic link to `target`, in
    /// one rename.
    fn replace_with_link(&self, path: &str, target: &str) -> io::Result<()>;

    /// Replaces whatever is at `path` with a regular file holding
    /// `content`, with permission bits `mode`, in one rename.
    fn replace_with_file(&self, path: &str, content: &[u8], mode: u32) -> io::Result<()>;

    /// Gives `path` its default SELinux label again, where that applies.
    /// Never fails: a label that cannot be restored is left as it is.
    fn relabel(&self, path: &str);

    /// The path a program run by [`Machine::run`] must be given for the
    /// absolute `path`.
    fn command_path(&self, path: &str) -> String;

    /// Seconds since the epoch.
    fn now(&self) -> u64;

    /// Waits `seconds`.
    fn sleep(&self, seconds: u64);
}

/// The real computer, below `root` (normally `/`).
#[derive(Debug, Clone)]
pub struct RealMachine {
    host: SystemHost,
    root: PathBuf,
}

impl RealMachine {
    /// The computer itself.
    pub fn new() -> Self {
        RealMachine::below(Path::new("/"))
    }

    /// The computer, with every file path taken below `root`. Commands are
    /// still the computer's own. For tests only.
    pub fn below(root: &Path) -> Self {
        RealMachine {
            host: SystemHost::new(),
            root: root.to_path_buf(),
        }
    }

    fn is_root(&self) -> bool {
        self.root == Path::new("/")
    }

    fn real(&self, path: &str) -> PathBuf {
        if self.is_root() {
            PathBuf::from(path)
        } else {
            self.root.join(path.trim_start_matches('/'))
        }
    }

    /// A name beside `path` for the file that is renamed over it.
    fn temporary_beside(path: &Path) -> io::Result<PathBuf> {
        let parent = path
            .parent()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "no parent directory"))?;
        let name = path
            .file_name()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "no file name"))?;
        Ok(parent.join(format!(".{}.tur-heal", name.to_string_lossy())))
    }
}

impl Default for RealMachine {
    fn default() -> Self {
        RealMachine::new()
    }
}

impl Machine for RealMachine {
    fn run(&self, program: &str, args: &[&str]) -> Result<CommandOutput, RunError> {
        self.host.run(
            program,
            args,
            Limits {
                timeout: COMMAND_TIMEOUT,
                max_bytes: COMMAND_OUTPUT_BYTES,
            },
        )
    }

    fn read_file(&self, path: &str) -> Result<Vec<u8>, ReadError> {
        let real = self.real(path);
        self.host
            .read_file(&real.to_string_lossy(), FILE_BYTES)
            .map(|content| content.data)
    }

    fn read_link(&self, path: &str) -> io::Result<String> {
        fs::read_link(self.real(path)).map(|target| target.to_string_lossy().into_owned())
    }

    fn mode(&self, path: &str) -> io::Result<u32> {
        fs::metadata(self.real(path)).map(|metadata| metadata.permissions().mode() & 0o7777)
    }

    fn replace_with_link(&self, path: &str, target: &str) -> io::Result<()> {
        let real = self.real(path);
        let temporary = Self::temporary_beside(&real)?;
        if let Err(error) = fs::remove_file(&temporary)
            && error.kind() != io::ErrorKind::NotFound
        {
            return Err(error);
        }
        std::os::unix::fs::symlink(target, &temporary)?;
        fs::rename(&temporary, &real)
    }

    fn replace_with_file(&self, path: &str, content: &[u8], mode: u32) -> io::Result<()> {
        let real = self.real(path);
        let temporary = Self::temporary_beside(&real)?;
        if let Err(error) = fs::remove_file(&temporary)
            && error.kind() != io::ErrorKind::NotFound
        {
            return Err(error);
        }
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(mode & 0o7777)
            .open(&temporary)?;
        file.write_all(content)?;
        file.sync_all()?;
        drop(file);
        fs::set_permissions(&temporary, fs::Permissions::from_mode(mode & 0o7777))?;
        fs::rename(&temporary, &real)
    }

    fn relabel(&self, path: &str) {
        if !self.is_root() || !Path::new("/usr/sbin/restorecon").exists() {
            return;
        }
        // A label that cannot be restored is left; the remedy's check
        // decides whether the repair holds.
        let _ = self.run("/usr/sbin/restorecon", &[path]);
    }

    fn command_path(&self, path: &str) -> String {
        self.real(path).to_string_lossy().into_owned()
    }

    fn now(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_secs())
    }

    fn sleep(&self, seconds: u64) {
        std::thread::sleep(Duration::from_secs(seconds));
    }
}

/// One entry of a [`FakeMachine`]'s file system.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FakeEntry {
    /// A regular file, with its content and permission bits.
    File(Vec<u8>, u32),
    /// A symbolic link, with its target as written.
    Link(String),
}

/// What a command of a [`FakeMachine`] answers.
pub type Answer = Result<CommandOutput, RunError>;

/// A machine that answers from tables. For tests.
///
/// A command is looked up by its whole line (program and arguments joined by
/// single spaces), then by its program alone. Anything not in the table is
/// not installed. An effect makes a command change the table when it runs,
/// so that a remedy changes what the next check sees.
#[derive(Debug, Default)]
pub struct FakeMachine {
    commands: RefCell<BTreeMap<String, Answer>>,
    effects: RefCell<BTreeMap<String, Vec<(String, Answer)>>>,
    files: RefCell<BTreeMap<String, FakeEntry>>,
    ran: RefCell<Vec<String>>,
    relabelled: RefCell<Vec<String>>,
    clock: Cell<u64>,
}

/// A successful command that printed `stdout`.
pub fn printed(stdout: &str) -> Answer {
    exited(0, stdout)
}

/// A command that exited with `code` after printing `stdout`.
pub fn exited(code: i32, stdout: &str) -> Answer {
    Ok(CommandOutput {
        exit_code: Some(code),
        stdout: stdout.as_bytes().to_vec(),
        stderr: Vec::new(),
        truncated: false,
    })
}

impl FakeMachine {
    /// A machine with no command, no file, and the clock at `now`.
    pub fn new(now: u64) -> Self {
        let machine = FakeMachine::default();
        machine.clock.set(now);
        machine
    }

    /// Makes `line` answer `answer`.
    pub fn with_command(self, line: &str, answer: Answer) -> Self {
        self.set_command(line, answer);
        self
    }

    /// Makes `line` answer `answer` from now on.
    pub fn set_command(&self, line: &str, answer: Answer) {
        self.commands.borrow_mut().insert(line.to_owned(), answer);
    }

    /// When `line` runs, `target` answers `answer` from then on.
    pub fn with_effect(self, line: &str, target: &str, answer: Answer) -> Self {
        self.effects
            .borrow_mut()
            .entry(line.to_owned())
            .or_default()
            .push((target.to_owned(), answer));
        self
    }

    /// Puts a regular file at `path`.
    pub fn with_file(self, path: &str, content: &str, mode: u32) -> Self {
        self.files.borrow_mut().insert(
            path.to_owned(),
            FakeEntry::File(content.as_bytes().to_vec(), mode),
        );
        self
    }

    /// Puts a symbolic link at `path`.
    pub fn with_link(self, path: &str, target: &str) -> Self {
        self.files
            .borrow_mut()
            .insert(path.to_owned(), FakeEntry::Link(target.to_owned()));
        self
    }

    /// What is at `path` now.
    pub fn entry(&self, path: &str) -> Option<FakeEntry> {
        self.files.borrow().get(path).cloned()
    }

    /// Every command line run so far, in order.
    pub fn ran(&self) -> Vec<String> {
        self.ran.borrow().clone()
    }

    /// Every path relabelled so far.
    pub fn relabelled(&self) -> Vec<String> {
        self.relabelled.borrow().clone()
    }

    /// Moves the clock forward.
    pub fn advance(&self, seconds: u64) {
        self.clock.set(self.clock.get() + seconds);
    }
}

impl Machine for FakeMachine {
    fn run(&self, program: &str, args: &[&str]) -> Result<CommandOutput, RunError> {
        let mut line = program.to_owned();
        for arg in args {
            line.push(' ');
            line.push_str(arg);
        }
        self.ran.borrow_mut().push(line.clone());
        let answer = {
            let commands = self.commands.borrow();
            commands
                .get(&line)
                .or_else(|| commands.get(program))
                .cloned()
                .unwrap_or(Err(RunError::NotFound))
        };
        if let Some(effects) = self.effects.borrow().get(&line) {
            for (target, result) in effects {
                self.set_command(target, result.clone());
            }
        }
        answer
    }

    fn read_file(&self, path: &str) -> Result<Vec<u8>, ReadError> {
        match self.files.borrow().get(path) {
            Some(FakeEntry::File(content, _)) => Ok(content.clone()),
            Some(FakeEntry::Link(_)) => Err(ReadError::Io("a link in the fake".to_owned())),
            None => Err(ReadError::NotFound),
        }
    }

    fn read_link(&self, path: &str) -> io::Result<String> {
        match self.files.borrow().get(path) {
            Some(FakeEntry::Link(target)) => Ok(target.clone()),
            Some(FakeEntry::File(..)) => Err(io::Error::from(io::ErrorKind::InvalidInput)),
            None => Err(io::Error::from(io::ErrorKind::NotFound)),
        }
    }

    fn mode(&self, path: &str) -> io::Result<u32> {
        match self.files.borrow().get(path) {
            Some(FakeEntry::File(_, mode)) => Ok(*mode),
            Some(FakeEntry::Link(_)) => Ok(0o777),
            None => Err(io::Error::from(io::ErrorKind::NotFound)),
        }
    }

    fn replace_with_link(&self, path: &str, target: &str) -> io::Result<()> {
        self.files
            .borrow_mut()
            .insert(path.to_owned(), FakeEntry::Link(target.to_owned()));
        Ok(())
    }

    fn replace_with_file(&self, path: &str, content: &[u8], mode: u32) -> io::Result<()> {
        self.files
            .borrow_mut()
            .insert(path.to_owned(), FakeEntry::File(content.to_vec(), mode));
        Ok(())
    }

    fn relabel(&self, path: &str) {
        self.relabelled.borrow_mut().push(path.to_owned());
    }

    fn command_path(&self, path: &str) -> String {
        path.to_owned()
    }

    fn now(&self) -> u64 {
        self.clock.get()
    }

    fn sleep(&self, seconds: u64) {
        self.advance(seconds);
    }
}
