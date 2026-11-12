# TUR: The Unified Realm
## Technical Architecture, Implementation Specification, and Development-Agent Handoff

**Document type:** Engineering specification and research execution plan  
**Project:** TUR (The Unified Realm)  
**Document revision:** 1.0  
**Date:** 12 November 2026  
**Primary target:** x86-64 UEFI PCs  
**Kernel architecture:** Linux  
**Primary objective:** Research and prototype a coherent desktop operating system that runs Linux applications natively, Windows applications through a compatibility layer, and macOS applications through a Darwin compatibility layer.  
**Intended audience:** Senior systems engineers, Linux distribution engineers, compatibility-layer developers, desktop developers, QA engineers, and autonomous development agents.

---

# 0. Instructions for the development agent

This document defines the intended architecture and implementation constraints for TUR. Treat it as the project baseline. Do not interpret aspirational goals as proof that an upstream component already supports them.

The development agent must:

1. Work incrementally. Produce small, reviewable changes with tests and build logs.
2. Prefer integrating maintained upstream projects over rewriting mature subsystems.
3. Keep upstream dependencies separately versioned. Do not create long-lived forks unless a documented requirement forces one.
4. Preserve the ability to boot the machine after every installer, kernel, bootloader, storage, or update change.
5. Never perform destructive storage operations without an explicit confirmation displaying the exact target device and its identity.
6. Never enable experimental APFS write support by default.
7. Never claim complete Windows or macOS compatibility. Compatibility is application-specific and must be measured.
8. Never load Windows `.sys` drivers or macOS `.kext` drivers into the Linux kernel. Device support must use Linux drivers, userspace adapters, or explicit VM passthrough.
9. Never collect or upload diagnostics silently. Diagnostics must be locally available, privacy-reviewed, and explicitly shared by the user.
10. Treat installer diagnostics, boot diagnostics, update rollback, and recovery as core deliverables, not post-release additions.
11. Keep the host Linux kernel as the single authority for CPU scheduling, memory management, block storage, networking, GPU driver ownership, and normal device ownership.
12. Where an application needs a complete Windows NT kernel or unsupported kernel driver, offer a genuine Windows VM or dual-boot path. Do not pretend Wine is a complete Windows kernel.
13. Document every deviation from this specification in an Architecture Decision Record (ADR).
14. Verify current upstream documentation, release versions, APIs, and project health before pinning dependencies. The repository list in this document is a starting point, not a substitute for dependency review.

## 0.1 Required working style

For each work item, the agent must supply:

- A short problem statement.
- The relevant architecture component.
- A design note or ADR if the change affects interfaces, boot, storage, security, or update behavior.
- An implementation plan.
- Tests and a reproducible test command.
- Performance impact where relevant.
- Failure modes and rollback behavior.
- A concise completion report with changed files, commit references, test results, known gaps, and log locations.

Do not merge a change merely because it compiles. Boot-critical and storage-critical work requires fault-injection and recovery tests.

---

# 1. Executive summary

TUR is a Linux-based operating system designed to provide one integrated desktop and one shared hardware platform for applications originating from three operating-system ecosystems:

- **Linux:** native ELF applications using the Linux ABI.
- **Windows:** PE applications using Win32/Win64 APIs through Wine, with Proton and graphics translation components available for gaming.
- **macOS:** Mach-O applications using Darwin system interfaces and reimplemented macOS frameworks through Darling.

The system uses one Linux kernel. It does not merge the Windows NT kernel, Apple's XNU kernel, and Linux. It does not boot three operating systems simultaneously as peers. Windows and macOS compatibility environments run in userspace on Linux. A genuine Windows installation remains available as an optional full operating-system backend through QEMU/KVM or dual boot for software that requires Windows kernel behavior, proprietary drivers, anti-cheat, or services that compatibility layers do not implement.

TUR must feel like one coherent operating system:

- One desktop shell and window manager.
- One application launcher and application catalog.
- One shared storage namespace.
- One host hardware and driver-management layer.
- One audio and networking service stack.
- Integrated notifications, clipboard, file dialogs, printing, screen capture, and drag-and-drop where technically supportable.
- One safe installer, update system, recovery workflow, and diagnostic bundle format.

The project's largest original engineering contribution should be the integration layer, especially:

1. **TUR Runtime Manager:** executable detection, runtime selection, dependency management, application profiles, and fallback policy.
2. **TUR Desktop Bridge:** integration of Wine and Darling windows and application behavior with KDE Plasma/Wayland.
3. **TUR Device Manager:** one host-level model for devices, permissions, drivers, firmware, and exclusive device assignment.
4. **TUR Storage Manager:** safe filesystem discovery, mounting, namespace presentation, metadata capability reporting, and compatibility mappings.
5. **TUR Diagnostics and Recovery:** detailed installer/boot feedback, persistent logs, support bundles, immutable updates, boot-health assessment, rollback, and recovery.

The first release is a research prototype for x86-64 UEFI hardware. It should prioritize reproducibility, observability, and recoverability over broad marketing claims.

---

# 2. Project goals, non-goals, and feasibility boundaries

## 2.1 Primary goals

1. Boot directly into a Linux-based TUR installation.
2. Run Linux ELF applications natively.
3. Run a defined and tested set of Windows applications through Wine.
4. Run a defined and tested set of macOS command-line applications through Darling.
5. Progress toward simple macOS GUI applications as Darling's framework and Wayland integration mature.
6. Present applications from all supported environments in one KDE Plasma desktop.
7. Use Linux-native drivers and host services for supported hardware.
8. Read and write common storage formats safely, especially NTFS, exFAT, ext4, and Btrfs.
9. Read APFS volumes through a read-only implementation where supported. Treat APFS write support as experimental and disabled by default.
10. Provide optional full Windows execution through a genuine Windows guest or dual boot.
11. Provide an installer with unusually detailed live status, error reporting, and exportable logs.
12. Make system upgrades atomic or transactional, automatically detect failed boots, and restore the previous known-good system deployment.
13. Keep user data separate from the operating-system deployment so the OS can be rolled back or reinstalled without intentionally deleting user files.
14. Establish a reproducible performance and compatibility benchmark suite.

## 2.2 Explicit non-goals for the initial prototype

- Reimplementing the Linux kernel.
- Merging Windows NT, XNU, and Linux kernel code.
- Supporting every Windows application or every macOS application.
- Loading Windows kernel drivers or macOS kernel extensions into Linux.
- Providing a complete implementation of Apple's proprietary frameworks, private APIs, iCloud, App Store services, or all Apple security mechanisms.
- Providing full Windows Update behavior through Wine.
- Running every UWP, MSIX, kernel-mode, anti-cheat, DRM, or device-driver-dependent Windows program.
- Providing a complete APFS read/write implementation in the initial release.
- Supporting every CPU architecture at launch.
- Building a new compositor, display server, filesystem, package manager, or general-purpose kernel when mature upstream projects exist.
- Bundling Microsoft's Windows binaries or Apple's macOS binaries without appropriate rights and licensing.

## 2.3 Feasibility boundary: compatibility is not equivalence

A file extension does not prove that an application is runnable. A `.exe` can be a Win32 app, installer, self-extracting archive, .NET app, .NET Native app, UWP-related component, driver installer, or a program that requires a Windows kernel driver. Likewise, a `.app` bundle is not necessarily portable outside macOS and may depend on private frameworks, code-signing rules, hardware features, or Apple services.

TUR must use executable inspection and compatibility profiles rather than extension-only routing.

**Compatibility claims must be evidence-based.** Maintain a tested application matrix with exact application version, runtime version, GPU, driver, result, known defects, and reproduction steps.

## 2.4 OpenCore is not part of the Linux runtime architecture

OpenCorePkg is a UEFI bootloader and platform-configuration project used in Hackintosh configurations. It is not a macOS application runtime and does not provide Darwin APIs to Linux. TUR should boot Linux directly through its supported UEFI boot chain. OpenCore is not required for TUR's normal operation.

Darling, not OpenCore, is the relevant project for running supported macOS applications on Linux. OpenCore may be studied separately for firmware/boot research, but it must not become a dependency of the TUR desktop or application execution path.

---

# 3. Core architecture principles

The following principles are mandatory unless an ADR documents an exception.

## 3.1 One kernel, one hardware authority

Linux owns:

- CPU scheduling and process execution.
- Virtual memory and page tables.
- Normal host process lifecycle.
- Kernel driver model and module loading.
- Physical storage block devices.
- Network interfaces and host networking.
- GPU DRM/KMS device ownership.
- USB host controller and normal USB device ownership.
- Audio devices through ALSA and PipeWire.
- Power management and thermal management.
- Host security modules and access control.

Wine and Darling implement application-facing compatibility contracts in userspace. They do not own the physical machine or replace the Linux kernel.

## 3.2 Native execution before translation

For x86-64 applications on an x86-64 host, execute application machine code directly on the physical CPU. Translate operating-system APIs, not CPU instructions. CPU emulation or binary translation is a later, optional architecture feature.

## 3.3 Host services are shared, not duplicated

One host service should manage each physical resource. Do not run separate competing host-level network managers, disk managers, audio servers, or GPU drivers for Windows and macOS environments.

Compatibility layers should adapt Windows/Darwin APIs to host services where possible.

## 3.4 Separate app compatibility from full OS virtualization

Wine and Darling are compatibility environments. QEMU/KVM Windows is a full guest OS. They have different performance, driver, security, and compatibility properties. TUR must expose these distinctions clearly in its internal architecture and user-facing diagnostics.

## 3.5 Recovery is a product feature

Every change capable of preventing boot, corrupting storage, breaking the display, or making user data inaccessible must have a documented rollback or recovery path.

## 3.6 Diagnose first, repair second

When boot or installation fails, TUR must preserve enough information to explain the failure. A recovery process that silently overwrites logs or retries indefinitely is unacceptable.

## 3.7 Reproducible builds and pinned dependencies

Every release image must have a manifest containing source revisions, build IDs, kernel config hash, package versions, firmware package versions, runtime revisions, and SBOM data. Do not track an unpinned `master` branch in production images.

---

# 4. Supported platform and hardware baseline

## 4.1 Initial target

- CPU architecture: x86-64 (AMD64/Intel 64).
- Firmware: UEFI.
- Partitioning: GPT as the normal installation format.
- Boot path: UEFI bootloader, signed UKI where practical, initramfs, Linux kernel.
- Memory: 16 GB minimum for development; 32 GB recommended; 64 GB recommended for concurrent TUR plus Windows VM development.
- Storage: NVMe SSD recommended.
- Graphics: GPU with maintained Linux DRM driver and adequate Vulkan support.
- First preferred validation hardware: AMD GPU with Mesa RADV; Intel integrated/discrete GPU with Mesa ANV as a secondary target. NVIDIA support should be tested separately with its supported driver stack.

