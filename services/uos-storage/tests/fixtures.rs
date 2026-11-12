// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

//! Fixture-driven tests of the public API.
//!
//! The fixtures under `tests/fixtures/` are hand-written in the shape of
//! `lsblk --json --bytes --output-all` output, with `blkid --output export`
//! style property files beside them. They were not captured from real
//! hardware and no real filesystem is involved.

use std::collections::BTreeSet;

use uos_storage::{
    Access, Encryption, Filesystem, IdSource, Inventory, MountPlan, MountRequest, NamespaceContext,
    Outcome, PlannedVolume, PolicyOverrides, Runtime, SnapshotProbe, StorageCode, UnsafeNtfsPolicy,
    UserName, VolumeId, canonical_path, plan,
};

struct Fixture {
    name: &'static str,
    lsblk: &'static str,
    properties: &'static [&'static str],
}

const DUAL_BOOT: &str = include_str!("fixtures/dual-boot.json");
const DUAL_BOOT_CLEAN: &str = include_str!("fixtures/dual-boot.clean.props");
const DUAL_BOOT_HIBERNATED: &str = include_str!("fixtures/dual-boot.hibernated.props");
const DUAL_BOOT_DIRTY: &str = include_str!("fixtures/dual-boot.dirty.props");
const REMOVABLE: &str = include_str!("fixtures/removable.json");
const REMOVABLE_PROPS: &str = include_str!("fixtures/removable.props");
const LINUX_DATA: &str = include_str!("fixtures/linux-data.json");
const LINUX_DATA_PROPS: &str = include_str!("fixtures/linux-data.props");
const FOREIGN: &str = include_str!("fixtures/foreign.json");
const FOREIGN_PROPS: &str = include_str!("fixtures/foreign.props");

const FIXTURES: &[Fixture] = &[
    Fixture {
        name: "dual-boot",
        lsblk: DUAL_BOOT,
        properties: &[DUAL_BOOT_CLEAN, DUAL_BOOT_HIBERNATED, DUAL_BOOT_DIRTY],
    },
    Fixture {
        name: "removable",
        lsblk: REMOVABLE,
        properties: &[REMOVABLE_PROPS],
    },
    Fixture {
        name: "linux-data",
        lsblk: LINUX_DATA,
        properties: &[LINUX_DATA_PROPS],
    },
    Fixture {
        name: "foreign",
        lsblk: FOREIGN,
        properties: &[FOREIGN_PROPS],
    },
];

const RW: MountRequest = MountRequest {
    access: Access::ReadWrite,
    uid: 1000,
    gid: 1000,
};
const RO: MountRequest = MountRequest {
    access: Access::ReadOnly,
    uid: 1000,
    gid: 1000,
};

fn inventory(lsblk: &str, properties: Option<&str>) -> Inventory {
    let mut probe = SnapshotProbe::from_lsblk_json(lsblk).unwrap();
    if let Some(properties) = properties {
        probe = probe.with_properties(properties).unwrap();
    }
    Inventory::from_probe(&probe).unwrap()
}

/// Every fixture, without properties and with each of its property files.
fn inventories() -> Vec<(String, Inventory)> {
    let mut all = Vec::new();
    for fixture in FIXTURES {
        all.push((
            format!("{} (lsblk only)", fixture.name),
            inventory(fixture.lsblk, None),
        ));
        for (index, properties) in fixture.properties.iter().enumerate() {
            all.push((
                format!("{} (properties {index})", fixture.name),
                inventory(fixture.lsblk, Some(properties)),
            ));
        }
    }
    all
}

fn jay() -> NamespaceContext {
    NamespaceContext {
        user: UserName::parse("jay"),
        ..NamespaceContext::default()
    }
}

fn default_plan(lsblk: &str, properties: Option<&str>) -> MountPlan {
    plan(
        &inventory(lsblk, properties),
        &RW,
        &PolicyOverrides::default(),
        &jay(),
    )
}

fn by_device<'a>(plan: &'a MountPlan, device: &str) -> &'a PlannedVolume {
    plan.volumes
        .iter()
        .find(|planned| {
            planned
                .volume
                .devices
                .iter()
                .any(|candidate| candidate == device)
        })
        .unwrap_or_else(|| panic!("no volume on {device}"))
}

