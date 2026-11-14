<!-- SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0 -->
# exp-inwin-1 — TUR as the solid floor under the person's own Windows, seen from the Windows side (experiment)

> **Status: hypotheses registered; nothing measured yet.** This first
> commit holds the hypotheses, the kill criteria and the experiment's code.
> Every result section below says NOT RUN until a run of the workflow
> `exp-inwin` fills it.

## 0. Scope, and how it changed before anything ran

The spike began on 14 November 2026 from the owner's question whether
"installing TUR may be a sort of super-charge your current Windows": TUR
installed *inside* Windows (shape A: TUR's image as a WSL2 distribution),
against TUR as the host with the person's Windows inside it (shape B, the
`dualrun` experiment), and a path between the two.

Before any experiment of this spike had run, the owner decided (14
November 2026): "I like B better and it still supercharges Windows: people
keep it but it's protected, layered and also controlled by P@x, fully
compatible with everything, and it has Linux, Android and some Mac
integrations. I think that's the winning part, because Linux is more
solid." And, the same day: after installing TUR, "C: should stay C:",
volume labels unchanged, and "possibly even keep the background and the
desktop dispositions, the favourite shortcuts and so on... make it look
after install that TUR just gave Windows some superpowers."

So this spike measures what shape B needs **from the Windows side**, on
real Windows, which only GitHub's Windows runners give this project:

1. P@x's guest agent inside the person's Windows: what it can see and
   operate, and what a seamless desktop needs from it.
2. The way in from Windows: a "get TUR" checker that only reads.
3. What TUR can read from a Windows installation to carry the person's
   desktop over.

Shape A (WSL2) is not tested. What was learned about it from documents
before the decision is kept in section 9, so the reasoning is not lost.

## 1. Hypotheses and kill criteria, as stated before measuring

Registered in this commit, before the first run of the workflow.

| | Hypothesis | Experiment | Kill criterion |
|---|---|---|---|
| H1 | **The guest agent sees.** On real Windows, UI Automation together with the plain window tree reaches at least what the plain window tree reached under Wine (163 of 172 controls, 95 %, exp-winsee-1) in the same applications, and also reaches what Wine could not show at all: modern (XAML) applications and the content of a web page | The winsee bridge, unmodified, built for real Windows, and a native UI Automation reader, against PuTTY 0.85, Notepad++ 8.9.9.1, 7-Zip 26.06 and the Inno Setup 7.1.1 wizard scored with winsee's own lists; Notepad, Explorer, Settings, WordPad's ribbon where Windows has it; a page of TUR's own with seven known controls in Edge, Chrome and Firefox | Under 80 % of winsee's listed controls found by UI Automation or the window tree in those four applications; or under half of the page's seven controls found in every browser: the agent cannot be the main way P@x sees the guest, and vision would have to be |
| H2 | **Its actions arrive.** Pressing, typing, selecting, toggling, opening a menu and opening an item arrive in every kind of application, verified by a route that does not ask the tier that acted | The same windows; each action checked by the application's own log, a control message (`WM_GETTEXT`, `BM_GETCHECK`, `IsWindowEnabled`), the window's title, or the picture of the screen | A kind of application (classic Win32, modern, web page, installer, ribbon) in which no action arrives |
| H3 | **The window list is cheap and current.** Every top-level window's rectangle, title, icon and place in the z-order can be listed in under 50 ms, and a move, a new title, minimising and closing are known through Windows' own events within 50 ms at the median and 200 ms at the 95th percentile, so a seamless desktop need not poll | `agent/desk.py`: 40 full listings timed; 25 rounds of four changes to a window of another process, the time from the call to the event's arrival | A full listing over 250 ms, or any of the four kinds of change without an event: the window list would have to come from somewhere else (the remote-display protocol's own window channel) |
| H4 | **The checker need not be an administrator to tell the truth.** Firmware type, free space, memory, BitLocker's state, Fast Startup, virtualisation and the disk's layout can each be read by a program without administrator rights by at least one route | `checker/check.py` run elevated, with a restricted token, and as a new standard account; each fact by every route known | BitLocker's state or the disk layout readable only with administrator rights: the checker must ask for them before it can say anything (a consent prompt before the first sentence) |
| H5 | **The person's desktop can be read from the disk.** Wallpaper and fit, colours, desktop shortcuts with targets, icon positions, taskbar pins in order, known folders, drive letters, default applications, regional settings, keyboard layouts and time zone live in files and registry files whose formats TUR can read without running Windows | `carry/carry.py`: a test wallpaper and test shortcuts of TUR's own put on a runner's desktop and read back; a reader of `.lnk` files written from the published format, compared with what Windows says of every Start-menu shortcut; the user's registry saved to a file, loaded as a stranger's and compared | The `.lnk` reader agrees with Windows on under 90 % of shortcut targets; or a listed item is found only in a form nobody outside Microsoft can read: that item is carried over by the guest agent at the first start of Windows under TUR, or not at all, and the report says which |

