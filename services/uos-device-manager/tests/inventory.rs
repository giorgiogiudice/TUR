// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

//! The inventory, the findings and the redacted view, checked against the
//! `laptop` fixture through the library.
//!
//! The fixture is a hand-written tree shaped like a small `/sys` and `/proc`,
//! loaded into an in-memory [`FakeProbe`]. Nothing here reads the hardware of
//! the machine the tests run on, and nothing here shows that the inventory is
//! right on real hardware: that needs a hardware runner and is not run.

mod support;

use std::collections::BTreeSet;
use std::path::Path;

use serde_json::Value;
use uos_device_manager::model::{
    BlockTransport, BootMode, Category, ConnectorStatus, FwupdStatus, IdBasis, InputBus,
    InputKind, NetKind, OperState, Removable, SecureBoot, StorageControllerKind,
    ThunderboltAuthorisation,
};
use uos_device_manager::{
    FakeProbe, FsProbe, Inventory, ProblemReport, ProbeCommand, Sensitive, Severity, collect,
    detect,
};

fn laptop() -> FakeProbe {
    FakeProbe::from_dir(Path::new(support::LAPTOP)).expect("the fixture loads")
}

fn inventory() -> Inventory {
    collect(&laptop())
}

fn json(inventory: &Inventory) -> Value {
    serde_json::to_value(inventory).expect("the inventory serialises")
}

/// Every value in the fixture that identifies the (imaginary) owner's
/// hardware. None of them may survive redaction, anywhere in the document.
const SECRETS: [&str; 21] = [
    "PF2ABCDE",
    "4c4c4544-0042-3510-8054-b9c04f564433",
    "L1HF2AB01CD",
    "4C530001230915112345",
    "f2c8d1a04e77",
    "001000001",
    "8c:16:45:aa:bb:01",
    "4c:79:6e:aa:bb:02",
    "00:e0:4c:68:00:01",
    // The same MAC address as udev writes it into the interface name.
    "00e04c680001",
    "52:54:00:12:34:56",
    "S5GXNX0R123456",
    "eui.0025385b21404321",
    "S3Z9NB0K123456",
    "naa.5002538e40a1b2c3",
    "a4:c3:f0:11:22:33",
    "d0:ab:cd:44:55:66",
    "c5010000-0070-6d18-23e4-1a2b3c4d5e6f",
    "00a1b2c3-0092-8f18-ffff-ffffffffffff",
    "LNXPWRBN/button/input0",
    "usb-0000:00:14.0-1/input0",
];

#[test]
fn the_directory_probe_and_the_in_memory_fake_agree() {
    let from_directory = collect(&FsProbe::fixture(support::LAPTOP));
    assert_eq!(from_directory, inventory());
}

#[test]
fn every_source_of_the_fixture_is_used() {
    let inventory = inventory();
    assert_eq!(inventory.schema_version, 1);
    assert!(!inventory.redacted);
    assert!(inventory.source_notes.is_empty(), "{:?}", inventory.source_notes);
}

#[test]
fn the_platform_is_described() {
    let platform = inventory().platform;
    assert_eq!(platform.sys_vendor.as_deref(), Some("Example Computer"));
    assert_eq!(platform.product_name.as_deref(), Some("Notebook 14 G2"));
    assert_eq!(platform.product_version.as_deref(), Some("1.0"));
    assert_eq!(platform.board_name.as_deref(), Some("NB14G2"));
    assert_eq!(platform.bios_vendor.as_deref(), Some("Example Firmware"));
    assert_eq!(platform.bios_version.as_deref(), Some("N3HET74W (1.46)"));
    assert_eq!(platform.bios_date.as_deref(), Some("03/15/2026"));
    assert_eq!(platform.chassis_type.as_deref(), Some("10"));
    assert_eq!(platform.product_serial.value(), Some("PF2ABCDE"));
    assert_eq!(
        platform.product_uuid.value(),
        Some("4c4c4544-0042-3510-8054-b9c04f564433")
    );
    assert_eq!(platform.boot_mode, BootMode::Uefi);
    assert_eq!(platform.secure_boot, SecureBoot::Enabled);
}