fn view_path(planned: &PlannedVolume, runtime: Runtime) -> Option<&str> {
    planned
        .views
        .iter()
        .find(|view| view.runtime == runtime)
        .map(|view| view.path.as_str())
}

#[test]
fn every_fixture_parses_and_has_volumes() {
    let all = inventories();
    assert_eq!(all.len(), 10);
    for (name, inventory) in &all {
        assert!(!inventory.volumes.is_empty(), "{name}");
    }
}

#[test]
fn identifiers_are_valid_unique_and_reproducible() {
    for fixture in FIXTURES {
        let first = inventory(fixture.lsblk, None);
        let second = inventory(fixture.lsblk, None);
        assert_eq!(first, second, "{}", fixture.name);
    }
    for (name, inventory) in inventories() {
        let mut seen = BTreeSet::new();
        for volume in &inventory.volumes {
            let id = volume.id.as_str();
            assert!(VolumeId::parse(id).is_some(), "{name}: {id}");
            assert!(
                seen.insert(id.to_owned()),
                "{name}: duplicate identifier {id}"
            );
            assert_eq!(canonical_path(&volume.id), format!("/run/uos/volumes/{id}"));
            assert!(!volume.devices.is_empty(), "{name}: {id}");
            assert_eq!(
                volume.ntfs_state.is_some(),
                volume.filesystem == Filesystem::Ntfs
            );
            assert_eq!(
                volume.hfs_journal.is_some(),
                volume.filesystem == Filesystem::HfsPlus
            );
        }
        let sorted: Vec<&VolumeId> = inventory.volumes.iter().map(|volume| &volume.id).collect();
        assert!(
            sorted.windows(2).all(|pair| pair[0] < pair[1]),
            "{name}: not sorted"
        );
    }
}

/// The "never mounted twice" invariant, checked independently of
/// `MountPlan::verify`, over every fixture, both requests, the whole
/// override space, and with and without a user.
#[test]
fn no_filesystem_is_planned_to_be_mounted_twice() {
    let mut plans = 0;
    let mut mounts_seen = 0;
    for (name, inventory) in inventories() {
        for request in [RO, RW] {
            for policy in PolicyOverrides::all() {
                for context in [NamespaceContext::default(), jay()] {
                    let plan = plan(&inventory, &request, &policy, &context);
                    plan.verify()
                        .unwrap_or_else(|problem| panic!("{name}: {problem}"));
                    plans += 1;

                    assert_eq!(plan.volumes.len(), inventory.volumes.len(), "{name}");
                    let mut targets = BTreeSet::new();
                    let mut sources = BTreeSet::new();
                    let mut filesystems = BTreeSet::new();
                    for planned in &plan.volumes {
                        let volume = &planned.volume;
                        let Some(mount) = &planned.mount else {
                            continue;
                        };
                        mounts_seen += 1;
                        assert!(planned.decision.is_mount(), "{name}");
                        assert!(!volume.system, "{name}: system storage is mounted");
                        assert!(
                            !volume.duplicate_identity,
                            "{name}: ambiguous volume is mounted"
                        );
                        assert!(
                            volume.mountpoints.is_empty(),
                            "{name}: mounted volume is mounted"
                        );
                        assert_eq!(mount.target, canonical_path(&volume.id), "{name}");
                        assert!(
                            targets.insert(mount.target.clone()),
                            "{name}: target reused"
                        );
                        for source in &mount.sources {
                            assert!(sources.insert(source.clone()), "{name}: {source} reused");
                        }
                        if let Some(uuid) = &volume.uuid {
                            let identity = (volume.filesystem, uuid.to_lowercase());
                            assert!(filesystems.insert(identity), "{name}: {uuid} mounted twice");
                        }
                    }
                    for planned in &plan.volumes {
                        for view in &planned.views {
                            assert_eq!(view.target, planned.canonical_path, "{name}");
                            assert!(
                                targets.contains(&view.target) || planned.already_mounted,
                                "{name}: view of something that is not mounted"
                            );
                            assert!(!sources.contains(&view.path), "{name}: view of a device");
                        }
                    }
                }
            }
        }
    }
    assert_eq!(plans, 10 * 2 * 16 * 2);
    assert!(mounts_seen > 1000, "{mounts_seen}");
}

