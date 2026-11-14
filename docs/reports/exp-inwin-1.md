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

NOT RUN.

## 7. What TUR can carry over from a Windows desktop

NOT RUN.

## 8. Product judgement

Not written yet.

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

Not written yet.

## 11. Decisions for the owner

Not written yet.