#[test]
fn pci_devices_are_listed_with_ids_class_and_driver() {
    let inventory = inventory();
    let addresses: Vec<&str> = inventory
        .pci
        .iter()
        .map(|device| device.address.as_str())
        .collect();
    assert_eq!(
        addresses,
        [
            "0000:00:00.0",
            "0000:00:01.0",
            "0000:00:02.0",
            "0000:00:06.0",
            "0000:00:0d.2",
            "0000:00:14.0",
            "0000:00:14.3",
            "0000:00:16.0",
            "0000:00:17.0",
            "0000:00:1c.0",
            "0000:00:1d.0",
            "0000:00:1f.0",
            "0000:00:1f.3",
            "0000:00:1f.6",
            "0000:01:00.0",
            "0000:02:00.0",
            "0000:03:00.0",
            "0000:04:00.0",
        ]
    );

    let gpu = &inventory.pci[2];
    assert_eq!(gpu.id, "pci-0000:00:02.0-8086:9a49");
    assert_eq!(gpu.id_basis, IdBasis::Topology);
    assert_eq!(gpu.sysfs_path, "/sys/devices/pci0000:00/0000:00:02.0");
    assert_eq!(gpu.vendor_id.as_deref(), Some("8086"));
    assert_eq!(gpu.device_id.as_deref(), Some("9a49"));
    assert_eq!(gpu.class.as_deref(), Some("030000"));
    assert_eq!(gpu.class_name.as_deref(), Some("Display controller"));
    assert_eq!(gpu.subsystem_vendor_id.as_deref(), Some("17aa"));
    assert_eq!(gpu.subsystem_device_id.as_deref(), Some("22d5"));
    assert_eq!(gpu.revision.as_deref(), Some("01"));
    assert_eq!(gpu.vendor_name.as_deref(), Some("Intel Corporation"));
    assert_eq!(
        gpu.device_name.as_deref(),
        Some("TigerLake-LP GT2 [Iris Xe Graphics]")
    );
    assert_eq!(gpu.driver.as_deref(), Some("i915"));
    assert_eq!(
        gpu.modalias.as_deref(),
        Some("pci:v00008086d00009A49sv000017AAsd000022D5bc03sc00i00")
    );
    let modules = gpu.kernel_modules.as_deref().expect("the alias table was read");
    assert_eq!(modules.len(), 1);
    assert_eq!(modules[0].name, "i915");
    assert!(modules[0].loaded);

    // The GPU's DRM card and the state of each connector.
    let display = gpu.gpu.as_ref().expect("a display controller has GPU details");
    assert_eq!(display.boot_vga, Some(true));
    assert_eq!(display.drm_cards.len(), 1);
    assert_eq!(display.drm_cards[0].name, "card1");
    let connectors: Vec<(&str, ConnectorStatus, Option<bool>)> = display.drm_cards[0]
        .connectors
        .iter()
        .map(|connector| (connector.name.as_str(), connector.status, connector.enabled))
        .collect();
    assert_eq!(
        connectors,
        [
            ("DP-1", ConnectorStatus::Disconnected, Some(false)),
            ("HDMI-A-1", ConnectorStatus::Disconnected, Some(false)),
            ("eDP-1", ConnectorStatus::Connected, Some(true)),
        ]
    );

    // A second GPU with no driver: no DRM card, and the module that should
    // drive it is loaded.
    let second = &inventory.pci[17];
    assert_eq!(
        second.sysfs_path,
        "/sys/devices/pci0000:00/0000:00:01.0/0000:04:00.0"
    );
    assert_eq!(second.driver, None);
    assert_eq!(second.class.as_deref(), Some("030200"));
    assert_eq!(second.device_name.as_deref(), Some("TU117M [GeForce MX550]"));
    let display = second.gpu.as_ref().expect("a display controller has GPU details");
    assert_eq!(display.boot_vga, Some(false));
    assert!(display.drm_cards.is_empty());
    let modules = second.kernel_modules.as_deref().unwrap();
    assert_eq!(modules[0].name, "nouveau");
    assert!(modules[0].loaded);

    // A device that is not a display controller has no GPU details; without
    // udev names the class name comes from the class code.
    let management = &inventory.pci[7];
    assert_eq!(management.gpu, None);
    assert_eq!(management.class_name.as_deref(), Some("Communication controller"));
    assert_eq!(management.vendor_name, None);
    assert_eq!(management.kernel_modules.as_ref().map(Vec::len), Some(0));

    // Two modules claim the audio controller; one is loaded.
    let audio = &inventory.pci[12];
    let modules: Vec<(&str, bool)> = audio
        .kernel_modules
        .as_deref()
        .unwrap()
        .iter()
        .map(|module| (module.name.as_str(), module.loaded))
        .collect();
    assert_eq!(
        modules,
        [("snd_hda_intel", true), ("snd_sof_pci_intel_tgl", false)]
    );
}

#[test]
fn usb_devices_are_listed_with_strings_speed_and_interface_drivers() {
    let inventory = inventory();
    let ports: Vec<&str> = inventory
        .usb
        .iter()
        .map(|device| device.bus_port.as_str())
        .collect();
    assert_eq!(
        ports,
        ["1-1", "1-3", "1-5", "1-10", "2-1", "2-2", "usb1", "usb2"]
    );

    let receiver = &inventory.usb[0];
    assert_eq!(receiver.id, "usb-1-1-046d:c52b");
    assert_eq!(receiver.id_basis, IdBasis::Topology);
    assert_eq!(
        receiver.sysfs_path,
        "/sys/devices/pci0000:00/0000:00:14.0/usb1/1-1"
    );
    assert_eq!(receiver.vendor_id.as_deref(), Some("046d"));
    assert_eq!(receiver.product_id.as_deref(), Some("c52b"));
    assert_eq!(receiver.class.as_deref(), Some("00"));
    assert_eq!(receiver.class_name.as_deref(), Some("Defined by the interfaces"));
    assert_eq!(receiver.manufacturer.as_deref(), Some("Logitech"));
    assert_eq!(receiver.product.as_deref(), Some("USB Receiver"));
    assert_eq!(receiver.serial, Sensitive::Absent);
    assert_eq!(receiver.speed_mbps.as_deref(), Some("12"));
    assert_eq!(receiver.usb_version.as_deref(), Some("2.00"));
    assert_eq!(receiver.removable, Removable::Removable);
    assert!(!receiver.is_root_hub);
    assert_eq!(receiver.interfaces.len(), 2);
    let keyboard = &receiver.interfaces[0];
    assert_eq!(keyboard.name, "1-1:1.0");
    assert_eq!(keyboard.class.as_deref(), Some("03"));
    assert_eq!(keyboard.subclass.as_deref(), Some("01"));
    assert_eq!(keyboard.protocol.as_deref(), Some("01"));
    assert_eq!(keyboard.class_name.as_deref(), Some("Human interface device"));
    assert_eq!(keyboard.driver.as_deref(), Some("usbhid"));
    assert_eq!(receiver.interfaces[1].protocol.as_deref(), Some("02"));

    // A vendor-specific interface with no kernel driver.
    let reader = &inventory.usb[1];
    assert_eq!(reader.removable, Removable::Fixed);
    assert_eq!(reader.interfaces[0].driver, None);
    assert_eq!(reader.interfaces[0].class_name.as_deref(), Some("Vendor specific"));

    // A device with a serial number: the identifier is the hash of it with
    // the machine identifier, computed independently for this assertion.
    let stick = &inventory.usb[4];
    assert_eq!(stick.id, "usb-0781:5581-sc98eb119b0e5002d");
    assert_eq!(stick.id_basis, IdBasis::HardwareIdentifier);
    assert_eq!(stick.serial.value(), Some("4C530001230915112345"));
    assert_eq!(stick.speed_mbps.as_deref(), Some("5000"));
    assert_eq!(stick.interfaces[0].driver.as_deref(), Some("usb-storage"));
    let modules = stick.interfaces[0].kernel_modules.as_deref().unwrap();
    assert_eq!(modules.len(), 1);
    assert_eq!(modules[0].name, "usb_storage");
    assert!(modules[0].loaded);

    let root_hub = &inventory.usb[6];
    assert!(root_hub.is_root_hub);
    assert_eq!(root_hub.id, "usb-usb1-1d6b:0002");
    // Its "serial" is the address of its controller, and is not reported.
    assert_eq!(root_hub.serial, Sensitive::Absent);
    assert_eq!(root_hub.class_name.as_deref(), Some("Hub"));
    assert_eq!(root_hub.removable, Removable::Unknown);
    assert_eq!(root_hub.interfaces[0].driver.as_deref(), Some("hub"));
}