#[test]
fn view_paths_are_safe_on_every_fixture() {
    for (name, inventory) in inventories() {
        let plan = plan(&inventory, &RW, &PolicyOverrides::default(), &jay());
        for planned in &plan.volumes {
            let name_in_path = &planned.display_name;
            assert!(
                !name_in_path.contains(['/', '\\', ':']),
                "{name}: {name_in_path:?}"
            );
            assert!(
                !name_in_path.chars().any(char::is_control),
                "{name}: {name_in_path:?}"
            );
            assert!(!name_in_path.starts_with('.'), "{name}: {name_in_path:?}");
            for view in &planned.views {
                assert!(
                    !view.path.split('/').any(|part| part == ".."),
                    "{name}: {}",
                    view.path
                );
                assert!(
                    !view.path.chars().any(char::is_control),
                    "{name}: {:?}",
                    view.path
                );
                match view.runtime {
                    Runtime::Linux => {
                        assert_eq!(view.path, format!("/run/media/jay/{name_in_path}"));
                    }
                    Runtime::Macos => assert_eq!(view.path, format!("/Volumes/{name_in_path}")),
                    Runtime::Android => {
                        assert_eq!(
                            view.path,
                            format!("/storage/emulated/0/Volumes/{name_in_path}")
                        );
                        assert!(view.requires_grant);
                    }
                    Runtime::Windows => {
                        let letter = view.drive_letter.unwrap();
                        assert!(('D'..='Y').contains(&letter), "{name}: {letter}");
                    }
                }
            }
        }
    }
}

#[test]
fn dual_boot_system_storage_is_never_offered() {
    for properties in [None, Some(DUAL_BOOT_CLEAN), Some(DUAL_BOOT_HIBERNATED)] {
        let plan = default_plan(DUAL_BOOT, properties);
        assert_eq!(plan.volumes.len(), 7);
        for device in [
            "/dev/nvme0n1p1",
            "/dev/nvme0n1p5",
            "/dev/nvme0n1p6",
            "/dev/mapper/luks-c0ffee00-1111-4222-8333-000000000c06",
        ] {
            let planned = by_device(&plan, device);
            assert!(planned.volume.system, "{device}");
            assert_eq!(planned.decision.outcome, Outcome::Refuse, "{device}");
            assert_eq!(planned.decision.code, StorageCode::SystemVolume, "{device}");
            assert!(
                planned.mount.is_none() && planned.views.is_empty(),
                "{device}"
            );
        }
        assert!(
            plan.volumes
                .iter()
                .all(|planned| !planned.volume.devices[0].contains("zram"))
        );
    }
}

#[test]
fn dual_boot_volume_facts_are_read_from_the_tree() {
    let plan = default_plan(DUAL_BOOT, Some(DUAL_BOOT_CLEAN));
    let esp = by_device(&plan, "/dev/nvme0n1p1");
    assert_eq!(esp.volume.id.as_str(), "fs-7a1b-22c3");
    assert_eq!(esp.volume.filesystem, Filesystem::Vfat);
    assert_eq!(esp.volume.fs_version.as_deref(), Some("FAT32"));
    assert_eq!(esp.volume.transport.as_deref(), Some("nvme"));
    assert!(!esp.volume.removable);

    let container = by_device(&plan, "/dev/nvme0n1p6");
    assert_eq!(container.volume.filesystem, Filesystem::Luks);
    assert_eq!(container.volume.encryption, Encryption::Unlocked);

    let root = by_device(
        &plan,
        "/dev/mapper/luks-c0ffee00-1111-4222-8333-000000000c06",
    );
    assert_eq!(root.volume.filesystem, Filesystem::Btrfs);
    assert_eq!(root.volume.encryption, Encryption::Mapping);
    assert_eq!(
        root.volume.mountpoints,
        ["/var", "/sysroot", "/etc", "/var/home"]
    );
    assert_eq!(root.volume.subvolumes.len(), 4);
    assert_eq!(root.volume.label.as_deref(), Some("uos"));

    let reserved = by_device(&plan, "/dev/nvme0n1p2");
    assert_eq!(reserved.volume.filesystem, Filesystem::Unknown);
    assert_eq!(reserved.volume.id_source, IdSource::PartitionUuid);
    assert_eq!(
        reserved.volume.id.as_str(),
        "part-5f0c1d2e-0002-4000-8000-00000000a001"
    );
    assert_eq!(reserved.decision.outcome, Outcome::Unsupported);
    assert_eq!(reserved.decision.code, StorageCode::FilesystemUnknown);
}

