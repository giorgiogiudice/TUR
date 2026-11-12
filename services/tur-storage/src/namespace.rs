// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! Mount paths and runtime visibility mappings (specification §11.4).
//!
//! Each mountable volume has exactly one real mount, at its canonical path
//! `/run/tur/volumes/<volume-id>`. Everything else is a view of that mount:
//!
//! * the user-facing path `/run/media/<user>/<name>` (a bind of the canonical
//!   mount);
//! * a Wine drive letter (a symbolic link in the prefix's `dosdevices`
//!   directory);
//! * `/Volumes/<name>` inside the macOS runtime (a bind in its namespace);
//! * a folder in Android's shared storage (a bind applied only after the user
//!   grants it).
//!
//! [`plan`] computes all of this for an inventory. [`MountPlan::verify`]
//! checks that no filesystem and no device is planned to be mounted twice.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

use crate::decision::{Decision, MountRequest, Outcome, decide};
use crate::policy::PolicyOverrides;
use crate::volume::{Inventory, Volume, VolumeId};

/// Directory that holds the canonical mounts.
pub const CANONICAL_ROOT: &str = "/run/tur/volumes";
/// Directory that holds the per-user, user-facing paths.
pub const USER_MEDIA_ROOT: &str = "/run/media";
/// Directory that holds volumes inside the macOS runtime.
pub const MACOS_VOLUMES_ROOT: &str = "/Volumes";
/// Directory in a Wine prefix that maps drive letters to host paths.
pub const WINE_DOSDEVICES_DIR: &str = "dosdevices";
/// Android's primary shared storage, as applications inside Android see it.
pub const ANDROID_SHARED_ROOT: &str = "/storage/emulated/0";
/// Folder in Android's shared storage that holds granted volumes.
pub const ANDROID_VOLUMES_DIR: &str = "Volumes";
/// Drive letters handed out to volumes, in order. `A` and `B` are left for
/// floppy drives, `C` is the prefix's own system drive and `Z` is Wine's
/// mapping of the host root.
pub const WINE_DRIVE_LETTERS: &str = "DEFGHIJKLMNOPQRSTUVWXY";
/// The longest display name derived from a label, in characters.
pub const MAX_LABEL_CHARS: usize = 48;

/// The canonical mount path of a volume.
pub fn canonical_path(id: &VolumeId) -> String {
    format!("{CANONICAL_ROOT}/{id}")
}

/// Characters removed from labels: control characters and invisible or
/// direction-changing format characters.
fn is_dropped(character: char) -> bool {
    character.is_control()
        || matches!(
            character,
            '\u{00AD}'
                | '\u{200B}'..='\u{200F}'
                | '\u{202A}'..='\u{202E}'
                | '\u{2060}'..='\u{2069}'
                | '\u{FEFF}'
                | '\u{FFF9}'..='\u{FFFB}'
        )
}

/// Characters that separate path components on some runtime: `/` on Linux,
/// macOS and Android, `\` and `:` on Windows, `:` in classic Mac paths.
fn is_separator(character: char) -> bool {
    matches!(character, '/' | '\\' | ':')
}

/// Turns a filesystem label into a name that is safe as one path component
/// on every runtime.
///
/// Control and invisible characters are removed, path separators become `_`,
/// runs of white space become one space, leading dots are removed (so the
/// result is never `.`, `..` or a hidden file) and the result is cut to
/// [`MAX_LABEL_CHARS`]. Returns `None` when nothing usable is left.
pub fn sanitise_label(raw: &str) -> Option<String> {
    let mut cleaned = String::new();
    let mut pending_space = false;
    for character in raw.chars() {
        // White space is tested first: tabs and line breaks are control
        // characters too, and they separate words rather than vanish.
        if character.is_whitespace() {
            pending_space = !cleaned.is_empty();
            continue;
        }
        if is_dropped(character) {
            continue;
        }
        if pending_space {
            cleaned.push(' ');
            pending_space = false;
        }
        cleaned.push(if is_separator(character) {
            '_'
        } else {
            character
        });
    }
    let without_dots = cleaned.trim_start_matches(['.', ' ']);
    let limited: String = without_dots.chars().take(MAX_LABEL_CHARS).collect();
    let name = limited.trim_end();
    let usable = name
        .chars()
        .any(|character| character != '_' && character != '.');
    usable.then(|| name.to_owned())
}