#[test]
fn network_interfaces_are_listed_with_kind_driver_and_state() {
    let inventory = inventory();
    let names: Vec<&str> = inventory
        .network
        .iter()
        .map(|interface| interface.name.as_str())
        .collect();
    // `bonding_masters`, a plain file in /sys/class/net, is not an interface.
    assert_eq!(
        names,
        ["enp0s31f6", "enx00e04c680001", "lo", "virbr0", "wlp0s20f3"]
    );

    let wired = &inventory.network[0];
    assert_eq!(wired.id, "net-mf571fdd4762574e3");
    assert_eq!(wired.id_basis, IdBasis::HardwareIdentifier);
    assert_eq!(wired.kind, NetKind::Ethernet);
    assert!(!wired.wireless);
    assert!(!wired.is_virtual);
    assert_eq!(wired.link_type, Some(1));
    assert_eq!(wired.driver.as_deref(), Some("e1000e"));
    assert_eq!(wired.operstate, OperState::Down);
    assert_eq!(wired.mac_address.value(), Some("8c:16:45:aa:bb:01"));
    assert_eq!(wired.parent.as_deref(), Some("pci-0000:00:1f.6-8086:15fc"));

    let adapter = &inventory.network[1];
    assert_eq!(adapter.kind, NetKind::Ethernet);
    assert_eq!(adapter.driver.as_deref(), Some("r8152"));
    assert_eq!(adapter.parent.as_deref(), Some("usb-0bda:8153-s06464f7630521add"));

    let loopback = &inventory.network[2];
    assert_eq!(loopback.id, "net-name-lo");
    assert_eq!(loopback.id_basis, IdBasis::KernelName);
    assert_eq!(loopback.kind, NetKind::Loopback);
    assert!(loopback.is_virtual);
    assert_eq!(loopback.driver, None);
    assert_eq!(loopback.operstate, OperState::Unknown);
    assert_eq!(loopback.mac_address, Sensitive::Absent);
    assert_eq!(loopback.parent, None);

    let bridge = &inventory.network[3];
    assert_eq!(bridge.kind, NetKind::Bridge);
    assert!(bridge.is_virtual);
    assert_eq!(bridge.id, "net-name-virbr0");

    let wireless = &inventory.network[4];
    assert_eq!(wireless.kind, NetKind::Wireless);
    assert!(wireless.wireless);
    assert_eq!(wireless.driver.as_deref(), Some("iwlwifi"));
    assert_eq!(wireless.operstate, OperState::Up);
    assert_eq!(wireless.parent.as_deref(), Some("pci-0000:00:14.3-8086:a0f0"));
}