#[test]
fn clean_windows_volume_mounts_read_write() {
    let plan = default_plan(DUAL_BOOT, Some(DUAL_BOOT_CLEAN));
    let windows = by_device(&plan, "/dev/nvme0n1p3");
    assert_eq!(windows.volume.id.as_str(), "fs-01d9a7c2e4f6b801");
    assert_eq!(windows.decision.outcome, Outcome::MountReadWrite);
    assert_eq!(windows.decision.code, StorageCode::MountReadWrite);
    assert_eq!(windows.decision.driver, Some("ntfs3"));
    assert_eq!(
        windows.canonical_path,
        "/run/uos/volumes/fs-01d9a7c2e4f6b801"
    );
    assert_eq!(windows.display_name, "Windows");
    let mount = windows.mount.as_ref().unwrap();
    assert_eq!(mount.sources, ["/dev/nvme0n1p3"]);
    assert_eq!(mount.mount_type, Some("ntfs3"));
    assert!(mount.options.iter().any(|option| option == "windows_names"));
    assert_eq!(
        view_path(windows, Runtime::Linux),
        Some("/run/media/jay/Windows")
    );
    assert_eq!(view_path(windows, Runtime::Windows), Some("dosdevices/d:"));
    assert_eq!(view_path(windows, Runtime::Macos), Some("/Volumes/Windows"));
    assert_eq!(
        view_path(windows, Runtime::Android),
        Some("/storage/emulated/0/Volumes/Windows")
    );
}

#[test]
fn hibernated_windows_volume_is_read_only_with_the_specification_message() {
    let plan = default_plan(DUAL_BOOT, Some(DUAL_BOOT_HIBERNATED));
    let windows = by_device(&plan, "/dev/nvme0n1p3");
    assert_eq!(windows.decision.outcome, Outcome::MountReadOnly);
    assert_eq!(windows.decision.code.as_str(), "UOS-STORAGE-NTFS-004");
    let message = windows.decision.render();
    assert!(message.starts_with(
        "UOS-STORAGE-NTFS-004\nThe Windows volume was not mounted for writing.\n\nReason:\nThe \
         volume appears to be hibernated or in an unsafe state.\n\nAction:\nBoot Windows, \
         disable Fast Startup/hibernation, shut down fully, then retry. UOS mounts the volume \
         read-only to protect your data.\n\nTechnical detail:\nDevice: \
         /dev/disk/by-uuid/01D9A7C2E4F6B801\nDriver: ntfs3\nMount policy: read-only\n"
    ));
    assert_eq!(windows.mount.as_ref().unwrap().options[0], "ro");
    assert!(windows.views.iter().all(|view| view.read_only));

    // The other NTFS volume on the same disk is clean and is not affected.
    let other = by_device(&plan, "/dev/nvme0n1p4");
    assert_eq!(other.decision.outcome, Outcome::MountReadWrite);

    // A stricter policy refuses instead; nothing makes the volume writable.
    for policy in PolicyOverrides::all() {
        for request in [RO, RW] {
            let plan = plan_with(DUAL_BOOT, Some(DUAL_BOOT_HIBERNATED), &request, &policy);
            let windows = by_device(&plan, "/dev/nvme0n1p3");
            assert_ne!(windows.decision.outcome, Outcome::MountReadWrite);
            let refused = policy.unsafe_ntfs == UnsafeNtfsPolicy::Refuse;
            assert_eq!(windows.decision.outcome == Outcome::Refuse, refused);
            assert_eq!(windows.mount.is_none(), refused);
        }
    }
}

