// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! What this machine is running, for `tur-diag status` and the manifest, and
//! which names identify it and its users, for the redactor.
//!
//! Everything is read through [`Host`]. A host that is not TUR has no
//! `/usr/lib/tur/release`; that is an ordinary answer, not an error.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::time::Duration;

use serde::Serialize;
use serde_json::Value;

use crate::host::{Host, Limits};
use crate::manifest::TurIdentity;
use crate::redact::KnownIdentifiers;

/// The file that identifies a TUR build, in shell `KEY=VALUE` form.
pub const RELEASE_FILE: &str = "/usr/lib/tur/release";
const OS_RELEASE_FILE: &str = "/etc/os-release";
const KERNEL_RELEASE_FILE: &str = "/proc/sys/kernel/osrelease";
const BOOT_ID_FILE: &str = "/proc/sys/kernel/random/boot_id";
const HOSTNAME_FILES: [&str; 2] = ["/proc/sys/kernel/hostname", "/etc/hostname"];
const MACHINE_ID_FILE: &str = "/etc/machine-id";
const PASSWD_FILE: &str = "/etc/passwd";
const PROCESS_STATUS_FILE: &str = "/proc/self/status";

/// Where TUR's healing service writes its state, with the number of failed
/// units it counted as root (docs/architecture/self-healing.md, section 6).
/// It lies under `/var/log`, which P@x's read-only sandbox shows.
pub const HEAL_STATE_FILE: &str = "/var/log/tur/heal/state.json";

const SMALL_FILE_BYTES: usize = 64 * 1024;
const PASSWD_BYTES: usize = 4 * 1024 * 1024;
const PROBE_LIMITS: Limits = Limits {
    timeout: Duration::from_secs(10),
    max_bytes: 1024 * 1024,
};

/// The UID range of ordinary accounts. System accounts and `nobody` are the
/// same on every machine and identify no one.
const ORDINARY_UIDS: std::ops::Range<u32> = 1000..60_000;

/// Parses shell-style `KEY=VALUE` lines, as in `os-release`.
///
/// Blank lines, comments and lines without `=` are ignored. Single or double
/// quotes around a value are removed, and backslash escapes inside double
/// quotes are resolved. No variable expansion is done.
pub fn parse_key_values(text: &str) -> BTreeMap<String, String> {
    let mut values = BTreeMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        let valid_key = !key.is_empty()
            && key
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_');
        if valid_key {
            values.insert(key.to_owned(), unquote(value.trim()));
        }
    }
    values
}

fn unquote(value: &str) -> String {
    if value.len() >= 2 && value.starts_with('\'') && value.ends_with('\'') {
        return value[1..value.len() - 1].to_owned();
    }
    if value.len() >= 2 && value.starts_with('"') && value.ends_with('"') {
        let inner = &value[1..value.len() - 1];
        let mut out = String::with_capacity(inner.len());
        let mut characters = inner.chars();
        while let Some(character) = characters.next() {
            if character == '\\' {
                if let Some(escaped) = characters.next() {
                    out.push(escaped);
                }
            } else {
                out.push(character);
            }
        }
        return out;
    }
    value.to_owned()
}

/// What the machine is running.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Identity {
    /// The TUR build and deployment. All `null` on a host that is not TUR.
    pub tur: TurIdentity,
    /// `PRETTY_NAME` from `os-release`.
    pub os_name: Option<String>,
    /// The running kernel's release.
    pub kernel_release: Option<String>,
    /// The architecture this program was built for.
    pub architecture: String,
    /// The kernel's ID for the current boot.
    pub boot_id: Option<String>,
}

/// The answer to `tur-diag status`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Status {
    /// What the machine is running.
    #[serde(flatten)]
    pub identity: Identity,
    /// How many systemd units are in the failed state, or `null` when
    /// neither `systemctl` nor the healing service's last count could be
    /// read.
    pub failed_units: Option<u64>,
    /// `null` when `failed_units` was counted now; otherwise when TUR's
    /// healing service counted it (UTC). Inside P@x's sandbox `systemctl`
    /// cannot reach the service manager, and the service's count is used.
    pub failed_units_as_of: Option<String>,
}

/// Whether this process is known to run without administrator rights: its
/// effective UID, from `/proc/self/status`, is not 0. When that cannot be
/// read, it is not known.
pub fn known_not_root(host: &dyn Host) -> bool {
    host.read_file(PROCESS_STATUS_FILE, SMALL_FILE_BYTES)
        .ok()
        .and_then(|content| {
            String::from_utf8_lossy(&content.data).lines().find_map(|line| {
                let ids = line.strip_prefix("Uid:")?;
                ids.split_whitespace().nth(1)?.parse::<u32>().ok()
            })
        })
        .is_some_and(|uid| uid != 0)
}