#[test]
fn sound_cards_and_storage_are_listed() {
    let inventory = inventory();

    assert_eq!(inventory.audio.len(), 1);
    let card = &inventory.audio[0];
    assert_eq!(card.id, "audio-pci-0000:00:1f.3-8086:a0c8-0");
    assert_eq!(card.id_basis, IdBasis::Topology);
    assert_eq!(card.index, 0);
    assert_eq!(card.card_id.as_deref(), Some("PCH"));
    assert_eq!(card.name.as_deref(), Some("HDA Intel PCH"));
    assert_eq!(
        card.long_name.as_deref(),
        Some("HDA Intel PCH at 0x603d1a8000 irq 163")
    );
    assert_eq!(card.alsa_driver.as_deref(), Some("HDA-Intel"));
    assert_eq!(card.driver.as_deref(), Some("snd_hda_intel"));
    assert_eq!(card.parent.as_deref(), Some("pci-0000:00:1f.3-8086:a0c8"));

    let controllers: Vec<(&str, StorageControllerKind, Option<&str>)> = inventory
        .storage_controllers
        .iter()
        .map(|controller| {
            (
                controller.id.as_str(),
                controller.kind,
                controller.driver.as_deref(),
            )
        })
        .collect();
    assert_eq!(
        controllers,
        [
            (
                "storage-pci-0000:00:17.0-8086:a0d3",
                StorageControllerKind::Sata,
                Some("ahci")
            ),
            (
                "storage-pci-0000:01:00.0-144d:a80a",
                StorageControllerKind::Nvme,
                Some("nvme")
            ),
            (
                "storage-usb-0781:5581-sc98eb119b0e5002d-if0",
                StorageControllerKind::UsbMassStorage,
                Some("usb-storage")
            ),
        ]
    );

    // Whole disks only: no partitions, no loop or device-mapper devices.
    let names: Vec<&str> = inventory
        .block
        .iter()
        .map(|device| device.name.as_str())
        .collect();
    assert_eq!(names, ["nvme0n1", "sda", "sdb"]);

    let nvme = &inventory.block[0];
    assert_eq!(nvme.id, "block-w0beb27a4938212bf");
    assert_eq!(nvme.id_basis, IdBasis::HardwareIdentifier);
    assert_eq!(nvme.model.as_deref(), Some("Samsung SSD 980 PRO 1TB"));
    assert_eq!(nvme.vendor, None);
    assert_eq!(nvme.serial.value(), Some("S5GXNX0R123456"));
    assert_eq!(nvme.wwid.value(), Some("eui.0025385b21404321"));
    assert_eq!(nvme.transport, BlockTransport::Nvme);
    assert_eq!(nvme.removable, Some(false));
    assert_eq!(nvme.rotational, Some(false));
    assert_eq!(nvme.size_bytes, Some(1_000_204_886_016));
    assert_eq!(nvme.driver.as_deref(), Some("nvme"));
    assert_eq!(nvme.parent.as_deref(), Some("pci-0000:01:00.0-144d:a80a"));

    let sata = &inventory.block[1];
    assert_eq!(sata.model.as_deref(), Some("Samsung SSD 860"));
    assert_eq!(sata.vendor.as_deref(), Some("ATA"));
    // The serial number of a SATA disk is not in sysfs; it comes from udev.
    assert_eq!(sata.serial.value(), Some("S3Z9NB0K123456"));
    assert_eq!(sata.wwid.value(), Some("naa.5002538e40a1b2c3"));
    assert_eq!(sata.transport, BlockTransport::Scsi);
    assert_eq!(sata.driver.as_deref(), Some("sd"));
    assert_eq!(sata.parent.as_deref(), Some("pci-0000:00:17.0-8086:a0d3"));

    let stick = &inventory.block[2];
    assert_eq!(stick.id, "block-sf6b9e102601b1823");
    assert_eq!(stick.transport, BlockTransport::Usb);
    assert_eq!(stick.removable, Some(true));
    assert_eq!(stick.wwid, Sensitive::Absent);
    assert_eq!(stick.parent.as_deref(), Some("usb-0781:5581-sc98eb119b0e5002d"));
}

#[test]
fn input_bluetooth_and_video_devices_are_listed() {
    let inventory = inventory();

    // `eventN` and `mice` are handlers, not devices; input12 sorts after input6.
    let inputs: Vec<(&str, InputBus, &str)> = inventory
        .input
        .iter()
        .map(|device| {
            (
                device.name.as_deref().unwrap_or(""),
                device.bus,
                device.id.as_str(),
            )
        })
        .collect();
    assert_eq!(
        inputs,
        [
            ("Power Button", InputBus::Host, "input-b0019-0000:0001-0"),
            (
                "AT Translated Set 2 keyboard",
                InputBus::I8042,
                "input-b0011-0001:0001-0"
            ),
            (
                "Logitech USB Receiver",
                InputBus::Usb,
                "input-usb-1-1-046d:c52b-0"
            ),
            (
                "Logitech USB Receiver Mouse",
                InputBus::Usb,
                "input-usb-1-1-046d:c52b-1"
            ),
            ("MX Anywhere 3", InputBus::Bluetooth, "input-b0005-046d:b023-0"),
        ]
    );
    assert_eq!(inventory.input[0].kinds, [InputKind::Key]);
    assert_eq!(inventory.input[0].uniq, Sensitive::Absent);
    assert_eq!(inventory.input[0].parent, None);
    assert_eq!(
        inventory.input[1].kinds,
        [InputKind::Keyboard, InputKind::Key]
    );
    assert_eq!(inventory.input[2].parent.as_deref(), Some("usb-1-1-046d:c52b"));
    assert_eq!(inventory.input[2].version.as_deref(), Some("0111"));
    assert_eq!(inventory.input[3].kinds, [InputKind::Mouse]);
    // For a Bluetooth device both fields are addresses.
    assert_eq!(inventory.input[4].phys.value(), Some("a4:c3:f0:11:22:33"));
    assert_eq!(inventory.input[4].uniq.value(), Some("d0:ab:cd:44:55:66"));

    // `hci0:256`, a connection, is not an adapter.
    assert_eq!(inventory.bluetooth.len(), 1);
    let adapter = &inventory.bluetooth[0];
    assert_eq!(adapter.name, "hci0");
    assert_eq!(adapter.id, "bluetooth-usb-1-10-8087:0026-0");
    assert_eq!(adapter.driver.as_deref(), Some("btusb"));
    assert_eq!(adapter.address, Sensitive::Absent);
    let rfkill = adapter.rfkill.as_ref().expect("the adapter has a kill switch");
    assert_eq!(rfkill.soft_blocked, Some(false));
    assert_eq!(rfkill.hard_blocked, Some(false));
    assert_eq!(adapter.parent.as_deref(), Some("usb-1-10-8087:0026"));

    assert_eq!(inventory.cameras.len(), 2);
    let capture = &inventory.cameras[0];
    assert_eq!(capture.node, "video0");
    assert_eq!(capture.name.as_deref(), Some("Integrated Camera: Integrated C"));
    assert_eq!(capture.index, Some(0));
    assert_eq!(capture.capture, Some(true));
    assert_eq!(capture.driver.as_deref(), Some("uvcvideo"));
    // The camera has a serial number, so its nodes inherit a hardware basis.
    assert_eq!(capture.id_basis, IdBasis::HardwareIdentifier);
    assert!(capture.id.starts_with("camera-usb-04f2:b6ea-s"));
    assert!(capture.id.ends_with("-0"));
    let metadata = &inventory.cameras[1];
    assert_eq!(metadata.capture, Some(false));
    assert!(metadata.id.ends_with("-1"));
    assert_eq!(metadata.parent, capture.parent);
}