fn base_name(volume: &Volume) -> String {
    volume
        .label
        .as_deref()
        .and_then(sanitise_label)
        .unwrap_or_else(|| volume.id.to_string())
}

/// Gives each volume a display name that is unique among them, ignoring
/// case, because the Windows and macOS runtimes usually do.
///
/// `assigned` holds names given earlier, for example to volumes that are
/// already mounted; a still-valid assigned name is kept so that a path does
/// not change while it is in use. Other volumes take their sanitised label,
/// or their identifier when there is no usable label, and the second and
/// later volumes with an equal name get ` (2)`, ` (3)`, … in identifier
/// order.
pub fn assign_display_names(
    volumes: &[&Volume],
    assigned: &BTreeMap<VolumeId, String>,
) -> BTreeMap<VolumeId, String> {
    let mut names: BTreeMap<VolumeId, String> = BTreeMap::new();
    let mut taken: BTreeSet<String> = BTreeSet::new();
    for &volume in volumes {
        if let Some(name) = assigned.get(&volume.id)
            && sanitise_label(name).as_deref() == Some(name.as_str())
            && taken.insert(name.to_lowercase())
        {
            names.insert(volume.id.clone(), name.clone());
        }
    }
    for &volume in volumes {
        if names.contains_key(&volume.id) {
            continue;
        }
        let base = base_name(volume);
        let mut candidate = base.clone();
        let mut counter: u32 = 2;
        while !taken.insert(candidate.to_lowercase()) {
            candidate = format!("{base} ({counter})");
            counter += 1;
        }
        names.insert(volume.id.clone(), candidate);
    }
    names
}

/// Assigns Wine drive letters from [`WINE_DRIVE_LETTERS`].
///
/// `pinned` holds letters given earlier; a valid, unclaimed pinned letter is
/// kept, because Windows applications store paths that contain it. Other
/// volumes take the first free letter in the order given. A volume gets no
/// letter when none is free.
pub fn assign_drive_letters(
    ids: &[&VolumeId],
    pinned: &BTreeMap<VolumeId, char>,
) -> BTreeMap<VolumeId, char> {
    let mut letters: BTreeMap<VolumeId, char> = BTreeMap::new();
    let mut used: BTreeSet<char> = BTreeSet::new();
    for &id in ids {
        if let Some(letter) = pinned.get(id).map(char::to_ascii_uppercase)
            && WINE_DRIVE_LETTERS.contains(letter)
            && used.insert(letter)
        {
            letters.insert(id.clone(), letter);
        }
    }
    for &id in ids {
        if letters.contains_key(id) {
            continue;
        }
        let free = WINE_DRIVE_LETTERS
            .chars()
            .find(|letter| !used.contains(letter));
        if let Some(letter) = free {
            used.insert(letter);
            letters.insert(id.clone(), letter);
        }
    }
    letters
}

/// A validated login name, safe as one path component.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct UserName(String);

impl UserName {
    /// Validates a login name: 1 to 64 characters from `A-Z a-z 0-9 _ . @ -`,
    /// starting with a letter, a digit or `_`.
    pub fn parse(text: &str) -> Option<UserName> {
        let first_ok = text
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_');
        let rest_ok = text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '@' | '-'));
        (first_ok && rest_ok && text.len() <= 64).then(|| UserName(text.to_owned()))
    }

    /// The name as text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

string_enum! {
    /// An application runtime that sees volumes.
    pub enum Runtime {
        /// Native Linux applications.
        Linux => "linux",
        /// The Windows runtime (Wine).
        Windows => "windows",
        /// The macOS runtime (Darling).
        Macos => "macos",
        /// The Android runtime.
        Android => "android",
    }
}