fn plan_with(
    lsblk: &str,
    properties: Option<&str>,
    request: &MountRequest,
    policy: &PolicyOverrides,
) -> MountPlan {
    plan(&inventory(lsblk, properties), request, policy, &jay())
}

#[test]
fn dirty_and_unknown_windows_volumes_are_read_only() {
    let plan = default_plan(DUAL_BOOT, Some(DUAL_BOOT_DIRTY));
    let dirty = by_device(&plan, "/dev/nvme0n1p3");
    assert_eq!(dirty.decision.outcome, Outcome::MountReadOnly);
    assert_eq!(dirty.decision.code.as_str(), "UOS-STORAGE-NTFS-005");
    // No state fact was supplied for this one.
    let unknown = by_device(&plan, "/dev/nvme0n1p4");
    assert_eq!(unknown.decision.outcome, Outcome::MountReadOnly);
    assert_eq!(unknown.decision.code.as_str(), "UOS-STORAGE-NTFS-004");

    // With lsblk alone, no NTFS volume is writable.
    let plan = default_plan(DUAL_BOOT, None);
    for device in ["/dev/nvme0n1p3", "/dev/nvme0n1p4"] {
        assert_eq!(
            by_device(&plan, device).decision.outcome,
            Outcome::MountReadOnly
        );
    }
}

#[test]
fn removable_media_decisions() {
    let plan = default_plan(REMOVABLE, Some(REMOVABLE_PROPS));
    assert_eq!(plan.volumes.len(), 9);

    let stick = by_device(&plan, "/dev/sda1");
    assert_eq!(stick.volume.id.as_str(), "fs-64a5-f00d");
    assert!(stick.volume.removable);
    assert_eq!(stick.decision.outcome, Outcome::MountReadWrite);
    assert_eq!(stick.decision.driver, Some("exfat"));
    assert!(
        stick
            .decision
            .options
            .iter()
            .any(|option| option == "nosuid")
    );
    assert!(
        stick
            .decision
            .options
            .iter()
            .any(|option| option == "nodev")
    );

    let camera = by_device(&plan, "/dev/sdb1");
    assert!(camera.volume.device_read_only);
    assert_eq!(camera.decision.outcome, Outcome::MountReadOnly);
    assert_eq!(camera.decision.code, StorageCode::DeviceReadOnly);

    let unlabelled = by_device(&plan, "/dev/sdc1");
    assert_eq!(unlabelled.volume.label, None);
    assert_eq!(unlabelled.display_name, "fs-9f8e-7d6c");
    assert_eq!(
        view_path(unlabelled, Runtime::Linux),
        Some("/run/media/jay/fs-9f8e-7d6c")
    );
    assert!(
        unlabelled
            .decision
            .options
            .iter()
            .any(|option| option == "flush")
    );

    let locked = by_device(&plan, "/dev/sdg1");
    assert_eq!(locked.volume.encryption, Encryption::Locked);
    assert_eq!(locked.volume.fs_version.as_deref(), Some("2"));
    assert_eq!(locked.decision.outcome, Outcome::NeedsUnlock);
    assert_eq!(locked.decision.code, StorageCode::EncryptedLocked);
    assert!(locked.mount.is_none() && locked.views.is_empty());

    let external = by_device(&plan, "/dev/sdh1");
    assert!(external.volume.removable);
    assert_eq!(external.decision.outcome, Outcome::MountReadWrite);

    let disc = by_device(&plan, "/dev/sr0");
    assert!(disc.volume.optical && disc.volume.device_read_only);
    assert_eq!(disc.volume.id.as_str(), "fs-2026-10-20-08-15-30-00");
    assert_eq!(disc.decision.outcome, Outcome::MountReadOnly);
    assert_eq!(disc.decision.code, StorageCode::FilesystemReadOnlyByDesign);
    assert_eq!(disc.decision.options, ["ro", "nosuid", "nodev"]);
    assert_eq!(disc.display_name, "Fedora-KDE-Live-45");

    // Without the state fact the external NTFS drive is not writable.
    let plan = default_plan(REMOVABLE, None);
    assert_eq!(
        by_device(&plan, "/dev/sdh1").decision.outcome,
        Outcome::MountReadOnly
    );
}