#[test]
fn thunderbolt_devices_are_listed_with_their_authorisation() {
    let inventory = inventory();
    // `domain0` and the retimer `0-0:1.1` are not devices.
    assert_eq!(inventory.thunderbolt.len(), 2);

    let host = &inventory.thunderbolt[0];
    assert_eq!(host.name, "0-0");
    assert!(host.is_host);
    assert_eq!(host.authorised, ThunderboltAuthorisation::Authorised);
    assert_eq!(host.vendor_id.as_deref(), Some("0109"));
    assert_eq!(host.device_id.as_deref(), Some("1602"));
    assert_eq!(host.security_level.as_deref(), Some("user"));

    let dock = &inventory.thunderbolt[1];
    assert_eq!(dock.name, "0-1");
    assert_eq!(dock.id, "thunderbolt-u879a490d3e0ec342");
    assert_eq!(dock.id_basis, IdBasis::HardwareIdentifier);
    assert!(!dock.is_host);
    assert_eq!(dock.authorised, ThunderboltAuthorisation::NotAuthorised);
    assert_eq!(dock.vendor_name.as_deref(), Some("CalDigit, Inc."));
    assert_eq!(dock.device_name.as_deref(), Some("TS3 Plus"));
    assert_eq!(
        dock.unique_id.value(),
        Some("00a1b2c3-0092-8f18-ffff-ffffffffffff")
    );
    assert_eq!(
        dock.sysfs_path,
        "/sys/devices/pci0000:00/0000:00:0d.2/domain0/0-0/0-1"
    );
}

#[test]
fn firmware_devices_and_pending_updates_are_listed() {
    let firmware = inventory().firmware;
    assert_eq!(firmware.fwupd, FwupdStatus::Available);
    assert_eq!(firmware.devices.len(), 3);

    let system = &firmware.devices[0];
    assert_eq!(system.name.as_deref(), Some("System Firmware"));
    assert_eq!(system.version.as_deref(), Some("1.46"));
    assert!(system.updatable);
    assert_eq!(system.update_state.as_deref(), Some("unknown"));
    // Only the release flagged as an upgrade is pending; the installed
    // version is also offered by fwupd and is not.
    assert_eq!(system.pending_updates.len(), 1);
    assert_eq!(system.pending_updates[0].version.as_deref(), Some("1.52"));
    assert_eq!(system.pending_updates[0].urgency.as_deref(), Some("high"));
    assert_eq!(system.pending_updates[0].remote.as_deref(), Some("lvfs"));

    let disk = &firmware.devices[1];
    assert_eq!(disk.serial.value(), Some("S5GXNX0R123456"));
    assert!(disk.pending_updates.is_empty());

    let camera = &firmware.devices[2];
    assert!(!camera.updatable);
    assert_eq!(camera.flags, ["registered"]);
}

#[test]
fn the_kernel_log_is_reduced_to_driver_and_firmware_failures() {
    let kernel = inventory().kernel;
    assert!(kernel.available);
    let firmware: Vec<(&str, &str)> = kernel
        .firmware_failures
        .iter()
        .map(|failure| {
            (
                failure.device.as_deref().unwrap_or(""),
                failure.firmware.as_str(),
            )
        })
        .collect();
    // The repeated message is kept once.
    assert_eq!(
        firmware,
        [
            ("0000:00:14.3", "iwlwifi-QuZ-a0-hr-b0-78.ucode"),
            ("0000:03:00.0", "rtw88/rtw8822c_fw.bin"),
        ]
    );
    assert_eq!(kernel.probe_failures.len(), 1);
    assert_eq!(kernel.probe_failures[0].device, "0000:04:00.0");
    assert_eq!(kernel.probe_failures[0].driver.as_deref(), Some("nouveau"));
    assert_eq!(kernel.probe_failures[0].error, Some(-12));

    // Nothing else from the log is kept: the line with a serial number is
    // not in the document.
    let text = serde_json::to_string(&kernel).unwrap();
    assert!(!text.contains("SerialNumber"));
    assert!(!text.contains("4C530001230915112345"));
}

#[test]
fn every_device_has_a_unique_well_formed_identifier() {
    let inventory = inventory();
    let devices = inventory.devices();
    assert_eq!(devices.len(), 18 + 8 + 5 + 1 + 3 + 3 + 5 + 1 + 2 + 2);

    let mut seen = BTreeSet::new();
    for device in &devices {
        let id = device.id();
        assert!(!id.is_empty());
        assert!(
            support::pattern_matches("^[A-Za-z0-9:._~-]+$", id),
            "{id} has an unexpected character"
        );
        assert!(seen.insert(id.to_owned()), "{id} is used twice");
        // The identifier leads back to the same device, in the same category.
        let found = inventory.find(id).expect("the identifier resolves");
        assert_eq!(found.id(), id);
        assert_eq!(found.category(), device.category());
        assert_eq!(&found, device);
    }
    assert_eq!(inventory.find("no-such-device"), None);

    let summaries = inventory.summaries();
    assert_eq!(summaries.len(), devices.len());
    assert_eq!(summaries[2].id, "pci-0000:00:02.0-8086:9a49");
    assert_eq!(summaries[2].category, Category::Pci);
    assert_eq!(summaries[2].name, "TigerLake-LP GT2 [Iris Xe Graphics]");
    assert_eq!(summaries[2].driver.as_deref(), Some("i915"));
    // No sensitive value is used as a display name.
    for summary in &summaries {
        for secret in SECRETS {
            if secret == "00e04c680001" {
                // The interface name itself, until redacted.
                continue;
            }
            assert!(!summary.name.contains(secret), "{}", summary.name);
            assert!(!summary.id.contains(secret), "{}", summary.id);
        }
    }
}

