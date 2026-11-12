# Boot menu of the installer medium (BSD-3-Clause, not the TUR licence)

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
| `grub.cfg.in`, with the volume label, the title and the kernel arguments filled in by `installer/iso/build-medium.sh` | `EFI/BOOT/grub.cfg` and `EFI/fedora/grub.cfg` on the medium's EFI system partition |
| `LICENSE` | `tur-installer/licenses/grub-menu.LICENSE` on the medium |

The second copy of the menu is at the path Fedora's GRUB binary has built in
as its own directory; the first is beside the binary. GRUB reads whichever it
looks for first.