fn read_trimmed(host: &dyn Host, path: &str) -> Option<String> {
    let content = host.read_file(path, SMALL_FILE_BYTES).ok()?;
    let text = String::from_utf8_lossy(&content.data).trim().to_owned();
    (!text.is_empty()).then_some(text)
}

/// The booted deployment's ostree checksum and image digest, from `bootc`.
///
/// `bootc` is started only as root. It needs root to answer, and its SELinux
/// type has a domain of its own (`install_t`): started from a process that
/// may gain no new privileges, as in P@x's sandbox, the change of domain is
/// refused and recorded as a denial on every call (issue #57).
fn deployment(host: &dyn Host) -> (Option<String>, Option<String>) {
    if known_not_root(host) {
        return (None, None);
    }
    let Ok(output) = host.run("bootc", &["status", "--format=json"], PROBE_LIMITS) else {
        return (None, None);
    };
    if !output.success() {
        return (None, None);
    }
    let Ok(status) = serde_json::from_slice::<Value>(&output.stdout) else {
        return (None, None);
    };
    let text_at = |pointer: &str| {
        status
            .pointer(pointer)
            .and_then(Value::as_str)
            .filter(|text| !text.is_empty())
            .map(str::to_owned)
    };
    (
        text_at("/status/booted/ostree/checksum"),
        text_at("/status/booted/image/imageDigest"),
    )
}

/// Reads what the machine is running.
pub fn gather_identity(host: &dyn Host) -> Identity {
    let release = read_trimmed(host, RELEASE_FILE)
        .map(|text| parse_key_values(&text))
        .unwrap_or_default();
    let os_release = read_trimmed(host, OS_RELEASE_FILE)
        .map(|text| parse_key_values(&text))
        .unwrap_or_default();
    let non_empty = |map: &BTreeMap<String, String>, key: &str| {
        map.get(key).filter(|value| !value.is_empty()).cloned()
    };
    let (deployment_checksum, image_digest) = deployment(host);
    Identity {
        tur: TurIdentity {
            build_id: non_empty(&release, "TUR_BUILD_ID"),
            version: non_empty(&release, "TUR_VERSION"),
            source_revision: non_empty(&release, "TUR_SOURCE_REVISION"),
            deployment_checksum,
            image_digest,
        },
        os_name: non_empty(&os_release, "PRETTY_NAME"),
        kernel_release: read_trimmed(host, KERNEL_RELEASE_FILE),
        architecture: std::env::consts::ARCH.to_owned(),
        boot_id: read_trimmed(host, BOOT_ID_FILE),
    }
}

/// The number of failed units TUR's healing service counted last, and when.
fn failed_units_of_the_healing_service(host: &dyn Host) -> Option<(u64, String)> {
    let content = host.read_file(HEAL_STATE_FILE, PASSWD_BYTES).ok()?;
    let state: Value = serde_json::from_slice(&content.data).ok()?;
    let count = state.get("failed_units")?.as_u64()?;
    let when = state.get("failed_units_utc")?.as_str()?.to_owned();
    Some((count, when))
}

/// Counts the systemd units in the failed state.
fn failed_units(host: &dyn Host) -> Option<u64> {
    let args = [
        "list-units",
        "--state=failed",
        "--all",
        "--no-legend",
        "--plain",
        "--no-pager",
    ];
    let output = host.run("systemctl", &args, PROBE_LIMITS).ok()?;
    if !output.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let count = text.lines().filter(|line| !line.trim().is_empty()).count();
    u64::try_from(count).ok()
}

/// Gathers everything `tur-diag status` shows.
pub fn gather_status(host: &dyn Host) -> Status {
    let (failed_units, failed_units_as_of) = match failed_units(host) {
        Some(count) => (Some(count), None),
        None => match failed_units_of_the_healing_service(host) {
            Some((count, when)) => (Some(count), Some(when)),
            None => (None, None),
        },
    };
    Status {
        identity: gather_identity(host),
        failed_units,
        failed_units_as_of,
    }
}