What a hosted runner cannot answer, stated now so it is not mistaken for a
result later: the runners are Windows Server 2025, Windows Server 2022 and
Windows 11 on ARM. **Windows 10 itself, Windows 11 on x64, Home editions,
a PC with BitLocker switched on, a laptop, a second monitor and a
Microsoft account are NOT RUN** unless a section says otherwise. Windows
Server 2022 is the Windows 10 generation of the shell, and is used as the
nearest available stand-in, named as such wherever it appears.

## 2. Method

- `tests/experiments/inwin/agent/`: `seer.py` (reads and operates
  applications), `uia.py` (UI Automation through comtypes 1.4.17, MIT,
  pinned by hash), `win.py` (plain Win32 through ctypes; pictures written
  as PNG without any library), `desk.py` (window list and events),
  `lower.py` (the agent without administrator rights), `truth-winsee.json`
  (the lists of controls counted by eye for exp-winsee-1, copied from pull
  request 126 at commit `622bfa6`), `form.html` (TUR's own page).
- `tests/experiments/inwin/checker/check.py`: the read-only checker.
- `tests/experiments/inwin/carry/`: `carry.py` (the reader), `lnk.py` (a
  reader of shortcut files from the published format [MS-SHLLINK]).
- `tests/experiments/inwin/rights.py`: runs a reader with three sets of
  rights and tabulates what each could read.
- `.github/workflows/exp-inwin.yml`: three jobs, all on Windows runners.
- Pictures and full reads of Microsoft's applications are kept for one day
  only, to count by eye; kept artifacts hold counts, rectangles, paths and
  hashes. Applications are fetched from their publishers by hash and thrown
  away. No Microsoft file is committed or uploaded.

## 3. P@x's guest agent on real Windows

NOT RUN.

## 4. The window list a seamless desktop needs

NOT RUN.

## 5. The interface between TUR and the guest agent (proposal; not built)

For the `dualrun` engineer, who owns the design of the session. This is
what the measurements of sections 3 and 4 suggest, and what the host must
hold to whatever the measurements say.

**Where things sit.** P@x's service, its policy engine, the model and its
keys stay on TUR, outside the virtual machine. In shape B the policy
engine is again *under* the applications it controls, which is the
property TUR inside Windows would have given up (section 9). Inside
Windows there is only an agent that holds no authority: it answers
requests and performs what TUR already decided, as the winsee bridge does
under Wine.

**Two parts inside Windows.**

- A *service part* that owns the channel to TUR, starts with Windows and
  starts the session part in each signed-in session.
- A *session part* that runs on the person's desktop with the person's
  rights. It reads windows, performs actions and reports events. Section
  3.4 measures what an agent without administrator rights can do to a
  window of a program that has them; that result decides whether the
  session part needs Windows' "UI access" right (a signed program in a
  protected folder), which is a signing cost for TUR and not a technical
  unknown.

**Transport.** A virtio serial port named for TUR
(`org.tur.guest.0`), not a network socket: it exists before the guest has
a network, no other machine can reach it, and inside Windows it can be
restricted to the service part. It needs the virtio serial driver in the
guest, which is an added driver (dualrun's H4 measures how far Windows
goes with none). Where no driver may be added, the same messages go over
a host-only network address with a key made at installation; the
messages do not change.

**Messages.** One JSON object a line, as the winsee bridge speaks, each
with a number; the agent's events carry a sequence number so that a lost
one is noticed.