#[test]
fn identifiers_survive_re_enumeration_where_the_hardware_allows() {
    let before = inventory();

    // The flash drive moves from port 2-1 to port 1-7. The fixture is edited
    // in memory: the device directory is rebuilt under the new port.
    let old = "/sys/devices/pci0000:00/0000:00:14.0/usb2/2-1";
    let new = "/sys/devices/pci0000:00/0000:00:14.0/usb1/1-7";
    let moved = laptop()
        .remove(old)
        .remove("/sys/bus/usb/devices/2-1")
        .remove("/sys/bus/usb/devices/2-1:1.0")
        .remove("/sys/class/block/sdb")
        .file(&format!("{new}/idVendor"), "0781\n")
        .file(&format!("{new}/idProduct"), "5581\n")
        .file(&format!("{new}/bDeviceClass"), "00\n")
        .file(&format!("{new}/serial"), "4C530001230915112345\n")
        .file(&format!("{new}/1-7:1.0/bInterfaceClass"), "08\n")
        .link(
            "/sys/bus/usb/devices/1-7",
            "../../../devices/pci0000:00/0000:00:14.0/usb1/1-7",
        );
    let after = collect(&moved);
    let stick = after
        .usb
        .iter()
        .find(|device| device.bus_port == "1-7")
        .expect("the drive is at its new port");
    assert_eq!(stick.id, "usb-0781:5581-sc98eb119b0e5002d");
    assert_eq!(stick.id, before.usb[4].id);

    // A device without a serial number is known only by its port, and every
    // PCI device only by its address: the document says which is which.
    assert_eq!(before.usb[0].id_basis, IdBasis::Topology);
    assert!(before.pci.iter().all(|device| device.id_basis == IdBasis::Topology));
}

#[test]
fn problems_are_found_ordered_and_explained() {
    let inventory = inventory();
    let findings = detect(&inventory);
    let listed: Vec<(Severity, &str, Option<&str>)> = findings
        .iter()
        .map(|finding| {
            (
                finding.severity,
                finding.code.as_str(),
                finding.device_id.as_deref(),
            )
        })
        .collect();
    assert_eq!(
        listed,
        [
            // The kernel says nouveau refused the second GPU.
            (
                Severity::Error,
                "UOS-DEVICE-DRIVER-003",
                Some("pci-0000:04:00.0-10de:25a2")
            ),
            // No module of this kernel claims the management controller.
            (
                Severity::Warning,
                "UOS-DEVICE-DRIVER-001",
                Some("pci-0000:00:16.0-8086:a0e0")
            ),
            // The second wireless adapter: firmware missing, and no interface.
            (
                Severity::Warning,
                "UOS-DEVICE-FIRMWARE-001",
                Some("pci-0000:03:00.0-10ec:c822")
            ),
            (
                Severity::Warning,
                "UOS-DEVICE-INACTIVE-001",
                Some("pci-0000:03:00.0-10ec:c822")
            ),
            // Bridges and a fingerprint reader without drivers: information.
            (
                Severity::Info,
                "UOS-DEVICE-DRIVER-001",
                Some("pci-0000:00:00.0-8086:9a14")
            ),
            (
                Severity::Info,
                "UOS-DEVICE-DRIVER-001",
                Some("pci-0000:00:1f.0-8086:a082")
            ),
            (
                Severity::Info,
                "UOS-DEVICE-DRIVER-001",
                Some("usb-06cb:00bd-sa3578c08cdb8febd")
            ),
            // A card reader whose module exists but is not loaded.
            (
                Severity::Info,
                "UOS-DEVICE-DRIVER-002",
                Some("pci-0000:02:00.0-10ec:522a")
            ),
            // The working wireless adapter asked for an optional file.
            (
                Severity::Info,
                "UOS-DEVICE-FIRMWARE-001",
                Some("pci-0000:00:14.3-8086:a0f0")
            ),
            (
                Severity::Info,
                "UOS-DEVICE-THUNDERBOLT-001",
                Some("thunderbolt-u879a490d3e0ec342")
            ),
        ]
    );

    let refused = &findings[0];
    assert_eq!(refused.category, Some(Category::Pci));
    assert_eq!(
        refused.user_message,
        "The driver nouveau did not accept the Display controller (NVIDIA Corporation TU117M [GeForce MX550]) at PCI address 0000:04:00.0 (kernel error -12), so the device will not work."
    );
    assert_eq!(refused.technical_detail["ids"], "10de:25a2");
    assert_eq!(refused.technical_detail["matching_modules"], "nouveau (loaded)");

    let firmware = &findings[2];
    assert_eq!(
        firmware.technical_detail["firmware_file"],
        "rtw88/rtw8822c_fw.bin"
    );
    assert!(firmware.user_message.contains("does not work without it"));

    // Every finding has both layers, and identifies a device in the inventory.
    for finding in &findings {
        assert!(!finding.title.is_empty());
        assert!(!finding.user_message.is_empty());
        assert!(!finding.next_step.is_empty());
        assert!(finding.technical_detail.contains_key("evidence"));
        let id = finding.device_id.as_deref().expect("a device of the fixture");
        let device = inventory.find(id).expect("the device is in the inventory");
        assert_eq!(Some(device.category()), finding.category);
    }
}