#[test]
fn equal_labels_get_distinct_names_and_letters() {
    let plan = default_plan(REMOVABLE, Some(REMOVABLE_PROPS));
    let first = by_device(&plan, "/dev/sdd1");
    let second = by_device(&plan, "/dev/sde1");
    assert_eq!(first.volume.label.as_deref(), Some("BACKUP"));
    assert_eq!(second.volume.label.as_deref(), Some("BACKUP"));
    assert_eq!(first.display_name, "BACKUP");
    assert_eq!(second.display_name, "BACKUP (2)");
    assert_eq!(
        view_path(first, Runtime::Linux),
        Some("/run/media/jay/BACKUP")
    );
    assert_eq!(
        view_path(second, Runtime::Linux),
        Some("/run/media/jay/BACKUP (2)")
    );
    assert_eq!(
        view_path(second, Runtime::Macos),
        Some("/Volumes/BACKUP (2)")
    );

    // Letters follow identifier order among the mounted volumes.
    let letters: Vec<(String, char)> = plan
        .volumes
        .iter()
        .filter_map(|planned| {
            planned
                .views
                .iter()
                .find_map(|view| view.drive_letter)
                .map(|letter| (planned.volume.devices[0].clone(), letter))
        })
        .collect();
    let expected = [
        ("/dev/sr0", 'D'),
        ("/dev/sdb1", 'E'),
        ("/dev/sdh1", 'F'),
        ("/dev/sda1", 'G'),
        ("/dev/sdc1", 'H'),
        ("/dev/sdd1", 'I'),
        ("/dev/sde1", 'J'),
        ("/dev/sdf1", 'K'),
    ];
    assert_eq!(letters.len(), expected.len());
    for ((device, letter), (expected_device, expected_letter)) in letters.iter().zip(expected) {
        assert_eq!(
            (device.as_str(), *letter),
            (expected_device, expected_letter)
        );
    }
}

#[test]
fn hostile_label_cannot_escape_the_media_directory() {
    let plan = default_plan(REMOVABLE, Some(REMOVABLE_PROPS));
    let hostile = by_device(&plan, "/dev/sdf1");
    assert_eq!(
        hostile.volume.label.as_deref(),
        Some("../../etc/\u{7}pass\nwd:x\\y\u{1b}[31m")
    );
    assert_eq!(hostile.display_name, "_.._etc_pass wd_x_y[31m");
    assert_eq!(
        view_path(hostile, Runtime::Linux),
        Some("/run/media/jay/_.._etc_pass wd_x_y[31m")
    );
    // The canonical path does not use the label at all.
    assert_eq!(
        hostile.canonical_path,
        "/run/uos/volumes/fs-f1f1f1f1-aaaa-4bbb-8ccc-000000000601"
    );
}

#[test]
fn linux_data_volumes_with_properties() {
    let plan = default_plan(LINUX_DATA, Some(LINUX_DATA_PROPS));
    assert_eq!(plan.volumes.len(), 7);

    let xfs = by_device(&plan, "/dev/sda1");
    assert_eq!(xfs.decision.outcome, Outcome::MountReadWrite);
    assert_eq!(xfs.decision.options, ["rw", "nosuid", "nodev"]);
    assert!(!xfs.volume.removable);

    let physical = by_device(&plan, "/dev/sda2");
    assert_eq!(physical.volume.filesystem, Filesystem::LvmMember);
    assert_eq!(physical.decision.outcome, Outcome::Refuse);
    assert_eq!(physical.decision.code, StorageCode::ContainerNotMountable);

    let logical = by_device(&plan, "/dev/mapper/vg0-media");
    assert_eq!(logical.volume.filesystem, Filesystem::Ext4);
    assert_eq!(logical.decision.outcome, Outcome::MountReadWrite);
    assert_eq!(logical.display_name, "media");

    let pool = by_device(&plan, "/dev/sdb");
    assert_eq!(pool.volume.devices, ["/dev/sdb", "/dev/sdc"]);
    assert_eq!(pool.volume.size_bytes, 2 * 8_001_563_222_016);
    assert!(!pool.volume.duplicate_identity);
    assert_eq!(pool.decision.outcome, Outcome::MountReadWrite);
    assert_eq!(
        pool.mount.as_ref().unwrap().sources,
        ["/dev/sdb", "/dev/sdc"]
    );

    let scratch = by_device(&plan, "/dev/nvme1n1p1");
    assert_eq!(scratch.volume.subvolumes, ["/@data", "/@snapshots"]);
    assert_eq!(scratch.decision.outcome, Outcome::Refuse);
    assert_eq!(scratch.decision.code, StorageCode::MountedElsewhere);
    assert!(scratch.decision.reason.contains("/var/mnt/scratch"));
    assert!(scratch.mount.is_none());

    let members = by_device(&plan, "/dev/sdd1");
    assert_eq!(members.volume.devices, ["/dev/sdd1", "/dev/sde1"]);
    assert_eq!(members.volume.filesystem, Filesystem::RaidMember);
    assert_eq!(members.decision.code, StorageCode::ContainerNotMountable);

    let array = by_device(&plan, "/dev/md0");
    assert_eq!(array.volume.filesystem, Filesystem::Ext4);
    assert!(!array.volume.duplicate_identity);
    assert_eq!(array.decision.outcome, Outcome::MountReadWrite);
}