These are development recommendations, not strict hardware minimums. TUR should publish a compatibility matrix rather than make broad universal hardware claims.

## 4.2 Architecture scope

Phase 1 supports x86-64 Linux host and x86-64 guest applications. ARM64 and mixed-architecture support are future research topics.

Potential future approaches include QEMU user-mode emulation, FEX, or other translation projects, but they should not be mixed into the initial x86-64 compatibility baseline.

## 4.3 Secure Boot

TUR should support a secure boot path with signed boot components and signed kernel modules. The selected distribution/build model must define:

- Who owns the signing keys.
- How test keys differ from production keys.
- How users enroll keys.
- How module signing is handled.
- How kernel lockdown affects debugging and third-party modules.
- How recovery works when a key is lost or a new key is not enrolled.

Never disable Secure Boot silently. The installer must explain unsupported or unsigned modules and present an explicit user decision.

---

# 5. Operating-system image, boot, update, and recovery architecture

This section is a top-level priority. TUR is a research distribution, so developers need the ability to install, update, break, diagnose, and recover it repeatedly.

## 5.1 Recommended distribution model

Use an image-based, transactional Linux distribution architecture rather than an unrestricted mutable root filesystem.

Recommended research foundation:

- `bootc` for container-image-based Linux OS delivery and installation workflows.
- OSTree as the deployment/rollback foundation used by the selected bootc integration.
- systemd for service management, boot assessment, journaling, and recovery services.
- dracut or the distribution's supported initramfs generator.
- UEFI bootloader integration supported by the chosen bootc version. Evaluate systemd-boot and GRUB based on required hardware and recovery support.

Do not assume every bootloader feature is supported identically by every bootc/OSTree release. Confirm the exact combination in CI before committing to it.

The `bootc install to-disk` documentation describes supported disk installation and partitioning options, including GPT/DPS defaults and options for more complex block layouts. Use it as a reference, not as permission to skip TUR's own installer validation and safety checks.

Reference: https://github.com/bootc-dev/bootc
Reference: https://github.com/ostreedev/ostree
Reference: https://github.com/ostreedev/ostree/blob/main/docs/atomic-upgrades.md
Reference: https://github.com/bootc-dev/bootc/blob/main/docs/src/man/bootc-install-to-disk.8.md

## 5.2 Filesystem and partition layout

The installer should support at least:

- GPT partition tables.
- EFI System Partition.
- A boot/root layout compatible with the selected bootc/OSTree installation mode.
- LUKS2 encryption for supported root/data layouts.
- Optional separate `/home` or dedicated user-data partition.
- Optional swap file or swap partition.
- Optional recovery partition only if it has a clear maintenance policy. A rescue USB/ISO is still required.

The installer must display the exact target disk by model, serial when available, capacity, connection type, existing partition table, and destructive consequences. The agent must never infer the target disk solely from a device path such as `/dev/sda`.

Before any destructive operation:

1. Enumerate all block devices.
2. Resolve stable identifiers using `/dev/disk/by-id` and `/dev/disk/by-path` where available.
3. Display the intended target and partition plan.
4. Require explicit confirmation.
5. Revalidate the device identity immediately before writing.
6. Record the operation plan and result in the installer log.

## 5.3 Atomic updates

System updates should deploy a new immutable OS image rather than mutate the active root in place.

Expected behavior:

- Fetch a signed image or OSTree deployment.
- Verify signatures and content checksums.
- Stage the new deployment without modifying the currently running deployment.
- Update bootloader entries atomically.
- Boot the new deployment with a bounded number of attempts.
- Run early health checks.
- Mark the deployment successful only after the system reaches a defined healthy state.
- If the boot fails or health checks fail, return automatically to the prior known-good deployment.

systemd-boot supports boot counting and fallback behavior for entries with attempt counters. systemd also documents automatic boot assessment. Validate the actual bootloader/system image combination in a hardware test, rather than assuming that enabling a configuration option proves end-to-end rollback.

References:
- https://www.freedesktop.org/software/systemd/man/latest/sd-boot.html
- https://github.com/systemd/systemd/blob/main/docs/AUTOMATIC_BOOT_ASSESSMENT.md
- https://ostreedev.github.io/ostree/atomic-upgrades/

## 5.4 Boot health gate

Implement `tur-boot-health.service` and a small health-check executable.

A deployment is not marked good until the following checks pass or are explicitly classified as non-blocking:

- Root filesystem is mounted and verified.
- `/usr` and critical runtime paths are readable.
- systemd reaches the intended graphical target.
- Required TUR core services start.
- Device enumeration completes without fatal udev/systemd errors.
- Display manager or compositor starts.
- A user session can be created.
- Storage manager starts.
- Network initialization completes or is recorded as unavailable (network must not be a hard requirement for local boot).
- TUR diagnostics service is operational.

The health gate must not wait forever for optional hardware, Wi-Fi, Bluetooth, external disks, or cloud services. Define bounded timeouts and classify optional failures.

A boot is considered successful when the required local conditions pass and the boot-success marker is written. The marker must be written only after the system reaches a usable session or an explicitly configured headless mode.

## 5.5 Persistent boot and installer logs

Use journald persistent storage. Do not keep all useful logs only in volatile `/run` or memory.

Required identifiers and metadata:

- TUR build ID.
- OS deployment checksum.
- Kernel release and config hash.
- Initramfs build ID.
- Boot ID.
- Boot attempt number.
- Hardware inventory snapshot.
- Firmware/BIOS version.
- Bootloader version and selected entry.
- Installer session ID.
- Installation target disk identity (redacted in shareable bundle by default).
- Phase start and end timestamps.
- Exit status and structured error code for each phase.

Use structured logs (journal fields or JSON lines) for TUR-created services. Human-readable messages are also required. Do not make developers parse arbitrary unstructured console text as the sole source of truth.

## 5.6 Early boot and crash evidence

Integrate:

- Kernel command-line verbosity mode.
- systemd journal persistence.
- `systemd-pstore` to archive persistent crash records from `/sys/fs/pstore` where firmware/platform supports it.
- Optional kdump/crashkernel for advanced debugging builds.
- Initramfs emergency shell with explicit instructions.
- Recovery boot entry using a known-good deployment.

pstore storage is small and hardware-dependent. It supplements, but does not replace, normal logs or kdump.

References:
- https://www.freedesktop.org/software/systemd/man/latest/systemd-pstore.service.html
- https://docs.kernel.org/admin-guide/ramoops.html
- https://docs.kernel.org/admin-guide/kdump/kdump.html

## 5.7 Recovery requirements

At minimum, recovery must offer:

1. Boot previous known-good OS deployment.
2. Boot fallback kernel/initramfs where available.
3. Boot rescue environment from installation media.
4. Open a read-only diagnostic shell.
5. Unlock supported encrypted volumes with user credentials/recovery key.
6. Export diagnostic bundle to USB or another user-selected destination.
7. Reinstall the OS without intentionally formatting user data, when the partition layout supports it.
8. Restore OS deployment from signed image or local cache.
9. Restore TUR configuration from a backup.

Do not promise recovery of encrypted user data if the user loses every valid key. Explain that encryption key loss is not recoverable by the OS vendor.

---

# 6. Installer, first boot, diagnostics, and development feedback

This is a differentiating requirement. TUR must explain what it is doing, what failed, where evidence is stored, and how the user can hand the evidence to a development agent.

## 6.1 Installer UX requirements

The installer must provide:

- Hardware detection screen.
- Boot mode and Secure Boot status.
- Disk inventory and partition plan.
- Filesystem and encryption choices.
- Bootloader configuration.
- Installation progress grouped by phases.
- Per-phase log access.
- Explicit error messages with stable error codes.
- Retry where retry is safe.
- Cancel where cancellation is safe.
- Safe reboot/recovery guidance after failure.
- Export diagnostic bundle.
- Offline operation. Network must not be required for install unless the user chooses online downloads.

The installer must avoid fake percentage precision. If a phase has no reliable total work estimate, show an indeterminate progress indicator with a concrete activity label.

## 6.2 Installer phase model

Use a state machine with durable phase transitions:

```text
START
  -> ENVIRONMENT_CHECK
  -> HARDWARE_INVENTORY
  -> STORAGE_DISCOVERY
  -> TARGET_SELECTION
  -> PARTITION_PLAN
  -> USER_CONFIRMATION
  -> PARTITIONING
  -> ENCRYPTION_SETUP (optional)
  -> FILESYSTEM_CREATION
  -> IMAGE_DEPLOYMENT
  -> BOOTLOADER_INSTALL
  -> INITRAMFS_CONFIGURATION
  -> FIRST_BOOT_CONFIGURATION
  -> INSTALL_VALIDATION
  -> COMPLETE
```

Every phase must emit:

- `phase_id`
- `phase_name`
- `state` (`pending`, `running`, `succeeded`, `warning`, `failed`, `skipped`, `cancelled`)
- `started_at`
- `finished_at`
- `duration_ms`
- `progress` if meaningful
- `error_code` if failed
- `summary`
- `technical_details`
- `log_reference`
- `recovery_action`

Persist the state machine in the installer session directory. If the installer crashes, the rescue environment should be able to read the last persisted state.

## 6.3 Human-readable error messages

Every error must have two layers:

**User message:** concise, tells the user what happened and what action to take.

**Technical detail:** exact command/component, exit code, stderr excerpt, device identifiers, relevant kernel/journal evidence, and a stable TUR error ID.

Example:

```text
TUR-STORAGE-NTFS-004
The Windows volume was not mounted for writing.

Reason:
The volume appears to be hibernated or in an unsafe state.

Action:
Boot Windows, disable Fast Startup/hibernation, shut down fully,
then retry. TUR mounted the volume read-only to protect your data.

Technical detail:
Device: /dev/disk/by-uuid/<redacted>
Driver: ntfs3
Mount policy: read-only
Evidence: journal entry reference <boot-id>/<cursor>
```

Error messages must never instruct users to force an unsafe filesystem mount as the default fix.

## 6.4 `tur-diag` diagnostic collector

Implement a CLI and GUI entry point:

```bash
tur-diag status
tur-diag boot-report
tur-diag hardware
tur-diag storage
tur-diag drivers
tur-diag runtimes
tur-diag collect --output /run/media/$USER/USB/TUR-support.tar.zst
tur-diag collect --since-boot
tur-diag collect --installer-session <SESSION_ID>
tur-diag verify-bundle <bundle>
```

The diagnostic collector should gather, where available:

- `uname -a`, kernel config summary, command line.
- `journalctl -b` and previous boot journal excerpts.
- `dmesg`.
- systemd failed units and critical-chain report.
- `lsblk -o` inventory, `blkid`, `/etc/fstab`, mount table.
- PCI inventory (`lspci -nnk`).
- USB inventory (`lsusb`, sysfs descriptors).
- Loaded modules and module errors.
- GPU driver, DRM devices, Vulkan summary, Mesa version.
- Display server, KWin version, session type, monitor layout summary.
- Audio devices and PipeWire graph summary.
- Network device and NetworkManager status.
- Bluetooth adapter status.
- Storage driver and mount options.
- TUR service status.
- Runtime versions and prefixes (not user application documents).
- App compatibility report and relevant crash logs when selected.
- Installer phase state and logs.
- Boot attempt and deployment metadata.
- pstore archive if present.
- `fwupdmgr get-devices` output where available.

Do not collect user documents, browser history, passwords, authentication tokens, private keys, application databases, or arbitrary home-directory content.

## 6.5 Privacy and redaction

Diagnostic bundles must be local by default. Upload is a separate explicit action.

Before export, show a preview and redact by default:

- Usernames.
- Hostnames.
- Serial numbers.
- MAC addresses.
- Public IP addresses.
- Disk UUIDs and unique identifiers where not required.
- Wi-Fi SSIDs.
- File paths containing usernames.

Provide a technical mode that allows a developer to include selected identifiers when needed. Make the privacy implications clear.

Bundle metadata must include a schema version. `tur-diag verify-bundle` must verify archive integrity and manifest checksums.

## 6.6 First-boot self-test

On the first graphical login, run a non-destructive self-test:

- Confirm kernel and initramfs build IDs match the installed deployment.
- Confirm systemd reached graphical target.
- Confirm KWin/Wayland started.
- Confirm GPU driver and Vulkan loader are detected.
- Confirm audio service is running.
- Confirm network manager starts.
- Confirm UDisks2 starts.
- Confirm required filesystems are present in `/proc/filesystems` or available as FUSE implementations.
- Confirm runtime manager can execute a minimal Linux binary.
- Confirm Wine can initialize a disposable prefix (if installed).
- Confirm Darling's loader and server can run a smoke test (if installed).
- Confirm diagnostic export path works.

The self-test must not modify user documents or install test applications without consent.

---

# 7. Runtime architecture and universal application manager

## 7.1 Runtime types

### Linux

- Executable format: ELF.
- Loader: kernel ELF loader and system dynamic linker.
- Execution: native Linux process.
- No compatibility layer required.

### Windows

- Executable format: PE/COFF.
- Primary runtime: Wine.
- Gaming runtime: Proton, selected for tested gaming profiles.
- Graphics: DXVK for D3D8/9/10/11 and VKD3D-Proton for D3D12 where supported.
- Synchronization: use Linux `ntsync` where kernel, Wine build, and configuration support it.
- Dependencies: managed per Wine prefix. Do not use one shared prefix for unrelated applications by default.

Wine translates Windows API calls into host-compatible behavior rather than emulating a complete Windows computer. It does not implement every NT kernel interface or Windows driver model.

### macOS

- Executable format: Mach-O, usually inside an `.app` bundle or installed through other macOS packaging conventions.
- Primary runtime: Darling.
- Components: Mach-O loader, `darlingserver`, Darwin syscall/POSIX interfaces, Mach IPC, framework reimplementations.
- GUI support: experimental and application-specific.
- Graphics: Darling's current Metal/Vulkan work must be treated as an evolving upstream dependency.
- Initial scope: x86-64 command-line tools and a narrow GUI test set.

Darling is a Linux runtime environment that implements macOS-facing system interfaces in userspace and reimplements frameworks. Its current README describes GUI support as active development, not complete macOS compatibility.

## 7.2 Universal Runtime Manager (`tur-runtime`)

Responsibilities:

- Inspect executable file format and architecture.
- Detect bundle/package/install formats.
- Identify dependencies and runtime needs where feasible.
- Choose native Linux, Wine, Proton, or Darling.
- Resolve application compatibility profile.
- Create and update isolated prefixes/environments.
- Register application desktop entries and file associations.
- Track runtime version and app version.
- Provide launch, stop, repair, reset, backup, and rollback operations.
- Collect app-specific diagnostic logs.
- Recommend full Windows VM fallback when a known requirement cannot be handled by compatibility mode.

The runtime manager must not execute arbitrary binaries automatically just because a file has a `.exe` or `.app` extension. The user must initiate execution, and downloaded/unknown applications should be subject to security policy.

## 7.3 Application profile schema

Use a versioned JSON or TOML schema. Example:

```json
{
  "schema_version": 1,
  "app_id": "com.example.application",
  "display_name": "Example Application",
  "binary": "/path/to/application.exe",
  "format": "pe",
  "architecture": "x86_64",
  "runtime": "wine",
  "runtime_channel": "stable",
  "runtime_version": "pinned-by-release-manifest",
  "prefix_id": "example-application",
  "graphics_backend": "dxvk",
  "sync_backend": "auto",
  "isolation_profile": "desktop-standard",
  "known_requirements": ["win32", "com"],
  "fallback": "windows-vm-offer",
  "test_status": "untested"
}
```

This is illustrative. The actual schema must be validated, versioned, and documented. Never put secrets in application profiles.

## 7.4 Runtime selection and fallback policy

Selection should combine:

- Executable format and CPU architecture.
- Known application database result.
- Application package metadata.
- Required APIs/libraries where detectable.
- User preferences.
- Runtime availability.
- Security profile.

Fallback tiers:

1. Native Linux execution.
2. Wine/Proton compatibility execution.
3. Darling compatibility execution.
4. Optional full Windows VM for Windows-only kernel or driver dependencies.
5. Explain unsupported status when no safe route exists.

Do not automatically migrate app data between Wine and a full Windows VM. Provide explicit export/import tools where formats permit.

---

# 8. Performance architecture

Performance is a first-class design requirement. Do not add translation or isolation layers unless they deliver a necessary compatibility or security benefit.

## 8.1 CPU

- Initial host target: x86-64.
- Same-architecture Windows and macOS binaries should execute their CPU instructions directly on the host CPU where the runtime supports them.
- No full CPU emulation for normal x86-64 applications.
- Avoid nested virtualization in the normal path.
- Preserve Linux scheduler behavior and CPU affinity support.
- Measure cross-runtime thread and process overhead.

## 8.2 Windows synchronization (`ntsync`)

Linux `ntsync` entered the mainline kernel in Linux 6.14. TUR should enable and test it where available, and pair it with a Wine build that supports it. Runtime must detect actual availability and fall back cleanly when missing.

Do not hard-code a minimum version number as the only capability check. Test the device/interface and runtime support at boot and record the result.

Reference: https://docs.kernel.org/userspace-api/ntsync.html (verify the current canonical URL when implementing)  
Reference: https://wiki.debian.org/Wine/NtsyncHowto

## 8.3 GPU and graphics

Linux owns the GPU and loads the native Linux DRM driver. Applications use host graphics interfaces through translation layers.

Suggested path:

- Linux OpenGL/Vulkan: Mesa or vendor-supported driver.
- Windows D3D8/9/10/11: DXVK to Vulkan.
- Windows D3D12: VKD3D-Proton to Vulkan.
- Windows Vulkan: Wine Vulkan path to host Vulkan.
- macOS Metal: Darling's implementation/translation path as it matures. Treat Metal compatibility as experimental.

Hardware support and Vulkan feature requirements change. Pin known-good versions for each TUR release and test against the GPU driver matrix. DXVK's current driver support page specifies version and Vulkan feature requirements; do not assume an older GPU or driver is compatible.

Repositories:
- https://github.com/doitsujin/dxvk
- https://github.com/HansKristian-Work/vkd3d-proton
- https://gitlab.freedesktop.org/mesa/mesa
- https://github.com/ValveSoftware/Proton

## 8.4 Graphics stack and compositor

Use Wayland with KWin. Support XWayland for applications that still need X11. Avoid forcing every compatibility app through XWayland if a stable native Wayland path exists.

Use DRM/KMS, Mesa, Vulkan loader, and the chosen GPU driver stack. The compositor must not perform unnecessary frame copies for ordinary windows. Test explicit sync, DMA-BUF import/export, buffer formats, fractional scaling, HDR, multi-monitor modes, VRR, and screen capture.

## 8.5 I/O and storage

- Use native kernel filesystem drivers wherever mature.
- Avoid FUSE for primary system storage.
- Use userspace filesystems only when no reliable kernel driver exists and the performance/safety trade-off is explicit.
- Avoid copying files between runtimes. Map the same host paths where semantics are safe.
- Preserve metadata when copying between filesystems where possible.
- Benchmark metadata-heavy workloads as well as sequential throughput.

## 8.6 Performance targets

Targets are engineering goals, not promises:

- Native Linux baseline: no material regression versus the same base distribution on identical hardware.
- Wine CPU workloads: aim for within 5-10% of comparable native workload where API behavior is not a bottleneck.
- DXVK gaming: aim for within 10-15% of native Windows for selected supported games, measured with the same hardware and settings. Some titles will be worse or unsupported.
- Darling: establish application-specific baselines. Do not set a global macOS performance claim until the framework coverage is broad enough.
- TUR shell idle overhead: keep CPU and memory use low and publish actual measured numbers.

Every benchmark must record hardware, kernel, driver, runtime versions, power profile, resolution, application version, warm-up procedure, and test repetitions.

---

# 9. Desktop and GUI architecture

## 9.1 Foundation

Use KDE Plasma 6 with Wayland as the desktop foundation. The version in the initial image should be pinned and tested; update to newer Plasma releases only through the normal compatibility and regression pipeline.

Primary components:

- Plasma Desktop and Workspace.
- KWin Wayland compositor.
- Qt 6 and QML for TUR-specific UI.
- Dolphin as the initial file manager.
- KRunner for search/command launch.
- KDE System Settings as the system-settings foundation.
- Discover or a customized software center for application catalog integration.
- XDG Desktop Portals for controlled access to files, screen capture, notifications and other desktop services.

References:
- https://kde.org/announcements/plasma/6/6.8.0/
- https://invent.kde.org/plasma/kwin
- https://invent.kde.org/plasma/plasma-desktop
- https://invent.kde.org/plasma/plasma-workspace

## 9.2 TUR visual modes

Provide optional visual profiles, not separate desktops:

- **Classic:** Windows 10-inspired taskbar, application launcher, system tray, conventional window controls.
- **Modern:** clean layout, adaptive panels, subtle translucency, modern search and notification surfaces.
- **Professional:** multi-monitor workspace, tiling/quick-layout controls, performance and runtime diagnostics.