#[test]
fn no_finding_suggests_a_driver_from_another_operating_system() {
    for finding in detect(&inventory()) {
        let text = format!(
            "{} {} {}",
            finding.title, finding.user_message, finding.next_step
        )
        .to_lowercase();
        for word in ["windows", "macos", "mac os", ".sys", ".kext", ".inf", "ndiswrapper"] {
            assert!(!text.contains(word), "{}: {text}", finding.code);
        }
    }
}

#[test]
fn the_output_agrees_with_the_schema() {
    let inventory = inventory();
    support::validate(&json(&inventory)).expect("the inventory matches the schema");
    support::validate(&json(&inventory.redacted())).expect("the redacted inventory matches");

    let report = serde_json::to_value(ProblemReport::of(&inventory)).unwrap();
    assert_eq!(report["findings"].as_array().map(Vec::len), Some(10));
    support::validate_as("problem_report", &report).expect("the findings match the schema");

    for device in inventory.devices() {
        let document = serde_json::to_value(device).unwrap();
        support::validate_as("device_document", &document)
            .unwrap_or_else(|error| panic!("{}: {error}", device.id()));
        let summary = serde_json::to_value(device.summary()).unwrap();
        support::validate_as("device_summary", &summary)
            .unwrap_or_else(|error| panic!("{}: {error}", device.id()));
    }

    // An inventory with nothing in it is valid too.
    let empty = collect(&FakeProbe::new());
    support::validate(&json(&empty)).expect("an empty inventory matches the schema");
    let report = serde_json::to_value(ProblemReport::of(&empty)).unwrap();
    support::validate_as("problem_report", &report).expect("an empty report matches");
}

#[test]
fn the_schema_check_notices_drift() {
    // The check above would be worthless if the validator accepted anything.
    let good = json(&inventory());
    assert!(support::validate(&good).is_ok());

    let mut missing = good.clone();
    missing["pci"][0].as_object_mut().unwrap().remove("driver");
    assert!(support::validate(&missing).unwrap_err().contains("missing field driver"));

    let mut extra = good.clone();
    extra["usb"][0]["colour"] = Value::from("red");
    assert!(support::validate(&extra).unwrap_err().contains("unexpected field colour"));

    let mut wrong_enum = good.clone();
    wrong_enum["network"][0]["kind"] = Value::from("carrier_pigeon");
    assert!(support::validate(&wrong_enum).is_err());

    let mut wrong_type = good.clone();
    wrong_type["block"][0]["size_bytes"] = Value::from("large");
    assert!(support::validate(&wrong_type).is_err());

    let mut wrong_pattern = good.clone();
    wrong_pattern["pci"][0]["vendor_id"] = Value::from("0x8086");
    assert!(support::validate(&wrong_pattern).is_err());

    let mut unmarked = good.clone();
    unmarked["usb"][4]["serial"] = Value::from("4C530001230915112345");
    assert!(support::validate(&unmarked).is_err());

    let mut wrong_version = good;
    wrong_version["schema_version"] = Value::from(2);
    assert!(support::validate(&wrong_version).is_err());
}

#[test]
fn redaction_removes_every_sensitive_value() {
    let inventory = inventory();
    let plain = json(&inventory);
    let redacted_inventory = inventory.redacted();
    let redacted = json(&redacted_inventory);
    assert_eq!(redacted["redacted"], Value::Bool(true));
    assert_eq!(plain["redacted"], Value::Bool(false));

    // The secrets are in the plain document, so their absence below means
    // something.
    let plain_text = plain.to_string().to_lowercase();
    for secret in SECRETS {
        assert!(plain_text.contains(&secret.to_lowercase()), "{secret} not in fixture");
    }
    let redacted_text = redacted.to_string().to_lowercase();
    for secret in SECRETS {
        assert!(
            !redacted_text.contains(&secret.to_lowercase()),
            "{secret} survived redaction"
        );
    }

    // Every field marked sensitive is empty, and says whether it held a value.
    let before = support::sensitive_fields(&plain);
    let after = support::sensitive_fields(&redacted);
    assert_eq!(before.len(), after.len());
    assert_eq!(before.len(), 39);
    let mut held_a_value = 0;
    for ((path, was), (_, now)) in before.iter().zip(&after) {
        assert_eq!(now["value"], Value::Null, "{path}");
        assert_eq!(now["sensitive"], Value::Bool(true), "{path}");
        assert_eq!(now["redacted"], Value::Bool(!was["value"].is_null()), "{path}");
        if !was["value"].is_null() {
            held_a_value += 1;
        }
    }
    assert_eq!(held_a_value, 26);

    // A MAC address that udev wrote into an interface name is removed there
    // and in the sysfs path as well.
    let adapter = &redacted_inventory.network[1];
    assert_eq!(adapter.name, "enx<redacted>");
    assert!(adapter.sysfs_path.ends_with("/net/enx<redacted>"));
    assert_eq!(adapter.mac_address, Sensitive::Redacted);
    assert_eq!(redacted_inventory.network[2].mac_address, Sensitive::Absent);
}

