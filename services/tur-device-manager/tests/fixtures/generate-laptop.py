#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""Generate the 'laptop' fixture tree for the tur-device-manager tests.

Usage: tests/fixtures/generate-laptop.py tests/fixtures/laptop

The tree is laid out like the root of a small system: sys/, proc/, etc/,
usr/lib/modules/ and recorded command output in commands/. It is hand-designed
test data, not a capture of a real machine. The generated tree is committed;
run this again only to change the fixture, and commit the result.
"""
import os
import shutil
import sys

root = sys.argv[1]
if os.path.exists(root):
    shutil.rmtree(root)

RELEASE = "6.19.2-300.fc45.x86_64"


def f(path, content):
    full = os.path.join(root, path.lstrip("/"))
    os.makedirs(os.path.dirname(full), exist_ok=True)
    mode = "wb" if isinstance(content, bytes) else "w"
    with open(full, mode) as handle:
        handle.write(content)


def link(path, target):
    full = os.path.join(root, path.lstrip("/"))
    os.makedirs(os.path.dirname(full), exist_ok=True)
    os.symlink(target, full)


def rel(from_dir, to_path):
    return os.path.relpath(to_path, from_dir)


def class_link(class_dir, name, device_dir):
    """/sys/class/<x>/<name> -> device directory."""
    link(f"{class_dir}/{name}", rel(class_dir, device_dir))


def driver_link(device_dir, bus, driver):
    link(f"{device_dir}/driver", rel(device_dir, f"/sys/bus/{bus}/drivers/{driver}"))


# ---------------------------------------------------------------- identity
f("/etc/machine-id", "0123456789abcdef0123456789abcdef\n")
f("/proc/sys/kernel/osrelease", RELEASE + "\n")

# ---------------------------------------------------------------- platform
dmi = "/sys/devices/virtual/dmi/id"
for name, value in {
    "sys_vendor": "Example Computer",
    "product_name": "Notebook 14 G2",
    "product_version": "1.0",
    "product_family": "Notebook 14",
    "chassis_type": "10",
    "board_vendor": "Example Computer",
    "board_name": "NB14G2",
    "bios_vendor": "Example Firmware",
    "bios_version": "N3HET74W (1.46)",
    "bios_date": "03/15/2026",
    "product_serial": "PF2ABCDE",
    "product_uuid": "4c4c4544-0042-3510-8054-b9c04f564433",
    "board_serial": "L1HF2AB01CD",
    "chassis_serial": "PF2ABCDE",
}.items():
    f(f"{dmi}/{name}", value + "\n")
class_link("/sys/class/dmi", "id", dmi)

f(
    "/sys/firmware/efi/efivars/SecureBoot-8be4df61-93ca-11d2-aa0d-00e098032b8c",
    bytes([6, 0, 0, 0, 1]),
)

# ---------------------------------------------------------------- PCI
PCI_ROOT = "/sys/devices/pci0000:00"
pci_dirs = {}


def pci(address, vendor, device, klass, driver, parent=None, sub=("17aa", "22d5"), rev="01"):
    base = pci_dirs[parent] if parent else PCI_ROOT
    d = f"{base}/{address}"
    pci_dirs[address] = d
    f(f"{d}/vendor", f"0x{vendor}\n")
    f(f"{d}/device", f"0x{device}\n")
    f(f"{d}/class", f"0x{klass}\n")
    f(f"{d}/subsystem_vendor", f"0x{sub[0]}\n")
    f(f"{d}/subsystem_device", f"0x{sub[1]}\n")
    f(f"{d}/revision", f"0x{rev}\n")
    f(
        f"{d}/modalias",
        "pci:v0000{}d0000{}sv0000{}sd0000{}bc{}sc{}i{}\n".format(
            vendor.upper(), device.upper(), sub[0].upper(), sub[1].upper(),
            klass[0:2].upper(), klass[2:4].upper(), klass[4:6].upper(),
        ),
    )
    if driver:
        driver_link(d, "pci", driver)
    class_link("/sys/bus/pci/devices", address, d)
    return d


pci("0000:00:00.0", "8086", "9a14", "060000", None)
pci("0000:00:01.0", "8086", "9a01", "060400", "pcieport")
gpu = pci("0000:00:02.0", "8086", "9a49", "030000", "i915")
pci("0000:00:06.0", "8086", "9a09", "060400", "pcieport")
tbt = pci("0000:00:0d.2", "8086", "9a1b", "0c0340", "thunderbolt")
xhci = pci("0000:00:14.0", "8086", "a0ed", "0c0330", "xhci_hcd")
wifi = pci("0000:00:14.3", "8086", "a0f0", "028000", "iwlwifi", sub=("8086", "0074"))
pci("0000:00:16.0", "8086", "a0e0", "078000", None)
ahci = pci("0000:00:17.0", "8086", "a0d3", "010601", "ahci")
pci("0000:00:1c.0", "8086", "a0b8", "060400", "pcieport")
pci("0000:00:1d.0", "8086", "a0b0", "060400", "pcieport")
pci("0000:00:1f.0", "8086", "a082", "060100", None)
hda = pci("0000:00:1f.3", "8086", "a0c8", "040380", "snd_hda_intel")
eth = pci("0000:00:1f.6", "8086", "15fc", "020000", "e1000e")
nvme = pci("0000:01:00.0", "144d", "a80a", "010802", "nvme", parent="0000:00:06.0", sub=("144d", "a801"))
pci("0000:02:00.0", "10ec", "522a", "ff0000", None, parent="0000:00:1c.0")
pci("0000:03:00.0", "10ec", "c822", "028000", "rtw_8822ce", parent="0000:00:1d.0")
nvidia = pci("0000:04:00.0", "10de", "25a2", "030200", None, parent="0000:00:01.0")

# GPU: one DRM card with three connectors, and a render node.
f(f"{gpu}/boot_vga", "1\n")
f(f"{nvidia}/boot_vga", "0\n")
for connector, status, enabled in [
    ("eDP-1", "connected", "enabled"),
    ("HDMI-A-1", "disconnected", "disabled"),
    ("DP-1", "disconnected", "disabled"),
]:
    f(f"{gpu}/drm/card1/card1-{connector}/status", status + "\n")
    f(f"{gpu}/drm/card1/card1-{connector}/enabled", enabled + "\n")
f(f"{gpu}/drm/card1/dev", "226:1\n")
f(f"{gpu}/drm/renderD128/dev", "226:128\n")
class_link("/sys/class/drm", "card1", f"{gpu}/drm/card1")

# ---------------------------------------------------------------- USB
def usb_device(d, name, attrs):
    for key, value in attrs.items():
        f(f"{d}/{key}", value + "\n")
    class_link("/sys/bus/usb/devices", name, d)


def usb_interface(device_dir, name, klass, sub, proto, driver, modalias):
    d = f"{device_dir}/{name}"
    f(f"{d}/bInterfaceClass", klass + "\n")
    f(f"{d}/bInterfaceSubClass", sub + "\n")
    f(f"{d}/bInterfaceProtocol", proto + "\n")
    f(f"{d}/modalias", modalias + "\n")
    if driver:
        driver_link(d, "usb", driver)
    class_link("/sys/bus/usb/devices", name, d)
    return d


usb1 = f"{xhci}/usb1"
usb_device(usb1, "usb1", {
    "idVendor": "1d6b", "idProduct": "0002", "bDeviceClass": "09",
    "manufacturer": f"Linux {RELEASE} xhci-hcd", "product": "xHCI Host Controller",
    "serial": "0000:00:14.0", "speed": "480", "version": " 2.00", "removable": "unknown",
})
usb_interface(usb1, "1-0:1.0", "09", "00", "00", "hub",
              "usb:v1D6Bp0002d0619dc09dsc00dp01ic09isc00ip00in00")

usb2 = f"{xhci}/usb2"
usb_device(usb2, "usb2", {
    "idVendor": "1d6b", "idProduct": "0003", "bDeviceClass": "09",
    "manufacturer": f"Linux {RELEASE} xhci-hcd", "product": "xHCI Host Controller",
    "serial": "0000:00:14.0", "speed": "10000", "version": " 3.10", "removable": "unknown",
})
usb_interface(usb2, "2-0:1.0", "09", "00", "00", "hub",
              "usb:v1D6Bp0003d0619dc09dsc00dp03ic09isc00ip00in00")

# Wireless receiver for a keyboard and mouse: no serial number.
recv = f"{usb1}/1-1"
usb_device(recv, "1-1", {
    "idVendor": "046d", "idProduct": "c52b", "bDeviceClass": "00",
    "manufacturer": "Logitech", "product": "USB Receiver",
    "speed": "12", "version": " 2.00", "removable": "removable",
})
recv0 = usb_interface(recv, "1-1:1.0", "03", "01", "01", "usbhid",
                      "usb:v046DpC52Bd1211dc00dsc00dp00ic03isc01ip01in00")
recv1 = usb_interface(recv, "1-1:1.1", "03", "01", "02", "usbhid",
                      "usb:v046DpC52Bd1211dc00dsc00dp00ic03isc01ip02in01")

# Fingerprint reader: vendor-specific interface, no kernel driver.
fp = f"{usb1}/1-3"
usb_device(fp, "1-3", {
    "idVendor": "06cb", "idProduct": "00bd", "bDeviceClass": "ff",
    "serial": "f2c8d1a04e77", "speed": "12", "version": " 2.00", "removable": "fixed",
})
usb_interface(fp, "1-3:1.0", "ff", "00", "00", None,
              "usb:v06CBp00BDd0000dcFFdsc10dpFFicFFisc00ip00in00")

# Built-in camera.
cam = f"{usb1}/1-5"
usb_device(cam, "1-5", {
    "idVendor": "04f2", "idProduct": "b6ea", "bDeviceClass": "ef",
    "manufacturer": "Chicony Electronics Co.,Ltd.", "product": "Integrated Camera",
    "serial": "0001", "speed": "480", "version": " 2.01", "removable": "fixed",
})
cam0 = usb_interface(cam, "1-5:1.0", "0e", "01", "00", "uvcvideo",
                     "usb:v04F2pB6EAd2711dcEFdsc02dp01ic0Eisc01ip00in00")
usb_interface(cam, "1-5:1.1", "0e", "02", "00", "uvcvideo",
              "usb:v04F2pB6EAd2711dcEFdsc02dp01ic0Eisc02ip00in01")

# Bluetooth adapter.
bt = f"{usb1}/1-10"
usb_device(bt, "1-10", {
    "idVendor": "8087", "idProduct": "0026", "bDeviceClass": "e0",
    "speed": "12", "version": " 2.01", "removable": "fixed",
})
bt0 = usb_interface(bt, "1-10:1.0", "e0", "01", "01", "btusb",
                    "usb:v8087p0026d0002dcE0dsc01dp01icE0isc01ip01in00")
usb_interface(bt, "1-10:1.1", "e0", "01", "01", "btusb",
              "usb:v8087p0026d0002dcE0dsc01dp01icE0isc01ip01in01")

# Flash drive with a serial number.
stick = f"{usb2}/2-1"
usb_device(stick, "2-1", {
    "idVendor": "0781", "idProduct": "5581", "bDeviceClass": "00",
    "manufacturer": "SanDisk", "product": "Ultra",
    "serial": "4C530001230915112345", "speed": "5000", "version": " 3.00",
    "removable": "removable",
})
stick0 = usb_interface(stick, "2-1:1.0", "08", "06", "50", "usb-storage",
                       "usb:v0781p5581d0100dc00dsc00dp00ic08isc06ip50in00")

# USB Ethernet adapter: udev names its interface after its MAC address.
nic = f"{usb2}/2-2"
usb_device(nic, "2-2", {
    "idVendor": "0bda", "idProduct": "8153", "bDeviceClass": "00",
    "manufacturer": "Realtek", "product": "USB 10/100/1000 LAN",
    "serial": "001000001", "speed": "5000", "version": " 3.00", "removable": "removable",
})
nic0 = usb_interface(nic, "2-2:1.0", "ff", "ff", "00", "r8152",
                     "usb:v0BDAp8153d3100dc00dsc00dp00icFFiscFFip00in00")

# ---------------------------------------------------------------- network
def net(name, device_dir, attrs, uevent, parent_link=True):
    d = f"{device_dir}/net/{name}" if parent_link else f"/sys/devices/virtual/net/{name}"
    for key, value in attrs.items():
        f(f"{d}/{key}", value + "\n")
    f(f"{d}/uevent", uevent)
    if parent_link:
        link(f"{d}/device", "../../../" + os.path.basename(device_dir))
    class_link("/sys/class/net", name, d)
    return d


net("enp0s31f6", eth,
    {"type": "1", "operstate": "down", "address": "8c:16:45:aa:bb:01", "addr_assign_type": "0"},
    "INTERFACE=enp0s31f6\nIFINDEX=2\n")
wlan = net("wlp0s20f3", wifi,
           {"type": "1", "operstate": "up", "address": "4c:79:6e:aa:bb:02", "addr_assign_type": "0"},
           "DEVTYPE=wlan\nINTERFACE=wlp0s20f3\nIFINDEX=3\n")
link(f"{wlan}/phy80211", "../../ieee80211/phy0")
net("enx00e04c680001", nic0,
    {"type": "1", "operstate": "up", "address": "00:e0:4c:68:00:01", "addr_assign_type": "0"},
    "INTERFACE=enx00e04c680001\nIFINDEX=5\n")
net("lo", None,
    {"type": "772", "operstate": "unknown", "address": "00:00:00:00:00:00", "addr_assign_type": "0"},
    "INTERFACE=lo\nIFINDEX=1\n", parent_link=False)
net("virbr0", None,
    {"type": "1", "operstate": "down", "address": "52:54:00:12:34:56", "addr_assign_type": "3"},
    "DEVTYPE=bridge\nINTERFACE=virbr0\nIFINDEX=4\n", parent_link=False)
# A plain file that lives in /sys/class/net on real systems.
f("/sys/class/net/bonding_masters", "\n")

# ---------------------------------------------------------------- audio
card = f"{hda}/sound/card0"
f(f"{card}/id", "PCH\n")
f(f"{card}/number", "0\n")
link(f"{card}/device", "../../../0000:00:1f.3")
class_link("/sys/class/sound", "card0", card)
f(f"{card}/controlC0/dev", "116:0\n")
class_link("/sys/class/sound", "controlC0", f"{card}/controlC0")
f(
    "/proc/asound/cards",
    " 0 [PCH            ]: HDA-Intel - HDA Intel PCH\n"
    "                      HDA Intel PCH at 0x603d1a8000 irq 163\n",
)

# ---------------------------------------------------------------- block
def block(name, d, attrs):
    for key, value in attrs.items():
        f(f"{d}/{key}", value + "\n")
    class_link("/sys/class/block", name, d)


nvme_ctrl = f"{nvme}/nvme/nvme0"
f(f"{nvme_ctrl}/model", "Samsung SSD 980 PRO 1TB                 \n")
f(f"{nvme_ctrl}/serial", "S5GXNX0R123456      \n")
ns = f"{nvme_ctrl}/nvme0n1"
block("nvme0n1", ns, {
    "size": "1953525168", "removable": "0", "queue/rotational": "0",
    "wwid": "eui.0025385b21404321",
})
link(f"{ns}/device", "../../nvme0")
block("nvme0n1p1", f"{ns}/nvme0n1p1", {"partition": "1", "size": "1228800"})
block("nvme0n1p2", f"{ns}/nvme0n1p2", {"partition": "2", "size": "1952294912"})

scsi0 = f"{ahci}/ata1/host0/target0:0:0/0:0:0:0"
f(f"{scsi0}/model", "Samsung SSD 860 \n")
f(f"{scsi0}/vendor", "ATA     \n")
f(f"{scsi0}/wwid", "naa.5002538e40a1b2c3\n")
driver_link(scsi0, "scsi", "sd")
sda = f"{scsi0}/block/sda"
block("sda", sda, {"size": "976773168", "removable": "0", "queue/rotational": "0"})
link(f"{sda}/device", "../../../0:0:0:0")
block("sda1", f"{sda}/sda1", {"partition": "1", "size": "976771072"})

scsi1 = f"{stick0}/host1/target1:0:0/1:0:0:0"
f(f"{scsi1}/model", "Ultra           \n")
f(f"{scsi1}/vendor", "SanDisk \n")
driver_link(scsi1, "scsi", "sd")
sdb = f"{scsi1}/block/sdb"
block("sdb", sdb, {"size": "60062500", "removable": "1", "queue/rotational": "1"})
link(f"{sdb}/device", "../../../1:0:0:0")

block("loop0", "/sys/devices/virtual/block/loop0", {"size": "0", "removable": "0"})
block("dm-0", "/sys/devices/virtual/block/dm-0", {"size": "1952290816", "removable": "0"})

# ---------------------------------------------------------------- input
def input_dev(number, d, name, phys, uniq, bus, vendor, product, version):
    f(f"{d}/name", name + "\n")
    f(f"{d}/phys", phys + "\n")
    f(f"{d}/uniq", uniq + "\n")
    f(f"{d}/id/bustype", bus + "\n")
    f(f"{d}/id/vendor", vendor + "\n")
    f(f"{d}/id/product", product + "\n")
    f(f"{d}/id/version", version + "\n")
    class_link("/sys/class/input", f"input{number}", d)
    f(f"{d}/event{number}/dev", f"13:{64 + number}\n")
    class_link("/sys/class/input", f"event{number}", f"{d}/event{number}")
    return d


in0 = input_dev(0, "/sys/devices/LNXSYSTM:00/LNXPWRBN:00/input/input0",
                "Power Button", "LNXPWRBN/button/input0", "", "0019", "0000", "0001", "0000")
in3 = input_dev(3, "/sys/devices/platform/i8042/serio0/input/input3",
                "AT Translated Set 2 keyboard", "isa0060/serio0/input0", "",
                "0011", "0001", "0001", "ab83")
in5 = input_dev(5, f"{recv0}/0003:046D:C52B.0001/input/input5",
                "Logitech USB Receiver", "usb-0000:00:14.0-1/input0", "",
                "0003", "046d", "c52b", "0111")
in6 = input_dev(6, f"{recv1}/0003:046D:C52B.0002/input/input6",
                "Logitech USB Receiver Mouse", "usb-0000:00:14.0-1/input1", "",
                "0003", "046d", "c52b", "0111")
# A Bluetooth mouse: phys is the adapter's address, uniq the mouse's.
in12 = input_dev(12, "/sys/devices/virtual/misc/uhid/0005:046D:B023.0004/input/input12",
                 "MX Anywhere 3", "a4:c3:f0:11:22:33", "d0:ab:cd:44:55:66",
                 "0005", "046d", "b023", "0015")
f("/sys/devices/virtual/input/mice/dev", "13:63\n")
class_link("/sys/class/input", "mice", "/sys/devices/virtual/input/mice")

# ---------------------------------------------------------------- bluetooth
hci = f"{bt0}/bluetooth/hci0"
f(f"{hci}/rfkill1/soft", "0\n")
f(f"{hci}/rfkill1/hard", "0\n")
f(f"{hci}/rfkill1/type", "bluetooth\n")
link(f"{hci}/device", "../../../1-10:1.0")
class_link("/sys/class/bluetooth", "hci0", hci)
f(f"{hci}/hci0:256/uevent", "DEVTYPE=link\n")
class_link("/sys/class/bluetooth", "hci0:256", f"{hci}/hci0:256")

# ---------------------------------------------------------------- cameras
for index in (0, 1):
    v = f"{cam0}/video4linux/video{index}"
    f(f"{v}/name", "Integrated Camera: Integrated C\n")
    f(f"{v}/index", f"{index}\n")
    link(f"{v}/device", "../../../1-5:1.0")
    class_link("/sys/class/video4linux", f"video{index}", v)

# ---------------------------------------------------------------- thunderbolt
dom = f"{tbt}/domain0"
f(f"{dom}/security", "user\n")
class_link("/sys/bus/thunderbolt/devices", "domain0", dom)
host = f"{dom}/0-0"
for key, value in {
    "vendor_name": "Example Computer", "device_name": "Notebook 14 G2",
    "vendor": "0x109", "device": "0x1602",
    "unique_id": "c5010000-0070-6d18-23e4-1a2b3c4d5e6f", "authorized": "1",
}.items():
    f(f"{host}/{key}", value + "\n")
class_link("/sys/bus/thunderbolt/devices", "0-0", host)
dock = f"{host}/0-1"
for key, value in {
    "vendor_name": "CalDigit, Inc.", "device_name": "TS3 Plus",
    "vendor": "0x3d", "device": "0x11",
    "unique_id": "00a1b2c3-0092-8f18-ffff-ffffffffffff", "authorized": "0",
}.items():
    f(f"{dock}/{key}", value + "\n")
class_link("/sys/bus/thunderbolt/devices", "0-1", dock)
f(f"{host}/0-0:1.1/nvm_version", "1.0\n")
class_link("/sys/bus/thunderbolt/devices", "0-0:1.1", f"{host}/0-0:1.1")

# ---------------------------------------------------------------- modules
for module in ["i915", "e1000e", "iwlwifi", "snd_hda_intel", "nvme", "ahci", "xhci_pci",
               "usbhid", "uvcvideo", "btusb", "r8152", "usb_storage", "rtw88_8822ce",
               "thunderbolt", "nouveau"]:
    f(f"/sys/module/{module}/initstate", "live\n")

f(f"/usr/lib/modules/{RELEASE}/modules.alias", """# Aliases extracted from modules themselves.
alias pci:v00008086d00009A49sv*sd*bc03sc*i* i915
alias pci:v00008086d000015FCsv*sd*bc*sc*i* e1000e
alias pci:v00008086d0000A0F0sv*sd*bc*sc*i* iwlwifi
alias pci:v00008086d0000A0C8sv*sd*bc*sc*i* snd_hda_intel
alias pci:v00008086d0000A0C8sv*sd*bc*sc*i* snd_sof_pci_intel_tgl
alias pci:v*d*sv*sd*bc01sc08i02* nvme
alias pci:v*d*sv*sd*bc01sc06i01* ahci
alias pci:v*d*sv*sd*bc0Csc03i30* xhci_pci
alias pci:v*d*sv*sd*bc0Csc03i40* thunderbolt
alias pci:v000010ECd0000522Asv*sd*bc*sc*i* rtsx_pci
alias pci:v000010ECd0000C822sv*sd*bc*sc*i* rtw88_8822ce
alias pci:v000010DEd*sv*sd*bc03sc*i* nouveau
alias usb:v*p*d*dc*dsc*dp*ic03isc*ip*in* usbhid
alias usb:v*p*d*dc*dsc*dp*ic0Eisc01ip00in* uvcvideo
alias usb:v8087p0026d*dc*dsc*dp*ic*isc*ip*in* btusb
alias usb:v0BDAp8153d*dc*dsc*dp*ic*isc*ip*in* r8152
alias usb:v*p*d*dc*dsc*dp*ic08isc06ip50in* usb-storage
alias usb:v*p*d*dc*dsc*dp*ic08isc06ip62in* uas
""")

# ---------------------------------------------------------------- commands
def udev_record(path, props):
    lines = [f"P: {path[len('/sys'):]}"]
    lines += [f"E: {key}={value}" for key, value in props.items()]
    return "\n".join(lines) + "\n\n"


udev = ""
udev += udev_record(gpu, {
    "SUBSYSTEM": "pci", "DRIVER": "i915",
    "ID_PCI_CLASS_FROM_DATABASE": "Display controller",
    "ID_VENDOR_FROM_DATABASE": "Intel Corporation",
    "ID_MODEL_FROM_DATABASE": "TigerLake-LP GT2 [Iris Xe Graphics]",
})
udev += udev_record(eth, {
    "SUBSYSTEM": "pci", "DRIVER": "e1000e",
    "ID_PCI_CLASS_FROM_DATABASE": "Network controller",
    "ID_VENDOR_FROM_DATABASE": "Intel Corporation",
    "ID_MODEL_FROM_DATABASE": "Ethernet Connection (13) I219-V",
})
udev += udev_record(wifi, {
    "SUBSYSTEM": "pci", "DRIVER": "iwlwifi",
    "ID_PCI_CLASS_FROM_DATABASE": "Network controller",
    "ID_VENDOR_FROM_DATABASE": "Intel Corporation",
    "ID_MODEL_FROM_DATABASE": "Wi-Fi 6 AX201",
})
udev += udev_record(pci_dirs["0000:03:00.0"], {
    "SUBSYSTEM": "pci", "DRIVER": "rtw_8822ce",
    "ID_PCI_CLASS_FROM_DATABASE": "Network controller",
    "ID_VENDOR_FROM_DATABASE": "Realtek Semiconductor Co., Ltd.",
    "ID_MODEL_FROM_DATABASE": "RTL8822CE 802.11ac PCIe Wireless Network Adapter",
})
udev += udev_record(nvidia, {
    "SUBSYSTEM": "pci",
    "ID_PCI_CLASS_FROM_DATABASE": "Display controller",
    "ID_VENDOR_FROM_DATABASE": "NVIDIA Corporation",
    "ID_MODEL_FROM_DATABASE": "TU117M [GeForce MX550]",
})
udev += udev_record(in0, {"SUBSYSTEM": "input", "ID_INPUT": "1", "ID_INPUT_KEY": "1"})
udev += udev_record(in3, {"SUBSYSTEM": "input", "ID_INPUT": "1", "ID_INPUT_KEY": "1",
                          "ID_INPUT_KEYBOARD": "1"})
udev += udev_record(in5, {"SUBSYSTEM": "input", "ID_INPUT": "1", "ID_INPUT_KEY": "1",
                          "ID_INPUT_KEYBOARD": "1", "ID_BUS": "usb"})
udev += udev_record(in6, {"SUBSYSTEM": "input", "ID_INPUT": "1", "ID_INPUT_MOUSE": "1",
                          "ID_BUS": "usb"})
udev += udev_record(in12, {"SUBSYSTEM": "input", "ID_INPUT": "1", "ID_INPUT_MOUSE": "1",
                           "ID_BUS": "bluetooth"})
udev += udev_record(sda, {"SUBSYSTEM": "block", "DEVTYPE": "disk",
                          "ID_SERIAL_SHORT": "S3Z9NB0K123456", "ID_WWN": "0x5002538e40a1b2c3"})
udev += udev_record(sdb, {"SUBSYSTEM": "block", "DEVTYPE": "disk",
                          "ID_SERIAL_SHORT": "4C530001230915112345"})
udev += udev_record(f"{cam0}/video4linux/video0", {"SUBSYSTEM": "video4linux",
                                                   "ID_V4L_CAPABILITIES": ":capture:"})
udev += udev_record(f"{cam0}/video4linux/video1", {"SUBSYSTEM": "video4linux",
                                                   "ID_V4L_CAPABILITIES": ":"})
f("/commands/udevadm-info-export-db.txt", udev)

f("/commands/journalctl-kernel.txt", f"""Linux version {RELEASE} (mockbuild@example) #1 SMP PREEMPT_DYNAMIC
pci 0000:04:00.0: [10de:25a2] type 00 class 0x030200 PCIe Endpoint
e1000e 0000:00:1f.6 enp0s31f6: renamed from eth0
iwlwifi 0000:00:14.3: Direct firmware load for iwlwifi-QuZ-a0-hr-b0-78.ucode failed with error -2
iwlwifi 0000:00:14.3: loaded firmware version 77.2df8986f.0 QuZ-a0-hr-b0-77.ucode op_mode iwlmvm
rtw_8822ce 0000:03:00.0: Direct firmware load for rtw88/rtw8822c_fw.bin failed with error -2
rtw_8822ce 0000:03:00.0: failed to request firmware
rtw_8822ce 0000:03:00.0: Direct firmware load for rtw88/rtw8822c_fw.bin failed with error -2
pci 0000:04:00.0: probe with driver nouveau failed with error -12
usb 2-1: SerialNumber: 4C530001230915112345
r8152 2-2:1.0 enx00e04c680001: renamed from eth1
""")

f("/commands/fwupdmgr-get-devices.json", """{
  "Devices" : [
    {
      "Name" : "System Firmware",
      "DeviceId" : "a45df35ac0e948ee180fe216a5f703f32dda163f",
      "Guid" : [ "7ceaf7a8-0611-4480-9e30-64d8de420c7c" ],
      "Plugin" : "uefi_capsule",
      "Flags" : [ "internal", "updatable", "require-ac", "supported", "registered", "needs-reboot" ],
      "Vendor" : "Example Computer",
      "Version" : "1.46",
      "VersionFormat" : "pair",
      "UpdateState" : 0,
      "Releases" : [
        {
          "Version" : "1.52",
          "RemoteId" : "lvfs",
          "Urgency" : "high",
          "Flags" : [ "trusted-payload", "trusted-metadata", "is-upgrade" ]
        },
        {
          "Version" : "1.46",
          "RemoteId" : "lvfs",
          "Urgency" : "medium",
          "Flags" : [ "trusted-payload", "trusted-metadata" ]
        }
      ]
    },
    {
      "Name" : "Samsung SSD 980 PRO 1TB",
      "DeviceId" : "71b677ca0f1bc2c5b804fa1d59e52064ce589293",
      "Serial" : "S5GXNX0R123456",
      "Plugin" : "nvme",
      "Flags" : [ "internal", "updatable", "require-ac", "supported", "registered", "needs-reboot" ],
      "Vendor" : "Samsung",
      "Version" : "5B2QGXA7",
      "VersionFormat" : "plain"
    },
    {
      "Name" : "Integrated Camera",
      "DeviceId" : "08d460be0f1f9f128413f816022a6439e0078018",
      "Plugin" : "usb",
      "Flags" : [ "registered" ],
      "Vendor" : "Chicony Electronics Co.,Ltd.",
      "Version" : "27.11"
    }
  ]
}
""")

print("fixture written to", root)