| Direction | Message | Carries |
|---|---|---|
| TUR to agent | `hello` | protocol version; the agent answers with its version, Windows' build, the session's rights |
| | `windows.list` | every top-level window: an identifier, rectangle and visible frame, title, class, program, z-order, whether it is on the taskbar, minimised, hidden by the compositor |
| | `windows.watch` | start sending the events below |
| | `window.read` | one window's controls in tur-pax-a11y's node model, by tier (UI Automation, then the window tree), with a node budget |
| | `element.act` | an action on a node, **with what TUR believes the node is** (name, kind, rectangle); the agent re-reads the node and refuses if it is no longer that |
| | `point.inspect` | what is at a screen point (winsee's rule for anything proposed by vision) |
| | `window.content` | the pixels of one window, also when it is covered (section 4.3) |
| | `icon.get` | a window's or an application's icon as raw pixels with a size |
| | `apps.list`, `open` | the Start menu's entries; start one; open a file with its default application |
| Agent to TUR | `window.created`, `.destroyed`, `.moved`, `.titled`, `.minimised`, `.restored`, `.foreground`, `.reordered` | the window's identifier and the new fact |
| | `session.locked`, `.unlocked`, `.secure-desktop` | the agent cannot see or act on the lock screen or a consent prompt; TUR must show the guest's whole screen for that moment and P@x must stop |

**What TUR must never trust from the guest.** Windows is the less trusted
side: anything that runs as SYSTEM inside it can speak on the channel, and
a compromised Windows can lie in every message.

1. *Everything read is data, never instruction.* Names, values and text
   of controls go to the model marked as content from an application, as
   P@x already does for Linux applications, and never change what P@x is
   allowed to do.
2. *A decision about TUR is never made from a guest's claim.* The policy
   engine may classify an action inside Windows from what the agent read
   (pressing "Delete" is class 2), but anything that crosses out of the
   guest (a file leaving a shared folder, the clipboard, a link opened on
   TUR, a device handed over, a credential) is decided from facts TUR
   holds itself.
3. *Geometry decides only where guest pixels may go, never where TUR's
   go.* A lying guest can report any rectangle. Guest pixels are drawn
   only inside windows TUR's compositor marks as belonging to Windows, in
   a mark the guest cannot draw, and never above TUR's own confirmation
   surfaces: P@x's confirmation, the authorisation dialog, the lock
   screen.
4. *Titles and icons are untrusted pictures and strings.* Length-limited,
   stripped of control characters, icons rebuilt on TUR from raw pixels
   with a size limit; nothing from the guest is parsed by a complex
   decoder on TUR.
5. *The agent is believed about the guest only as far as the guest
   matters.* An action it reports done may not have happened. Where the
   result matters outside Windows, TUR checks it outside Windows (the
   file on the volume, the layer's contents).
6. *Volume.* Every message has a size limit and events a rate limit;
   section 4.2's numbers say how many events one drag produces.
7. *Nothing secret goes in.* No model key, no P@x memory, no policy
   store. A request names an action; it never carries why.

**What the guest may ask of TUR.** Nothing, in the first version. A
request from inside Windows to open something on TUR is a later feature
with its own decision record, because it is the direction in which a
compromised guest reaches out.

## 6. The way in from Windows

### 6.1 What a checker can read, and with which rights

NOT RUN.

### 6.2 "Get TUR", as a design (not built)

A person starts on Windows. The way in is one small Windows program of
TUR's own, and it has four steps. Only the first exists as a prototype
(`checker/check.py`), and it only reads.

1. **Look** (no administrator rights, no consent prompt, nothing
   changed). The facts of section 6.1, turned into plain sentences: what
   stands in the way, what TUR will do about it, what the person must do
   first. The refusals are the installer's own, met early: BitLocker
   (`TUR-INSTALL-SHRINK-001`), hibernation or Fast Startup (`-003`), a
   volume that needs checking (`-004`), too little room (`-005`: 25 GiB
   for TUR, 16 GiB left free for Windows; pull request 116).
2. **Explain.** One page: Windows stays, C: stays C:, nothing is deleted;
   how much room TUR takes; that Windows will still start by itself; how
   to undo it. If the drive is encrypted: save the recovery key first,
   with the place Microsoft keeps it named.
3. **Prepare** (one consent prompt, then only what Windows' own tools
   do): switch Fast Startup off; let Windows shrink its own volume, which
   is safer than shrinking it from outside because Windows moves its own
   files while it runs; suspend BitLocker for the change with Windows'
   own command; write the handover file; put the installer on a USB
   stick, or stage it on the disk and ask the firmware to start it once.
   Nothing here is built and nothing in this experiment changes a disk.
4. **Hand over.** The PC restarts into TUR's installer (pull request
   112), which finds the free space Windows made and the handover file.

**The handover file** (`tur-handover.json`, beside the staged installer
or on the stick) is a hint, never an authority. The installer checks
every fact again itself and still asks for its own typed confirmation
with the device's identity (AGENTS.md's hard rule).