/// Formats a status for a person: one screen, one fact per line.
pub fn render_status(status: &Status) -> String {
    const UNKNOWN: &str = "unknown";
    let identity = &status.identity;
    let tur = &identity.tur;
    let mut out = String::new();

    let build = match (&tur.build_id, &tur.version) {
        (Some(build_id), Some(version)) => format!("{build_id} (version {version})"),
        (Some(build_id), None) => build_id.clone(),
        (None, _) => format!("not a TUR installation ({RELEASE_FILE} is absent)"),
    };
    let kernel = identity.kernel_release.as_deref().unwrap_or(UNKNOWN);
    let architecture = &identity.architecture;
    let failed = match (status.failed_units, &status.failed_units_as_of) {
        (Some(count), None) => count.to_string(),
        (Some(count), Some(when)) => format!("{count} (counted by TUR's healing service, {when})"),
        (None, _) => "unknown (systemctl could not be asked)".to_owned(),
    };
    let rows = [
        ("TUR build", build),
        (
            "Source revision",
            tur.source_revision
                .clone()
                .unwrap_or_else(|| UNKNOWN.to_owned()),
        ),
        (
            "Deployment",
            tur.deployment_checksum
                .clone()
                .unwrap_or_else(|| UNKNOWN.to_owned()),
        ),
        (
            "System",
            identity
                .os_name
                .clone()
                .unwrap_or_else(|| UNKNOWN.to_owned()),
        ),
        ("Kernel", format!("{kernel} ({architecture})")),
        (
            "Boot ID",
            identity
                .boot_id
                .clone()
                .unwrap_or_else(|| UNKNOWN.to_owned()),
        ),
        ("Failed units", failed),
    ];
    for (label, value) in rows {
        // Writing to a String cannot fail.
        let _ = writeln!(out, "{label:<16} {value}");
    }
    out
}

fn push_unique(list: &mut Vec<String>, value: &str) {
    let value = value.trim();
    if !value.is_empty() && !list.iter().any(|known| known == value) {
        list.push(value.to_owned());
    }
}

/// The names of ordinary accounts in a `passwd` file: login name, real name
/// (the first part of the GECOS field) and the last component of the home
/// directory.
pub fn passwd_names(passwd: &str) -> Vec<String> {
    let mut names = Vec::new();
    for line in passwd.lines() {
        let fields: Vec<&str> = line.split(':').collect();
        if fields.len() < 6 {
            continue;
        }
        let Ok(uid) = fields[2].parse::<u32>() else {
            continue;
        };
        if !ORDINARY_UIDS.contains(&uid) {
            continue;
        }
        push_unique(&mut names, fields[0]);
        let real_name = fields[4].split(',').next().unwrap_or("").trim();
        if real_name.len() >= 3 {
            push_unique(&mut names, real_name);
        }
        if let Some(home_name) = fields[5].rsplit('/').next() {
            push_unique(&mut names, home_name);
        }
    }
    names
}