#[test]
fn redaction_keeps_identifiers_and_everything_that_is_not_sensitive() {
    let inventory = inventory();
    let redacted = inventory.redacted();

    // Identifiers are the same in both views, so a device seen in a redacted
    // listing can be asked for by its identifier.
    let ids = |inventory: &Inventory| -> Vec<String> {
        inventory
            .summaries()
            .into_iter()
            .map(|summary| summary.id)
            .collect()
    };
    assert_eq!(ids(&inventory), ids(&redacted));

    assert_eq!(inventory.pci, redacted.pci);
    assert_eq!(inventory.storage_controllers, redacted.storage_controllers);
    assert_eq!(inventory.cameras, redacted.cameras);
    assert_eq!(inventory.kernel, redacted.kernel);
    assert_eq!(redacted.usb[4].product.as_deref(), Some("Ultra"));
    assert_eq!(redacted.usb[4].serial, Sensitive::Redacted);
    assert_eq!(redacted.block[0].model.as_deref(), Some("Samsung SSD 980 PRO 1TB"));
    assert_eq!(redacted.platform.sys_vendor.as_deref(), Some("Example Computer"));

    // Redacting twice changes nothing more, and the findings are the same.
    assert_eq!(redacted.redacted(), redacted);
    let codes = |inventory: &Inventory| -> Vec<String> {
        detect(inventory)
            .iter()
            .map(|finding| finding.code.to_string())
            .collect()
    };
    assert_eq!(codes(&inventory), codes(&redacted));
}

#[test]
fn hostile_descriptor_strings_cannot_break_the_document() {
    // The fixture's flash drive now reports strings chosen by an attacker.
    let stick = "/sys/devices/pci0000:00/0000:00:14.0/usb2/2-1";
    let mut long = b"A very long product name ".to_vec();
    long.resize(4096, b'Z');
    let hostile = laptop()
        .file(
            &format!("{stick}/manufacturer"),
            b"\x1b[2J\x1b[1;1H\x07Trusted Vendor\r\n\x00\x08\x7f".as_slice(),
        )
        .file(&format!("{stick}/product"), long.as_slice())
        .file(
            &format!("{stick}/serial"),
            b"\xff\xfe\xc0\xaf\xed\xa0\x80 \xe2\x80\xaeSERIAL\"},{\"x\":\"".as_slice(),
        )
        .file(&format!("{stick}/idVendor"), b"\x00\x00\x00\x00".as_slice())
        .file(&format!("{stick}/speed"), b"-1e309".as_slice())
        .file(&format!("{stick}/2-1:1.0/modalias"), vec![b'*'; 4096]);

    let inventory = collect(&hostile);
    let stick = inventory
        .usb
        .iter()
        .find(|device| device.bus_port == "2-1")
        .expect("the drive is still listed");
    assert_eq!(stick.manufacturer.as_deref(), Some("[2J [1;1H Trusted Vendor"));
    assert_eq!(stick.product.as_deref().map(|text| text.chars().count()), Some(256));
    assert_eq!(stick.vendor_id, None);
    assert_eq!(stick.speed_mbps, None);
    // Without a vendor ID there is no hash; the identifier is the port.
    assert_eq!(stick.id, "usb-2-1");
    assert_eq!(stick.interfaces[0].modalias, None);

    let document = json(&inventory);
    support::validate(&document).expect("the document still matches the schema");
    for (path, text) in support::strings(&document) {
        assert!(!text.chars().any(char::is_control), "{path}: {text:?}");
        assert!(!text.contains('\u{202e}'), "{path}: {text:?}");
        assert!(text.chars().count() <= 1024, "{path} is too long");
    }
    // The serialised form is one valid JSON document, whatever the serial
    // number tried to inject.
    let text = serde_json::to_string(&inventory).unwrap();
    let reparsed: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(reparsed, document);
    assert_eq!(reparsed["usb"].as_array().map(Vec::len), Some(8));

    // The findings and the redacted view are still produced.
    let report = serde_json::to_value(ProblemReport::of(&inventory)).unwrap();
    support::validate_as("problem_report", &report).expect("the findings match the schema");
    support::validate(&json(&inventory.redacted())).expect("the redacted view matches");
}

#[test]
fn hostile_command_output_cannot_break_the_document() {
    let mut garbage = Vec::new();
    for index in 0..200_000_u32 {
        garbage.extend_from_slice(&index.to_le_bytes());
        garbage.extend_from_slice(b"\nP: /devices/\xff\x00\nE: \x1b=\x1b\n");
    }
    let hostile = laptop()
        .command(ProbeCommand::UdevExportDb, garbage.as_slice())
        .command(ProbeCommand::KernelLog, garbage.as_slice())
        .command(ProbeCommand::FwupdGetDevices, garbage.as_slice())
        .file("/proc/asound/cards", garbage.as_slice())
        .file(
            "/usr/lib/modules/6.19.2-300.fc45.x86_64/modules.alias",
            garbage.as_slice(),
        )
        .file("/etc/machine-id", garbage.as_slice());

    let inventory = collect(&hostile);
    // The devices are still there, from sysfs.
    assert_eq!(inventory.pci.len(), 18);
    assert_eq!(inventory.usb.len(), 8);
    assert_eq!(inventory.firmware.fwupd, FwupdStatus::Invalid);
    assert!(inventory.kernel.firmware_failures.is_empty());
    // Without a usable machine identifier nothing is hashed.
    assert!(
        inventory
            .usb
            .iter()
            .all(|device| device.id_basis == IdBasis::Topology)
    );
    let sources: Vec<&str> = inventory
        .source_notes
        .iter()
        .map(|note| note.source.as_str())
        .collect();
    assert_eq!(sources, ["machine-id", "fwupd"]);

    let document = json(&inventory);
    support::validate(&document).expect("the document still matches the schema");
    for (path, text) in support::strings(&document) {
        assert!(!text.chars().any(char::is_control), "{path}: {text:?}");
    }
    let report = serde_json::to_value(ProblemReport::of(&inventory)).unwrap();
    support::validate_as("problem_report", &report).expect("the findings match the schema");
}