| Field | From | The installer uses it to |
|---|---|---|
| `windows`: build, edition, architecture | section 6.1 | choose wording; refuse what it does not support |
| `disk`: the disk's GUID, the Windows partition's GUID and size, the free stretch Windows made | `Get-Partition` | find the same disk and the same stretch; a mismatch means "look again", never "trust the file" |
| `prepared`: Fast Startup off, hibernation off, BitLocker suspended until, volume clean | step 3 | say what was done; it still reads the volume's own flags |
| `person`: display name, language, keyboard layouts, time zone, clock in local time or UTC | section 7 | pre-fill the first-run questions |
| `carry`: the manifest of section 7 (wallpaper file and fit, colours, shortcuts, pins, drive letters and labels, known folders) | section 7's reader | carry the desktop over; read again from the disk where the disk has it |
| `signature` | none | not signed: it is written by a program on the less trusted side. It carries nothing that must be secret and nothing that is acted on without a second look |

**What pull request 112's installer would need, to pick this up:** read
the file if present; prefer free space Windows made over shrinking from
outside; show the checker's sentences next to its own findings and stop
if they disagree; record the Windows partition's identity for the
launcher of `dualrun`; hand `person` and `carry` to the first-run
set-up. None of that changes its safety rules.

## 7. What TUR can carry over from a Windows desktop

### 7.1 What was read on the runners, and with which rights

NOT RUN.

### 7.2 Where each thing lives on the disk (for `dualrun` and the installer)

TUR's installer sees the disk before Windows ever runs under TUR. Nearly
everything is in three registry files and a few folders:

- `Users\<name>\NTUSER.DAT`: the person's own settings (what a running
  Windows shows as HKCU);
- `Windows\System32\config\SOFTWARE` and `...\SYSTEM`: the machine's
  (HKLM\SOFTWARE and HKLM\SYSTEM). On the disk there is no
  `CurrentControlSet`: `SYSTEM\Select\Current` names the `ControlSet00N`
  to read;
- `Windows\System32\config\SAM`: the accounts. A running Windows shows it
  to nobody; on the disk it is an ordinary registry file.

All four are in the registry's file format ("regf"), which the `hivex`
library reads on Linux. **Reading them with hivex is NOT RUN here**; what
section 7.1 shows is that a saved copy of the file, loaded as a
stranger's, holds the same values the running Windows reports.

The column "Seen" says whether the value was present and read on a
runner (section 7.1), or is described from documentation only.

NOT WRITTEN YET: the table is filled from the first run.

### 7.3 Shortcuts, icon positions, pins: what the measurement found

NOT RUN.

## 8. Product judgement

An engineer's opinion, for the owner to accept or reject. Only the
sentences that cite a section rest on a measurement.

**The sentence.** "TUR gives your Windows superpowers" is a better front
door than "replace Windows", and shape B is the only shape in which it is
true: the person keeps their Windows, their programs, their files and
their licence, and TUR puts under it the things Windows cannot give
itself: undo, layers, repair from outside, an assistant that is not
inside the thing it controls, and three more kinds of application. "Add,
don't switch" removes the one objection every replacement meets (what
about my programs?) without a compatibility claim TUR may not make.

**The risk that sentence carries.** It promises that nothing is lost.
The first lost file or unbootable Windows ends it. So the order of work
is the order of TUR's own priority list: safety of the person's Windows
before any integration.

**Who it is for.** A person with one Windows PC, not a developer, who
would not install "Linux" but would accept "more for the PC you have";
most sharply, the owner of a Windows 10 PC that Windows 11 refuses.
Windows 10 left support in October 2025; when its paid extended
security updates end for consumers was read from Microsoft's page by a
research pass as 12 October 2027, and the page refused a second,
automated read, so that date is **unconfirmed**. Either way the day
comes, and TUR underneath can keep such a PC useful and can keep the old
Windows away from the network when it is no longer patched. Not
for: a PC managed by an employer, a gaming PC whose games use kernel
anti-cheat (they refuse virtual machines; they still run when Windows is
started by itself), a PC with under 8 GiB of memory.

