// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma Singleton

import QtQuick

// The installer's words for what the engine reports in codes: sizes,
// connections, what is on a disk, what a partition is. Every sentence a
// person reads about a disk is made here, so that the disk list, the
// confirmation and the drawer say the same thing the same way.
QtObject {
    id: words

    // Binary units, as the confirmation screen of the design shows them
    // (desktop-experience.md, 11.5): "931.5 GiB".
    function size(bytes: real): string {
        const units = ["B", "KiB", "MiB", "GiB", "TiB"]
        let value = bytes
        let unit = 0
        while (value >= 1024 && unit < units.length - 1) {
            value /= 1024
            unit++
        }
        return unit === 0 ? Math.round(value) + " " + units[0] : value.toFixed(1) + " " + units[unit]
    }

    function connection(disk: var): string {
        const transport = String(disk.transport || "").toLowerCase()
        if (transport === "nvme")
            return qsTr("NVMe")
        if (transport === "sata" || transport === "ata")
            return qsTr("SATA")
        if (transport === "usb")
            return qsTr("USB")
        if (transport === "mmc")
            return qsTr("memory card")
        if (String(disk.name || "").startsWith("vd"))
            return qsTr("virtual disk")
        if (transport.length > 0)
            return transport.toUpperCase()
        return qsTr("connection not reported")
    }

    // The disk's model; a disk that reports none (a virtual disk, some USB
    // enclosures) is named by the end of its serial number instead.
    function diskName(disk: var): string {
        const model = String(disk.model || "").trim()
        if (model.length > 0)
            return model
        const serial = String(disk.serial || disk.wwn || "")
        if (serial.length > 0)
            return qsTr("Disk …%1").arg(serial.length > 8 ? serial.slice(-8) : serial)
        return qsTr("Unnamed disk")
    }

    // "Samsung SSD 980 PRO · 931.5 GiB · NVMe"
    function diskLine(disk: var): string {
        return words.diskName(disk) + " · " + words.size(disk.size || 0) + " · " + words.connection(disk)
    }

    function serialLine(disk: var): string {
        const serial = String(disk.serial || disk.wwn || "")
        if (serial.length === 0)
            return qsTr("No serial number reported")
        const end = serial.length > 8 ? serial.slice(-8) : serial
        return qsTr("Serial number ends in %1").arg(end)
    }

    function choosable(disk: var): bool {
        return disk.status === "ok"
    }

    // What is on a disk, in plain words.
    function contents(disk: var): string {
        if (disk.status === "installer-medium")
            return qsTr("This is the medium TUR is running from. It cannot be chosen.")
        const found = []
        if (disk.windows)
            found.push(qsTr("Windows"))
        if (disk.bitlocker)
            found.push(qsTr("data encrypted with BitLocker"))
        if (!disk.windows && (disk.ntfs || 0) > 0)
            found.push(qsTr("Windows files (NTFS)"))
        if ((disk.linux || 0) > 0)
            found.push(qsTr("Linux"))
        if (found.length > 0)
            return qsTr("Holds %1.").arg(found.join(qsTr(", ")))
        if ((disk.partition_count || 0) === 0)
            return qsTr("Empty: no partitions.")
        return qsTr("Holds partitions with data TUR does not recognise.")
    }

    // Why a disk cannot be chosen, when it cannot.
    function refusal(disk: var): string {
        switch (disk.status) {
        case "ok":
        case "installer-medium":
            return ""
        case "too-small":
            return qsTr("Too small: TUR needs at least %1.").arg(words.size(disk.minimum || 21474836480))
        case "read-only":
            return qsTr("The disk is read-only.")
        case "in-use":
            return qsTr("Something on the disk is in use.")
        case "no-stable-identity":
            return qsTr("The disk reports no serial number, so it cannot be told apart from others safely.")
        case "no-medium":
            return qsTr("The drive holds no medium.")
        default:
            return String(disk.why || "")
        }
    }

    // Installing into the free space beside Windows (ADR-0028): the engine
    // lists, for a disk that holds Windows, whether it can be done and why not.
    function besidePossible(disk: var): bool {
        return disk !== null && disk !== undefined && disk.beside !== undefined && disk.beside !== null
               && disk.beside.possible === true
    }

    // Why TUR cannot go beside Windows on this disk, in words, when it cannot.
    function besideWhyNot(disk: var): string {
        if (disk === null || disk === undefined || !disk.windows)
            return qsTr("Only on a disk that holds Windows.")
        const room = disk.beside || {}
        switch (String(room.code || "")) {
        case "":
            return room.possible === true ? "" : qsTr("Not available on this disk.")
        case "TUR-INSTALL-BESIDE-001":
            return qsTr("Windows on this disk starts the old BIOS way. TUR starts only the UEFI way.")
        case "TUR-INSTALL-BESIDE-002":
            return qsTr("TUR could not find exactly one EFI system partition with Windows' start-up files on this disk.")
        case "TUR-INSTALL-BESIDE-003":
            return qsTr("Not enough free space: the largest free space is %1, and TUR needs %2. Make room in Windows first: open Disk Management, right-click the Windows partition and choose Shrink Volume.")
                .arg(words.size(room.free_size || 0)).arg(words.size(disk.minimum || 21474836480))
        case "TUR-INSTALL-BESIDE-004":
            return qsTr("Windows' EFI system partition is too full for TUR's start-up files: %1 free, %2 needed.")
                .arg(words.size(room.esp_free || 0)).arg(words.size(room.esp_needed || 0))
        case "TUR-INSTALL-BESIDE-005":
            return qsTr("Windows' EFI system partition already holds another Linux system's start-up files, which TUR's would replace.")
        default:
            return String(room.why || "")
        }
    }

    function holdsData(disk: var): bool {
        return (disk.partition_count || 0) > 0
    }

    function partitionKind(kind: string): string {
        switch (kind) {
        case "windows-boot":
            return qsTr("Windows start-up files")
        case "esp":
            return qsTr("EFI system")
        case "bios-boot":
            return qsTr("BIOS boot")
        case "microsoft-reserved":
            return qsTr("Microsoft reserved")
        case "windows-recovery":
            return qsTr("Windows recovery")
        case "ntfs":
            return qsTr("Windows files (NTFS)")
        case "bitlocker":
            return qsTr("Encrypted with BitLocker")
        case "linux":
            return qsTr("Linux")
        case "fat":
            return qsTr("FAT")
        case "optical":
            return qsTr("Installer medium")
        case "unknown":
            return qsTr("No recognised filesystem")
        default:
            return qsTr("Other filesystem")
        }
    }

    // "2 · Windows files (NTFS) · 930.0 GiB · Windows (C:)"
    function partitionLine(part: var): string {
        const pieces = [String(part.number || part.name || ""), words.partitionKind(part.kind), words.size(part.size || 0)]
        const name = String(part.label || part.partlabel || "")
        if (name.length > 0)
            pieces.push("“" + name + "”")
        return pieces.join(" · ")
    }

    // The engine's code for a check, as a sentence.
    function check(item: var, facts: var): string {
        switch (item.id) {
        case "boot-mode":
            return item.state === "ok" ? qsTr("Started in UEFI mode.") : qsTr("Not started in UEFI mode.")
        case "processor":
            return item.state === "ok" ? qsTr("64-bit x86 processor.") : qsTr("This processor is not a 64-bit x86 processor.")
        case "memory":
            return qsTr("Memory: %1.").arg(words.size(facts.memory_bytes || 0))
        case "secure-boot":
            switch (facts.secure_boot) {
            case "enabled":
                return qsTr("Secure Boot is on. TUR starts with it on.")
            case "disabled":
                return qsTr("Secure Boot is off. TUR also starts with it on.")
            case "unsupported":
                return qsTr("This firmware has no Secure Boot.")
            default:
                return qsTr("Secure Boot: its state could not be read.")
            }
        case "payload":
            return item.state === "ok" ? qsTr("The TUR image on this medium is complete and undamaged.")
                                       : qsTr("The TUR image on this medium cannot be used.")
        default:
            return String(item.text || "")
        }
    }
}