All profiles use the same KWin session and underlying application services. Changing visual profiles must not restart running applications.

## 9.3 TUR Desktop Bridge (`tur-desktop-bridge`)

Responsibilities:

- Import Wine and Darling application windows into the host Wayland session where supported.
- Normalize desktop entries and application icons.
- Register application menu entries and file associations.
- Provide common notification behavior.
- Integrate clipboard access through supported host interfaces.
- Integrate file selection through portals or compatibility-specific adapters.
- Normalize drag-and-drop where possible.
- Provide screen capture and screen-sharing access through portals.
- Preserve application identity, runtime, and process metadata for task switching and diagnostics.

Do not assume every Wine or Darling window will behave as a native Wayland surface. XWayland and compatibility fallback paths must remain available and testable.

## 9.4 Shared desktop services

Build around D-Bus and XDG Desktop Portals. Prefer existing portal backends over bespoke file pickers or screen capture services.

Common services:

- File chooser.
- Clipboard.
- Notifications.
- Screen capture and screen sharing.
- Printing.
- Open URI.
- Application launching.
- Device permission prompts.
- Session inhibition and power actions.

Each runtime adapter must state which services are supported, partially supported, or unavailable.

## 9.5 GUI quality requirements

- Responsive interaction under normal multi-application load.
- No blocking UI thread during disk scans, runtime setup, package installation, or hardware discovery.
- High-DPI and fractional scaling tests.
- Keyboard navigation and accessibility.
- Multiple monitor support.
- Screen reader and input-method testing.
- Theme consistency without forcing foreign apps to abandon their own UI conventions.
- Clear visual indication of runtime environment in app properties and diagnostics, without cluttering every window.

---

# 10. Hardware and device integration

## 10.1 Device ownership principle

Linux owns the physical device by default. Compatibility runtimes consume host services or host APIs. Device passthrough is explicit and exclusive.

The same physical USB device must not be controlled by a Linux driver and a Windows VM driver at the same time.

## 10.2 Device Manager (`tur-device-manager`)

Core responsibilities:

- Subscribe to udev device events.
- Read sysfs and device descriptors.
- Map device IDs to kernel drivers and firmware status.
- Expose a stable device inventory through D-Bus.
- Track permission and ownership state.
- Coordinate device hotplug and removal.
- Offer diagnostics for unknown/unbound devices.
- Manage optional passthrough to a VM.
- Record driver binding/unbinding operations.

Do not invent a second kernel driver database. Use kernel module aliases, modalias, sysfs, udev, and distribution package metadata.

## 10.3 Device categories

| Category | Host implementation | Compatibility integration |
|---|---|---|
| USB storage | Linux block layer + UDisks2 | Shared volume namespace |
| USB HID | HID/input subsystem + libinput | Wine input APIs and Darling input frameworks as supported |
| USB serial | TTY/udev | COM-port mapping for Wine; serial access adapters |
| USB WinUSB device | libusb/udev permissions | Wine WinUSB support where compatible |
| Webcam | V4L2/libcamera + PipeWire | DirectShow/Media Foundation adapters in Wine; AVFoundation work in Darling |
| Audio interface | ALSA + PipeWire | WASAPI/DirectSound in Wine; CoreAudio in Darling |
| Bluetooth | BlueZ | Host pairing and profile integration |
| Wi-Fi/Ethernet | Linux drivers + NetworkManager | Host networking |
| Printer | CUPS/IPP | Windows printing compatibility and macOS printing adapter |
| Scanner | SANE | Runtime adapters where available |
| GPU | DRM/KMS + Mesa/vendor driver | Vulkan translation and VM passthrough |
| Thunderbolt | Linux Thunderbolt subsystem + bolt | Host authorization and device policy |
| Firmware | fwupd/LVFS | Host firmware update service |
| Fingerprint | fprintd/driver | PAM login integration; foreign app API work later |

## 10.4 USB device state machine

A USB device should pass through explicit states:

`discovered -> identified -> driver_bound -> permission_assigned -> available`

For passthrough:

`available -> handoff_requested -> host_driver_unbound -> guest_attached -> guest_owned -> guest_detached -> host_rebind_attempted -> available`

The device manager must log every transition. If guest detach fails, do not silently attempt conflicting ownership. Show the device as unavailable and provide a recovery action.

## 10.5 Firmware

Integrate fwupd and supported LVFS metadata. Firmware update flow must include AC power/battery checks where required, signature verification, clear reboot requirements, recovery instructions, and update history.

Do not update firmware automatically without explicit user policy. A failed firmware update is materially different from a failed application update.

## 10.6 Driver strategy

- Use in-tree kernel drivers whenever possible.
- Package additional Linux drivers only after license, Secure Boot signing, update, and recovery review.
- Never distribute Windows or macOS kernel drivers as if Linux could load them.
- Record unsupported devices and provide `tur-diag hardware` output.
- Keep a hardware compatibility database with PCI/USB IDs, kernel driver, firmware, test status, and known caveats.

---

# 11. Universal storage architecture

## 11.1 Goals

- Detect disks and partitions safely.
- Support common partition tables and volume managers.
- Mount supported filesystems with safe options.
- Present shared volumes to native Linux apps, Wine, and Darling.
- Preserve metadata and security semantics where possible.
- Refuse unsafe writes when a volume is dirty, hibernated, or unsupported.
- Provide clear status and recovery instructions.

## 11.2 Partition and volume discovery

Use:

- udev and sysfs for discovery.
- `blkid`/libblkid for filesystem identification.
- UDisks2 and libblockdev for storage operations.
- GPT and MBR support as baseline.
- LVM and Linux MD RAID where included in the selected installer/storage model.
- LUKS2 through cryptsetup.
- Apple Partition Map only as a legacy read/discovery feature, not a primary install target.

## 11.3 Filesystem policy

| Filesystem | Initial policy | Implementation | Notes |
|---|---|---|---|
| ext4 | Read/write | Linux kernel | Primary supported Linux filesystem |
| Btrfs | Read/write | Linux kernel | Good fit for snapshots and subvolumes |
| XFS | Read/write | Linux kernel | Supported data volume |
| NTFS | Read/write when clean | Linux NTFS3 | Refuse unsafe writes to hibernated/dirty volumes |
| exFAT | Read/write | Linux kernel | Good removable-media format |
| FAT32/VFAT | Read/write | Linux kernel | EFI and removable media |
| HFS+ | Policy-dependent | Linux kernel driver | Caution with journaled volumes; validate before write |
| APFS | Read-only initially | apfs-fuse | Incomplete feature coverage; FUSE overhead |
| APFS experimental | Disabled by default | linux-apfs-rw | Reverse-engineered, write risk and encryption gaps |
| ReFS | Unsupported initially | No mature general-purpose path assumed | Do not advertise as supported |
| ZFS | Optional | OpenZFS | Separate licensing/kernel compatibility review |

References:
- NTFS3: https://docs.kernel.org/filesystems/ntfs3.html
- APFS-FUSE: https://github.com/sgan81/apfs-fuse
- Experimental APFS write project: https://github.com/linux-apfs/linux-apfs-rw

APFS-FUSE currently describes itself as read-only and notes unsupported or incomplete behaviors such as firmlinks and some compression methods. The experimental `linux-apfs-rw` project explicitly warns of corruption risk and has significant limitations, including encryption limitations. TUR must therefore make APFS read-only by default.

## 11.4 Mount namespace

Use stable internal mount paths based on filesystem UUID or a generated volume identifier, for example:

`/run/tur/volumes/<volume-id>`

Expose user-friendly paths such as `/run/media/<user>/<label>` and, where needed for compatibility, a controlled `/Volumes/<label>` mapping for Darling. Wine drive letters map to the same host mount through Wine's prefix configuration.

Do not mount the same physical filesystem multiple times just to satisfy different runtimes unless a namespace or security boundary requires it. Prefer bind mounts or runtime-specific namespace mappings referencing one canonical host mount.

## 11.5 File semantics and metadata

Track support for:

- POSIX permissions and ownership.
- ACLs.
- Extended attributes.
- Windows alternate data streams.
- Windows security descriptors.
- Apple resource forks.
- Case-sensitive/case-insensitive lookup.
- Symbolic links and hard links.
- Sparse files.
- Compression.
- Snapshots and copy-on-write behavior.

Do not silently discard metadata during cross-filesystem copy. When a destination filesystem cannot represent a source feature, notify the user or preserve metadata in a documented sidecar format only if the user explicitly chooses that mode.

## 11.6 NTFS safety

If an NTFS volume is hibernated, dirty, or otherwise unsafe, mount read-only or refuse mounting based on driver state. Tell the user to boot Windows and perform a full shutdown/disable hibernation where needed. Never recommend forced writable mounting as the default recovery action.

## 11.7 Universal Storage service

Implement `tur-storage` as a D-Bus service with:

- Volume inventory.
- Filesystem type and driver.
- Read/write capability.
- Encryption/locked status.
- Health and dirty-state information where available.
- Mount/unmount operations.
- Runtime visibility mappings.
- Stable error codes.
- Diagnostic references.

The service should orchestrate UDisks2/libblockdev rather than replace them.

---

# 12. Security and isolation

TUR runs applications from ecosystems with different security models. The system must not weaken the host merely to improve compatibility.

## 12.1 Host security

Use the selected distribution's supported security baseline. Evaluate SELinux or AppArmor and choose one primary policy framework for the first product image. Do not ship an unmaintained custom LSM policy framework.

Use:

- polkit for privileged operations.
- D-Bus policy and service ownership controls.
- Wayland/XDG portals for desktop resource access.
- Bubblewrap/namespaces for application isolation where compatible.
- seccomp and Landlock where appropriate.
- Signed packages/images.
- Secure Boot support.
- No Wine/Darling runtime as root.

## 12.2 Wine isolation

Each app should have an independent prefix by default. A prefix contains application registry state, DLL overrides, installed dependencies, and app-specific configuration.

Do not expose the full host filesystem to every Windows application by default. Use a least-privilege mapping. The user may grant access to selected directories through a desktop portal-like flow.

## 12.3 Darling isolation

Darling's userspace server and framework implementation should run under a constrained user context. Validate which namespaces and kernel capabilities it requires. Document any feature that needs broader permissions. Do not grant unrestricted host access to satisfy one GUI feature without an ADR and threat review.

## 12.4 VM security

For full Windows mode:

- Use QEMU/KVM.
- Use UEFI firmware such as OVMF.
- Use a virtual TPM where required by the Windows version/configuration.
- Use virtio devices where supported.
- Use VFIO only for compatible, isolated PCI devices.
- Keep the guest network policy explicit.
- Maintain Windows licensing and activation compliance.
- Never imply that VM mode shares the Linux kernel.

