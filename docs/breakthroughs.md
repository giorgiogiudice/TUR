<!-- SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0 -->
# Goals and breakthroughs

TUR is meant to do things no desktop operating system does today. This page
names the bets we are making, what each would change for the person at the
computer, and what has actually been shown so far, with the evidence.

The rule for this page is the same as for the rest of the project: **measured,
not promised**. "Proven" means a test ran and passed in continuous
integration, with the run and report linked. Everything here was tested on
build machines and virtual machines; none of it has yet been tested on real
hardware unless it says so.

| Bet | The goal | Where it stands |
|---|---|---|
| [1. Windows drivers without a Windows kernel](#1-windows-drivers-without-a-windows-kernel) | A device that only has a Windows driver works on TUR, safely | **Breakthrough in the lab**: real Windows USB drivers, including Microsoft's own KMDF sample, run as ordinary sandboxed programs |
| [2. A compatibility autopilot](#2-a-compatibility-autopilot) | A Windows application that fails gets fixed by the computer, not by a forum search | **Working**: the right fix found in 12 of 12 fixable test cases, never touching the real copy |
| [3. An assistant that can operate any application](#3-an-assistant-that-can-operate-any-application) | Say what you want; P@x does it in the applications you already use, within limits you can trust | **Working for Qt, GTK and Firefox**; Windows applications not yet |
| [4. Private AI on your own computer](#4-private-ai-on-your-own-computer) | A capable assistant with no account, no bill and nothing leaving the machine | **Working**: models chosen to fit the hardware, downloaded on first use, P@x runs on them |
| [5. A system that heals itself](#5-a-system-that-heals-itself) | A problem seen on one computer becomes a tested fix for every computer, automatically and with consent | **Parts built** (problem reports, signed updates, automatic rollback); the loop is not connected yet |

---

## 1. Windows drivers without a Windows kernel

**The goal.** Some devices (lab equipment, older scanners, hobby electronics,
odd USB gadgets) only come with a Windows driver. On Linux they simply do not
work. TUR wants them to work, without giving up what makes TUR safe.

**The obvious route is the wrong one.** Loading a Windows driver into the Linux
kernel would let one foreign, unreviewable program crash or compromise the
whole machine. TUR's rule stays: no Windows driver ever runs in the kernel
([AGENTS.md](../AGENTS.md)).

**The breakthrough: run the driver as an ordinary program.** Wine already has a
small "driver host" that can run a Windows driver as a normal user program,
talking to the device through Linux's own USB interfaces. It had never been
taken this far: most modern Windows USB drivers are built on Microsoft's
driver framework, KMDF, which Wine did not have at all.

What was shown ([first report](reports/windows-drivers-1.md),
[second report](reports/windows-drivers-2.md),
[proposed decision](adr/0026-windows-device-drivers-in-user-space.md)):

- **A Windows USB driver installs, loads and moves data** on an emulated USB
  device, at 11 to 20 MiB/s, inside a sandbox shaped like TUR's that can see
  only the one device it was given. Pulling the device away while in use does
  not hang the application.
- **USB keyboards, mice, gamepads and other HID devices** work through Wine's
  existing path.
- **The KMDF framework, rebuilt from Microsoft's own open-source code** (MIT
  licence) with open-source compilers. No Microsoft binary is downloaded or
  used. The parts Microsoft did not publish were written for TUR and are
  released under MIT as well.
- **Sixteen changes to Wine** take its support for what the framework needs
  from 118 working functions, with 49 missing or crashing, to 160 working and
  none missing or crashing.
- **Microsoft's unmodified `kmdf_fx2` sample driver runs** against an emulated
  OSR USB-FX2 learning board. Its own test program reads and writes data,
  lights the bar graph and 7-segment display and receives the switch
  interrupts. All 62 checks pass.

As far as we know, KMDF drivers had not run in Wine before.

**Not shown yet, and known limits.**

- Every test used emulated devices. Real hardware is next.
- A timed-out transfer cannot be cancelled the normal way, and a device that
  Linux has already claimed with its own driver makes a transfer hang. Both are
  Wine issues with a known fix path.
- Not in the TUR image yet. Drivers for graphics, storage, networking and
  anything with kernel-level anti-cheat are out of scope: those stay with
  Linux's own drivers or a Windows virtual machine.
- **These changes cannot go into Wine as they are.** The Wine project does
  not accept code written by an AI model, because the copyright status of
  such code is unsettled (its maintainer's statement on the developers' list,
  9 April 2026). TUR's changes were written that way. So TUR carries them as
  its own small patch queue, and what it can offer Wine is the findings: what
  is missing, how real Windows behaves, and the proof that the approach
  works. The draft proposal in the [second report](reports/windows-drivers-2.md),
  section 10, was written before this was known and will be rewritten as a
  report of findings.

## 2. A compatibility autopilot

**The goal.** When a Windows application fails under Wine today, the person is
left searching forums for registry tweaks. TUR's computer should do that work
itself: work out why it failed, try the known remedies, and keep what works.

**What it does.** `tur-runtime app autopilot` runs the failing application in a
**throwaway copy** of its setup, reads which kind of failure it is (twelve
kinds, each with a stable code), tries the remedies a sourced catalogue knows
for that kind, one by one and then in pairs, and stops at the first that makes
the application's check pass. The person's real copy is only read, and its
fingerprint before and after must match. Applying the fix is the person's
choice, with a backup and one-step undo
([design](architecture/compat-autopilot.md),
[report](reports/autopilot-1.md)).

What was shown: in **13 test cases** (a program that needs Windows 11, one that
ships a broken DLL, one that breaks on a case-sensitive path, a Japanese
program that crashes in the wrong code page, one that needs a small screen or
a high-DPI screen, and 7-Zip, Notepad++, PuTTY, Inno Setup and VLC each with a
part removed), it found the right remedy **every time it was fixable, usually
on the first try, in 7 to 25 seconds**. The thirteenth case needed a Microsoft
component that TUR may not supply; the autopilot said so and suggested the
Windows guest instead of guessing.

**Next.** Real, demanding applications; sharing results so one person's fix
helps everyone; running it on the TUR image with a graphics card.

## 3. An assistant that can operate any application

**The goal.** P@x should be able to do what you can do: open an application,
find the right control, type, press, and check the result, across Linux,
Windows and Android applications, while the dangerous actions stay under your
control.

**What it does.** P@x reads and operates other applications' windows through
the accessibility interface that screen readers use, through a separate
program that is the only part allowed to touch it. Every action is checked by
TUR's policy engine twice, once when planned and again just before it
happens. Reading is always allowed; ordinary actions are allowed; anything
that sends, pays, buys, deletes, installs, confirms or signs is refused unless
you approve it ([design](architecture/pax-accessibility.md),
[report](reports/pax-7.md)).

What was shown:

- It reads and acts in **Qt, GTK 3, GTK 4 and Firefox** applications.
- It **refused** "press Send", "click Delete draft" and typing a line ending
  that would submit a form, and nothing happened in the application.
- Switching P@x's on-screen indicator off **stopped a task** with the pointer
  already resting on a button, before the press.
- P@x has carried out tasks with **the real Claude** and with **a real local
  model** (Qwen3 4B), besides hundreds of scripted checks on every change.

**Not yet.** Windows applications show nothing to the accessibility interface
under Wine, so P@x cannot see inside them yet; a bridge for that is planned.
Typing into Firefox is reported as done but does not arrive. Approving a
dangerous action from the desktop and voice control are not built yet.

## 4. Private AI on your own computer

**The goal.** Everyone should be able to have a capable assistant without an
account, a bill, or sending what they ask to someone else's computer.

**What it does.** `tur-ai-local` reads the computer's processor, memory and
graphics card and sorts a curated list of open models (Gemma 4, Qwen3,
Ministral 3) into "runs well", "slow" and "does not fit", with plain reasons.
The model downloads, checked against its publisher's exact version, the first
time you choose it. P@x can use it exactly like a cloud provider, and every
model's settings (temperature, effort and the rest) can be adjusted
([design](architecture/local-ai.md), [first report](reports/local-ai-1.md),
[models report](reports/pax-6.md)).

P@x also works with your own key from Anthropic, OpenAI, Google, DeepSeek,
Mistral, xAI, Groq, OpenRouter, Together and any other service that speaks the
same language, added as data rather than code
([report](reports/pax-4.md)).

**Next.** A faster engine built for TUR (with graphics-card support), and local
speech so the assistant can work fully offline.

## 5. A system that heals itself

**The goal.** When something breaks on one computer, P@x should explain it and
repair what it safely can. When the cause is in TUR itself, the person can
choose to send a report with personal details removed. The fix then reaches
every computer as a signed, tested update, which undoes itself if it makes
things worse.

**The parts that exist.**

- **Problem reports** a person chooses to send, with personal details removed
  on the computer and again on receipt, kept for 90 days
  ([ADR-0024](adr/0024-problem-reports.md),
  [design](architecture/problem-reports.md)).
- **Signed updates**: the tested system is signed and published, and a
  computer refuses an update whose signature is missing or wrong, with
  nothing installed. Proven on a booted test machine; publishing is switched
  off until the project's release key is in place
  ([ADR-0023](adr/0023-publishing-signed-updates.md),
  [design](architecture/publishing.md), [report](reports/release-1.md)).
- **Automatic rollback**: a new version that fails to start is replaced by the
  previous one by itself, proven in 26 failure scenarios on every change
  ([report](reports/TUR-005.md)).

**Next.** Connecting them: P@x notices a fault, diagnoses it, repairs what it
may, checks the repair held, and, with consent, sends the report that turns
into the next update.

---

## What comes next

The ideas TUR is testing now, each written as a hypothesis that may fail
(running applications' portable payloads without any compatibility layer, a
Windows command line judged against real Windows, a compatibility layer that
repairs itself, P@x operating Windows applications, Android by translation, and
drivers learned from a recording) are on [the frontier page](frontier.md).

## The bigger goals

These bets serve the goals TUR was started for
([specification](architecture/specification.md)):

- **Every application, one desktop.** Linux, Windows, Android and, as an
  experiment, macOS applications as ordinary windows, from one application
  centre.
- **A computer that explains and repairs itself**, and an update that undoes
  itself when it goes wrong.
- **An assistant you can talk to** that does the work for you, with limits
  built into the system, not left to the AI's judgement.
- **Private by default.** Nothing is collected behind your back.
- **Free for people**, for schools and for charities.

If you can help with any of these (Wine, drivers, accessibility, local AI,
testing on real hardware), see [how to help](../README.md#tur-needs-you).
