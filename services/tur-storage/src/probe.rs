// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! Where volume facts come from.
//!
//! The policy core never looks at a device. It asks a [`Probe`] for two
//! things: the block-device tree in the shape of
//! `lsblk --json --bytes --output-all`, and `blkid`-style `KEY=value`
//! properties for a device. [`SnapshotProbe`] answers from text captured
//! earlier, which is what the tests use. [`LiveProbe`] answers for the
//! machine it runs on, and does so by running exactly one program, `lsblk`,
//! with exactly those arguments: a program that reads. It runs nothing else,
//! opens no device itself and never writes. It has no `blkid` properties, so
//! on a live system an NTFS volume's state and an HFS+ volume's journal are
//! unknown, which the decision treats as unsafe.
//!
//! Facts that neither tool reports are passed as extra properties with a
//! `TUR_` prefix:
//!
//! * `TUR_NTFS_STATE` = `clean` | `dirty` | `hibernated` | `unknown`
//! * `TUR_HFS_JOURNAL` = `journalled` | `not-journalled` | `unknown`
//!
//! A missing property means `unknown`, which the decision treats as unsafe.

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Deserializer};

/// Property carrying the NTFS volume state.
pub const PROP_NTFS_STATE: &str = "TUR_NTFS_STATE";
/// Property carrying the HFS+ journal state.
pub const PROP_HFS_JOURNAL: &str = "TUR_HFS_JOURNAL";
/// blkid property carrying the per-device identifier of a Btrfs member.
pub const PROP_UUID_SUB: &str = "UUID_SUB";

/// A source of volume facts.
pub trait Probe {
    /// The block-device tree, as `lsblk --json --bytes --output-all` reports it.
    fn block_devices(&self) -> Result<Vec<LsblkDevice>, ProbeError>;

    /// `blkid`-style properties for a device path such as `/dev/sda1`. An
    /// unknown device has no properties.
    fn properties(&self, device: &str) -> BTreeMap<String, String>;
}

/// Why probe input could not be used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProbeError {
    /// The program the live probe runs could not be run, failed, or printed
    /// something that cannot be used.
    Command(String),
    /// The lsblk document is not valid JSON or does not have the expected shape.
    Lsblk(String),
    /// The property text is malformed.
    Properties {
        /// One-based line number.
        line: usize,
        /// What is wrong with the line.
        message: String,
    },
}

impl fmt::Display for ProbeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProbeError::Command(message) => write!(f, "live probe: {message}"),
            ProbeError::Lsblk(message) => write!(f, "lsblk JSON: {message}"),
            ProbeError::Properties { line, message } => {
                write!(f, "property file line {line}: {message}")
            }
        }
    }
}

impl std::error::Error for ProbeError {}

/// One block device as `lsblk` reports it. Only the columns the policy core
/// uses are kept; every other column is ignored.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct LsblkDevice {
    /// Device name, for example `sda1` or `luks-…`.
    #[serde(default)]
    pub name: Option<String>,
    /// Kernel name, for example `dm-0`.
    #[serde(default)]
    pub kname: Option<String>,
    /// Device path, for example `/dev/sda1`.
    #[serde(default)]
    pub path: Option<String>,
    /// Device type: `disk`, `part`, `crypt`, `lvm`, `rom`, `loop`, `raid1`, ….
    #[serde(default, rename = "type")]
    pub device_type: Option<String>,
    /// Filesystem or container signature (blkid `TYPE`).
    #[serde(default)]
    pub fstype: Option<String>,
    /// Filesystem version, for example `FAT32` or `2` for LUKS2.
    #[serde(default)]
    pub fsver: Option<String>,
    /// Filesystem label.
    #[serde(default)]
    pub label: Option<String>,
    /// Filesystem UUID.
    #[serde(default)]
    pub uuid: Option<String>,
    /// Partition UUID.
    #[serde(default)]
    pub partuuid: Option<String>,
    /// Partition type code or GUID.
    #[serde(default)]
    pub parttype: Option<String>,
    /// Partition type name.
    #[serde(default)]
    pub parttypename: Option<String>,
    /// Size in bytes.
    #[serde(default, deserialize_with = "flexible_u64")]
    pub size: Option<u64>,
    /// Start offset in sectors.
    #[serde(default, deserialize_with = "flexible_u64")]
    pub start: Option<u64>,
    /// Removable-media flag.
    #[serde(default, deserialize_with = "flexible_bool")]
    pub rm: Option<bool>,
    /// Hot-pluggable flag.
    #[serde(default, deserialize_with = "flexible_bool")]
    pub hotplug: Option<bool>,
    /// Read-only flag.
    #[serde(default, deserialize_with = "flexible_bool")]
    pub ro: Option<bool>,
    /// Transport, for example `usb`, `nvme` or `sata`.
    #[serde(default)]
    pub tran: Option<String>,
    /// Hardware serial number.
    #[serde(default)]
    pub serial: Option<String>,
    /// World-wide name.
    #[serde(default)]
    pub wwn: Option<String>,
    /// Hardware model.
    #[serde(default)]
    pub model: Option<String>,
    /// Every mount point of the device.
    #[serde(default, deserialize_with = "string_list")]
    pub mountpoints: Vec<String>,
    /// The single mount point reported by older `lsblk` versions.
    #[serde(default)]
    pub mountpoint: Option<String>,
    /// Filesystem roots of the mounts, which for Btrfs are subvolume paths.
    #[serde(default, deserialize_with = "string_list")]
    pub fsroots: Vec<String>,
    /// Devices stacked on this one: partitions, mappings, logical volumes.
    #[serde(default)]
    pub children: Vec<LsblkDevice>,
}