---

# 13. Full Windows execution mode

This is an optional compatibility backend, not the default application execution model.

## 13.1 Purpose

Use a genuine Windows NT kernel for software that depends on:

- Windows kernel drivers.
- Kernel-mode anti-cheat or DRM.
- Unsupported NT services.
- Application installers that assume a full Windows environment.
- Vendor hardware management utilities requiring Windows driver stacks.
- Applications with unacceptable Wine compatibility.

## 13.2 Mode A: QEMU/KVM

Use QEMU/KVM for simultaneous Linux and Windows operation.

Possible components:

- QEMU.
- KVM.
- libvirt/virt-manager or a TUR VM manager.
- OVMF UEFI firmware.
- virtio storage/network/display where suitable.
- VFIO for PCIe GPU/device passthrough where hardware supports it.
- Looking Glass for low-latency guest display integration where compatible.

The VM must be optional. Do not require it to launch ordinary Wine-compatible apps.

## 13.3 Mode B: dual boot

Offer a documented dual-boot path for cases where direct hardware access and native Windows drivers matter more than simultaneous operation.

Do not rely on OpenCore for the normal Linux boot. Use the TUR/Linux bootloader for TUR and register the Windows boot entry through standard UEFI mechanisms.

## 13.4 User experience

The user may select “Run in full Windows” from an application's compatibility settings. TUR should explain:

- This starts or resumes a real Windows installation/VM.
- The application may not share the normal Linux process or filesystem semantics.
- GPU/device passthrough may require a reboot or exclusive device handoff.
- Windows requires an appropriately licensed installation.
- The selected Windows edition must still be within its applicable support lifecycle or enrolled in a valid Extended Security Updates program.

**Windows 10 lifecycle warning:** As of this document's date (12 November 2026), ordinary Windows 10 Home and Pro 22H2 reached end of support on 14 October 2025. Certain LTSC and IoT LTSC editions have separate lifecycle dates and licensing terms. The VM wizard must identify the edition/version where possible and show a support-status notice based on current Microsoft lifecycle data. Do not describe an unsupported Windows 10 Home/Pro installation as a secure, fully supported daily-driver environment. References: https://learn.microsoft.com/en-us/windows/release-health/release-information and https://learn.microsoft.com/en-us/lifecycle/products/windows-10-home-and-pro.

---

# 14. Installer and system deployment

## 14.1 Installer approach

Prototype the image install using bootc's supported installation workflow. A custom TUR graphical installer can orchestrate this process after safety checks and partition planning are mature. Calamares may be evaluated for UI and partitioning modules, but do not assume it natively solves every bootc/OSTree deployment requirement.

The installer should be separable from the installed system image and should support offline installation from the ISO.

## 14.2 Installation modes

- Erase target disk and install TUR.
- Install alongside an existing operating system where partitioning and bootloader detection are reliable.
- Manual partitioning for advanced users.
- Encrypted installation.
- Reinstall TUR while preserving a supported user-data partition.
- Live environment with diagnostics but no installation.

## 14.3 Preflight checks

- UEFI boot mode.
- Secure Boot state.
- CPU architecture and required instruction support.
- RAM and disk capacity.
- GPU/display support.
- Storage controller visibility.
- Existing partition table and mounted volumes.
- Encryption and filesystem recognition.
- Network availability (informational unless needed for optional download).
- Whether the installer itself booted in safe graphics mode.

## 14.4 Installer logs

All installer commands must be executed through a wrapper that records command, sanitized arguments, start/end time, exit code, stdout/stderr, and phase ID. Never log passwords, encryption keys, tokens, or private key material.

The installer must be able to export a support bundle even if installation fails before the target root is mounted.

---

# 15. Update, rollback, backup, and recovery policy

## 15.1 OS updates

- Signed image/commit.
- Content verification.
- Staged deployment.
- Atomic activation.
- Bounded boot attempts.
- Health assessment.
- Automatic fallback.
- Retain at least one previous known-good deployment, subject to available disk space.
- Preserve user data separately.

## 15.2 Runtime updates

Wine, Proton, DXVK, VKD3D-Proton, Darling, Mesa, KDE and other user-facing components should have version manifests. Runtime upgrades must not silently mutate every Wine prefix or Darling environment.

For each application, retain enough metadata to reproduce the previous runtime configuration. Support pinning a working runtime version per application.

## 15.3 Configuration backup

Back up TUR configuration separately from user documents. Include:

- TUR service configuration.
- Runtime profiles.
- Device policy configuration.
- Desktop settings.
- Application registrations.
- Mount policy (not encryption keys).

Do not automatically include large app prefixes unless the user selects them. Prefixes may contain licensed application files and user data; backup policy must explain this.

## 15.4 Recovery menu

At boot show:

- TUR current deployment.
- TUR previous deployment.
- Fallback kernel if available.
- Rescue environment.
- Firmware setup option where firmware exposes one.
- Windows boot entry if installed and detected.

The recovery interface should identify the selected deployment and show build ID and date.

---

# 16. Service APIs and integration contracts

Prefer D-Bus for system services and documented command-line interfaces for diagnostics and automation.

Suggested TUR services:

| Service | Suggested D-Bus name | Main function |
|---|---|---|
| Runtime manager | `org.tur.Runtime1` | Detect and launch applications |
| Device manager | `org.tur.Device1` | Device inventory and handoff |
| Storage manager | `org.tur.Storage1` | Volume inventory and policy |
| Desktop bridge | `org.tur.DesktopBridge1` | Runtime-to-desktop integration |
| Diagnostics | `org.tur.Diagnostics1` | Reports and support bundles |
| Update manager | `org.tur.Update1` | Stage, deploy, assess, rollback |
| Installer | `org.tur.Installer1` | Installation phase state |

D-Bus names are proposed and must be registered in an interface specification before implementation. Every method must define authorization, return types, error names, timeout policy, and cancellation semantics.

Use versioned API schemas. Do not expose unstable internal C++ or Rust types as public system interfaces.

---

# 17. Repository shortlist and integration strategy

This is a starting shortlist. Verify current release, branch, maintenance status, build system, license, submodules, and transitive dependencies before pinning.

| Project | Repository | TUR role | Strategy |
|---|---|---|---|
| Linux kernel | https://github.com/torvalds/linux | Host kernel | Use distribution kernel; maintain small config/patch set |
| systemd | https://github.com/systemd/systemd | Init, services, logging, boot assessment | Integrate |
| bootc | https://github.com/bootc-dev/bootc | Image-based install/update | Integrate and test |
| OSTree | https://github.com/ostreedev/ostree | Atomic deployments/rollback | Integrate |
| Wine | https://github.com/wine-mirror/wine | Windows compatibility | Integrate; patch only with reason |
| Proton | https://github.com/ValveSoftware/Proton | Gaming-focused Wine package | Integrate as optional runtime channel |
| DXVK | https://github.com/doitsujin/dxvk | D3D8/9/10/11 to Vulkan | Integrate |
| VKD3D-Proton | https://github.com/HansKristian-Work/vkd3d-proton | D3D12 to Vulkan | Integrate |
| Darling | https://github.com/darlinghq/darling | macOS compatibility | Integrate and maintain research patches separately |
| Mesa | https://gitlab.freedesktop.org/mesa/mesa | Open graphics stack | Integrate distribution builds |
| KDE Plasma | https://invent.kde.org/plasma | Desktop | Integrate; theme and extend |
| KWin | https://invent.kde.org/plasma/kwin | Wayland compositor | Integrate; avoid deep fork |
| UDisks2 | https://github.com/storaged-project/udisks | Disk management | Integrate |
| libblockdev | https://github.com/storaged-project/libblockdev | Block device operations | Integrate |
| PipeWire | https://gitlab.freedesktop.org/pipewire/pipewire | Audio/video graph | Integrate |
| NetworkManager | https://gitlab.freedesktop.org/NetworkManager/NetworkManager | Networking | Integrate |
| BlueZ | https://github.com/bluez/bluez | Bluetooth | Integrate |
| fwupd | https://github.com/fwupd/fwupd | Firmware updates | Integrate |
| Flatpak | https://github.com/flatpak/flatpak | Desktop app packaging/sandboxing | Integrate where appropriate |
| Bubblewrap | https://github.com/containers/bubblewrap | Process sandboxing | Integrate |
| XDG Desktop Portals | https://github.com/flatpak/xdg-desktop-portal | Desktop permission/portal APIs | Integrate |
| Calamares | https://github.com/calamares/calamares | Possible installer UI foundation | Evaluate; do not assume bootc integration |
| QEMU | https://github.com/qemu/qemu | Windows VM backend | Integrate optional |
| libvirt | https://github.com/libvirt/libvirt | VM management | Integrate optional |
| Looking Glass | https://github.com/gnif/LookingGlass | VM display integration | Evaluate |
| apfs-fuse | https://github.com/sgan81/apfs-fuse | APFS read-only access | Optional, strict read-only |
| linux-apfs-rw | https://github.com/linux-apfs/linux-apfs-rw | APFS write research | Research branch only; never default |

## 17.1 Forking policy

- Integrate upstream releases using distribution packaging where possible.
- Keep patches in a small TUR patch queue.
- Rebase frequently.
- Submit generic fixes upstream.
- Do not silently change upstream licenses.
- Maintain a `THIRD_PARTY_NOTICES` and SPDX SBOM.
- Record source commit, patch list, build flags, and binary hash for every shipped dependency.

## 17.2 License and redistribution review

Before shipping a component, record:

- SPDX license identifier.
- Copyright notices.
- Required source-code offer or source distribution obligations.
- Static/dynamic linking implications.
- Bundled binary redistribution rights.
- Trademark usage constraints.
- Whether component pulls proprietary codecs, firmware, fonts, or SDKs.

Do not bundle proprietary Apple frameworks or Windows system binaries as a shortcut. TUR should provide compatibility infrastructure and require users to supply software they are legally entitled to use. Legal review is required before public distribution.

---

# 18. Build, CI, release engineering, and repository layout

## 18.1 Main TUR repository

The main TUR repository should own TUR code, image definitions, configuration, tests, and integration metadata. It should not copy entire upstream repositories into the tree unless there is a clear reason.

Suggested structure:

```text
tur/
  README.md
  CONTRIBUTING.md
  AGENTS.md
  SECURITY.md
  LICENSES/
  THIRD_PARTY_NOTICES/
  sbom/
  docs/
    architecture/
    adr/
    install/
    recovery/
    compatibility/
    performance/
  image/
    Containerfile
    packages/
    kernel-config/
    boot/
    branding/
  services/
    tur-runtime/
    tur-device-manager/
    tur-storage/
    tur-desktop-bridge/
    tur-diagnostics/
    tur-update/
  desktop/
    plasma-config/
    themes/
    launcher/
    device-center/
    installer-ui/
  runtimes/
    wine/
    proton/
    darling/
    dxvk/
    vkd3d-proton/
  tests/
    unit/
    integration/
    boot/
    installer/
    storage/
    hardware/
    compatibility/
    performance/
  scripts/
  release/
```

