<!-- SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0 -->
# The frontier: hypotheses under test

[Goals and breakthroughs](breakthroughs.md) records what TUR has shown. This
page records what TUR is **trying next**: ideas that may fail. Each one is
written as a hypothesis with an experiment and a kill criterion, decided before
anything is measured. Results, including negative ones, go into a report under
[`reports/`](reports/) and this page is updated.

Nothing on this page is a promise. A hypothesis is listed as *open* until its
experiment has run.

## The stance: one kernel, every ecosystem, the assistant in the middle

**Why not a new shared kernel.** A kernel that natively speaks Windows,
Android and Linux at once sounds like the direct route. It was tried (the
Linux Unified Kernel project) and abandoned, and for a reason that still
holds: what an application needs from "its" kernel is mostly reachable from
ordinary programs, and the few parts that are not have been going into Linux
itself, one at a time, reviewed and maintained by the kernel's own community:

| What a foreign application needs from the kernel | Where it lives in Linux today |
|---|---|
| Windows' synchronisation objects, at full speed | the `ntsync` driver |
| Windows programs that make system calls directly | syscall user dispatch |
| Windows' case-insensitive file names | case folding in ext4 and other filesystems |
| Windows' "wait for any of these" | `futex_waitv` |
| Android's inter-process calls | the binder driver and binderfs |
| Windows device drivers | none, deliberately: TUR runs them as ordinary programs ([breakthrough 1](breakthroughs.md#1-windows-drivers-without-a-windows-kernel)) |

So the shared kernel already exists, in the only form that can be kept safe
and current: it is Linux, with the necessary organs from the other systems
transplanted. TUR's job is to use every one of them, measure that it does, and
build what no kernel can provide: the layer where the ecosystems meet.

**The assistant in the middle.** The second idea behind this page is that the
AI is not only a helper for the person. It is also the adapter between
ecosystems and the engine that closes compatibility gaps: it operates
applications of every kind through one policy, it writes the missing pieces of
the compatibility layers and proves them against the real systems, and it
turns one computer's failure into everyone's fix.

## Hypotheses

| # | Hypothesis | Experiment | Kill criterion | Status |
|---|---|---|---|---|
| F1 | **Re-hosting.** Many "Windows-only" and "Mac-only" applications are a portable payload (Electron, .NET, Java, Godot, Ren'Py, LÖVE) in a thin shell. TUR can take the payload out of the person's own installer and run it on the native Linux build of the same runtime: no Wine, no virtual machine, native speed, visible to P@x. This would be TUR's first real route for Mac applications. | Take only the Windows and macOS packages of open-source applications that also publish a Linux build; re-host; compare with the official Linux build. | Fewer than 30% of the Electron sample reach a working window | Open (`exp-rehost`) |
| F2 | **A Windows terminal.** A person can follow a Windows command-line tutorial on TUR: cmd, the classic tools and PowerShell behave as on Windows, or TUR says plainly what the equivalent is. | At least 150 everyday commands, judged against a real Windows machine. | None: the result is a scorecard and the best design | Open (`exp-wincli`) |
| F3 | **A compatibility layer that repairs itself.** An automated loop generates small test programs, runs them on real Windows and on Wine, finds the differences, writes the fix and verifies it on both. The Wine project does not accept code written by an AI model, so the fixes live in TUR's own patch queue and Wine receives the findings as bug reports. | At least 300 checks in areas real applications hit; at least five verified fixes. | Nearly everything found is already known to Wine or irrelevant to real applications | Open (`exp-windiff`) |
| F4 | **P@x sees Windows applications.** A small program of TUR's inside the Windows environment reads an application's controls (UI Automation, then MSAA, then the plain window structure) and gives P@x the same tools, under the same policy, as for Linux applications. Where there is no structure at all, a small local vision model finds the control in a picture of the window. | Real Windows applications from TUR's tests; share of controls found and operated. Vision scored on pictures whose true answers are known. | Under half the controls reachable; vision under 60% on plain dialogs | Open (`exp-winsee`) |
| F5 | **Android by translation first.** As with Windows, an Android application can run as an ordinary program with a native window, with no Android system underneath, and a full Android container is the fallback. Also: how much of Android needs no ARM translation at all? | 30 open-source applications under the Android Translation Layer; a census of native code across F-Droid; an Android container started in continuous integration. | Under 10% of the sample reach a usable first screen | Open (`exp-android`) |
| F6 | **Driver synthesis.** Given only a recording of the conversation between a device and its Windows driver, an AI can write a working native Linux driver. TUR would learn a device once, from its Windows driver, and every TUR computer would then support it natively. | An invented device with a protocol that exists nowhere else; an engineer who sees only the recording writes the Linux driver; a hidden test suite scores it with values never recorded. | The blind driver fails the handshake or more than half the features | Open (`exp-drvsynth`) |

## Next in line

Ideas waiting for a slot, each to be written up the same way before it starts:

- **Layered Windows.** One shared, read-only Windows base and a thin layer per
  application, so installing a Windows application takes seconds and costs only
  its own bytes, while applications that belong together (an office suite and
  its add-ins) can see each other.
- **Your existing Windows, inside TUR.** On a computer that already has
  Windows, start that same installation in a window from TUR, with its
  applications and licences, instead of restarting into it: the fallback that
  makes "everything runs" true, with the data-protection rules that requires.
- **One bus for every ecosystem.** Links, files, sharing, notifications and
  scriptable functions of Linux, Windows and Android applications in one
  registry, so any application can hand work to any other, and P@x can call an
  application's own functions instead of pressing its buttons.
- **Speed, measured against Windows.** The same programs timed on a real
  Windows machine and on TUR, published, with the kernel features above
  switched on and off.

## Rules for experiments

- Hypothesis and kill criterion first; then the measurement.
- Experiments live in `tests/experiments/<name>/` with their own workflow, and
  never in the image until a decision record accepts them.
- TUR's standing rules are not suspended: no foreign driver in the kernel, no
  Microsoft, Apple or Google binaries shipped, nothing that helps bypass a
  licence, nothing collected silently, and the assistant acts only through the
  policy engine.
- Real Windows is observed only from outside, through its documented
  behaviour, never through its source code.
- Upstream projects' rules are checked before anything is offered to them.
  Wine does not accept AI-written code; TUR offers it findings, not patches.
- Negative results are published.