#[derive(Debug, Deserialize)]
struct LsblkDocument {
    blockdevices: Vec<LsblkDevice>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum FlexibleNumber {
    Number(u64),
    Text(String),
}

#[derive(Deserialize)]
#[serde(untagged)]
enum FlexibleBool {
    Bool(bool),
    Number(u64),
    Text(String),
}

/// Accepts a JSON number, a decimal string or null. `lsblk` prints byte sizes
/// as numbers only with `--bytes`; a human-readable size such as `"465.8G"`
/// is rejected rather than guessed.
fn flexible_u64<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<u64>, D::Error> {
    match Option::<FlexibleNumber>::deserialize(deserializer)? {
        None => Ok(None),
        Some(FlexibleNumber::Number(number)) => Ok(Some(number)),
        Some(FlexibleNumber::Text(text)) => {
            let trimmed = text.trim();
            if trimmed.is_empty() {
                return Ok(None);
            }
            trimmed.parse::<u64>().map(Some).map_err(|_| {
                serde::de::Error::custom(format!(
                    "\"{trimmed}\" is not a whole number; run lsblk with --bytes"
                ))
            })
        }
    }
}

/// Accepts `true`/`false`, `0`/`1` as number or string, or null. Older
/// `lsblk` versions print flags as strings.
fn flexible_bool<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<bool>, D::Error> {
    match Option::<FlexibleBool>::deserialize(deserializer)? {
        None => Ok(None),
        Some(FlexibleBool::Bool(value)) => Ok(Some(value)),
        Some(FlexibleBool::Number(0)) => Ok(Some(false)),
        Some(FlexibleBool::Number(1)) => Ok(Some(true)),
        Some(FlexibleBool::Number(other)) => Err(serde::de::Error::custom(format!(
            "{other} is not a flag value"
        ))),
        Some(FlexibleBool::Text(text)) => match text.trim() {
            "" => Ok(None),
            "0" | "false" => Ok(Some(false)),
            "1" | "true" => Ok(Some(true)),
            other => Err(serde::de::Error::custom(format!(
                "\"{other}\" is not a flag value"
            ))),
        },
    }
}

/// Accepts an array of strings and nulls, or null. `lsblk` prints `[null]`
/// for a device that is not mounted.
fn string_list<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<String>, D::Error> {
    let raw = Option::<Vec<Option<String>>>::deserialize(deserializer)?;
    Ok(raw.unwrap_or_default().into_iter().flatten().collect())
}

/// Parses the JSON printed by `lsblk --json --bytes --output-all`.
pub fn parse_lsblk_json(json: &str) -> Result<Vec<LsblkDevice>, ProbeError> {
    serde_json::from_str::<LsblkDocument>(json)
        .map(|document| document.blockdevices)
        .map_err(|error| ProbeError::Lsblk(error.to_string()))
}

/// Parses `blkid --output export` text: blocks of `KEY=value` lines separated
/// by blank lines, each block starting with `DEVNAME=<device path>`.
///
/// Lines starting with `#` are comments. Values are taken literally; blkid's
/// escaping of unsafe characters is not undone, so labels are read from the
/// lsblk JSON, never from here.
pub fn parse_properties(
    text: &str,
) -> Result<BTreeMap<String, BTreeMap<String, String>>, ProbeError> {
    let mut devices: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
    let mut current: Option<String> = None;
    for (index, raw_line) in text.lines().enumerate() {
        let line = raw_line.trim();
        if line.is_empty() {
            current = None;
            continue;
        }
        if line.starts_with('#') {
            continue;
        }
        let malformed = |message: &str| ProbeError::Properties {
            line: index + 1,
            message: message.to_owned(),
        };
        let (key, value) = line
            .split_once('=')
            .ok_or_else(|| malformed("expected KEY=value"))?;
        let key = key.trim();
        if key.is_empty() {
            return Err(malformed("empty key"));
        }
        if key == "DEVNAME" {
            if value.is_empty() {
                return Err(malformed("empty DEVNAME"));
            }
            devices.entry(value.to_owned()).or_default();
            current = Some(value.to_owned());
            continue;
        }
        let Some(device) = current.as_ref() else {
            return Err(malformed("property before DEVNAME"));
        };
        devices
            .entry(device.clone())
            .or_default()
            .insert(key.to_owned(), value.to_owned());
    }
    Ok(devices)
}

/// A probe that answers from text captured earlier.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SnapshotProbe {
    devices: Vec<LsblkDevice>,
    properties: BTreeMap<String, BTreeMap<String, String>>,
}