## 18.2 Build manifest

Every build must produce a machine-readable manifest:

```json
{
  "schema_version": 1,
  "tur_version": "0.1.0-dev",
  "build_id": "ci-generated-immutable-id",
  "architecture": "x86_64",
  "kernel_release": "build-value",
  "kernel_config_sha256": "build-value",
  "image_digest": "build-value",
  "bootloader": "build-value",
  "desktop": "KDE Plasma",
  "runtime_versions": {
    "wine": "pinned-version",
    "proton": "pinned-version-or-absent",
    "darling": "pinned-commit",
    "dxvk": "pinned-version",
    "vkd3d_proton": "pinned-version"
  },
  "sbom": "sbom.spdx.json"
}
```

The manifest must report actual values from the build, not copied placeholders.

## 18.3 CI pipeline

Required stages:

1. Formatting and linting.
2. Static analysis.
3. Unit tests.
4. Service API schema validation.
5. Build TUR services.
6. Build container/OS image.
7. SBOM generation and license checks.
8. VM boot smoke test.
9. Installer automated test against disposable disk images.
10. Update/rollback test.
11. Diagnostic bundle schema test.
12. Wine smoke tests.
13. Darling smoke tests.
14. Graphics capability smoke tests.
15. Performance benchmark on dedicated hardware runners.

Do not run destructive installer tests on developer hosts. Use disposable VM disk images and dedicated test hardware.

---

# 19. Testing and acceptance criteria

## 19.1 Native Linux acceptance

- System boots repeatedly on supported test hardware.
- KDE session starts and remains responsive.
- Storage, networking, audio, USB and GPU work.
- Native app launch and file operations are no worse than the base distribution beyond defined measurement tolerance.

## 19.2 Windows compatibility acceptance

- A PE executable is identified correctly.
- Wine runtime and prefix are selected.
- A tested Win32/Win64 app launches from the TUR application launcher.
- App window participates in KWin task switching.
- Clipboard and file access work for the supported test profile.
- DXVK test workload uses Vulkan when applicable.
- `ntsync` is detected and used when supported.
- Unsupported driver/kernel requirements are explained rather than hidden.

## 19.3 macOS compatibility acceptance

- Mach-O architecture and format are identified.
- Darling loader/server starts as an unprivileged user.
- A supported CLI application launches and returns expected output.
- Application logs are captured by TUR diagnostics.
- Unsupported framework calls produce useful errors.
- No claim of full macOS app compatibility is made.

GUI acceptance should be a separate gate and require a specifically enumerated test app set.

## 19.4 Storage acceptance

- GPT/MBR layouts are detected.
- NTFS3 clean volume mounts read/write.
- Hibernated/dirty NTFS volume is protected from unsafe writes.
- exFAT, ext4, Btrfs and XFS mount correctly.
- APFS-FUSE mounts read-only where supported.
- Experimental APFS write is absent from production images or visibly disabled.
- Every mount can be unmounted cleanly.
- No duplicate mount ownership is created across runtimes.
- Filesystem errors are represented with stable TUR error codes.

## 19.5 Installer and recovery acceptance

- Installer detects target hardware.
- Installer records every phase.
- Installer stops before destructive operations if target identity changes.
- An induced failure creates an exportable diagnostic bundle.
- A power interruption during deployment leaves either the old or new valid deployment, not a half-written live root.
- A failed new deployment returns to the previous deployment within the configured boot-attempt policy.
- Rescue media can mount and inspect installed volumes.
- Reinstall workflow preserves user data when the user chooses the supported preserve-data path.

## 19.6 Performance acceptance

Every supported benchmark set must publish:

- Base OS result.
- TUR result.
- Hardware and firmware.
- Kernel and driver versions.
- Runtime versions.
- Test commands and configuration.
- Median and variance across repeated runs.
- Known environmental effects (power profile, thermal state, background load).

Do not use a single screenshot or one run as a performance claim.

---

# 20. Development roadmap with concrete deliverables

## Phase 0: Architecture and build foundation

Deliverables:

- Main repository initialized.
- `AGENTS.md`, contribution guide, threat model, ADR process.
- Dependency/license inventory.
- Image build pipeline.
- CI that builds an ISO/image.
- VM boot test.

Exit criteria: a reproducible TUR image boots in QEMU and emits a build manifest.

## Phase 1: Boot, diagnostics, and recovery first

Deliverables:

- UEFI boot.
- Kernel/initramfs integration.
- Persistent journal.
- `tur-diag` CLI.
- Installer phase state machine.
- Recovery boot entry.
- Atomic update proof-of-concept.
- Boot health assessment.

Exit criteria: intentionally break the next deployment and prove automatic fallback. Export a diagnostic bundle from both successful and failed boot paths.

## Phase 2: Hardware and storage foundation

Deliverables:

- udev inventory service.
- UDisks2/libblockdev integration.
- Device Center UI prototype.
- NetworkManager, BlueZ, PipeWire, fwupd integration.
- NTFS3, exFAT, ext4, Btrfs and XFS policy.
- APFS read-only experiment.

Exit criteria: hotplug USB devices, disks, audio and network without reboot; diagnostics show useful state.

## Phase 3: Native desktop and TUR shell

Deliverables:

- KDE Plasma Wayland image.
- TUR branding and visual modes.
- Application launcher integration.
- TUR Desktop Bridge skeleton.
- Portal integration.

Exit criteria: polished responsive desktop with no runtime compatibility dependencies.

## Phase 4: Windows runtime

Deliverables:

- Wine integration.
- Proton optional channel.
- DXVK/VKD3D packaging.
- NTSync detection.
- Prefix manager.
- Application profiles and test database.
- Windows app launcher/file association.

Exit criteria: defined Windows applications launch, integrate with desktop, and produce reproducible test reports.

## Phase 5: macOS runtime

Deliverables:

- Darling build/integration.
- CLI app launcher.
- Darwin dependency diagnostics.
- Filesystem mapping.
- Audio/display integration where upstream support allows.
- GUI application test matrix.

Exit criteria: a published list of working macOS CLI apps and an explicitly limited GUI subset.

## Phase 6: Full Windows backend

Deliverables:

- QEMU/KVM integration.
- VM creation wizard.
- OVMF and virtual TPM configuration where required.
- Optional GPU/USB passthrough.
- Looking Glass evaluation.
- Windows app fallback linking.

Exit criteria: a genuine, properly licensed Windows guest launches and receives an explicitly assigned device without conflicts with the host.

## Phase 7: Hardening and release engineering

Deliverables:

- Security review.
- License audit.
- SBOM and signed artifacts.
- Hardware compatibility matrix.
- Performance regression dashboard.
- Recovery documentation.
- Release process and update channels.

Exit criteria: independent tester can install, use, update, deliberately break, recover, and submit useful diagnostics without developer intervention.

---

# 21. Mandatory operational feedback for development agents

At the end of every implementation task, produce a report using this template:

```text
TUR DEVELOPMENT REPORT

Task ID:
Subsystem:
Build ID:
Source revision:

SUMMARY
- What changed:
- Why it changed:

FILES
- Added:
- Modified:
- Removed:

BUILD
- Command:
- Result:
- Warnings:

TESTS
- Unit tests:
- Integration tests:
- Boot tests:
- Hardware tests:
- Performance tests:

DIAGNOSTICS
- Log paths:
- Diagnostic bundle path:
- Relevant error IDs:

RECOVERY
- Rollback procedure:
- Recovery tested:

KNOWN LIMITATIONS
- ...

NEXT SAFE ACTION
- ...
```

The agent must not say "working" without stating what was built and tested. If the machine cannot boot, the report must include the last successful boot ID, selected deployment, relevant pstore/journal evidence, and recovery action attempted.

---

# 22. Initial task backlog for the first development agent

The first agent should not begin by modifying Wine or Darling. It should establish a reliable, observable base system.

## Task TUR-001: Create repository and architecture records

- Create main repository structure.
- Add this specification.
- Add `AGENTS.md` with development constraints.
- Add ADR templates.
- Add dependency inventory and licensing checklist.

## Task TUR-002: Build minimal TUR image

- Choose bootc/OSTree distribution foundation.
- Pin base image and kernel.
- Install KDE Plasma Wayland and core system services.
- Build reproducible image.
- Generate manifest and SBOM.

## Task TUR-003: QEMU smoke boot

- Boot image under UEFI QEMU.
- Verify graphical session.
- Verify network and storage.
- Capture serial console and journal.
- Save screenshot and test report.

## Task TUR-004: Persistent diagnostics

- Configure journald persistence.
- Implement `tur-diag status` and `tur-diag collect`.
- Add bundle manifest and checksum.
- Add privacy redaction.
- Test export to a removable disk image.

## Task TUR-005: Boot fallback proof

- Create second OS deployment.
- Deliberately make it fail a health check.
- Verify bounded retry and previous-deployment fallback.
- Verify logs identify the failed deployment and reason.

## Task TUR-006: Installer prototype

- Detect disks and partition tables.
- Implement non-destructive preflight and dry-run plan.
- Use disposable VM images for destructive integration tests.
- Persist all phase states and logs.
- Export failure report.

## Task TUR-007: Hardware inventory

- Implement D-Bus device inventory using udev/sysfs.
- Report PCI, USB, GPU, network, audio, storage and firmware status.
- Never change device ownership in this task.

Only after these tasks pass should the agent proceed to integrate Wine and Darling.

---

# 23. Open questions to resolve through ADRs

These questions do not block the first bootable prototype but must be resolved before a public beta:

1. Which bootloader is the best supported combination for the chosen bootc/OSTree release and target hardware: systemd-boot or GRUB?
2. Which base distribution provides the best kernel, Mesa, Wine, Darling, KDE, security and update balance?
3. Should TUR use SELinux or AppArmor as its primary mandatory access-control framework?
4. Should the initial OS image be built as a bootc container image or assembled through a more traditional distribution image builder?
5. Which Wine version/channel is the baseline, and how should Proton be exposed outside Steam?
6. What minimum Darling feature set is required for the first macOS compatibility milestone?
7. How will the project handle Apple Silicon-only Mach-O applications on an x86-64 host? Defer initially; later compare FEX/QEMU-based approaches and licensing/technical constraints.
8. What is the official APFS policy after initial read-only support?
9. Which Windows VM stack is the supported backend, and which GPU passthrough configurations will be validated?
10. How should runtime and application data be backed up without unnecessarily duplicating large prefixes?
11. Which telemetry, if any, is opt-in? Default should be local-only diagnostics.
12. Which visual mode is the default: Classic or Modern?