**What the first usable version must contain** (and nothing else):

1. The way in, starting on Windows: the checker that only reads (section
   6), a plain account of what will happen, the preparation Windows does
   best itself (Fast Startup off, its own shrink), and the handover to
   TUR's installer (pull requests 112 and 116).
2. Windows starts by itself exactly as before, from TUR's boot menu.
   This is the guarantee behind every other sentence.
3. The person's Windows started inside TUR on a layer that can be thrown
   away, with TUR's refusals when it is not safe (dualrun H1, H2, H6).
4. The desktop carried over: wallpaper, shortcuts on the desktop, the
   pinned applications, drive letters and labels as they were (section 7;
   dualrun H8).
5. Windows applications reachable from TUR's launcher, in the guest's
   own screen first; each in its own window only when dualrun's H5 has an
   answer for Home editions.
6. P@x seeing and operating Windows applications through the guest agent
   under the unchanged policy engine (sections 3 and 5).

**What to cut from the first version.** TUR inside Windows (WSL2) and the
virtual-machine appliance (section 9). Sign-in to Windows without a
second password. Exact desktop icon positions if they cost more than a
day (section 7.3). Android and Mac applications *inside* the Windows
story: they are TUR's, and arrive with TUR. Moving Windows to another PC.
Anything that changes Windows' own files beyond what its own tools do.

**What two shapes would have cost.** Shape A beside B is a second update
mechanism, a second display path, a second support matrix (every Windows
build times every WSL release), and an assistant with a different trust
model. For a very small team that is the difference between one product
and none. The owner's decision removes it.