string_enum! {
    /// How a view refers to the canonical mount. None of these mounts the
    /// filesystem a second time.
    pub enum ViewKind {
        /// A bind mount of the canonical mount.
        Bind => "bind",
        /// A symbolic link to the canonical mount.
        Symlink => "symlink",
        /// A bind mount that is applied only after the user grants it.
        Grant => "grant",
    }
}

/// One runtime's view of a canonical mount.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct View {
    /// The runtime that sees the view.
    pub runtime: Runtime,
    /// How the view refers to the canonical mount.
    pub kind: ViewKind,
    /// Where the view appears. For Linux, an absolute host path. For Windows,
    /// a path relative to the Wine prefix. For macOS and Android, an absolute
    /// path inside that runtime.
    pub path: String,
    /// The canonical mount the view refers to.
    pub target: String,
    /// Whether the view is read-only. A view of a read-only mount always is.
    pub read_only: bool,
    /// Whether the user must grant the view before it exists.
    pub requires_grant: bool,
    /// The Wine drive letter, for the Windows view.
    pub drive_letter: Option<char>,
}

/// The one real mount of a volume.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MountOperation {
    /// The block devices to mount.
    pub sources: Vec<String>,
    /// The canonical path to mount them at.
    pub target: String,
    /// The driver.
    pub driver: Option<&'static str>,
    /// The filesystem type for the mount call, for kernel drivers.
    pub mount_type: Option<&'static str>,
    /// The exact mount options.
    pub options: Vec<String>,
    /// Whether the mount is read-only.
    pub read_only: bool,
}

/// A volume with its decision, its one mount and its views.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PlannedVolume {
    /// The volume facts.
    #[serde(flatten)]
    pub volume: Volume,
    /// The name used in user-facing paths.
    pub display_name: String,
    /// The canonical mount path.
    pub canonical_path: String,
    /// Whether the volume is already mounted at its canonical path.
    pub already_mounted: bool,
    /// The mount decision for the requested access.
    pub decision: Decision,
    /// The mount to perform: present when the decision is a mount and the
    /// volume is not already mounted at its canonical path.
    pub mount: Option<MountOperation>,
    /// The runtime views. Empty unless the decision is a mount.
    pub views: Vec<View>,
}

/// What the caller already knows about names and letters in use.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NamespaceContext {
    /// The user whose `/run/media/<user>` holds the user-facing paths. With
    /// no user, no Linux view is planned.
    pub user: Option<UserName>,
    /// Display names given earlier, to be kept.
    pub assigned_names: BTreeMap<VolumeId, String>,
    /// Wine drive letters given earlier, to be kept.
    pub pinned_letters: BTreeMap<VolumeId, char>,
}

/// The plan for a whole inventory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MountPlan {
    /// Every volume of the inventory, in identifier order.
    pub volumes: Vec<PlannedVolume>,
}