Every ADR must record context, options, decision, consequences, and date.

---

# 24. Risk register

| Risk | Severity | Mitigation |
|---|---|---|
| Darling GUI/API coverage is incomplete | High | Limit initial claims; upstream collaboration; app-specific matrix |
| Windows kernel drivers cannot run as Linux drivers | High | Native Linux driver search; VM passthrough; dual boot fallback |
| APFS writes corrupt user data | Critical | Read-only default; experimental branch only; image-based tests |
| Bootloader/update regression causes unbootable system | Critical | Atomic deployments, boot counting, previous deployment, rescue ISO |
| Installer selects wrong disk | Critical | Stable device identity, plan preview, revalidation, explicit confirmation |
| Third-party runtime update breaks prefixes | High | Per-app runtime pinning, prefix backup, regression suite |
| GPU driver regression | High | Known-good kernel/Mesa matrix, fallback kernel, safe graphics mode |
| Secure Boot blocks modules | Medium/High | Signing pipeline, enrollment workflow, clear diagnostics |
| Wine/Proton API gaps | High | Compatibility database, VM fallback, app-specific profiles |
| Inconsistent cross-filesystem metadata | Medium/High | Capability registry, metadata-preserving copy, user warnings |
| Privacy leakage in support bundle | High | Local-only default, redaction, preview, schema tests |
| Upstream license incompatibility | High | SPDX audit, SBOM, legal review before distribution |

---

# 25. Documentation and source references

The following official or upstream resources should be read before implementation. Re-check their current status and API documentation at the time a dependency is pinned.

## Core runtime projects

- Wine: https://www.winehq.org/ and https://github.com/wine-mirror/wine
- Proton: https://github.com/ValveSoftware/Proton
- Darling: https://github.com/darlinghq/darling
- DXVK: https://github.com/doitsujin/dxvk
- VKD3D-Proton: https://github.com/HansKristian-Work/vkd3d-proton

## Linux kernel and performance

- Linux kernel: https://github.com/torvalds/linux
- NTSync kernel interface: https://docs.kernel.org/userspace-api/ntsync.html
- GPU driver developer guide: https://dri.freedesktop.org/docs/drm/gpu/index.html
- Mesa: https://gitlab.freedesktop.org/mesa/mesa

## Desktop

- KDE Plasma: https://kde.org/plasma-desktop/
- Plasma 6.8 release: https://kde.org/announcements/plasma/6/6.8.0/
- KWin: https://invent.kde.org/plasma/kwin
- XDG Desktop Portal: https://flatpak.github.io/xdg-desktop-portal/
- Wayland: https://wayland.freedesktop.org/

## Storage and devices

- NTFS3: https://docs.kernel.org/filesystems/ntfs3.html
- APFS-FUSE: https://github.com/sgan81/apfs-fuse
- Experimental APFS write: https://github.com/linux-apfs/linux-apfs-rw
- UDisks2: https://github.com/storaged-project/udisks
- libblockdev: https://github.com/storaged-project/libblockdev
- PipeWire: https://pipewire.org/
- NetworkManager: https://networkmanager.dev/
- BlueZ: https://github.com/bluez/bluez
- fwupd: https://github.com/fwupd/fwupd
- libinput: https://gitlab.freedesktop.org/libinput/libinput

## Boot, updates, recovery

- bootc: https://github.com/bootc-dev/bootc
- bootc install to disk: https://github.com/bootc-dev/bootc/blob/main/docs/src/man/bootc-install-to-disk.8.md
- OSTree: https://github.com/ostreedev/ostree
- OSTree atomic upgrades: https://ostreedev.github.io/ostree/atomic-upgrades/
- systemd boot assessment: https://github.com/systemd/systemd/blob/main/docs/AUTOMATIC_BOOT_ASSESSMENT.md
- systemd-boot: https://www.freedesktop.org/software/systemd/man/latest/sd-boot.html
- systemd-pstore: https://www.freedesktop.org/software/systemd/man/latest/systemd-pstore.service.html
- Kernel kdump: https://docs.kernel.org/admin-guide/kdump/kdump.html

## Virtualization

- QEMU: https://github.com/qemu/qemu
- libvirt: https://github.com/libvirt/libvirt
- Looking Glass: https://github.com/gnif/LookingGlass

## Notes on current status

The architecture in this document uses the state of upstream projects as understood on 12 November 2026. It is not a guarantee that every upstream component supports all proposed integrations. The development agent must inspect the current code, release notes, issue tracker, and test coverage before pinning versions.

---

# 26. Definition of success

TUR succeeds as a research project when an independent developer can:

1. Build a reproducible TUR image from documented instructions.
2. Install it on supported x86-64 UEFI hardware without risking an unselected disk.
3. Understand every installer phase and recover logs after a failure.
4. Boot into a modern, responsive KDE Wayland desktop.
5. Use common hardware through one Linux-owned device stack.
6. Mount supported NTFS, exFAT and Linux filesystems safely.
7. Launch Linux applications natively.
8. Launch a documented set of Windows applications through Wine/Proton.
9. Launch a documented set of macOS command-line applications through Darling.
10. See applications from all supported runtimes in one launcher and window-management environment.
11. Update the system transactionally and automatically recover from a failed deployment.
12. Export a diagnostic bundle that gives a development agent enough technical evidence to identify most boot, installer, device, storage, runtime and desktop issues.
13. Use an optional genuine Windows VM for software that requires an actual Windows kernel.

The project should be judged by reproducibility, measurable compatibility, performance, observability, and recoverability. It should not be judged by the number of operating-system logos in the boot screen or by claims of universal application support.

**End of specification.**

---

# 27. Detailed boot observability and failure-handling specification

This appendix expands the boot feedback requirements into an implementation contract. The agent should implement this before adding risky kernel patches, filesystem experiments, or desktop runtime changes.

## 27.1 Boot stages and visible progress

The boot path must expose stable stage names. These stage names should appear in the boot splash/debug console, structured journal records, and diagnostic bundle.

| Stage ID | Stage | Required evidence | Typical failure class |
|---|---|---|---|
| `BOOT-001` | UEFI handoff | firmware mode, selected EFI entry, Secure Boot state if readable | firmware/boot entry |
| `BOOT-010` | Bootloader selection | boot entry ID, deployment ID, attempt counter | bootloader/config |
| `BOOT-020` | Kernel load | kernel build ID, command line, module-signature status | kernel/image |
| `BOOT-030` | Initramfs start | initramfs build ID, initramfs hook list | initramfs |
| `BOOT-040` | Storage discovery | visible block devices, controller drivers | storage driver |
| `BOOT-050` | Root unlock | LUKS device identifier (redacted), unlock method and result, never key material | encryption |
| `BOOT-060` | Root filesystem mount | filesystem, mount options, deployment ID | filesystem/deployment |
| `BOOT-070` | Switch-root | root path and initramfs completion | initramfs/systemd |
| `BOOT-080` | Core system services | failed units, service timing, dependency failures | system service |
| `BOOT-090` | Device enumeration | PCI/USB inventory summary, failed driver binds | hardware |
| `BOOT-100` | Graphics initialization | DRM device, driver, modeset status, Vulkan availability if testable | graphics |
| `BOOT-110` | Display manager | selected display manager and status | login |
| `BOOT-120` | Wayland compositor | KWin version, output count, session status | desktop |
| `BOOT-130` | TUR session services | runtime manager, storage, device manager, desktop bridge | TUR integration |
| `BOOT-140` | User session healthy | user session ID and health result | session |
| `BOOT-150` | Boot marked successful | active deployment, health-check summary, success marker | boot assessment |

The boot splash must not fake a percentage when the system has no accurate estimate. Show the current named phase and a spinner instead. In diagnostic mode, show timestamps and the last completed stage.

## 27.2 Boot modes

Provide the following boot menu entries where available:

- Normal TUR.
- Previous known-good TUR deployment.
- Fallback kernel/initramfs.
- Safe graphics mode (conservative display path; no untested proprietary graphics module).
- Recovery shell.
- Rescue ISO/USB path.
- Windows boot entry if detected.
- Firmware setup entry if supported.

The menu should display the TUR build ID and deployment date. It should distinguish the currently selected deployment from the last known-good deployment.

## 27.3 Safe graphics mode

Safe graphics mode is intended to reach a usable recovery desktop when the normal GPU driver or display stack fails.

Requirements:

- Avoid loading optional third-party GPU modules where possible.
- Use a basic firmware framebuffer or supported simple DRM path.
- Start a minimal Wayland/X11 recovery session only if the selected path is reliable.
- Disable optional compositing effects.
- Provide an obvious diagnostics launcher and log-export action.
- Preserve the original boot journal and graphics logs.

Do not call safe graphics mode a guaranteed recovery route. A complete display hardware failure still requires a text console or external rescue path.

## 27.4 Boot-stage heartbeat protocol

A small `tur-boot-agent` should publish structured stage transitions to journald and, where practical, to a lightweight local status interface.

Example journal fields:

```text
MESSAGE=Boot stage completed: graphics initialization
TUR_BOOT_STAGE=BOOT-100
TUR_BOOT_STAGE_STATE=succeeded
TUR_BOOT_ID=<boot-id>
TUR_DEPLOYMENT=<deployment-checksum>
TUR_DURATION_MS=824
TUR_ERROR_CODE=
```

The boot agent must not block boot-critical services. It should use bounded IPC timeouts and fall back to journal-only operation if the status UI is unavailable.

## 27.5 Recovery decision table

| Failure | Automatic response | User-visible response | Evidence to preserve |
|---|---|---|---|
| New deployment repeatedly fails | Boot previous deployment | “The update did not pass boot checks. Previous version restored.” | deployment ID, boot attempts, journal, failed units |
| Root disk absent | Stop before mounting wrong device | “System disk was not found.” | controller inventory, initramfs log, firmware data |
| LUKS unlock failure | Retry only within safe bounded policy, then recovery | “Encrypted volume was not unlocked.” | device UUID redacted, cryptsetup error; never key material |
| Root filesystem check failure | Do not force read/write mount | Recovery shell / filesystem-specific repair guidance | fsck output, kernel log, mount result |
| GPU driver crash | Offer safe graphics and previous kernel | “Graphics driver failed to initialize.” | DRM logs, module list, PCI IDs |
| KWin fails | Offer text console and fallback session if packaged | “Desktop did not start.” | compositor logs, session journal |
| TUR service fails | Start desktop if noncritical, mark health degraded | “A TUR service failed. Diagnostics are available.” | unit status, service logs |
| Installer power loss | Read persisted installer session and revalidate disk state | “Previous installation did not finish.” | phase journal, disk plan, command results |
| Update download/signature failure | Keep current deployment active | “Update was not applied.” | URL/repository ID, signature and checksum error, no secrets |
| pstore contains panic data | Archive it during next boot | Show crash report available | raw pstore record, boot ID correlation |

