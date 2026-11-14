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

## 5. The interface between TUR and the guest agent

Not written yet.

## 6. The way in from Windows

NOT RUN.

## 7. What TUR can carry over from a Windows desktop

NOT RUN.

## 8. Product judgement

Not written yet.

## 9. Shape A (TUR inside Windows): what was learned, and why B

Not written yet.

## 10. Limits

Not written yet.

## 11. Decisions for the owner

Not written yet.