**What still depends on Microsoft in shape B.** Less, but not nothing:
Windows must accept being started as a guest on the same PC (activation:
dualrun H3, and a lawyer's reading of the licence); BitLocker must be
suspended or its key supplied; the agent lives on Windows' accessibility
interfaces, which are public, stable and required by law in several
markets, and so are the least likely part of Windows to be withdrawn.

**The three experiments that would most change this judgement.**

1. *The owner's own Windows 10 PC, start to finish*: checker, shrink,
   install beside, start Windows inside TUR, start it alone again. Every
   hosted runner is a server or an ARM machine; none has BitLocker, OEM
   activation, a real firmware or a real graphics driver. If activation
   fails or Windows needs repair, the sentence needs changing.
2. *A Windows 11 Home guest with no Remote Desktop*: is each application
   in its own window achievable from an agent (dualrun H5b, with section
   4.3 here as its input)? If not, the first version is "your Windows in
   a window", which is a weaker product than "your Windows applications
   among TUR's".
3. *Ten real people's desktops*: the carry-over reader on PCs that have
   lived (OneDrive folder redirection, a Microsoft account, two monitors,
   years of pins). The runners' desktops are empty; section 7's formats
   are right only for what was on them.

## 9. Shape A (TUR inside Windows): what was learned, and why B

**Nothing in this section was measured.** It is what Microsoft's
documents, source and release notes said on 14 November 2026, read before
the owner's decision. It is kept so the reasoning is not lost.

| Question | What the documents say | Source |
|---|---|---|
| May TUR be distributed as a WSL distribution? | Yes. WSL is MIT-licensed since May 2025. A distribution is a root file system in a `.wsl` file that "can be distributed however you please"; a person installs it by double click or `wsl --install --from-file`. Redistributing WSL's own installer is not granted anywhere that was found, and is not needed | microsoft/WSL `LICENSE`; learn.microsoft.com, "Build a custom Linux distribution for WSL" |
| Which Windows? | WSL 3.0.5 (3 November 2026) installs on Windows 10 from build 19041 and on Windows 11, Home editions included; one package for both. Graphical applications need Windows 10 build 19044 or later | microsoft/WSL releases and `wslinstall` source; WSL FAQ; "Run Linux GUI apps" |
| Can TUR's image run there as it is? | No. WSL takes a flat root file system and brings its own kernel and its own start-up: "The archive should not contain a kernel or an initramfs". TUR's image is a bootc image whose updates, rollback and boot health belong to its own boot. Inside WSL none of that exists; updates would be a second mechanism | the same guide |
| The kernel | Microsoft's, 6.18.58, one for every distribution of the Windows account. `CONFIG_ANDROID_BINDER_IPC` is not set and `CONFIG_NTSYNC` is not set: TUR's Android container route and Wine's fast synchronisation are not there. A custom kernel can be named in `.wslconfig`, but it then replaces the kernel under the person's other distributions and Docker Desktop too | `arch/x86/configs/config-wsl` in microsoft/WSL2-Linux-Kernel, branch 6.18; "Advanced settings configuration in WSL" |
| The desktop | WSLg shows single application windows: "does not provide a full desktop experience". No system tray (an issue open since 2021), no Windows input methods, its own scaling path. The repository had packaging commits only in 2026 | microsoft/wslg README and issues |
| Devices | USB only through a third-party tool, one device at a time, an administrator's step per device. Disks only whole and only with administrator rights. Files across the boundary are slow by Microsoft's own description | learn.microsoft.com, "Connect USB devices", "Mount a Linux disk in WSL 2", "Comparing WSL versions" |
| Android | Microsoft's own Android subsystem ended on 5 March 2025; nothing replaced it. Waydroid in WSL2 is a community recipe with a custom kernel and software rendering | learn.microsoft.com, "Windows Subsystem for Android"; a community gist |
| Runners | `windows-2025` has WSL 3.0.5 preinstalled and WSL2 was reported working there; `windows-11-arm` not | actions/runner-images; Vampire/setup-wsl |

**Why B, in engineering terms.** The owner's sentence is "Linux is more
solid", and the table agrees:

1. *The floor.* In shape A the floor is Windows and Microsoft's kernel.
   TUR's first priority (the system boots reliably and recovers) and its
   rule that the Linux kernel is the single authority for hardware cannot
   be kept by a guest. In shape B they hold, and Windows gains them:
   layers, undo, repair from outside.
2. *The assistant's safety.* On Windows the policy engine would run
   beside or inside the system whose applications it controls, and any
   program with administrator rights could alter it. In shape B it sits
   outside the virtual machine.
3. *What the person gets.* Shape A adds Linux applications in windows and
   an assistant. It cannot protect Windows, cannot carry Android by the
   container route without replacing Microsoft's kernel, and cannot be
   TUR's own desktop.
4. *Dependence.* A product that lives inside WSL lasts as long as
   Microsoft ships WSL in that form. Microsoft ended its own Android
   subsystem with a year's notice, and in September 2026 shipped its own
   container runtime in WSL, the place Docker Desktop had occupied.
5. *Cost.* Two shapes are two update mechanisms, two display paths, two
   support matrices, for a very small team.

What shape A had that B does not: nothing to lose on the first day. That
need is real and is answered differently in section 8 (the way in only
reads until the person agrees; nothing of Windows is deleted; Windows
still starts by itself).

**A "try TUR in a window first" preview.** A plain virtual-machine
appliance of TUR for Windows (not WSL) is nearly free to produce from the
same image, and it is the wrong demonstration. Inside a virtual machine
on Windows, TUR cannot show the person's own Windows running inside it,
which is the promise; graphics are unaccelerated; Android by container
may not start. It would show a slow Linux desktop to exactly the person
TUR wants to convince otherwise. Hyper-V's manager is also absent from
Home editions, so it would lean on a third-party hypervisor. The preview
that shows the real thing at no risk is TUR's **live medium**: started
from a USB stick, it changes nothing, runs on the person's real hardware,
and can show the checker's findings and the person's own wallpaper and
shortcuts read from the disk (section 7). Recommendation: do not build
the appliance now; revisit it only as a by-product for reviewers and the
press once the image is public.

## 10. Limits

1. **The runners are not a person's PC.** Windows Server 2025, Windows
   Server 2022 and Windows 11 on ARM, in Microsoft's cloud, as an
   administrator with consent prompts switched off, on a 1024 by 768
   screen with one monitor, no BitLocker, no battery, no Microsoft
   account, a desktop nobody has lived on. Windows 10 itself, Windows 11
   on x64 and Home editions are NOT RUN.
2. **The agent here is a Python script**, not the product's native
   program. What UI Automation exposes does not depend on the client's
   language; speed and memory do, and are not claimed.
3. **The lists of controls** for PuTTY, Notepad++, 7-Zip and Inno Setup
   were counted by eye for exp-winsee-1 in pictures taken under Wine. The
   same builds draw the same controls on Windows, but nobody recounted
   them in the Windows pictures before scoring; where a row differs the
   text says so. For Microsoft's applications there is no such list
   unless section 3 says one was counted.
4. **Verification is independent only where the table says so.** A
   selection that only the same tier confirms is marked weak.
5. **Rights.** "Restricted" is the administrator's account with its
   administrator rights removed by `runas /trustlevel`, which is close to
   but not the same as a program started by a person at an unanswered
   consent prompt. "Standard" is a new account with an empty profile, so
   per-user facts under it describe an empty profile.
6. **Reading is not carrying over.** Section 7 says what can be read and
   in what form. Nothing was applied to a TUR desktop; that design is
   `dualrun`'s.
7. **Formats were read from what one runner image held.** A value that
   was absent on the runners (a slideshow, a second monitor, a OneDrive
   folder, a Microsoft account, a PIN) is described from documentation
   and marked so; it was not seen.
8. **Section 9 is documents only.**
9. **No Microsoft file left the runner** in a kept artifact. Pictures of
   Microsoft's applications were kept one day to count controls by eye
   and are gone.

## 11. Decisions for the owner

No numbered decision record is written here (the spike's rules). The
text below is offered for one.

**Proposed decision text.**

> *TUR is the floor under the person's own Windows.* TUR is installed
> beside an existing Windows and starts that Windows inside itself; it is
> not offered as a program that runs inside Windows. TUR's image is not
> shipped as a WSL distribution and no part of TUR's safety depends on a
> component that runs inside Windows.
>
> *The agent inside Windows holds no authority.* P@x's service, policy
> engine, model access and memory stay on TUR. A small program of TUR's
> own inside the person's Windows reads applications through Windows'
> public accessibility interfaces and the window tree, performs only what
> TUR sent, and reports windows and events. Everything it sends is data
> from the less trusted side: it is never an instruction, never the basis
> of a decision about TUR itself, and never decides where TUR's own
> confirmation surfaces are drawn.
>
> *The way in only reads until the person agrees.* TUR's program for
> Windows inspects the PC without administrator rights and changes
> nothing. After consent it may ask Windows to do what Windows' own tools
> do (switch Fast Startup off, shrink its own volume, suspend BitLocker
> for the change) and may stage TUR's installer. It never writes to a
> partition itself. What it hands to the installer is a hint that the
> installer verifies.
>
> *The desktop is carried over from the disk, and shown before it is
> applied.* Wallpaper, colours, desktop shortcuts, pinned applications,
> drive letters and labels, known folders and regional settings are read
> from the Windows installation, stay on the PC, and are listed for the
> person to accept. No Microsoft artwork is copied into TUR's image or
> sent anywhere; the person's own files are referenced where they lie.

**Decisions only the owner can make.**

1. **Accept that text**, or change the sentence TUR is sold with.
2. **A code-signing identity for Windows.** The "get TUR" program and the
   guest agent are Windows programs. Unsigned, the first thing a person
   sees is Windows' warning about an unknown publisher; and Windows gives
   the right to operate windows of elevated programs only to signed
   programs (section 3.4). A certificate costs money each year and binds
   a legal name to TUR. Yes or no, and in whose name.
3. **How much the Windows program may do after consent**: only explain
   (the person switches Fast Startup off and shrinks the volume by hand,
   as pull request 112 assumes today), or do those steps through
   Windows' own tools. The second is the "superpowers" experience; the
   first has no failure TUR can be blamed for.
4. **Staging without a USB stick.** Putting the installer on the disk
   and asking the firmware to start it once removes the stick from the
   story and adds a change to the EFI partition made from Windows. It
   needs its own experiment and decision; this spike only names it.
5. **A driver added to the person's Windows, or none.** The channel to
   the agent is cleanest over a virtio serial port, which needs a
   third-party signed driver installed in Windows. `dualrun` measures how
   far Windows goes with none. Adding one is a change to the person's
   Windows that outlives a discarded layer only if the layer is kept.
6. **What is carried over in the first version** (section 7's table):
   the proposal is everything marked "from the disk", with icon positions
   and Start pins only if section 7 shows them readable.
7. **Who reads the carry-over data, and when**: the Windows program
   while Windows runs (it can ask Windows itself, and sees what only a
   signed-in session can), TUR's installer from the disk (it needs no
   trust in anything that ran on Windows), or both with the disk winning.
   The proposal is both.
8. **The licence question** (running the person's Windows as a guest on
   the same PC) stays with the owner's lawyer; it is `dualrun`'s H3, and
   nothing in this spike changes it.
