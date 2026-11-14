# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""The QEMU command line, from one description, for the guest and for the CI stand-in PC.

Only devices Windows has drivers for out of the box: an NVMe or AHCI disk, an
Intel e1000e network card, xHCI USB, a standard VGA adapter, a 16550 serial
port, a TPM 2.0 interface, and (for the guest) Hyper-V's bus with its
dynamic-memory device. No virtio device: an unmodified Windows has no driver
for one.
"""

from __future__ import annotations

import os
import pathlib

FIRMWARE = [
    # (code with Secure Boot support, blank variables, variables with Microsoft's keys enrolled)
    ("/usr/share/OVMF/OVMF_CODE_4M.secboot.fd", "/usr/share/OVMF/OVMF_VARS_4M.fd", "/usr/share/OVMF/OVMF_VARS_4M.ms.fd"),
    ("/usr/share/edk2/ovmf/OVMF_CODE_4M.secboot.qcow2", None, None),
    ("/usr/share/edk2/ovmf/OVMF_CODE.secboot.fd", "/usr/share/edk2/ovmf/OVMF_VARS.fd",
     "/usr/share/edk2/ovmf/OVMF_VARS.secboot.fd"),
]

# Hyper-V enlightenments, most complete first. The launcher falls back to the
# next set when QEMU or the kernel refuses one.
HYPERV_SETS = [
    ["hv-relaxed", "hv-vapic", "hv-spinlocks=0x1fff", "hv-time", "hv-vpindex", "hv-synic", "hv-stimer",
     "hv-runtime", "hv-reset", "hv-frequencies", "hv-tlbflush", "hv-ipi"],
    ["hv-relaxed", "hv-vapic", "hv-spinlocks=0x1fff", "hv-time", "hv-vpindex", "hv-synic", "hv-stimer"],
    [],
]

DMI_FIELDS = {
    "manufacturer": "sys_vendor", "product": "product_name", "version": "product_version",
    "serial": "product_serial", "uuid": "product_uuid", "sku": "product_sku", "family": "product_family",
    "board_manufacturer": "board_vendor", "board_product": "board_name", "board_version": "board_version",
    "board_serial": "board_serial", "chassis_manufacturer": "chassis_vendor", "chassis_serial": "chassis_serial",
    "bios_vendor": "bios_vendor", "bios_version": "bios_version", "bios_date": "bios_date",
}


def firmware() -> dict:
    for code, blank, enrolled in FIRMWARE:
        if blank and os.path.exists(code) and os.path.exists(blank):
            return {"code": code, "vars_blank": blank,
                    "vars_enrolled": enrolled if enrolled and os.path.exists(enrolled) else None}
    raise FileNotFoundError("no OVMF firmware with Secure Boot support was found")


def host_identity() -> dict:
    """This computer's SMBIOS strings (root is needed for the serial numbers and the UUID)."""
    out = {}
    for key, name in DMI_FIELDS.items():
        try:
            value = pathlib.Path("/sys/class/dmi/id", name).read_text().strip()
        except OSError:
            continue
        if value:
            out[key] = value
    return out


def _escape(value: str) -> str:
    return str(value).replace(",", ",,")


def _smbios(identity: dict) -> list[str]:
    def group(kind: int, mapping: dict[str, str]) -> list[str]:
        parts = [f"{option}={_escape(identity[key])}" for option, key in mapping.items() if identity.get(key)]
        return ["-smbios", f"type={kind}," + ",".join(parts)] if parts else []

    return (group(0, {"vendor": "bios_vendor", "version": "bios_version", "date": "bios_date"})
            + group(1, {"manufacturer": "manufacturer", "product": "product", "version": "version",
                        "serial": "serial", "uuid": "uuid", "sku": "sku", "family": "family"})
            + group(2, {"manufacturer": "board_manufacturer", "product": "board_product",
                        "version": "board_version", "serial": "board_serial"})
            + group(3, {"manufacturer": "chassis_manufacturer", "serial": "chassis_serial"}))