/// Plans the mounts and runtime views for an inventory.
pub fn plan(
    inventory: &Inventory,
    request: &MountRequest,
    policy: &PolicyOverrides,
    context: &NamespaceContext,
) -> MountPlan {
    let decisions: Vec<Decision> = inventory
        .volumes
        .iter()
        .map(|volume| decide(volume, request, policy))
        .collect();
    let mountable: Vec<&Volume> = inventory
        .volumes
        .iter()
        .zip(&decisions)
        .filter(|(_, decision)| decision.is_mount())
        .map(|(volume, _)| volume)
        .collect();
    let names = assign_display_names(&mountable, &context.assigned_names);
    let ids: Vec<&VolumeId> = mountable.iter().map(|volume| &volume.id).collect();
    let letters = assign_drive_letters(&ids, &context.pinned_letters);

    let mut volumes: Vec<PlannedVolume> = Vec::new();
    for (volume, decision) in inventory.volumes.iter().zip(decisions) {
        let canonical = canonical_path(&volume.id);
        let display_name = names
            .get(&volume.id)
            .cloned()
            .unwrap_or_else(|| base_name(volume));
        let already_mounted = volume.mountpoints.contains(&canonical);
        let mut mount = None;
        let mut views = Vec::new();
        if decision.is_mount() {
            let read_only = decision.outcome == Outcome::MountReadOnly;
            if !already_mounted {
                mount = Some(MountOperation {
                    sources: volume.devices.clone(),
                    target: canonical.clone(),
                    driver: decision.driver,
                    mount_type: decision.mount_type,
                    options: decision.options.clone(),
                    read_only,
                });
            }
            let view = |runtime: Runtime,
                        kind: ViewKind,
                        path: String,
                        requires_grant: bool,
                        drive_letter: Option<char>| View {
                runtime,
                kind,
                path,
                target: canonical.clone(),
                read_only,
                requires_grant,
                drive_letter,
            };
            if let Some(user) = &context.user {
                let path = format!("{USER_MEDIA_ROOT}/{}/{display_name}", user.as_str());
                views.push(view(Runtime::Linux, ViewKind::Bind, path, false, None));
            }
            if let Some(&letter) = letters.get(&volume.id) {
                let path = format!("{WINE_DOSDEVICES_DIR}/{}:", letter.to_ascii_lowercase());
                views.push(view(
                    Runtime::Windows,
                    ViewKind::Symlink,
                    path,
                    false,
                    Some(letter),
                ));
            }
            let macos = format!("{MACOS_VOLUMES_ROOT}/{display_name}");
            views.push(view(Runtime::Macos, ViewKind::Bind, macos, false, None));
            let android = format!("{ANDROID_SHARED_ROOT}/{ANDROID_VOLUMES_DIR}/{display_name}");
            views.push(view(Runtime::Android, ViewKind::Grant, android, true, None));
        }
        volumes.push(PlannedVolume {
            volume: volume.clone(),
            display_name,
            canonical_path: canonical,
            already_mounted,
            decision,
            mount,
            views,
        });
    }
    MountPlan { volumes }
}

impl MountPlan {
    /// Finds a planned volume by identifier.
    pub fn find(&self, id: &str) -> Option<&PlannedVolume> {
        self.volumes
            .iter()
            .find(|planned| planned.volume.id.as_str() == id)
    }