## 27.6 Kernel crash and persistent evidence

Configure supported pstore backends where the platform supports them. Integrate systemd-pstore to archive records before they are overwritten. Kdump is an optional advanced-debug feature and must be opt-in or part of a dedicated developer image because it reserves memory and adds operational complexity.

A crash report should correlate:

- Kernel release.
- Build ID.
- Boot ID.
- Deployment checksum.
- Last completed boot stage.
- pstore record.
- Previous boot journal.
- Hardware inventory.

---

# 28. Detailed installer transaction and recovery specification

## 28.1 Installer transaction model

The installer must divide work into transactions with explicit commit boundaries.

Examples:

- Hardware inventory is non-destructive and can be repeated.
- Partition plan generation is non-destructive.
- Partition table write is destructive and requires confirmation.
- LUKS formatting is destructive and requires confirmation.
- Filesystem creation is destructive and requires confirmation.
- OS deployment writes files to the target but must not modify the currently running live environment.
- Boot entry update is a separate transaction with verification.

The installer must persist a transaction record before and after each destructive operation. If power fails, the next run must inspect the actual device state and compare it with the recorded plan before offering resume, repair, or restart.

Never resume a destructive operation blindly based only on a stale state file.

## 28.2 Idempotency requirements

An operation is idempotent only if running it twice produces the same safe outcome without unintended data loss. Mark each installer phase as:

- `idempotent`: safe to repeat after verifying current state.
- `reconcilable`: can inspect and reconcile an incomplete operation.
- `non_repeatable`: requires a fresh confirmation and a new operation record.

Partition table writes, LUKS format, and filesystem creation must be treated as non-repeatable destructive operations unless a robust state inspection proves they have not begun.

## 28.3 Installer state storage

Persist a session directory in the live environment and copy it to the target when the target is mounted. Suggested layout:

```text
/var/log/tur-installer/<session-id>/
  session.json
  phases.jsonl
  hardware.json
  storage-inventory.json
  partition-plan.json
  commands/
    0001-blkid.json
    0002-parted-print.json
    0003-partition-create.json
  logs/
    installer-ui.log
    storage.log
    bootloader.log
    deployment.log
  support-manifest.json
```

Also preserve a copy on the live environment until the user exports the bundle or the live session ends. If the installer fails before the target is mounted, the support bundle must remain available on the live environment.

## 28.4 Installer UX states

The GUI should have a persistent diagnostics drawer visible throughout installation. It must show:

- Current phase.
- Completed phases.
- Warnings.
- Last command/component.
- Detailed log button.
- Export diagnostics button.

A non-expert user sees a clear summary. A developer can expand the technical detail without restarting the installer.

## 28.5 Destructive operation confirmation

The final confirmation screen should include:

- Exact physical drive model and capacity.
- Serial/WWN if available, displayed with a privacy warning.
- Stable `/dev/disk/by-id` path.
- Existing partitions and detected filesystems.
- New partition layout.
- Which existing data will be destroyed.
- Whether another operating system was detected.
- Whether user data will be preserved.
- Encryption choice and key-recovery explanation.

The confirmation must require an explicit button or typed phrase for full-disk erasure. A default focused “Continue” button is not sufficient for destructive disk operations.

---

# 29. Diagnostic bundle format and agent-feedback loop

## 29.1 Bundle layout

A support bundle should be deterministic and versioned:

```text
tur-support-<build-id>-<boot-id>.tar.zst
  manifest.json
  README.txt
  redaction-report.json
  system/
    os-release.txt
    build-manifest.json
    kernel.txt
    kernel-cmdline.txt
    bootloader.txt
    failed-units.txt
    systemd-analyze.txt
  boot/
    current-journal.jsonl
    previous-journal.jsonl
    kernel-ring-buffer.txt
    boot-stages.jsonl
    pstore/
  hardware/
    pci.json
    usb.json
    block-devices.json
    gpu.json
    audio.json
    network.json
    firmware.json
  storage/
    filesystems.json
    mounts.json
    udisks-status.json
    installer-plan.json
  desktop/
    session.json
    compositor.log
    portal-status.json
  runtimes/
    wine.json
    proton.json
    darling.json
    application-results.json
  installer/
    session.json
    phases.jsonl
    command-results/
  services/
    tur-service-status.json
    service-logs/
  checksums.sha256
```

The collector must omit a category when access is unavailable and record why. Do not fail the entire bundle because one command is missing.

## 29.2 Bundle schema

`manifest.json` must contain:

- Schema version.
- TUR build ID.
- TUR deployment checksum.
- Boot ID.
- Timestamp in UTC.
- Architecture.
- Kernel release.
- Installer session ID, if applicable.
- Redaction profile.
- Collector version.
- File list with SHA-256 checksums.
- Missing-data list and reasons.

## 29.3 Error code registry

Use stable, namespaced codes. Do not change the meaning of an existing code after release.

Suggested families:

- `TUR-BOOT-*`
- `TUR-INSTALL-*`
- `TUR-UPDATE-*`
- `TUR-STORAGE-*`
- `TUR-DEVICE-*`
- `TUR-GRAPHICS-*`
- `TUR-AUDIO-*`
- `TUR-NETWORK-*`
- `TUR-WINDOWS-*`
- `TUR-DARWIN-*`
- `TUR-DESKTOP-*`
- `TUR-SECURITY-*`
- `TUR-DIAG-*`

Each code must have a registry entry with title, severity, detection criteria, user message, technical explanation, recovery action, and tests.

## 29.4 Development-agent diagnostic loop

The intended development workflow is:

1. User/QA reproduces a problem.
2. TUR records phase and structured error code.
3. User exports diagnostic bundle locally.
4. Development agent reads manifest first, then the relevant subsystem files.
5. Agent identifies probable cause and supporting evidence.
6. Agent proposes a minimal fix and adds a regression test.
7. CI builds a new image and runs the relevant test.
8. Test image boots in a disposable VM or test machine.
9. Failure-injection test verifies that the same issue is detected and recovered.
10. Agent emits a completion report with test results and residual limitations.

Do not have an automated agent blindly change bootloader, partition, encryption, kernel, or firmware settings on a user's live computer. The agent should propose a patch or a carefully reviewed command sequence, and destructive actions require explicit user approval.

---

# 30. Desktop responsiveness and performance regression specification

## 30.1 UI responsiveness

- Do not perform package installation, runtime initialization, filesystem scans, or hardware probing on the GUI thread.
- All long operations need progress events and cancellation semantics.
- KWin compositor must remain responsive while compatibility apps start or stop.
- Desktop services should use bounded IPC timeouts and backpressure.
- Avoid unbounded event queues for hotplug notifications.
- Use lazy application catalog scanning and cached metadata.
- Indexing must be throttled on battery and under high system load.

## 30.2 Frame pacing

Benchmark frame times, not only average FPS. Test 60 Hz, 120 Hz, variable refresh, fractional scaling, multi-monitor mixed refresh, HDR where supported, and windowed/fullscreen transitions.

Avoid unnecessary GPU copies and format conversions. Validate DMA-BUF sharing between compositor, runtime, and graphics stack where applicable.

## 30.3 Startup and memory budgets

Set initial measurable budgets after a base KDE image exists. Do not invent stable absolute numbers before collecting baseline data.

Track:

- Cold boot time and warm boot time.
- Login-to-usable-desktop time.
- Shell idle RSS.
- Compositor RSS.
- TUR service RSS.
- Runtime idle RSS.
- Application cold and warm launch time.
- Memory pressure and swap activity with mixed app load.

The system must not preload Darling or Wine's full runtime into memory merely to advertise instant application launch. Use caching only after measuring its memory/performance trade-off.

## 30.4 Power and thermal behavior

Use Linux power management and system power profiles. The device manager should expose thermal and power status. Compatibility runtimes must not prevent the host from entering sleep or idle states without a clear inhibitor reason.

Test suspend/resume with USB, Bluetooth, Wi-Fi, audio and GPU devices attached. Device ownership passed to a VM must be restored or reported as requiring reinitialization after suspend/resume.

---

# 31. Additional hardware validation matrix

The initial hardware matrix should include at least:

| Test class | Required cases |
|---|---|
| Firmware | UEFI with Secure Boot on/off; firmware setup entry; boot order changes |
| CPU | AMD x86-64 and Intel x86-64; virtualization extensions on/off |
| Memory | Normal boot; memory pressure; suspend/resume |
| Storage | SATA SSD; NVMe; USB mass storage; GPT; LUKS; NTFS/exFAT/ext4/Btrfs |
| GPU | AMD RADV; Intel ANV; NVIDIA supported driver path; safe graphics mode |
| Display | Single monitor; dual monitor; mixed DPI; mixed refresh; hotplug |
| USB | HID; storage; serial; audio; webcam; WinUSB test device |
| Audio | Internal HDA; USB audio; Bluetooth headset; suspend/resume |
| Network | Ethernet; Wi-Fi; Bluetooth PAN if supported; link loss/reconnect |
| Bluetooth | Mouse; keyboard; audio headset; pairing removal and re-pairing |
| Firmware | fwupd-supported device update in isolated test environment |
| VM | Windows guest without passthrough; with USB passthrough; with GPU passthrough on validated hardware |

Every test result must be tied to a hardware profile and firmware version. A successful VM test does not prove bare-metal compatibility and vice versa.

---

# 32. Final engineering priorities

The development agent must preserve this priority order:

1. Make TUR reliably bootable and recoverable.
2. Make installation safe, observable, and exportable for diagnosis.
3. Make Linux hardware, storage, networking, audio, and display work well.
4. Deliver a responsive and polished KDE Wayland desktop.
5. Integrate Windows compatibility and validate it with real applications.
6. Integrate Darling and publish an honest macOS compatibility matrix.
7. Add the shared desktop bridge and runtime manager.
8. Add full Windows VM mode for the cases compatibility layers cannot support.
9. Expand APFS and advanced device integration only with data-protection tests.

Do not reverse this order. An impressive application launcher is not valuable if an update can leave the system unbootable, and universal filesystem access is not valuable if it risks corrupting a user's Mac or Windows volume.

**The defining property of TUR is not that it claims to run every application. It is that it offers one coherent, high-performance, diagnosable, and recoverable host environment for multiple application ecosystems.**
