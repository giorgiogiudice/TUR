# GRUB fragment (BSD-3-Clause, not the TUR licence)

Everything in this directory is under the BSD 3-Clause licence in
[`LICENSE`](LICENSE), **not** under the TUR licence.

`08_tur_boot_count.cfg` is a GRUB script fragment. bootupd concatenates it into
`/boot/grub2/grub.cfg` when TUR is installed, and GRUB interprets it as part of
that file. It is therefore code that runs inside another program, which the
TUR licence does not cover (`AGENTS.md`, licence boundary rules; ADR-0003;
ADR-0014). It follows greenboot's `grub2/08_greenboot.cfg`
(https://github.com/fedora-iot/greenboot-rs, tag v0.16.5) and keeps that
project's licence and copyright notice.

The fragment is **contract version 1** of TUR's boot-attempt counting
([boot-health-and-rollback.md](../../../docs/architecture/boot-health-and-rollback.md),
section 5.3). It is written into `grub.cfg` once, at installation, and is not
updated afterwards. Changing it needs an ADR and a migration for installed
machines.

In the image:

| Source | Installed as |
|---|---|
| `08_tur_boot_count.cfg` | `/usr/lib/bootupd/grub2-static/configs.d/08_tur_boot_count.cfg` |
| `LICENSE` | `/usr/share/licenses/tur-grub-boot-count/LICENSE` |