#[test]
fn multi_device_sets_without_member_identifiers_are_refused_not_merged() {
    let plan = default_plan(LINUX_DATA, None);
    assert_eq!(plan.volumes.len(), 9);
    for device in ["/dev/sdb", "/dev/sdc", "/dev/sdd1", "/dev/sde1"] {
        let planned = by_device(&plan, device);
        assert_eq!(planned.volume.devices.len(), 1, "{device}");
        assert!(planned.volume.duplicate_identity, "{device}");
        assert_eq!(planned.decision.outcome, Outcome::Refuse, "{device}");
        assert_eq!(
            planned.decision.code,
            StorageCode::DuplicateIdentity,
            "{device}"
        );
        assert!(planned.mount.is_none(), "{device}");
    }
}

#[test]
fn foreign_filesystems_follow_the_policy_table() {
    let plan = default_plan(FOREIGN, Some(FOREIGN_PROPS));
    assert_eq!(plan.volumes.len(), 12);
    let expected = [
        (
            "/dev/sda1",
            Outcome::MountReadOnly,
            StorageCode::HfsPlusJournaled,
        ),
        (
            "/dev/sda2",
            Outcome::MountReadOnly,
            StorageCode::HfsPlusWriteDisabled,
        ),
        (
            "/dev/sda3",
            Outcome::Unsupported,
            StorageCode::ApfsNotEnabled,
        ),
        (
            "/dev/sdb1",
            Outcome::Unsupported,
            StorageCode::FilesystemUnsupported,
        ),
        (
            "/dev/sdb2",
            Outcome::Unsupported,
            StorageCode::BitLockerUnsupported,
        ),
        (
            "/dev/sdb3",
            Outcome::Unsupported,
            StorageCode::FilesystemUnknown,
        ),
        (
            "/dev/sdb4",
            Outcome::Unsupported,
            StorageCode::FilesystemUnknown,
        ),
        (
            "/dev/sdc1",
            Outcome::Unsupported,
            StorageCode::FilesystemUnsupported,
        ),
        ("/dev/sdd1", Outcome::Refuse, StorageCode::DuplicateIdentity),
        ("/dev/sde1", Outcome::Refuse, StorageCode::DuplicateIdentity),
        (
            "/dev/sdf1",
            Outcome::MountReadOnly,
            StorageCode::FilesystemReadOnlyByDesign,
        ),
        (
            "/dev/sdf2",
            Outcome::MountReadWrite,
            StorageCode::MountReadWrite,
        ),
    ];
    // The hybrid image's whole-disk signature is not a thirteenth volume.
    assert!(
        plan.volumes
            .iter()
            .all(|planned| planned.volume.devices[0] != "/dev/sdf")
    );
    for (device, outcome, code) in expected {
        let planned = by_device(&plan, device);
        assert_eq!(planned.decision.outcome, outcome, "{device}");
        assert_eq!(planned.decision.code, code, "{device}");
    }
    assert_eq!(
        by_device(&plan, "/dev/sdb3").volume.signature.as_deref(),
        Some("bcachefs")
    );
    assert_eq!(by_device(&plan, "/dev/sdb4").volume.signature, None);
    assert_eq!(
        by_device(&plan, "/dev/sdb2").volume.id_source,
        IdSource::PartitionUuid
    );
    for device in ["/dev/sdd1", "/dev/sde1"] {
        assert_eq!(
            by_device(&plan, device).volume.id_source,
            IdSource::HardwareIdentity
        );
    }
}