impl SnapshotProbe {
    /// Builds a probe from `lsblk --json --bytes --output-all` output.
    pub fn from_lsblk_json(json: &str) -> Result<SnapshotProbe, ProbeError> {
        Ok(SnapshotProbe {
            devices: parse_lsblk_json(json)?,
            properties: BTreeMap::new(),
        })
    }

    /// Adds `blkid --output export` style properties. Later calls add to and
    /// replace earlier properties for the same device.
    pub fn with_properties(mut self, text: &str) -> Result<SnapshotProbe, ProbeError> {
        for (device, properties) in parse_properties(text)? {
            self.properties
                .entry(device)
                .or_default()
                .extend(properties);
        }
        Ok(self)
    }
}

impl Probe for SnapshotProbe {
    fn block_devices(&self) -> Result<Vec<LsblkDevice>, ProbeError> {
        Ok(self.devices.clone())
    }

    fn properties(&self, device: &str) -> BTreeMap<String, String> {
        self.properties.get(device).cloned().unwrap_or_default()
    }
}

/// What a program printed and how it ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandOutput {
    /// The exit status; `None` if a signal ended the program.
    pub status: Option<i32>,
    /// What it wrote to standard output.
    pub stdout: Vec<u8>,
    /// What it wrote to standard error.
    pub stderr: Vec<u8>,
}

/// Runs a program and waits for it. The live probe is handed one, so that a
/// test can stand in for the machine and can see exactly what would be run.
pub trait CommandRunner {
    /// Run `program` with `arguments`. No shell is involved.
    ///
    /// # Errors
    ///
    /// The program could not be started.
    fn run(&self, program: &str, arguments: &[&str]) -> Result<CommandOutput, String>;
}

/// The runner for the real machine. It passes nothing of its own
/// surroundings on: the program gets no standard input and an environment
/// of one fixed variable.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemRunner;

impl CommandRunner for SystemRunner {
    fn run(&self, program: &str, arguments: &[&str]) -> Result<CommandOutput, String> {
        let output = std::process::Command::new(program)
            .args(arguments)
            .stdin(std::process::Stdio::null())
            .env_clear()
            .env("LC_ALL", "C.UTF-8")
            .output()
            .map_err(|error| error.to_string())?;
        Ok(CommandOutput {
            status: output.status.code(),
            stdout: output.stdout,
            stderr: output.stderr,
        })
    }
}

/// The one program the live probe runs: a full path, never looked up.
pub const LSBLK_PROGRAM: &str = "/usr/bin/lsblk";

/// Its arguments. `lsblk` with these lists block devices and changes
/// nothing.
pub const LSBLK_ARGUMENTS: [&str; 3] = ["--json", "--bytes", "--output-all"];

/// The most output the live probe accepts, in bytes.
pub const LSBLK_OUTPUT_MAX: usize = 16 * 1024 * 1024;

/// A probe of the machine it runs on.
///
/// The block-device tree is captured once, when the probe is made, by
/// running [`LSBLK_PROGRAM`] with [`LSBLK_ARGUMENTS`]. Nothing else is run
/// and nothing is written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveProbe {
    captured: SnapshotProbe,
}