/// Collects the names that identify this machine and its users, so that the
/// redactor can replace them wherever they appear.
///
/// `session_users` are the login names of whoever is running the program
/// (from the environment). `/etc/passwd` is read for the names of ordinary
/// accounts; it is used here and never copied into a bundle.
pub fn gather_known_identifiers(host: &dyn Host, session_users: &[String]) -> KnownIdentifiers {
    let mut known = KnownIdentifiers::default();
    for name in session_users {
        push_unique(&mut known.usernames, name);
    }
    if let Ok(content) = host.read_file(PASSWD_FILE, PASSWD_BYTES) {
        for name in passwd_names(&String::from_utf8_lossy(&content.data)) {
            push_unique(&mut known.usernames, &name);
        }
    }
    for path in HOSTNAME_FILES {
        if let Some(name) = read_trimmed(host, path) {
            push_unique(&mut known.hostnames, &name);
        }
    }
    if let Some(id) = read_trimmed(host, MACHINE_ID_FILE) {
        push_unique(&mut known.machine_ids, &id);
    }
    if let Some(id) = read_trimmed(host, BOOT_ID_FILE) {
        push_unique(&mut known.preserved_ids, &id);
    }
    known
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::FakeHost;

    const RELEASE: &str = "TUR_VERSION=0.1.0\n\
                           TUR_BUILD_ID=20261112.g0123456789ab\n\
                           TUR_SOURCE_REVISION=0123456789abcdef0123456789abcdef01234567\n\
                           TUR_BASE_IMAGE=quay.io/fedora/fedora-kinoite\n";

    fn tur_host() -> FakeHost {
        FakeHost::new()
            .with_file(RELEASE_FILE, RELEASE)
            .with_file(
                "/etc/os-release",
                "NAME=\"Fedora Linux\"\nPRETTY_NAME=\"TUR 0.1.0 (based on Fedora Linux 45)\"\nID=fedora\n",
            )
            .with_file("/proc/sys/kernel/osrelease", "6.17.1-300.fc45.x86_64\n")
            .with_file(
                "/proc/sys/kernel/random/boot_id",
                "0f0e0d0c-0b0a-4909-8807-060504030201\n",
            )
            .with_command(
                "bootc",
                r#"{"apiVersion":"org.containers.bootc/v1","status":{"booted":{"image":{"imageDigest":"sha256:abc"},"ostree":{"checksum":"feedc0de","deploySerial":0}}}}"#,
            )
            .with_command("systemctl", "foo.service loaded failed failed Foo\n\nbar.mount loaded failed failed Bar\n")
    }

    #[test]
    fn key_values_are_parsed_like_a_shell_would() {
        let parsed = parse_key_values(
            "# comment\n\nA=1\nB=\"two words\"\nC='single'\nD=\"say \\\"hi\\\"\"\n  E = spaced \nnot a pair\n=empty\nBAD KEY=x\nF=\n",
        );
        let expected: BTreeMap<String, String> = [
            ("A", "1"),
            ("B", "two words"),
            ("C", "single"),
            ("D", "say \"hi\""),
            ("E", "spaced"),
            ("F", ""),
        ]
        .into_iter()
        .map(|(key, value)| (key.to_owned(), value.to_owned()))
        .collect();
        assert_eq!(parsed, expected);
    }

    #[test]
    fn identity_of_a_tur_host() {
        let identity = gather_identity(&tur_host());
        assert_eq!(
            identity.tur.build_id.as_deref(),
            Some("20261112.g0123456789ab")
        );
        assert_eq!(identity.tur.version.as_deref(), Some("0.1.0"));
        assert_eq!(
            identity.tur.source_revision.as_deref(),
            Some("0123456789abcdef0123456789abcdef01234567")
        );
        assert_eq!(
            identity.tur.deployment_checksum.as_deref(),
            Some("feedc0de")
        );
        assert_eq!(identity.tur.image_digest.as_deref(), Some("sha256:abc"));
        assert_eq!(
            identity.os_name.as_deref(),
            Some("TUR 0.1.0 (based on Fedora Linux 45)")
        );
        assert_eq!(
            identity.kernel_release.as_deref(),
            Some("6.17.1-300.fc45.x86_64")
        );
        assert_eq!(
            identity.boot_id.as_deref(),
            Some("0f0e0d0c-0b0a-4909-8807-060504030201")
        );
        assert_eq!(identity.architecture, std::env::consts::ARCH);
    }

    #[test]
    fn a_host_that_is_not_tur_is_not_an_error() {
        let status = gather_status(&FakeHost::new());
        assert_eq!(status.identity.tur, TurIdentity::default());
        assert_eq!(status.identity.os_name, None);
        assert_eq!(status.identity.kernel_release, None);
        assert_eq!(status.identity.boot_id, None);
        assert_eq!(status.failed_units, None);

        let text = render_status(&status);
        assert!(text.contains("not a TUR installation"), "{text}");
        assert!(text.contains("/usr/lib/tur/release"), "{text}");
        assert!(text.contains("unknown"), "{text}");
    }

    #[test]
    fn status_counts_failed_units_and_fits_one_screen() {
        let status = gather_status(&tur_host());
        assert_eq!(status.failed_units, Some(2));

        let text = render_status(&status);
        assert!(
            text.contains("20261112.g0123456789ab (version 0.1.0)"),
            "{text}"
        );
        assert!(text.contains("6.17.1-300.fc45.x86_64"), "{text}");
        assert!(
            text.contains("0f0e0d0c-0b0a-4909-8807-060504030201"),
            "{text}"
        );
        assert!(text.contains("feedc0de"), "{text}");
        let failed_line = text
            .lines()
            .find(|line| line.starts_with("Failed units"))
            .expect("there is a failed-units line");
        assert!(failed_line.ends_with(" 2"), "{failed_line}");
        assert!(text.lines().count() <= 24);
        assert!(text.lines().all(|line| line.len() <= 100), "{text}");
    }

    #[test]
    fn no_failed_units_is_zero_not_unknown() {
        let host = FakeHost::new().with_command("systemctl", "");
        assert_eq!(gather_status(&host).failed_units, Some(0));
        let broken = FakeHost::new().with_failing_command("systemctl", 1, "no bus");
        assert_eq!(gather_status(&broken).failed_units, None);
    }

    #[test]
    fn bootc_is_not_started_without_administrator_rights() {
        // Issue #57: in P@x's sandbox, with no new privileges, starting
        // bootc is refused by SELinux and recorded as a denial.
        let ordinary = tur_host().with_file("/proc/self/status", "Name:\ttur-diag\nUid:\t1000\t1000\t1000\t1000\n");
        assert!(known_not_root(&ordinary));
        assert_eq!(gather_identity(&ordinary).tur.deployment_checksum, None);
        let root = tur_host().with_file("/proc/self/status", "Uid:\t0\t0\t0\t0\n");
        assert!(!known_not_root(&root));
        assert_eq!(
            gather_identity(&root).tur.deployment_checksum.as_deref(),
            Some("feedc0de")
        );
        // Not known (no /proc here): bootc is asked, as before.
        assert!(!known_not_root(&tur_host()));
    }

    #[test]
    fn without_the_service_manager_the_healing_services_count_is_used() {
        let state = r#"{"failed_units": 3, "failed_units_utc": "2026-11-13T10:00:00Z"}"#;
        let sandboxed = FakeHost::new()
            .with_failing_command("systemctl", 1, "Failed to connect to bus")
            .with_file(HEAL_STATE_FILE, state);
        let status = gather_status(&sandboxed);
        assert_eq!(status.failed_units, Some(3));
        assert_eq!(status.failed_units_as_of.as_deref(), Some("2026-11-13T10:00:00Z"));
        assert!(render_status(&status).contains("counted by TUR's healing service"));
        // Counted now when systemctl answers.
        let live = FakeHost::new().with_command("systemctl", "").with_file(HEAL_STATE_FILE, state);
        let status = gather_status(&live);
        assert_eq!((status.failed_units, status.failed_units_as_of), (Some(0), None));
    }

    #[test]
    fn status_serialises_to_flat_json() {
        let value = serde_json::to_value(gather_status(&tur_host())).expect("status serialises");
        assert_eq!(value["tur"]["build_id"], "20261112.g0123456789ab");
        assert_eq!(value["kernel_release"], "6.17.1-300.fc45.x86_64");
        assert_eq!(value["failed_units"], 2);
        assert_eq!(value["boot_id"], "0f0e0d0c-0b0a-4909-8807-060504030201");

        let absent =
            serde_json::to_value(gather_status(&FakeHost::new())).expect("status serialises");
        assert_eq!(absent["tur"]["build_id"], Value::Null);
        assert_eq!(absent["failed_units"], Value::Null);
    }

    #[test]
    fn unusable_bootc_output_leaves_the_deployment_unknown() {
        for host in [
            FakeHost::new().with_command("bootc", "not json"),
            FakeHost::new().with_command("bootc", r#"{"status":{"booted":null}}"#),
            FakeHost::new().with_failing_command("bootc", 1, "must be root"),
        ] {
            let identity = gather_identity(&host);
            assert_eq!(identity.tur.deployment_checksum, None);
            assert_eq!(identity.tur.image_digest, None);
        }
    }

    #[test]
    fn ordinary_accounts_are_found_in_passwd() {
        let passwd = "root:x:0:0:Super User:/root:/bin/bash\n\
                      daemon:x:2:2:daemon:/sbin:/usr/sbin/nologin\n\
                      alice:x:1000:1000:Alice Example,,,:/var/home/alice:/bin/bash\n\
                      bob:x:1001:1001::/home/robert:/bin/zsh\n\
                      nobody:x:65534:65534:Kernel Overflow User:/:/usr/sbin/nologin\n\
                      broken line\n";
        assert_eq!(
            passwd_names(passwd),
            ["alice", "Alice Example", "bob", "robert"]
        );
    }

    #[test]
    fn known_identifiers_come_from_the_host_and_the_session() {
        let host = FakeHost::new()
            .with_file(
                "/etc/passwd",
                "alice:x:1000:1000:Alice Example:/home/alice:/bin/bash\n",
            )
            .with_file("/proc/sys/kernel/hostname", "workstation-7\n")
            .with_file("/etc/hostname", "workstation-7.lan.example.org\n")
            .with_file("/etc/machine-id", "0123456789abcdef0123456789abcdef\n")
            .with_file(
                "/proc/sys/kernel/random/boot_id",
                "0f0e0d0c-0b0a-4909-8807-060504030201\n",
            );
        let known = gather_known_identifiers(&host, &["carol".to_owned(), "alice".to_owned()]);
        assert_eq!(known.usernames, ["carol", "alice", "Alice Example"]);
        assert_eq!(
            known.hostnames,
            ["workstation-7", "workstation-7.lan.example.org"]
        );
        assert_eq!(known.machine_ids, ["0123456789abcdef0123456789abcdef"]);
        assert_eq!(
            known.preserved_ids,
            ["0f0e0d0c-0b0a-4909-8807-060504030201"]
        );

        let nothing = gather_known_identifiers(&FakeHost::new(), &[]);
        assert_eq!(nothing, KnownIdentifiers::default());
    }
}