#[test]
fn foreign_filesystems_under_every_override() {
    for policy in PolicyOverrides::all() {
        for request in [RO, RW] {
            let plan = plan_with(FOREIGN, Some(FOREIGN_PROPS), &request, &policy);

            // APFS: read-only when enabled, never writable, never the write driver.
            let apfs = by_device(&plan, "/dev/sda3");
            assert_ne!(apfs.decision.outcome, Outcome::MountReadWrite);
            assert_eq!(apfs.decision.is_mount(), policy.apfs_read_only);
            if let Some(mount) = &apfs.mount {
                assert!(mount.read_only);
                assert_eq!(mount.driver, Some("apfs-fuse"));
                assert_eq!(mount.options[0], "ro");
            }

            // HFS+: only the non-journalled volume, only with the opt-in.
            let journalled = by_device(&plan, "/dev/sda1");
            assert_eq!(journalled.decision.outcome, Outcome::MountReadOnly);
            let plain = by_device(&plan, "/dev/sda2");
            let writable = policy.hfsplus_write && request.access == Access::ReadWrite;
            assert_eq!(plain.decision.outcome == Outcome::MountReadWrite, writable);

            // Nothing unsupported, unknown or ambiguous is ever mounted.
            for device in [
                "/dev/sdb1",
                "/dev/sdb2",
                "/dev/sdb3",
                "/dev/sdb4",
                "/dev/sdc1",
                "/dev/sdd1",
                "/dev/sde1",
            ] {
                let planned = by_device(&plan, device);
                assert!(!planned.decision.is_mount(), "{device}");
                assert!(
                    planned.mount.is_none() && planned.views.is_empty(),
                    "{device}"
                );
            }
        }
    }
}

#[test]
fn hfsplus_without_a_journal_fact_is_treated_as_journalled() {
    let allow = PolicyOverrides {
        hfsplus_write: true,
        ..PolicyOverrides::default()
    };
    let plan = plan_with(FOREIGN, None, &RW, &allow);
    for device in ["/dev/sda1", "/dev/sda2"] {
        let planned = by_device(&plan, device);
        assert_eq!(planned.decision.outcome, Outcome::MountReadOnly, "{device}");
        assert_eq!(
            planned.decision.code,
            StorageCode::HfsPlusJournaled,
            "{device}"
        );
    }
}

#[test]
fn plans_serialise_to_json_with_stable_text_forms() {
    let plan = default_plan(DUAL_BOOT, Some(DUAL_BOOT_HIBERNATED));
    let value = serde_json::to_value(&plan).unwrap();
    let windows = value["volumes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|volume| volume["id"] == "fs-01d9a7c2e4f6b801")
        .unwrap();
    assert_eq!(windows["filesystem"], "ntfs");
    assert_eq!(windows["ntfs_state"], "hibernated");
    assert_eq!(windows["id_source"], "filesystem-uuid");
    assert_eq!(windows["encryption"], "none");
    assert_eq!(windows["decision"]["outcome"], "mount-read-only");
    assert_eq!(windows["decision"]["code"], "UOS-STORAGE-NTFS-004");
    assert_eq!(windows["decision"]["severity"], "warning");
    assert_eq!(windows["decision"]["requested_access"], "read-write");
    assert_eq!(windows["decision"]["implementation"], "kernel");
    assert_eq!(windows["mount"]["read_only"], true);
    assert_eq!(windows["views"][1]["runtime"], "windows");
    assert_eq!(windows["views"][1]["kind"], "symlink");
    assert_eq!(windows["views"][1]["drive_letter"], "D");
}