impl LiveProbe {
    /// Capture the block devices of this machine through `runner`.
    ///
    /// # Errors
    ///
    /// `lsblk` could not be run, did not exit with 0, printed more than
    /// [`LSBLK_OUTPUT_MAX`] bytes, or printed something that is not the
    /// document it is asked for.
    pub fn capture(runner: &dyn CommandRunner) -> Result<LiveProbe, ProbeError> {
        let output = runner
            .run(LSBLK_PROGRAM, &LSBLK_ARGUMENTS)
            .map_err(|error| {
                ProbeError::Command(format!(
                    "{LSBLK_PROGRAM} could not be run: {}",
                    one_line(&error)
                ))
            })?;
        if output.status != Some(0) {
            let status = output
                .status
                .map_or_else(|| "a signal".to_owned(), |code| format!("status {code}"));
            let said = one_line(&String::from_utf8_lossy(&output.stderr));
            return Err(ProbeError::Command(format!(
                "{LSBLK_PROGRAM} ended with {status}: {said}"
            )));
        }
        if output.stdout.len() > LSBLK_OUTPUT_MAX {
            return Err(ProbeError::Command(format!(
                "{LSBLK_PROGRAM} printed more than {LSBLK_OUTPUT_MAX} bytes"
            )));
        }
        let text = String::from_utf8(output.stdout).map_err(|_| {
            ProbeError::Command(format!("{LSBLK_PROGRAM} printed text that is not UTF-8"))
        })?;
        Ok(LiveProbe {
            captured: SnapshotProbe::from_lsblk_json(&text)?,
        })
    }

    /// Adds `blkid --output export` style properties from a file the caller
    /// has, as [`SnapshotProbe::with_properties`] does. The live probe
    /// collects none itself.
    pub fn with_properties(self, text: &str) -> Result<LiveProbe, ProbeError> {
        Ok(LiveProbe {
            captured: self.captured.with_properties(text)?,
        })
    }
}

impl Probe for LiveProbe {
    fn block_devices(&self) -> Result<Vec<LsblkDevice>, ProbeError> {
        self.captured.block_devices()
    }

    fn properties(&self, device: &str) -> BTreeMap<String, String> {
        self.captured.properties(device)
    }
}

