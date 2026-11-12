# Licence review

This review covers the licences in the built TUR image, the obligations that
distributing the image would create, and how TUR's noncommercial licence
([ADR-0003](../adr/0003-licence.md)) sits beside the components TUR ships with.

It is not legal advice. Its findings are input to the lawyer's review that the
specification requires before public distribution (§17.2); the questions that
review needs to answer are collected in [section 6](#6-open-questions-for-legal-review).

Companion documents: [upstream-projects.md](upstream-projects.md) (one row per
upstream project, with sources) and [image-licences.csv](image-licences.csv)
(one row per package in the image).

## 1. What was examined

| Item | Value |
|---|---|
| Image | TUR 0.1.0-dev, build `20261112.gdf32bacdf174`, CI run 43147702795 |
| Base | `quay.io/fedora/fedora-kinoite:45`, version 45.20261112.0 |
| SBOM | `sbom.spdx.json` from the `tur-image-metadata` artifact (SPDX 2.3, syft 1.57.1), SHA-256 `fef24e716e58e003ca18021b79fb35dd8b36e7faf398fd4977b1370909926cb6` |
| Packages | 1,750 RPM packages, all supplied by the Fedora Project, built from 1,114 source packages |
| Licence data | The licence each package declares in its RPM `License` tag. Nobody has audited the source of each package against its tag |
| Date | 12 November 2026 |

The image examined was the Fedora base plus TUR identity files. No patch and no
additional package is in the image, so everything below about the packages is
a statement about the Fedora Kinoite base as TUR would redistribute it.

Since [TUR-006](../reports/TUR-006.md) the image also holds TUR's own
programs, each a Rust binary with third-party crates linked in. They are not
packages and are not in the figures of section 2. What they contain is listed
in [image-crates.csv](image-crates.csv) and described in
[`THIRD_PARTY_NOTICES.md`](../../THIRD_PARTY_NOTICES.md); items 3, 8 and 13 of
section 3 say what that changed.

To regenerate the package list after a rebase, and the list of crates after a
change to a component's dependencies:

```sh
scripts/dev/ci.py artifact RUN_ID tur-image-metadata /tmp/tur-meta
scripts/sbom-licences.py /tmp/tur-meta/sbom.spdx.json > docs/dependencies/image-licences.csv
cp /tmp/tur-meta/image-crates.csv docs/dependencies/image-crates.csv
```

## 2. Licences in the built image

### 2.1 Families

Each package is counted once, by the strongest obligation its declared licence
expression requires.

| Family | Packages | Share | Made up of |
|---|---:|---:|---|
| Strong copyleft | 739 | 42.2% | GPL-2.0 family only: 456. GPL-3.0 required: 265. AGPL-3.0: 18 |
| Permissive | 472 | 27.0% | MIT, BSD, Apache-2.0, ISC, Zlib, public-domain dedications and similar |
| Weak copyleft | 430 | 24.6% | LGPL: 393. MPL: 20. GPL with a linking or output exception: 13. Other (APSL-2.0, SISSL, a CDDL-or-LGPL choice): 4 |
| Documentation, fonts and other content | 85 | 4.9% | Fonts (OFL-1.1, Bitstream-Vera): 69. Documentation, artwork and data (GFDL, Creative Commons, Fedora logos, non-copyrightable files): 16 |
| Firmware and other non-free but redistributable | 24 | 1.4% | `LicenseRef-Fedora-Firmware` |
| **Total** | **1,750** | 100% | |

How the families were derived, so that the table can be reproduced from the CSV:

- The expression is parsed. `AND` takes the strongest family among its parts;
  `OR` takes the weakest, because the recipient may choose.
- A package that mentions a firmware licence anywhere is in the firmware family.
- A GPL identifier with an exception that limits its reach (GCC, Classpath,
  Libtool, Autoconf, Bison, Texinfo, Font, GNU-compiler, SANE or Linux-syscall
  exceptions) counts as weak copyleft. The Qt and cryptsetup-OpenSSL exceptions do
  not change the family.
- A package is in the content family only if every licence it requires is a
  documentation, font or content licence.
- Legacy tags are placed by their meaning (`LGPLv2+` is weak copyleft).

The method errs towards the stronger family: a library package that is LGPL
except for one GPL tool counts as strong copyleft. It classifies declared tags,
not source files.

### 2.2 Most common licences

| Whole expression | Packages | | Identifier, anywhere in an expression | Packages |
|---|---:|---|---|---:|
| `GPL-2.0-or-later` | 213 | | `GPL-2.0-or-later` | 520 |
| `LGPL-2.1-or-later` | 138 | | `LGPL-2.1-or-later` | 377 |
| `MIT` | 122 | | `MIT` | 366 |
| `OFL-1.1` | 66 | | `BSD-3-Clause` | 337 |
| `GPL-3.0-or-later` | 61 | | `LGPL-2.0-or-later` | 233 |
| `BSD-3-Clause` | 56 | | `GPL-2.0-only` | 224 |
| `GPL-2.0-only` | 39 | | `CC0-1.0` | 204 |
| `LGPL-2.0-or-later` | 33 | | `BSD-2-Clause` | 187 |
| `Apache-2.0` | 30 | | `LGPL-2.1-only` | 177 |
| `LGPL-3.0-only OR GPL-3.0-only WITH Qt-GPL-exception-1.0` | 30 | | `GPL-3.0-or-later` | 164 |

The CSV has 522 distinct expressions and 239 distinct identifiers; 1,006 packages
declare a single identifier and 744 a compound expression. CI's `licences.txt`
counts 525 distinct expressions for the same image: the SBOM tool rewrote the tag
of five packages to its canonical SPDX spelling (for example `zlib` to `Zlib`),
which merges three pairs of expressions. The package count is identical.

No package in the image declares a noncommercial or no-derivatives licence.

### 2.3 Packages that deserve attention

| Concern | Packages | Why it matters |
|---|---|---|
| Fedora trademarks | `fedora-logos` (`LicenseRef-Fedora-Logos`); `fedora-release-common`, `fedora-release-kinoite`, `fedora-release-identity-kinoite`, `fedora-release-ostree-desktop` | The `fedora-logos` package states that it "may not be distributed with anything but unmodified packages from Fedora Project". Fedora's trademark guidelines require `fedora-logos` and `fedora-release` to be removed from a combination of Fedora and other material. **This blocks publication of the image as it is built today.** |
| Other Fedora branding | `plasma-lookandfeel-fedora`, `plasma-welcome-fedora`, `f45-backgrounds-base`, `f45-backgrounds-kde`, `fedora-workstation-backgrounds`, `fedora-bookmarks`, `fedora-chromium-config`, `fedora-chromium-config-kde` | Freely licensed, but they present the system as Fedora. To be reviewed with the rebrand. |
| Firmware blobs | `linux-firmware`, `linux-firmware-whence`, `microcode_ctl`, `amd-gpu-firmware`, `amd-ucode-firmware`, `atheros-firmware`, `brcmfmac-firmware`, `cirrus-audio-firmware`, `intel-audio-firmware`, `intel-gpu-firmware`, `intel-npu-firmware`, `intel-vsc-firmware`, `iwlbluetooth-firmware`, `iwlegacy-firmware`, `iwlwifi-dvm-firmware`, `iwlwifi-mld-firmware`, `iwlwifi-mvm-firmware`, `libertas-firmware`, `mt7xxx-firmware`, `nvidia-gpu-firmware`, `nxpwireless-firmware`, `qcom-wwan-firmware`, `realtek-firmware`, `tiwilink-firmware` | Binary only. Fedora accepts a firmware licence that forbids modification and reverse engineering, restricts use to specific hardware, or allows redistribution only as part of the redistributor's product. These are field-of-use terms. TUR must ship the files unmodified with their licence files, and the review must confirm that TUR's distribution fits each vendor's terms. This is the image's "redistributable, no modification permitted" category; no package uses that older tag. One more firmware package, `alsa-sof-firmware`, is under `BSD-3-Clause AND Apache-2.0` and is counted as permissive. |
| Codecs | `ffmpeg-free`, `libavcodec-free` and the other `*-free` FFmpeg libraries (`GPL-3.0-or-later`); `fdk-aac-free` (`FDK-AAC`); `noopenh264`; `faad2-libs`, `xvidcore`, `libmpeg2`, `liba52`, `mpg123-libs`, `lame`, `twolame-libs`; `libdvdread`, `libdvdnav`, `libbluray`; `gstreamer1-plugin-libav`, `gstreamer1-plugins-ugly-free` | The copyright licences are free. The risk is patents. The image holds Fedora's own set: Fedora's `-free` build of FFmpeg, and `noopenh264`, a placeholder implementation of the OpenH264 library rather than Cisco's binary. The `FDK-AAC` licence grants no patent licence and allows use "only for purposes that are authorized by appropriate patent licenses". No DVD or Blu-ray decryption library is present. |
| AGPL | `ghostscript`, `ghostscript-tools-fontutils`, `ghostscript-tools-printing`, `libgs`, `libijs`, `jbig2dec-libs` (`AGPL-3.0-or-later`); twelve `urw-base35-*` font packages (`AGPL-3.0-only AND GPL-2.0-or-later`) | Shipped unmodified, the obligation is the same source offer as for the GPL. If TUR ever modifies Ghostscript and lets users interact with it over a network, it must offer them the modified source. |
| GPL-only Qt modules | `qt6-qtcharts`, `qt6-qtdatavis3d`, `qt6-qthttpserver`, `qt6-qtnetworkauth`, `qt6-qtquicktimeline`, `qt6-qtvirtualkeyboard` (`GPL-3.0-only WITH Qt-GPL-exception-1.0`) | TUR's own noncommercial code must not link these. See section 4. |
| Apple Public Source License | `hfsplus-tools` (`APSL-2.0`), `capstone` (includes `APSL-2.0`) | A copyleft licence whose source obligation is also triggered by deploying modified code, including to provide a service. Shipped unmodified: source offer only. |
| Other unusual copyleft | `libtirpc` (`SISSL`), `libsecret` (`GPL-2.0-or-later OR TGPPL-1.0`), `virtualbox-guest-additions` (`GPL-3.0-only AND (GPL-3.0-only OR CDDL-1.0)`), `LibRaw` (`CDDL-1.0 OR LGPL-2.1-only`), kernel packages (`copyleft-next-0.3.1`, `Linux-OpenIB`, `CDDL-1.0` as alternatives to the GPL) | Rare licences. In each case a GPL or LGPL alternative or companion exists; nothing here adds an obligation beyond the checklist in section 3. |
| No patent grant | `kernel*` and `svt-av1-libs` (`BSD-3-Clause-Clear`); `rav1e-libs`, `svt-av1-libs` (Alliance for Open Media patent licence); `libwebp` (`Google-Patent-WebM`) | Permissive copyright terms that either exclude a patent licence or attach a separate one. For awareness. |
| Legacy or malformed tags | 31 packages with pre-SPDX tags, among them `firefox`, `qt6-qtwebengine`, `qt6-qtpdf`, `gstreamer1-plugin-libav`, `kf6-kimageformats`, `libva-intel-media-driver`, `phonon-qt6`, `unar`; `kf6-solid` (`CCO-1.0`, a typing error for `CC0-1.0`); `plasma-activities-stats` (a comma in place of `AND`) | The tag cannot be validated automatically. `qt6-qtwebengine` in particular is built on the Chromium project, and its many components sit behind one legacy tag. |
| Fedora-specific identifiers | `LicenseRef-Fedora-Public-Domain` (74 packages), `LicenseRef-Fedora-UltraPermissive` (4), `LicenseRef-Not-Copyrightable` (5), `LicenseRef-KFQF-Accepted-GPL` (2), `LicenseRef-KDE-Accepted-LGPL` (1), and single uses of `LicenseRef-DMIT`, `LicenseRef-OASIS`, `LicenseRef-WS-Addressing`, `LicenseRef-Discovery`, `LicenseRef-docbook-dtds` | Not on the SPDX list. Their texts live in Fedora's licence data, not in the SBOM. |
| Third-party repository definitions | `fedora-workstation-repositories`, `fedora-flathub-remote`, `fedora-third-party` (`MIT`) | The packages are free, but they exist to enable software from outside Fedora, some of it proprietary. What they enable by default was not examined. |

## 3. Obligations that distributing the image would create

TUR does not publish an image. Nothing here is a breach today; the "Status"
column says what exists now, and the last column what must exist before the
first image is published to a registry or offered for download.

| # | Obligation | Status | Needed before the first published image |
|---|---|---|---|
| 1 | Know what is shipped: an SPDX SBOM for every build | Done | CI produces `sbom.spdx.json` and `licences.txt` for every image. Keep them with each published image. |
| 2 | A readable list of packages and licences | Done | [image-licences.csv](image-licences.csv), regenerated at each rebase. |
| 3 | SBOM covers files TUR adds outside packages | Partly done | Since TUR-006 the SBOM lists each TUR program, with version and SHA-256, and each crate linked into it (`scripts/sbom-programs.py`). TUR's units, configuration, the catalogue and the tool manifests are still not in it as files; the build manifest names the catalogue and the tool manifests by hash. |
| 4 | Source for copyleft components (GPL, LGPL, AGPL, MPL and similar): at least 1,169 of the 1,750 packages, built from 1,114 source packages | Not applicable yet | Decide and build the mechanism. The conservative option is to publish, beside every image, the exact source packages it was built from (a source image or an archive), and to keep them for as long as the image is offered. The SBOM already names each package's source RPM. The alternative under GPL-2.0 is a written offer valid for three years. Relying on Fedora to keep every pinned version downloadable leaves compliance outside TUR's control. |
| 5 | Source for TUR's own changes to copyleft components | Not applicable yet | No patch exists. When one does: publish it under the upstream licence, in a marked directory with its own licence file (ADR-0003), and include it in the source of item 4. |
| 6 | Licence texts and copyright notices accompany the binaries | Not done (inherited from Fedora, untested) | Fedora packages install their licence files under `/usr/share/licenses`. Nothing in the `Containerfile` removes them, but no test checks it. Add a check to `tests/image/smoke.sh`. |
| 7 | Third-party notice | Done | [`THIRD_PARTY_NOTICES.md`](../../THIRD_PARTY_NOTICES.md) at the repository root. Install a copy in the image with item 8. |
| 8 | TUR's own licence and notice travel with TUR's files | Done | The PolyForm licence requires that anyone who receives the software also receives the terms, or their URL, and the `Required Notice:` line in `NOTICE`. Since TUR-006 the image holds both in `/usr/share/licenses/tur/`, and the smoke test checks it. |
| 9 | Remove Fedora's marks | Done in the image (TUR-008), except that a published image must be squashed or re-chunked so that the base's layers, which still hold `fedora-logos`, are not published | `fedora-logos` and the `fedora-release-*` packages are replaced by Fedora's `generic-logos` and `generic-release`, TUR's identity and mark are installed, the other Fedora-branded packages are removed or emptied, and the `image` and `boot` workflows fail when a mark comes back ([TUR-008](../reports/TUR-008.md)). The statement of origin is the owner's line (desktop-experience.md, 17.1), pending legal review. Originally: Remove `fedora-logos` and the `fedora-release-*` packages and replace them with TUR's own or with Fedora's `generic-logos` and `generic-release`, which exist for this purpose. Review the other Fedora-branded packages in section 2.3. Display the notice the guidelines require: that the software is not provided or supported by the Fedora Project, and that official Fedora is available at https://fedoraproject.org/. Planned as part of the Phase 3 rebrand; it must not slip behind the first publication. |
| 10 | Describe the licence of the image truthfully | Done in the repository; not done in the image | `README.md` and `THIRD_PARTY_NOTICES.md` say that the noncommercial terms cover TUR's own code only. The image has no `org.opencontainers.image.licenses` label; if one is added it must not name the PolyForm licence alone. Download pages must not impose noncommercial terms on the image as a whole. |
| 11 | Firmware shipped unmodified with its licence files | Not applicable yet (inherited) | Do not strip, repack or modify the firmware packages. Legal review of the vendor terms (section 6). |
| 12 | Codec and patent position | Not applicable yet | The image has only Fedora's set. Write down the rule that no codec package from outside Fedora is added without review. |
| 13 | LGPL terms for TUR programs that link LGPL libraries | Done for what exists | Since TUR-006 the image holds TUR programs. Each links the C library (glibc, LGPL-2.1-or-later) and `libgcc_s` dynamically, as every program of the distribution does, and the user can replace them. Everything linked statically is a Rust crate or the Rust standard library, and the `image` workflow fails if a crate's licence is not on the allow-list of permissive licences in `scripts/sbom-programs.py`: no LGPL or GPL crate can be linked in unnoticed. The crates' licence files are in the image ([`THIRD_PARTY_NOTICES.md`](../../THIRD_PARTY_NOTICES.md)). The allow-list judges the licence a crate declares, not its source. The desktop programs in C++ (Qt, LGPL-3.0) are not in the image yet; the rule for them is in `desktop/ALLOWED_IMPORTS`. |
| 14 | SPDX identifier on every TUR source file, and a CI check that noncommercial code and upstream-licensed directories are not mixed | Partly done | Files carry the header. The CI check that ADR-0003 calls for does not exist. |
| 15 | Installation information for GPL-3.0 and LGPL-3.0 components | Not applicable | Arises only if TUR is distributed inside a consumer device whose software the user cannot replace. TUR uses Fedora's signed boot chain unmodified and the user controls the machine. Revisit if TUR ever signs its own boot chain or ships on hardware. |
| 16 | AGPL network clause | Not applicable | Ghostscript is unmodified. |
| 17 | Legal review (§17.2) | Not done | Required before public distribution. Section 6 lists the questions. |

## 4. The noncommercial licence and the components TUR ships with

### 4.1 What ADR-0003 already says

- The noncommercial terms apply only to TUR's own separate work: standalone
  services, tools, image definitions and documentation that reach other
  components over D-Bus, sockets or the command line.
- They do not and cannot apply to the third-party components in the image, to
  patches to an upstream project, or to code that links into a GPL work such as a
  KWin or Plasma plugin or a kernel module.
- Such code lives in marked directories with its own licence file. A new
  component is a separate process unless an ADR records why not.

This review found nothing that contradicts ADR-0003. It found several places
where the boundary falls somewhere the ADR does not spell out.

### 4.2 The image as a whole cannot be noncommercial

The GPL treats a separate work placed on the same medium as "mere aggregation",
so TUR's noncommercial programs may sit in one image with GPL programs. The
reverse also holds: the GPL forbids "any further restrictions" on recipients,
so nothing TUR writes can limit what a recipient does with the 1,750 Fedora
packages.

The practical consequence for the project owner: **a business may lawfully take
the TUR image, remove or simply not use TUR's own files, and use everything else
commercially.** The noncommercial restriction protects what TUR writes, not the
operating system that TUR assembles. Statements such as "TUR is not for business
use" are accurate for TUR's own code and inaccurate for the image, and should be
worded with that in mind.

### 4.3 Where TUR code comes closest to copyleft code

The Free Software Foundation's position is that a plugin which is dynamically
loaded into a GPL program, calls its functions and shares its data structures
forms a single program with it, and so must be under a GPL-compatible licence.
A licence that forbids commercial use is not GPL-compatible. ADR-0003 adopts the
same position. Applied project by project:

| Project | What TUR plans or may want | Reading under ADR-0003 | Can it be noncommercial? |
|---|---|---|---|
| KDE Plasma | The three visual modes (specification §9.2); launcher, panels, lock and login screens; System Settings pages; a software centre | Applets, look-and-feel packages, System Settings modules, KRunner and Discover plugins are loaded into `GPL-2.0-or-later` programs. QML and JavaScript packages are in the same position as compiled plugins: they run inside the host and use its interfaces. Anything started from a copy of Breeze or another Plasma package is a modification and keeps the upstream licence whatever TUR decides. | **No.** These parts must be GPL-compatible, in a marked directory. A standalone TUR application that only links LGPL libraries (Qt, KDE Frameworks) can be. Themes that are pure data (colour schemes, wallpapers, icons drawn by TUR) are TUR's own; that boundary is a question for the review. |
| KWin | Window rules, tiling and quick-layout controls, effects, importing compatibility-layer windows (`tur-desktop-bridge`) | Effects, scripts, decorations and plugins run inside KWin (`GPL-2.0-or-later`). A Wayland client or a D-Bus caller is a separate program. | **No** for anything loaded by KWin. **Yes** for `tur-desktop-bridge` as a separate process using public protocols. |
| Qt 6 | TUR's own QML and Rust user interface | Most modules are `LGPL-3.0-only`: linking is permitted on LGPL terms. Some modules are `GPL-3.0-only`. | **Yes**, on LGPL terms and only with LGPL modules. Not with Qt Virtual Keyboard, Qt Wayland Compositor, Qt Quick 3D, Qt Quick Timeline, Qt Graphs or the other GPL-only modules. |
| Calamares | A possible installer front end | Modules, view steps and branding QML load into a `GPL-3.0-or-later` program and link `libcalamares`, also `GPL-3.0-or-later`. | **No** for modules. The TUR installer logic can be, as a separate program that a small GPL module starts. A TUR installer written on Qt alone avoids the question. |
| Linux kernel | None planned. Looking Glass's `kvmfr` and linux-apfs-rw are out-of-tree modules TUR might build | A module is GPL-2.0-compatible or it taints the kernel and loses access to GPL-only symbols. TUR uses Fedora's kernel unmodified. | **No**, and a module also needs an ADR and a Secure Boot signing answer. |
| dracut, SELinux policy, Plymouth | Boot health gate, boot-stage reporting, policy for TUR services | A dracut module is shell sourced by dracut (`GPL-2.0-or-later`). A policy module derived from Fedora's policy (`GPL-2.0-or-later`) is a modification of it. Licences here are Fedora's package tags, not checked upstream. | **No** for a dracut module or a policy derived from Fedora's. **Yes** for a systemd unit, a separate helper the initramfs runs, or a policy module written from scratch; the last is a question for the review. |
| Wine, Proton | Patches "only with reason"; prefix management; `tur-runtime` | Patches are `LGPL-2.1-or-later` (Wine) or the component's licence (Proton). A program that starts Wine is separate. | Patches: **no**. `tur-runtime`: **yes**. |
| Darling | Research patches | Patches are `GPL-3.0-or-later` or the submodule's licence. | Patches: **no**. A separate launcher: **yes**. |
| Waydroid | Android runtime integration | `GPL-3.0-or-later` Python. Driving it through its command line is separate. Importing its Python modules into TUR code, or changing it, is not. | **Yes**, as a separate process only. |
| FreeRDP | Seamless guest windows | `Apache-2.0`. | **Yes**, including linking. Keep notices. |
| WinApps | Possible basis for the guest tier | `AGPL-3.0-only`, with files of no licence. | **No.** See 4.4. |
| WinBoat | Possible basis for the guest tier | `MIT`. | **Yes.** |
| BlueZ, NetworkManager, UDisks2 | Device and storage services | D-Bus is the supported interface. `libbluetooth`, NetworkManager daemon plugins and UDisks modules are GPL. | **Yes** over D-Bus. **No** if linked with `libbluetooth` or built as a daemon plugin. |
| LGPL libraries (`libsystemd`, `libostree`, `libnm`, `libudisks2`, `libblockdev`, `libflatpak`, `libfwupd`, `libvirt`, KDE Frameworks) | Convenience over calling D-Bus directly | The LGPL allows a program that uses the library to be distributed "under terms of your choice", provided those terms permit modification for the customer's own use and reverse engineering to debug such modifications. | **Yes, probably.** The PolyForm licence permits modification for permitted purposes and does not forbid reverse engineering. Whether its limit to noncommercial purposes satisfies the LGPL proviso is a question for the review. |

### 4.4 Flagged

Projects whose licence would make a planned TUR component impossible to keep
noncommercial:

| # | Finding | Effect |
|---|---|---|
| F1 | **The desktop work described in specification §9 is largely plugin work.** The visual modes, panels, launcher, lock and login screens and System Settings pages are Plasma and KWin extensions. | That code must be GPL-compatible. Anyone, including a business, may then use it under the GPL. If the owner wants the desktop experience itself to be noncommercial, it has to be built as standalone programs on Qt and LGPL libraries, which is a larger piece of work than theming Plasma. This needs a decision, and the `desktop/` directory needs splitting into noncommercial applications and GPL-licensed Plasma and KWin packages. |
| F2 | **Calamares modules** would be GPL-3.0. | Only matters if Calamares is adopted. ADR-0007 already defers a custom installer. |
| F3 | **WinApps** is AGPL-3.0, and by its own statement still contains files from an unlicensed, all-rights-reserved original. | Do not adapt it and do not redistribute it. Prefer FreeRDP directly, or WinBoat (MIT). |
| F4 | **GPL-only Qt modules.** | A noncommercial TUR program that used one would have to become GPL. Enforce by review of each new Qt import. |

Projects whose terms forbid, or may forbid, the use TUR intends:

| # | Finding | Effect |
|---|---|---|
| F5 | **`fedora-logos`** may not be distributed with anything but unmodified Fedora packages. | Removed from the image since TUR-008; its files remain in the base image's layers until a published image is squashed or re-chunked. Checklist item 9. |
| F6 | **Proton** contains `lsteamclient`, built from Valve's Steamworks SDK under a proprietary licence that grants development use and redistribution of the SDK's own redistributable binaries only. | TUR does not redistribute Proton. The user's machine fetches it. |
| F7 | **Google Mobile Services** are available only under a licence from Google. Waydroid offers a `GAPPS` image type and documents self-certification for it. | TUR never ships, preselects or automates it. |
| F8 | **ARM translation libraries** (`libhoudini`, `libndk_translation`) are not part of Waydroid. The community script that installs them says it takes one from a Windows Subsystem for Android image and the other from device firmware. No licence permitting redistribution was found. | TUR never ships or fetches them. Android applications built only for ARM are out of scope on x86-64 unless a licensed translator exists. |
| F9 | **Firmware** licences may restrict use to specific hardware and redistribution to the redistributor's own product. | Compatible with shipping an operating system, by Fedora's assessment. To be confirmed for TUR as a separate redistributor. |
| F10 | **`FDK-AAC`** permits use only for purposes covered by patent licences. | Inherited from Fedora, which ships a reduced build. For the review. |
| F11 | **Microsoft Windows and Apple macOS** terms were not read for this inventory. ADR-0007 records that Apple permits macOS guests only on Apple hardware and that the Windows guest terms must be confirmed. | No macOS guest. The Windows guest tier depends on an answer. |

### 4.5 A caution on "separate process"

ADR-0003 treats D-Bus, sockets and the command line as the boundary. The Free
Software Foundation's own guidance is slightly narrower: separate processes that
"establish intimate communication by sharing complex data structures" can still
form one program. A TUR component that talks to a GPL program over a stable,
documented, general-purpose interface is on safe ground. One that depends on a
private interface built only for it, for example a TUR-specific protocol added to
KWin by a TUR patch, is not clearly separate. Prefer public interfaces, and
record the interface in an ADR when a new one is needed.

## 5. Things TUR must never ship

| Never ship | Reason | Where the user gets it |
|---|---|---|
| Microsoft binaries: Windows installation images, system DLLs, fonts, runtime redistributables, Windows drivers | Proprietary. AGENTS.md and specification §17.2 forbid it. Windows `.sys` drivers must never be loaded into the Linux kernel. | The user supplies a licensed Windows and installs Microsoft's components inside it. |
| Apple binaries: macOS, frameworks, fonts, firmware, `.kext` drivers | Proprietary. AGENTS.md and §17.2. ADR-0007: no macOS guest. | Not available through TUR. |
| Google Mobile Services, the Play Store, any "GApps" package or `GAPPS` Android image | Licensed only by agreement with Google. | The user's choice and responsibility, outside TUR. |
| ARM translation libraries (`libhoudini`, `libndk_translation`) | Proprietary; no redistribution licence found. | Not available through TUR. |
| Proton builds, the Steam client, Steamworks SDK components | Proton bundles a component under Valve's proprietary SDK licence; the Steam client is proprietary. | Fetched on the user's machine by umu-launcher or Steam. |
| Fedora's trademarks: `fedora-logos`, `fedora-release-*`, Fedora artwork presented as TUR | The package licence and Fedora's trademark guidelines. | Replaced by TUR's own identity. |
| Codec packages from outside Fedora (full FFmpeg builds, H.264 and H.265 encoders, DVD decryption), and Cisco's OpenH264 binary rehosted by TUR | Patent licences and, for DVD decryption, anti-circumvention law in some jurisdictions. The image holds only what Fedora ships. Cisco's terms were not read for this inventory. | The user may add them from third-party repositories under their own responsibility. |
| Proprietary kernel modules and drivers | Proprietary, and the kernel marks itself tainted when one is loaded. AGENTS.md keeps the Fedora kernel as the single authority and requires an ADR for anything added to it. | The user may install them. |
| WinApps, or code derived from it | Section 4.4, F3. | The user may install it. |
| Signed virtio-win driver images, until their terms are confirmed | Unverified. | Downloaded by the user from the publisher. |
| Any application, game or Flatpak that is not free software | TUR provides compatibility infrastructure; the user supplies software they are entitled to use (§17.2). | The publisher or the user's own media. |

"Fetched on the user's machine" is not a loophole to be relied on without
review. If TUR automates a download, chooses the source and runs the result, a
court may see TUR as taking part in the distribution. That is question 6 below.

## 6. Open questions for legal review

1. **The licence boundary.** Is ADR-0003's line, a separate process with a
   defined interface, sufficient in the jurisdictions TUR will be distributed in?
   Does the caution in section 4.5 change it?
2. **QML, JavaScript and data loaded by Plasma and KWin.** Is a QML applet, a
   look-and-feel package or a KWin script a derivative of the host, as assumed
   here? Where does pure theme data (colours, images, layout configuration)
   fall?
3. **LGPL and a field-of-use restriction.** Does a licence that permits
   modification only for noncommercial purposes satisfy LGPL-2.1 section 6 and
   LGPL-3.0 section 4 for a program that links Qt, KDE Frameworks or
   `libsystemd`?
4. **Describing the product.** How should TUR state its licence so that it
   neither imposes "further restrictions" on GPL components nor understates the
   restriction on its own code? Is a compilation copyright in the image claimed,
   and if so under what terms?
5. **Source obligations for an image.** Is hosting the exact source packages
   beside each image sufficient, and for how long must they be kept? Is a
   written offer preferable? Does the answer differ between GPL-2.0, GPL-3.0 and
   AGPL-3.0 components?
6. **Assisted downloads.** When TUR, on the user's request, downloads Proton, the
   Steam Linux Runtime, an Android image, a Windows installation image or guest
   drivers and runs them, is TUR a distributor of that software? Does it matter
   whether the user is shown the upstream terms first?
7. **Fedora marks.** After `fedora-logos` and `fedora-release` are replaced, may
   TUR say "based on Fedora Linux", keep `ID=fedora` in `os-release` for tooling,
   and use Fedora's package repositories and signing keys? Is "Fedora Remix" the
   only permitted way to refer to Fedora?
8. **Firmware.** Do the vendor licences in `linux-firmware` permit redistribution
   by TUR, a noncommercial project that is not Fedora, in an image offered for
   download?
9. **Patents.** Is the codec set inherited from Fedora acceptable in the
   jurisdictions TUR targets? Does the noncommercial nature of TUR change the
   position for any codec?
10. **Windows in a guest.** Under what Microsoft terms may a user run their
    Windows licence in a QEMU guest and display its applications remotely on the
    same machine? May TUR automate the installation? May TUR distribute a small
    agent that runs inside the guest?
11. **Android.** What are the terms of the LineageOS-based images Waydroid
    downloads, and may TUR point users at them?
12. **Darling.** Before any Darling binary is built for distribution: the
    licences of its submodules, in particular Apple's open-source releases, and
    whether they can be combined and shipped as Darling does.
13. **Contributions.** ADR-0003 leaves open whether outside contributions need a
    contributor agreement. `CONTRIBUTING.md` already licenses contributions
    inbound under the project licence; is that enough if the owner later wants
    to offer commercial licences?
14. **Export control.** The image contains cryptographic software. Does
    publishing it create notification or other duties for the publisher?

## 7. Sources

Upstream licences and the Fedora, Qt, KDE and Google pages are listed with URLs
in [upstream-projects.md](upstream-projects.md#sources). Sources used only in
this review, all read on 12 November 2026:

| Subject | URL |
|---|---|
| PolyForm Noncommercial License 1.0.0 | [`LICENSE.md`](../../LICENSE.md); https://polyformproject.org/licenses/noncommercial/1.0.0 |
| GPL-2.0 text: aggregation (section 2), source (section 3), no further restrictions (section 6) | https://github.com/libvirt/libvirt/blob/master/COPYING |
| LGPL-2.1 text: section 6 | https://github.com/flatpak/flatpak/blob/main/COPYING |
| LGPL-3.0 text: section 4 | https://spdx.org/licenses/LGPL-3.0-only.html |
| GPL-3.0 text: installation information (section 6) | https://github.com/rhboot/grub2/blob/HEAD/COPYING |
| AGPL-3.0 text: section 13 | https://github.com/winapps-org/winapps/blob/main/LICENSE.md |
| `APSL-2.0` text: "Externally Deploy" | https://spdx.org/licenses/APSL-2.0.html |
| Kernel module licensing and tainting | https://github.com/torvalds/linux/blob/master/Documentation/process/license-rules.rst |
| `noopenh264` package description | https://src.fedoraproject.org/rpms/noopenh264/blob/rawhide/f/noopenh264.spec |
| Free Software Foundation on plugins and combined programs | https://www.gnu.org/licenses/gpl-faq.html#GPLPlugins |
| Fedora firmware licence criteria | https://docs.fedoraproject.org/en-US/legal/license-approval/ |
| Fedora trademark guidelines | https://docs.fedoraproject.org/en-US/legal/trademarks/ |
| `fedora-logos` package terms | https://src.fedoraproject.org/rpms/fedora-logos/blob/rawhide/f/fedora-logos.spec |
| `generic-release` and `generic-logos` | https://src.fedoraproject.org/rpms/generic-release/blob/rawhide/f/generic-release.spec ; https://src.fedoraproject.org/rpms/generic-logos/blob/rawhide/f/generic-logos.spec |
| `FDK-AAC` licence text | https://spdx.org/licenses/FDK-AAC.html |
| Steamworks SDK licence as carried in Proton | https://github.com/ValveSoftware/Proton/blob/HEAD/lsteamclient/LICENSE |
| Waydroid image types and Google certification | https://docs.waydro.id/faq/google-play-certification |
| Origin of the ARM translation libraries | https://github.com/casualsnek/waydroid_script/blob/main/README.md |
| Google Mobile Services licensing | https://www.android.com/gms/ |
| Package names, versions and licence tags | Image SBOM, CI run 43147702795 |
