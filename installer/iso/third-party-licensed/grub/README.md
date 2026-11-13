# GRUB files of the installer medium (BSD-3-Clause, not the TUR licence)

Everything in this directory is under the BSD 3-Clause licence in
[`LICENSE`](LICENSE), **not** under the TUR licence.

`grub.cfg.in` is the boot menu of the installer medium
([ADR-0018](../../../../docs/adr/0018-first-installer-medium.md)). GRUB
interprets it, so it is code that runs inside another program, which the TUR
licence does not cover (`AGENTS.md`, licence boundary rules; ADR-0003). It was
written for TUR and is not derived from another project's file. It carries the
same permissive licence as the other GRUB fragment TUR ships
(`image/third-party-licensed/grub`), which is compatible with GRUB's own
GPL-3.0-or-later.

On the medium:

| Source | Installed as |
|---|---|
| `grub.cfg.in`, with the volume label, the title and the kernel arguments filled in by `installer/iso/build-medium.sh` | `EFI/BOOT/grub.cfg` and `EFI/fedora/grub.cfg`, on the medium's EFI system partition and again in its ISO 9660 filesystem |
| `LICENSE` | `tur-installer/licenses/grub-menu.LICENSE` on the medium |

`EFI/fedora` is the directory Fedora's GRUB binary has built in as its own;
`EFI/BOOT` is where the binary is. GRUB reads whichever it looks for first.
Started from a USB stick, GRUB looks on the EFI system partition; started from
an optical drive, it looks in the disc's ISO 9660 filesystem.

`windows-entry.cfg.in` is the "Windows" entry of TUR's own boot menu on a
computer where TUR was installed beside Windows on the same disk
([ADR-0028](../../../../docs/adr/0028-install-beside-windows.md)). It too is
GRUB script, written for TUR and not derived from another project's file.

| Source | Installed as |
|---|---|
| `windows-entry.cfg.in` | `/usr/lib/tur-installer/grub/windows-entry.cfg.in` in the installer environment; the installer writes it, with `@ESP_UUID@` replaced by the UUID of the shared EFI system partition, to `/boot/grub2/custom.cfg` on TUR's partition |
| `LICENSE` | `/usr/lib/tur-installer/grub/LICENSE` in the installer environment |
