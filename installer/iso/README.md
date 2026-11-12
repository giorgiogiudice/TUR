# Installer medium

The first way to put TUR on a real computer: an image for a USB stick, also
readable as an optical disc, that starts under UEFI and installs the TUR image
it carries onto one whole disk. The decision and what it leaves out are in
[ADR-0018](../../docs/adr/0018-first-installer-medium.md); how to use the
medium is in [the guide](../../docs/guides/install-on-a-test-pc.md); what was
measured is in [TUR-007](../../docs/reports/TUR-007.md).

It is built and tested in CI only (workflow `iso`). Nothing here is built or
started on a development machine.

| Path | What it is | Licence |
|---|---|---|
| `build-medium.sh` | Builds the medium from a TUR image and records what went into it | TUR |
| `medium-manifest.py` | Writes the manifest that travels beside the medium | TUR |
| `Containerfile` | The installer environment: the image to install, plus Fedora's `dracut-live` (with its dependencies `dracut-network` and `fuse`) and the installer | TUR |
| `rootfs/usr/libexec/tur-installer/tur-install` | The installer: a text program on the first console | TUR |
| `rootfs/usr/lib/systemd/system/` | The two units that start it instead of the desktop | TUR |
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

The medium must not be published or passed on while the image contains
Fedora's logos and release packages (issue 11). The `iso` workflow uploads it
only when started by hand with `upload_medium` set.