    /// Checks the invariants of a plan:
    ///
    /// * every volume appears once and has its own canonical path;
    /// * a filesystem is mounted at most once, at its canonical path, and no
    ///   block device is a source of two mounts;
    /// * a volume already mounted at its canonical path is not mounted again;
    /// * views exist only for mounted volumes, refer to the canonical mount,
    ///   never to a block device, are read-only when the mount is, and no two
    ///   views of one runtime share a path.
    ///
    /// Returns a description of the first violation.
    pub fn verify(&self) -> Result<(), String> {
        let mut ids: BTreeSet<&str> = BTreeSet::new();
        let mut targets: BTreeSet<&str> = BTreeSet::new();
        let mut sources: BTreeSet<&str> = BTreeSet::new();
        let mut view_paths: BTreeSet<(Runtime, &str)> = BTreeSet::new();
        for planned in &self.volumes {
            let id = planned.volume.id.as_str();
            if !ids.insert(id) {
                return Err(format!("volume {id} appears twice"));
            }
            let canonical = canonical_path(&planned.volume.id);
            if planned.canonical_path != canonical {
                return Err(format!(
                    "volume {id} has a canonical path that is not its own"
                ));
            }
            let mounts = planned.decision.is_mount();
            if !mounts && (planned.mount.is_some() || !planned.views.is_empty()) {
                return Err(format!(
                    "volume {id} has a mount or views without a mount decision"
                ));
            }
            if let Some(mount) = &planned.mount {
                if planned.already_mounted {
                    return Err(format!(
                        "volume {id} is already mounted and is planned again"
                    ));
                }
                if mount.target != canonical {
                    return Err(format!("volume {id} is mounted outside its canonical path"));
                }
                if !targets.insert(mount.target.as_str()) {
                    return Err(format!("two mounts share the target {}", mount.target));
                }
                if mount.sources != planned.volume.devices || mount.sources.is_empty() {
                    return Err(format!(
                        "volume {id} is mounted from devices that are not its own"
                    ));
                }
                for source in &mount.sources {
                    if !sources.insert(source.as_str()) {
                        return Err(format!("device {source} is a source of two mounts"));
                    }
                }
                let read_only = planned.decision.outcome == Outcome::MountReadOnly;
                if mount.read_only != read_only || mount.options != planned.decision.options {
                    return Err(format!(
                        "volume {id} is mounted differently from its decision"
                    ));
                }
            }
            for view in &planned.views {
                if view.target != canonical {
                    return Err(format!("a view of volume {id} does not refer to its mount"));
                }
                if view.path.starts_with("/dev/") || view.target.starts_with("/dev/") {
                    return Err(format!("a view of volume {id} refers to a block device"));
                }
                if planned.decision.outcome == Outcome::MountReadOnly && !view.read_only {
                    return Err(format!("a view of read-only volume {id} is writable"));
                }
                if !view_paths.insert((view.runtime, view.path.as_str())) {
                    return Err(format!(
                        "two {} views share the path {}",
                        view.runtime, view.path
                    ));
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decision::Access;
    use crate::filesystem::Filesystem;
    use crate::volume::testing;

    const RW: MountRequest = MountRequest {
        access: Access::ReadWrite,
        uid: 1000,
        gid: 1000,
    };

    fn labelled(filesystem: Filesystem, id: &str, label: Option<&str>) -> Volume {
        let mut volume = testing::volume_with_id(filesystem, id);
        volume.label = label.map(str::to_owned);
        volume.devices = vec![format!("/dev/{id}")];
        volume
    }

    fn inventory(mut volumes: Vec<Volume>) -> Inventory {
        volumes.sort_by(|a, b| a.id.cmp(&b.id));
        Inventory { volumes }
    }

    fn context(user: &str) -> NamespaceContext {
        NamespaceContext {
            user: UserName::parse(user),
            ..NamespaceContext::default()
        }
    }

    #[test]
    fn canonical_path_uses_the_identifier() {
        let id = VolumeId::parse("fs-1234-abcd").unwrap();
        assert_eq!(canonical_path(&id), "/run/tur/volumes/fs-1234-abcd");
    }

    #[test]
    fn labels_are_made_safe_for_paths() {
        let cases = [
            ("Windows", Some("Windows")),
            ("  My  Data\t Disk ", Some("My Data Disk")),
            ("../../etc/passwd", Some("_.._etc_passwd")),
            ("a/b\\c:d", Some("a_b_c_d")),
            (
                "bell\u{7}and\nnewline\u{1b}[31m",
                Some("belland newline[31m"),
            ),
            ("right\u{202E}to-left", Some("rightto-left")),
            ("zero\u{200B}width", Some("zerowidth")),
            (".hidden", Some("hidden")),
            ("...", None),
            ("..", None),
            (".", None),
            ("", None),
            ("   ", None),
            ("///", None),
            ("\u{0}\u{1}\u{2}", None),
            ("Ünïcödé 磁盘", Some("Ünïcödé 磁盘")),
        ];
        for (raw, expected) in cases {
            assert_eq!(sanitise_label(raw).as_deref(), expected, "{raw:?}");
        }
    }

    #[test]
    fn sanitised_labels_are_always_one_safe_path_component() {
        let nasty = [
            "../..",
            "/",
            "a/../../b",
            "\\\\server\\share",
            "C:",
            "name\u{0}null",
            " . ",
            ". .",
            "\u{202E}\u{202D}",
            "x".repeat(500).as_str(),
            "/run/media/root",
            "..\u{200B}.",
        ]
        .map(str::to_owned);
        for raw in &nasty {
            if let Some(name) = sanitise_label(raw) {
                assert!(!name.is_empty());
                assert!(name != "." && name != "..", "{raw:?} -> {name:?}");
                assert!(!name.starts_with('.'), "{raw:?} -> {name:?}");
                assert!(!name.contains(['/', '\\', ':']), "{raw:?} -> {name:?}");
                assert!(!name.chars().any(char::is_control), "{raw:?} -> {name:?}");
                assert!(name.chars().count() <= MAX_LABEL_CHARS);
                assert!(name.len() <= 255);
                assert_eq!(name.trim(), name);
                assert_eq!(
                    sanitise_label(&name).as_deref(),
                    Some(name.as_str()),
                    "not stable"
                );
            }
        }
    }

    #[test]
    fn user_names_are_validated() {
        for good in [
            "jay",
            "user_1",
            "first.last",
            "user@example.org",
            "_svc",
            "9lives",
        ] {
            assert!(UserName::parse(good).is_some(), "{good:?}");
        }
        for bad in [
            "",
            ".",
            "..",
            "a/b",
            "-rf",
            ".hidden",
            "name with space",
            "ü",
            "a\nb",
        ] {
            assert!(UserName::parse(bad).is_none(), "{bad:?}");
        }
        assert!(UserName::parse(&"a".repeat(65)).is_none());
    }

    #[test]
    fn equal_labels_are_de_duplicated_ignoring_case() {
        let a = labelled(Filesystem::Ext4, "fs-aaaa", Some("BACKUP"));
        let b = labelled(Filesystem::Exfat, "fs-bbbb", Some("backup"));
        let c = labelled(Filesystem::Vfat, "fs-cccc", Some("BACKUP"));
        let d = labelled(Filesystem::Vfat, "fs-dddd", Some("BACKUP (2)"));
        let e = labelled(Filesystem::Vfat, "fs-eeee", None);
        let names = assign_display_names(&[&a, &b, &c, &d, &e], &BTreeMap::new());
        assert_eq!(names[&a.id], "BACKUP");
        assert_eq!(names[&b.id], "backup (2)");
        assert_eq!(names[&c.id], "BACKUP (3)");
        assert_eq!(names[&d.id], "BACKUP (2) (2)");
        assert_eq!(names[&e.id], "fs-eeee");
        let distinct: BTreeSet<String> = names.values().map(|name| name.to_lowercase()).collect();
        assert_eq!(distinct.len(), 5);
    }

    #[test]
    fn assigned_names_are_kept_when_still_valid() {
        let a = labelled(Filesystem::Ext4, "fs-aaaa", Some("BACKUP"));
        let b = labelled(Filesystem::Exfat, "fs-bbbb", Some("BACKUP"));
        let mut assigned = BTreeMap::new();
        assigned.insert(b.id.clone(), "BACKUP".to_owned());
        let names = assign_display_names(&[&a, &b], &assigned);
        assert_eq!(names[&b.id], "BACKUP");
        assert_eq!(names[&a.id], "BACKUP (2)");

        // An assigned name that is not a safe path component is ignored.
        assigned.insert(b.id.clone(), "../escape".to_owned());
        let names = assign_display_names(&[&a, &b], &assigned);
        assert_eq!(names[&a.id], "BACKUP");
        assert_eq!(names[&b.id], "BACKUP (2)");
    }

    #[test]
    fn drive_letters_skip_reserved_letters_and_respect_pins() {
        let ids: Vec<VolumeId> = (0..25)
            .map(|index| VolumeId::parse(&format!("fs-{index:04}")).unwrap())
            .collect();
        let refs: Vec<&VolumeId> = ids.iter().collect();
        let letters = assign_drive_letters(&refs, &BTreeMap::new());
        assert_eq!(letters.len(), 22);
        assert_eq!(letters[&ids[0]], 'D');
        assert_eq!(letters[&ids[21]], 'Y');
        assert!(!letters.contains_key(&ids[22]));
        for letter in letters.values() {
            assert!(!['A', 'B', 'C', 'Z'].contains(letter));
        }
        let distinct: BTreeSet<&char> = letters.values().collect();
        assert_eq!(distinct.len(), 22);

        let mut pinned = BTreeMap::new();
        pinned.insert(ids[1].clone(), 'd');
        pinned.insert(ids[2].clone(), 'C');
        pinned.insert(ids[3].clone(), 'D');
        let letters = assign_drive_letters(&refs[..4], &pinned);
        assert_eq!(letters[&ids[1]], 'D');
        assert_eq!(letters[&ids[0]], 'E');
        assert_eq!(letters[&ids[2]], 'F');
        assert_eq!(letters[&ids[3]], 'G');
    }

    #[test]
    fn every_runtime_sees_the_one_canonical_mount() {
        let data = labelled(Filesystem::Ext4, "fs-aaaa", Some("Data"));
        let plan = plan(
            &inventory(vec![data]),
            &RW,
            &PolicyOverrides::default(),
            &context("jay"),
        );
        plan.verify().unwrap();
        let planned = &plan.volumes[0];
        assert_eq!(planned.canonical_path, "/run/tur/volumes/fs-aaaa");
        let mount = planned.mount.as_ref().unwrap();
        assert_eq!(mount.target, "/run/tur/volumes/fs-aaaa");
        assert_eq!(mount.sources, ["/dev/fs-aaaa"]);
        assert_eq!(mount.options, planned.decision.options);
        assert!(!mount.read_only);

        let view = |runtime: Runtime| planned.views.iter().find(|v| v.runtime == runtime).unwrap();
        assert_eq!(planned.views.len(), 4);
        assert!(
            planned
                .views
                .iter()
                .all(|v| v.target == "/run/tur/volumes/fs-aaaa")
        );
        assert!(planned.views.iter().all(|v| !v.read_only));
        assert_eq!(view(Runtime::Linux).path, "/run/media/jay/Data");
        assert_eq!(view(Runtime::Linux).kind, ViewKind::Bind);
        assert_eq!(view(Runtime::Windows).path, "dosdevices/d:");
        assert_eq!(view(Runtime::Windows).kind, ViewKind::Symlink);
        assert_eq!(view(Runtime::Windows).drive_letter, Some('D'));
        assert_eq!(view(Runtime::Macos).path, "/Volumes/Data");
        assert_eq!(
            view(Runtime::Android).path,
            "/storage/emulated/0/Volumes/Data"
        );
        assert_eq!(view(Runtime::Android).kind, ViewKind::Grant);
        assert!(view(Runtime::Android).requires_grant);
        assert!(!view(Runtime::Linux).requires_grant);
    }

    #[test]
    fn views_of_a_read_only_mount_are_read_only() {
        let mut windows = labelled(Filesystem::Ntfs, "fs-aaaa", Some("Windows"));
        windows.ntfs_state = Some(crate::volume::NtfsState::Hibernated);
        let plan = plan(
            &inventory(vec![windows]),
            &RW,
            &PolicyOverrides::default(),
            &context("jay"),
        );
        plan.verify().unwrap();
        let planned = &plan.volumes[0];
        assert_eq!(planned.decision.outcome, Outcome::MountReadOnly);
        assert!(planned.mount.as_ref().unwrap().read_only);
        assert!(!planned.views.is_empty());
        assert!(planned.views.iter().all(|view| view.read_only));
    }

    #[test]
    fn volumes_that_are_not_mounted_have_no_mount_and_no_views() {
        let mut system = labelled(Filesystem::Ext4, "fs-aaaa", Some("root"));
        system.system = true;
        let locked = labelled(Filesystem::Luks, "fs-bbbb", None);
        let refs = labelled(Filesystem::Refs, "fs-cccc", Some("ReFS"));
        let plan = plan(
            &inventory(vec![system, locked, refs]),
            &RW,
            &PolicyOverrides::default(),
            &context("jay"),
        );
        plan.verify().unwrap();
        for planned in &plan.volumes {
            assert!(planned.mount.is_none());
            assert!(planned.views.is_empty());
        }
        assert_eq!(plan.find("fs-cccc").unwrap().display_name, "ReFS");
        assert_eq!(plan.find("fs-bbbb").unwrap().display_name, "fs-bbbb");
        assert!(plan.find("fs-zzzz").is_none());
    }

    #[test]
    fn names_and_letters_are_shared_only_among_mounted_volumes() {
        let mut system = labelled(Filesystem::Ext4, "fs-aaaa", Some("Data"));
        system.system = true;
        let stick = labelled(Filesystem::Exfat, "fs-bbbb", Some("Data"));
        let other = labelled(Filesystem::Vfat, "fs-cccc", Some("DATA"));
        let plan = plan(
            &inventory(vec![system, stick, other]),
            &RW,
            &PolicyOverrides::default(),
            &context("jay"),
        );
        plan.verify().unwrap();
        let stick = plan.find("fs-bbbb").unwrap();
        let other = plan.find("fs-cccc").unwrap();
        assert_eq!(stick.display_name, "Data");
        assert_eq!(other.display_name, "DATA (2)");
        assert_eq!(
            stick.views.iter().find_map(|view| view.drive_letter),
            Some('D')
        );
        assert_eq!(
            other.views.iter().find_map(|view| view.drive_letter),
            Some('E')
        );
    }

    #[test]
    fn no_user_means_no_linux_view() {
        let data = labelled(Filesystem::Ext4, "fs-aaaa", Some("Data"));
        let plan = plan(
            &inventory(vec![data]),
            &RW,
            &PolicyOverrides::default(),
            &NamespaceContext::default(),
        );
        plan.verify().unwrap();
        assert!(
            plan.volumes[0]
                .views
                .iter()
                .all(|view| view.runtime != Runtime::Linux)
        );
        assert_eq!(plan.volumes[0].views.len(), 3);
    }

    #[test]
    fn a_volume_already_at_its_canonical_path_is_not_mounted_again() {
        let mut data = labelled(Filesystem::Ext4, "fs-aaaa", Some("Data"));
        data.mountpoints = vec!["/run/tur/volumes/fs-aaaa".to_owned()];
        let plan = plan(
            &inventory(vec![data]),
            &RW,
            &PolicyOverrides::default(),
            &context("jay"),
        );
        plan.verify().unwrap();
        let planned = &plan.volumes[0];
        assert!(planned.already_mounted);
        assert!(planned.mount.is_none());
        assert_eq!(planned.views.len(), 4);
    }

    #[test]
    fn verify_rejects_a_second_mount_of_the_same_device_or_target() {
        let a = labelled(Filesystem::Ext4, "fs-aaaa", Some("A"));
        let b = labelled(Filesystem::Ext4, "fs-bbbb", Some("B"));
        let good = plan(
            &inventory(vec![a, b]),
            &RW,
            &PolicyOverrides::default(),
            &NamespaceContext::default(),
        );
        good.verify().unwrap();

        let mut same_device = good.clone();
        same_device.volumes[1].volume.devices = same_device.volumes[0].volume.devices.clone();
        same_device.volumes[1].mount.as_mut().unwrap().sources =
            same_device.volumes[0].volume.devices.clone();
        assert!(
            same_device
                .verify()
                .unwrap_err()
                .contains("source of two mounts")
        );

        let mut same_target = good.clone();
        same_target.volumes[1].mount.as_mut().unwrap().target =
            same_target.volumes[0].canonical_path.clone();
        assert!(same_target.verify().is_err());

        let mut twice = good.clone();
        twice.volumes[1] = twice.volumes[0].clone();
        assert!(twice.verify().unwrap_err().contains("appears twice"));

        let mut device_view = good.clone();
        device_view.volumes[0].views[0].target = "/dev/fs-aaaa".to_owned();
        assert!(device_view.verify().is_err());

        let mut remount = good.clone();
        remount.volumes[0].already_mounted = true;
        assert!(remount.verify().unwrap_err().contains("planned again"));

        let mut writable_view = good;
        writable_view.volumes[0].decision.outcome = Outcome::MountReadOnly;
        assert!(writable_view.verify().is_err());
    }
}