def build(cfg: dict) -> list[str]:
    """The argument list. `cfg` is plain data; see the keys read below."""
    cpu = "host" + "".join("," + flag for flag in cfg.get("hyperv", []))
    if cfg.get("cpu_extra"):
        cpu += "," + cfg["cpu_extra"]
    cpus = int(cfg.get("cpus", 2))
    args = [
        cfg.get("binary", "qemu-system-x86_64"),
        "-name", cfg.get("name", "tur-dualrun"),
        "-nodefaults", "-no-user-config",
        "-machine", "q35,accel=kvm,smm=on",
        "-cpu", cpu,
        "-smp", f"{cpus},sockets=1,cores={cpus},threads=1",
        "-m", str(int(cfg.get("memory_mb", 4096))),
        "-global", "driver=cfi.pflash01,property=secure,value=on",
        "-drive", f"if=pflash,format=raw,unit=0,readonly=on,file={cfg['code']}",
        "-drive", f"if=pflash,format=raw,unit=1,file={cfg['vars']}",
        "-rtc", "base=utc",
        "-vga", "std",
        "-display", cfg.get("display", "none"),
        "-device", "qemu-xhci,id=xhci",
        "-device", "usb-tablet,bus=xhci.0",
        "-msg", "timestamp=on",
    ]
    if cfg.get("no_sleep_states"):
        # No S3 and no S4: the guest cannot sleep or hibernate, and Fast Startup
        # cannot leave a hibernation image made on virtual hardware.
        args += ["-global", "ICH9-LPC.disable_s3=1", "-global", "ICH9-LPC.disable_s4=1"]
    args += _smbios(cfg.get("identity") or {})
    for table in cfg.get("acpi_tables", []):
        args += ["-acpitable", f"file={table}"]

    disk = cfg["disk"]
    drive = (f"file={disk['path']},if=none,id=win,format={disk.get('format', 'raw')},"
             f"cache={disk.get('cache', 'none')},discard=unmap")
    if disk.get("readonly"):
        drive += ",readonly=on"
    args += ["-drive", drive]
    serial = _escape(disk.get("serial", "TURDUALRUN0001"))
    boot = f",bootindex={disk['bootindex']}" if "bootindex" in disk else ""
    if disk.get("controller", "nvme") == "nvme":
        args += ["-device", f"nvme,drive=win,serial={serial}{boot}"]
    else:
        args += ["-device", f"ide-hd,drive=win,bus=ide.0,serial={serial}{boot}"]

    for number, cd in enumerate(cfg.get("cdroms", [])):
        args += ["-drive", f"file={cd['path']},if=none,id=cd{number},media=cdrom,readonly=on",
                 "-device", f"ide-cd,drive=cd{number},bus=ide.{number + 1}"
                 + (f",bootindex={cd['bootindex']}" if "bootindex" in cd else "")]
    for number, stick in enumerate(cfg.get("usb_disks", [])):
        args += ["-drive", f"file={stick['path']},if=none,id=usb{number},format=raw",
                 "-device", f"usb-storage,drive=usb{number},bus=xhci.0,removable=on,id=usbdev{number}"]

    if cfg.get("tpm_socket"):
        args += ["-chardev", f"socket,id=chrtpm,path={cfg['tpm_socket']}",
                 "-tpmdev", "emulator,id=tpm0,chardev=chrtpm", "-device", "tpm-crb,tpmdev=tpm0"]
    net = cfg.get("net")
    if net is not None:
        netdev = "user,id=n0,restrict=" + ("on" if net.get("restrict", True) else "off")
        for forward in net.get("hostfwd", []):
            netdev += f",hostfwd={forward}"
        args += ["-netdev", netdev, "-device", f"e1000e,netdev=n0,mac={net.get('mac', '52:54:00:d0:a1:01')},romfile="]
    if cfg.get("balloon"):
        args += ["-device", "vmbus-bridge", "-device", "hv-balloon,id=hvb"]
    if cfg.get("qmp"):
        args += ["-qmp", f"unix:{cfg['qmp']},server=on,wait=off"]
    if cfg.get("serial"):
        args += ["-chardev", f"socket,id=agent,path={cfg['serial']},server=on,wait=off", "-serial", "chardev:agent"]
    if cfg.get("pidfile"):
        args += ["-pidfile", cfg["pidfile"]]
    if cfg.get("incoming"):
        args += ["-incoming", cfg["incoming"]]
    args += cfg.get("extra", [])
    return args
