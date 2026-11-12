# ADR-0006: Android runtime — Waydroid as an opt-in OS container

- **Status:** Accepted
- **Date:** 2026-11-12
- **Specification sections:** §3.1 to §3.4, §7, §12.1, §15.2, §17.2; none of them
  covers Android in revision 1.0. See the
  [Android runtime design](../architecture/android-runtime.md) (addendum B)

## Context

A requirement added after revision 1.0 of the specification: UOS should run
Android applications, and they should "fully work" as far as that is possible.
The specification covers Linux, Windows and macOS applications and a full
Windows guest. It says nothing about Android.

Constraints already decided:

- The base is Fedora 45 Kinoite on bootc with the stock Fedora kernel and SELinux
  enforcing. An out-of-tree kernel module needs its own ADR
  ([ADR-0001](0001-base-distribution.md)).
- Nothing proprietary may be bundled, and UOS's own code must stay a separate
  work from copyleft components ([ADR-0003](0003-licence.md)).
- No compatibility runtime runs as root (§12.1).
- Compatibility is measured, never claimed (§0, rule 7).

What is known, checked on 12 November 2026 and referenced in addendum B:

- Waydroid runs a complete Android system in an LXC container on the host
  kernel. It is maintained, GPL-3.0, and packaged in Fedora 45 with an SELinux
  policy module.
- It needs the binder driver. **Measured** on the UOS image (CI run
  43147702795, kernel 7.3.3-300.fc45): `CONFIG_ANDROID_BINDER_IPC_RUST=y`. The
  stock kernel has binder, through the Rust driver.
- Its official images are Android 13, which Google's November 2026 security
  bulletin no longer covers.
- Its daemon runs as root, and the container's root is not shown to be mapped to
  an unprivileged host user.
- Google's Play Integrity device verdict needs a certified device with
  hardware-backed attestation. No container or virtual machine outside Google's
  own products obtains it.
- No open ARM-to-x86-64 translator for Android exists. The two in use are
  proprietary.

What is not known: whether Waydroid and its image run correctly on this kernel's
Rust binder driver under SELinux enforcing; how well a virtual machine can
present Android applications as separate desktop windows.

## Options considered

1. **Waydroid as an OS container on the host kernel.** Runs the widest range of
   applications; integrates with the desktop today; packaged for the base. Needs
   a root daemon and a privileged container, against §12.1. Depends on one small
   upstream and on its Android 13 image.
2. **Android in a virtual machine** (Waydroid in a small Linux guest, or an AOSP
   virtual device, under KVM). No root container on the host and no binder
   dependency on the host. Costs reserved memory and a guest kernel that UOS must
   maintain. Per-application windows from a guest are documented in QEMU but
   unproven for this use. No packaged Android guest exists for Fedora.
3. **Android Translation Layer.** Reimplements the Android API with no container
   and no privilege, the same model as Wine. Fits UOS best in principle. By its
   own description it runs few applications today.
4. **An out-of-tree binder module**, should the stock kernel lose binder. Breaks
   Fedora's signed boot chain for every user and puts community-maintained code
   in the host kernel.
5. **No Android support.** No cost, and the requirement is not met.

None of options 1 to 4 makes banking, DRM-video or integrity-checked
applications work, and none removes the need for a proprietary translator for
ARM-only applications.

## Decision

Proposed: UOS runs Android applications with **Waydroid, from the Fedora
package, as an opt-in OS container**, driven by `uos-runtime` and integrated
through the existing UOS services, as specified in addendum B.

Proposed with it:

- **A recorded exception to §12.1.** The Waydroid container service runs as
  root. The exception holds only under the conditions of addendum B, section
  10.3: off by default, the container policy of section 10.2 in place before
  users are offered the feature, review in phase 7, and evaluation of a virtual
  machine backend as the way to end the exception.
- **Nothing proprietary is shipped or fetched by default.** No Google Mobile
  Services, no Play Store, no ARM translation layer, no Widevine. The Android
  image itself is not part of the OS image; it is fetched on opt-in and pinned
  by digest.
- **No out-of-tree host kernel module and no custom host kernel for Android.**
  If binder becomes unusable on the stock kernel, the fallback is a virtual
  machine with its own kernel.
- **Stated limits.** UOS publishes what does not work and why (addendum B,
  section 11) and never describes Android support as complete.

## Consequences

- The specification gains a fourth ecosystem and a third kind of runtime, the OS
  container, between a compatibility runtime and a full guest. The amendments
  are listed in addendum B, section 15. `AGENTS.md` needs Android in its
  priority order and hard rules.
- The owner's "fully work" is met only for applications that do not depend on
  Google certification, phone hardware or ARM-only code. That must be said to
  users before they enable the feature.
- UOS takes on a security position weaker than for Wine or Darling: a root
  daemon, and Android applications that are less contained than on a phone.
  This is acceptable only because the feature is off by default.
- All Android applications of a user share one Android system. The per-application
  environment model of §7.2 and §12.2 does not apply to Android.
- UOS depends on the Rust binder driver in the Fedora kernel, which has had
  defects affecting Waydroid, and on upstream Waydroid's image, which is on an
  Android version without Google security fixes until upstream ships a newer
  one.
- `uos-update` must manage a second kind of payload: Android images outside the
  OS deployment, with their own pin, verification and rollback.
- UOS pins upstream's published image binaries by digest and does not build
  Android from source. This is a gap against §3.7, accepted because hosted CI
  cannot build Android ([ADR-0002](0002-ci-and-build-infrastructure.md)).
- Patches to Waydroid, if any, are GPL-3.0 and live in a marked directory. UOS
  code talks to Waydroid over its command line and D-Bus only.
- The AI control layer gains a second semantic tier, the Android accessibility
  tree, if it proves workable. Addendum A, section 4, is amended accordingly.

## Verification

Before or while implementing, and recorded in the task reports:

- **Binder in practice** (milestone B1): on the booted image, `binder` is listed
  in `/proc/filesystems`, binderfs mounts, the three device nodes exist, and
  Waydroid boots the pinned image with SELinux enforcing and no denial. This also
  shows whether the open large-transaction defect of the Rust driver affects the
  kernel in use.
- **Container privileges:** whether the container has a user namespace, which
  capabilities it holds, and which SELinux domain its processes run in, read from
  a running system rather than from source.
- **Policy:** that the device-node allowlist, withheld audio capture and firewall
  zone of addendum B, section 10.2, can be applied without forking Waydroid, or
  which upstream change is needed.
- **Image management:** that Waydroid's updater is disabled when the image path
  is managed by UOS, and that an image update and rollback preserve user data.
- **Licence review** of the Android images and of every optional component,
  before public distribution (§17.2).
- **Virtual machine backend** (milestone B7): a prototype with Wayland
  passthrough, leading to a further ADR on whether it ships and whether it
  replaces the container.
- The decision is shown to hold when the acceptance criteria of addendum B,
  section 13.1, pass in CI, and when a first compatibility matrix is published
  from real hardware.

## Owner's decision (12 November 2026)

Accepted, including the conditional exception to the "no runtime as root" rule
for Waydroid's daemon and container. The micro-VM route stays the planned way to
remove that exception.

The owner also asked for Google Play to be integrated.
[ADR-0011](0011-application-centre-and-stores.md) records how: Google Play is a
guided, one-step opt-in in the application centre and through P@x. The image
containing Google's services is never shipped; it is fetched from its publisher
when the user opts in and accepts Google's terms, and the user registers the
device with Google. The limits set out in this record and in addendum B still
apply after opting in: applications that require Google's certified-device check
will refuse to run.

