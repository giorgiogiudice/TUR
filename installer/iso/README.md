# Installer medium

The first way to put TUR on a real computer: an image for a USB stick, also
readable as an optical disc, that starts under UEFI and installs the TUR image
it carries onto one whole disk. By default it starts a live TUR desktop with
a graphical installer; the text installer is the boot menu's second entry.
The decisions and what they leave out are in
[ADR-0018](../../docs/adr/0018-first-installer-medium.md) and
[ADR-0025](../../docs/adr/0025-live-session-and-graphical-installer.md); how
to use the medium is in [the guide](../../docs/guides/install-on-a-test-pc.md);
what was measured is in [TUR-007](../../docs/reports/TUR-007.md) and
[TUR-009](../../docs/reports/TUR-009.md).

It is built and tested in CI only (workflow `iso`). Nothing here is built or
started on a development machine.

| Path | What it is | Licence |
|---|---|---|
| `build-medium.sh` | Builds the medium from a TUR image and records what went into it | TUR |
| `medium-manifest.py` | Writes the manifest that travels beside the medium | TUR |
| `Containerfile` | The installer environment: the image to install, plus Fedora's `dracut-live` (with its dependencies `dracut-network` and `fuse`), the live session, and both installers; the graphical one is compiled in a first stage made from the image itself | TUR |
| `Containerfile.containerignore` | What the build of the environment is sent (its context is the repository root) | TUR |
| `rootfs/usr/lib/tur-installer/engine.sh` | The installation engine: every check of a disk, the phrase, the one `bootc install` command, keeping the log. Both installers use it | TUR |
| `rootfs/usr/libexec/tur-installer/tur-install` | The text installer, on the first console (boot menu: "Install TUR (text mode)") | TUR |
| `rootfs/usr/libexec/tur-installer/tur-install-engine` | The engine's command line for the graphical installer, which starts it through pkexec, one step at a time | TUR |
| `rootfs/usr/libexec/tur-installer/tur-installer-wizard` | Starts the graphical installer (`desktop/apps/tur-installer`) | TUR |
| `rootfs/usr/libexec/tur-installer/tur-live-transcript` | Copies the graphical installer's state lines to the serial port, for tests/iso | TUR |
| `rootfs/usr/share/polkit-1/` | The engine's polkit action, and the rule that allows it to the live user | TUR |
| `rootfs/etc/plasmalogin.conf.d/`, `rootfs/etc/xdg/autostart/`, `rootfs/usr/lib/tur-installer/live-home/` | The live session: automatic sign-in, the installer opened at sign-in, the live user's settings | TUR |
| `rootfs/usr/lib/systemd/system/` | The text installer's two units, and the state-line copier's | TUR |
| `third-party-licensed/grub/` | The medium's boot menu, interpreted by GRUB | BSD-3-Clause |

## What is on the medium

```text
EFI system partition   Fedora's signed shim and GRUB, unchanged, and the boot menu
/boot                  the image's kernel, unchanged, and an initramfs for starting from the medium
/LiveOS/squashfs.img   the installer environment
/payload               the TUR image that is installed, as an OCI layout
/tur-installer         what the medium is, every file by SHA-256, licences
```

The installer runs the installer the image itself carries:

```sh
bootc install to-disk --source-imgref oci:/run/initramfs/live/payload:payload \
    --target-imgref <name> --generic-image --wipe --filesystem ext4 <the chosen disk>
```

`bootc install` takes the bootloader files, GRUB's configuration fragments and
its installation settings from the root filesystem it runs in. That is why the
installer environment is the image being installed and not a smaller system:
`build-medium.sh` stops if anything `bootc install` reads differs between the
two.

## Building

```sh
PODMAN="sudo podman" SKOPEO="sudo skopeo" \
    installer/iso/build-medium.sh localhost/tur:ci WORK_DIR OUT_DIR
```

`WORK_DIR/medium/tur-installer.iso` is the medium. `OUT_DIR` receives
`tur-installer.manifest.json` and `tur-installer.SHA256SUMS`.

The `iso` workflow uploads the medium only when started by hand with
`upload_medium` set; `release-installer.yml` publishes previews from such a
run. Fedora's marks are replaced in the image (TUR-008); the boot chain is
Fedora's signed shim, GRUB and kernel, unchanged; a legal review of
distribution has not been completed.