/// The first line of a message, without control characters and at most 200
/// characters long: what a program said, made safe to repeat.
fn one_line(text: &str) -> String {
    text.lines()
        .next()
        .unwrap_or("")
        .chars()
        .filter(|character| !character.is_control())
        .take(200)
        .collect()
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;

    /// A runner that runs nothing: it records what it was asked to run and
    /// answers with a prepared result.
    struct FakeRunner {
        asked: RefCell<Vec<(String, Vec<String>)>>,
        answer: Result<CommandOutput, String>,
    }

    impl FakeRunner {
        fn printing(stdout: &str) -> FakeRunner {
            FakeRunner::ending(Some(0), stdout.as_bytes(), b"")
        }

        fn ending(status: Option<i32>, stdout: &[u8], stderr: &[u8]) -> FakeRunner {
            FakeRunner {
                asked: RefCell::new(Vec::new()),
                answer: Ok(CommandOutput {
                    status,
                    stdout: stdout.to_vec(),
                    stderr: stderr.to_vec(),
                }),
            }
        }

        fn failing(message: &str) -> FakeRunner {
            FakeRunner {
                asked: RefCell::new(Vec::new()),
                answer: Err(message.to_owned()),
            }
        }
    }

    impl CommandRunner for FakeRunner {
        fn run(&self, program: &str, arguments: &[&str]) -> Result<CommandOutput, String> {
            self.asked.borrow_mut().push((
                program.to_owned(),
                arguments.iter().map(|argument| (*argument).to_owned()).collect(),
            ));
            self.answer.clone()
        }
    }

    const ONE_DISK: &str = r#"{"blockdevices":[
        {"name":"sda","path":"/dev/sda","type":"disk","size":1000,"rm":false,
         "children":[{"name":"sda1","path":"/dev/sda1","type":"part","size":900,
                      "fstype":"ext4","uuid":"0a0a0a0a-1111-4bbb-8ccc-000000000a01"}]}
    ]}"#;

    #[test]
    fn the_live_probe_runs_lsblk_and_nothing_else() {
        let runner = FakeRunner::printing(ONE_DISK);
        let probe = LiveProbe::capture(&runner).expect("a usable document");
        // One program was run, once: the one that lists block devices, by
        // its full path, with the arguments that make it print the document
        // and nothing that makes it change anything.
        let asked = runner.asked.borrow();
        assert_eq!(asked.len(), 1);
        assert_eq!(asked[0].0, "/usr/bin/lsblk");
        assert_eq!(asked[0].1, ["--json", "--bytes", "--output-all"]);
        // Reading the probe again runs nothing more.
        let devices = probe.block_devices().expect("devices");
        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].children[0].fstype.as_deref(), Some("ext4"));
        let _ = probe.block_devices();
        let _ = probe.properties("/dev/sda1");
        assert_eq!(asked.len(), 1);
        // It is the document a snapshot of the same text gives.
        let snapshot = SnapshotProbe::from_lsblk_json(ONE_DISK).expect("a snapshot");
        assert_eq!(probe.block_devices(), snapshot.block_devices());
        // The live probe has no properties of its own.
        assert!(probe.properties("/dev/sda1").is_empty());
        // The constants are the whole command line: nothing in them writes.
        assert_eq!(LSBLK_PROGRAM, "/usr/bin/lsblk");
        for argument in LSBLK_ARGUMENTS {
            assert!(argument.starts_with("--"), "{argument}");
            assert!(!argument.contains('='), "{argument}");
        }
    }

    #[test]
    fn properties_from_a_file_can_be_added_to_the_live_probe() {
        let runner = FakeRunner::printing(ONE_DISK);
        let probe = LiveProbe::capture(&runner)
            .expect("a usable document")
            .with_properties("DEVNAME=/dev/sda1\nTUR_NTFS_STATE=clean\n")
            .expect("valid properties");
        assert_eq!(
            probe.properties("/dev/sda1").get(PROP_NTFS_STATE).map(String::as_str),
            Some("clean")
        );
        assert!(
            LiveProbe::capture(&runner)
                .expect("a usable document")
                .with_properties("no equals sign")
                .is_err()
        );
    }

    #[test]
    fn a_live_probe_that_cannot_be_trusted_is_an_error() {
        let command = |runner: &FakeRunner| match LiveProbe::capture(runner) {
            Err(ProbeError::Command(message)) => message,
            other => panic!("not a command error: {other:?}"),
        };
        // The program is not there.
        let message = command(&FakeRunner::failing("No such file or directory"));
        assert!(message.contains("/usr/bin/lsblk could not be run"), "{message}");
        // It failed, and what it said is repeated on one line without
        // control characters.
        let message = command(&FakeRunner::ending(
            Some(32),
            b"",
            b"lsblk: \x1b[31mfailed\x07 to access sysfs\nsecond line\n",
        ));
        assert!(message.contains("status 32"), "{message}");
        assert!(message.contains("[31mfailed to access sysfs"), "{message}");
        assert!(!message.contains('\u{1b}') && !message.contains("second line"));
        // A signal ended it.
        let message = command(&FakeRunner::ending(None, b"", b""));
        assert!(message.contains("a signal"), "{message}");
        // It printed something that is not text.
        let message = command(&FakeRunner::ending(Some(0), b"\xff\xfe", b""));
        assert!(message.contains("not UTF-8"), "{message}");
        // Output from a failed run is not used, however good it looks.
        let message = command(&FakeRunner::ending(Some(1), ONE_DISK.as_bytes(), b""));
        assert!(message.contains("status 1"), "{message}");

        // It succeeded and printed something that is not the document.
        for text in ["", "not json", "{}", r#"{"blockdevices": 3}"#] {
            assert!(
                matches!(
                    LiveProbe::capture(&FakeRunner::printing(text)),
                    Err(ProbeError::Lsblk(_))
                ),
                "{text:?}"
            );
        }
        assert_eq!(
            ProbeError::Command("x".to_owned()).to_string(),
            "live probe: x"
        );
    }

    #[test]
    fn parses_numbers_flags_and_null_lists() {
        let json = r#"{"blockdevices":[
            {"name":"sda","type":"disk","size":1000,"rm":true,"ro":false,"hotplug":"1",
             "mountpoints":[null],"fsroots":[null],
             "children":[{"name":"sda1","path":"/dev/sda1","type":"part","size":"900",
                          "fstype":"ext4","rm":"0","mountpoints":["/mnt/a",null,"/mnt/b"]}]}
        ]}"#;
        let devices = parse_lsblk_json(json).unwrap();
        assert_eq!(devices.len(), 1);
        let disk = &devices[0];
        assert_eq!(disk.size, Some(1000));
        assert_eq!(disk.rm, Some(true));
        assert_eq!(disk.ro, Some(false));
        assert_eq!(disk.hotplug, Some(true));
        assert!(disk.mountpoints.is_empty());
        assert!(disk.fsroots.is_empty());
        let part = &disk.children[0];
        assert_eq!(part.size, Some(900));
        assert_eq!(part.rm, Some(false));
        assert_eq!(part.device_type.as_deref(), Some("part"));
        assert_eq!(
            part.mountpoints,
            vec!["/mnt/a".to_owned(), "/mnt/b".to_owned()]
        );
    }

    #[test]
    fn missing_columns_default_to_absent() {
        let devices = parse_lsblk_json(r#"{"blockdevices":[{"name":"sdb"}]}"#).unwrap();
        let disk = &devices[0];
        assert_eq!(disk.size, None);
        assert_eq!(disk.rm, None);
        assert!(disk.children.is_empty());
        assert!(disk.mountpoints.is_empty());
    }

    #[test]
    fn unknown_columns_are_ignored() {
        let json =
            r#"{"blockdevices":[{"name":"sdb","maj:min":"8:16","fsuse%":null,"zoned":"none"}]}"#;
        assert_eq!(parse_lsblk_json(json).unwrap().len(), 1);
    }

    #[test]
    fn human_readable_sizes_are_rejected() {
        let error =
            parse_lsblk_json(r#"{"blockdevices":[{"name":"sdb","size":"465.8G"}]}"#).unwrap_err();
        assert!(error.to_string().contains("--bytes"), "{error}");
    }

    #[test]
    fn malformed_documents_are_rejected() {
        assert!(matches!(
            parse_lsblk_json("not json"),
            Err(ProbeError::Lsblk(_))
        ));
        assert!(matches!(parse_lsblk_json("{}"), Err(ProbeError::Lsblk(_))));
        assert!(matches!(
            parse_lsblk_json(r#"{"blockdevices":{}}"#),
            Err(ProbeError::Lsblk(_))
        ));
        assert!(matches!(
            parse_lsblk_json(r#"{"blockdevices":[{"name":"sdb","rm":"maybe"}]}"#),
            Err(ProbeError::Lsblk(_))
        ));
    }

    #[test]
    fn parses_property_blocks() {
        let text = "# captured by hand\nDEVNAME=/dev/sda1\nUUID=AAAA\nTUR_NTFS_STATE=clean\n\n\
                    DEVNAME=/dev/sdb1\nUUID_SUB=1234\n";
        let devices = parse_properties(text).unwrap();
        assert_eq!(devices.len(), 2);
        assert_eq!(devices["/dev/sda1"]["TUR_NTFS_STATE"], "clean");
        assert_eq!(devices["/dev/sda1"]["UUID"], "AAAA");
        assert_eq!(devices["/dev/sdb1"]["UUID_SUB"], "1234");
    }

    #[test]
    fn property_errors_carry_the_line_number() {
        assert_eq!(
            parse_properties("UUID=AAAA\n"),
            Err(ProbeError::Properties {
                line: 1,
                message: "property before DEVNAME".to_owned()
            })
        );
        assert_eq!(
            parse_properties("DEVNAME=/dev/sda1\nnonsense\n"),
            Err(ProbeError::Properties {
                line: 2,
                message: "expected KEY=value".to_owned()
            })
        );
        assert_eq!(
            parse_properties("DEVNAME=/dev/sda1\n\nUUID=AAAA\n"),
            Err(ProbeError::Properties {
                line: 3,
                message: "property before DEVNAME".to_owned()
            })
        );
    }

    #[test]
    fn snapshot_probe_returns_properties_per_device() {
        let probe = SnapshotProbe::from_lsblk_json(r#"{"blockdevices":[]}"#)
            .unwrap()
            .with_properties("DEVNAME=/dev/sda1\nTUR_NTFS_STATE=dirty\n")
            .unwrap()
            .with_properties("DEVNAME=/dev/sda1\nTUR_NTFS_STATE=clean\n")
            .unwrap();
        assert_eq!(probe.properties("/dev/sda1")[PROP_NTFS_STATE], "clean");
        assert!(probe.properties("/dev/sdz9").is_empty());
        assert!(probe.block_devices().unwrap().is_empty());
    }
}
