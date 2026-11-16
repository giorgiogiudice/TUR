<!-- SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0 -->
# exp-inwin-1 — TUR as the solid floor under the person's own Windows, seen from the Windows side (experiment)

> **Status: both runs read; every hypothesis settled.** H1, H2 and H3
> hold. H4 holds for the disk layout but **is killed for BitLocker at the
> person's own level**: no route gave BitLocker's state to a program at
> medium integrity without administrator rights (section 6.1). H5 holds:
> every listed item is read from the files without Windows except Start
> pins, which are carried by the agent (the registered fallback); the
> shortcut reader agrees with Windows on 91.7 to 95.9 % of targets. For
> dualrun's H5(b): a covered window's own content was obtained, identical
> to the window shown in front, for every kind of window (section 4.3).
> Runs 43552339780 on `a0aa992` and 43755897776 on `000e7a1` (15 and 16
> November 2026).

Two things were added to the experiment after the first commit and before
any run, and are registered here for that reason:

- **For `dualrun`'s H5(b)** (each Windows application in its own window
  without Remote Desktop): whether the content of a window that another
  window covers can be had from inside Windows by asking the window itself
  (`PrintWindow`), for a classic window, Explorer, a XAML application and
  a Chromium browser. The kill criterion is `dualrun`'s own: the content
  of a covered window cannot be obtained.
- **For H5**: the registry file is also read by a reader of the file
  format that asks Windows nothing (`carry/regf.py`), and compared value
  by value with the running Windows. Kill: any of the listed values
  differs or cannot be read.

```text
TUR DEVELOPMENT REPORT

Task ID:          exp-inwin-1 (time-boxed research spike `inwin`)
Subsystem:        P@x (AI control layer: acting through what an application
                  exposes, here inside a Windows guest); installation (the
                  way in from Windows); desktop (what is carried over)
Build ID:         not applicable: this task builds no image
Source revision:  exp/inwin (pull request 132); the run a number comes
                  from is named next to it

SUMMARY
- What changed: an experiment, not a feature. Scripts that run on
  GitHub's Windows runners: a guest agent's view of real applications
  (UI Automation natively, and the winsee bridge built unmodified for
  real Windows), with actions verified; the window list and its events;
  a "get TUR" checker that only reads; a reader of what TUR can carry
  over from a Windows desktop, with readers of shortcut files and
  registry files written from their published formats.
- Why: the owner's decision of 14 November 2026 that TUR is the floor
  under the person's own Windows. Everything that decision needs from
  inside Windows can be measured only on real Windows.

FILES
- Added: tests/experiments/inwin/** (agent/, checker/, carry/, probe.py,
  rights.py, requirements.txt), .github/workflows/exp-inwin.yml,
  docs/reports/exp-inwin-1.md
- Modified: docs/frontier.md (one row, F8)
- Removed: none

BUILD
- Command: the winsee experiment's bridge/build.sh, at commit 622bfa6 of
  pull request 126, with the Windows runner's mingw-w64 gcc; in CI only.
  Nothing was built or run on a development machine.
- Result: built (job bridge of run 43552339780): tur-winbridge.exe,
  tur-winhook.dll, tw-controls.exe, tw-attack.exe, 64-bit

TESTS
- Integration: workflow exp-inwin, run 43552339780 on a0aa992: success
  (jobs bridge; see and way-in on windows-2025, windows-2022,
  windows-11-arm); run 43755897776 on 000e7a1: success (the same jobs)
- Boot tests: NOT RUN (nothing here can affect boot)
- Hardware tests: NOT RUN (hosted runners; no PC)

KNOWN LIMITATIONS
- See section 10.

NEXT SAFE ACTION
- Read the first run's tables. Nothing here is wired into tur-pax,
  the installer, the image or any service.
```

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
  reader of shortcut files from the published format [MS-SHLLINK]),
  `regf.py` (a reader of registry files), `layout.py` (a reader of the
  desktop's icon layout, whose structure was read off run 43552339780's
  values; added after that run).
- `tests/experiments/inwin/rights.py`: runs a reader with three sets of
  rights and tabulates what each could read.
- `.github/workflows/exp-inwin.yml`: three jobs, all on Windows runners.
- Runs: **43552339780** on `a0aa992` (the first; read on 15 November 2026
  after its one-day pictures had expired) and **43755897776** on
  `000e7a1` (the second, after the fixes listed in section 3.4; read on 16
  November 2026 within its pictures' day). Where a number differs between
  them, the text names the run; the counts of section 3.1 were identical.
- Pictures and full reads of Microsoft's applications are kept for one day
  only, to count by eye; kept artifacts hold counts, rectangles, paths and
  hashes. Applications are fetched from their publishers by hash and thrown
  away. No Microsoft file is committed or uploaded.

## 3. P@x's guest agent on real Windows

`agent/seer.py`. Each window was read four ways: UI Automation natively
(`uia`), and the winsee bridge's three tiers (`bridge-win32`, the plain
window tree; `bridge-msaa`; `bridge-uia`). The bridge is
`tur-winbridge.exe` of pull request 126 at `622bfa6`, built unmodified with
the runner's mingw-w64 gcc in job `bridge` (64-bit, with its hook library;
it answered every request on all three runners, the ARM one included, where
it ran as an x64 program under Windows' emulation). Counts are from run
43552339780 and were the same in run 43755897776 except Firefox's.

### 3.1 What it sees (H1)

**Windows with a list of controls.** The lists are winsee's, counted by eye
under Wine (section 10.3), and TUR's own (its test window's source, its
form page). A control counts when a tier reports it on screen with the
listed name and kind. The counts were identical on the three runners; the
one difference is that Notepad++'s Preferences was not reached on the ARM
runner in run 43552339780 (section 3.2).

| Window (controls listed) | UI Automation | window tree | MSAA | bridge's UI Automation | UI Automation or window tree |
|---|---|---|---|---|---|
| PuTTY 0.85, configuration (38) | 38 | 38 | 38 | 38 | 38 |
| Notepad++ 8.9.9.1, main window (50) | 49 | 50 | 32 | 49 | 50 |
| Notepad++, Preferences (31) | 31 | 31 | 31 | 31 | 31 |
| 7-Zip 26.06, File Manager (25) | 25 | 25 | 12 | 25 | 25 |
| Inno Setup 7.1.1, licence page (7) | 7 | 7 | 7 | 7 | 7 |
| **The four applications (151)** | **150 (99 %)** | **151 (100 %)** | 120 (79 %) | 150 | **151 (100 %)** |
| TUR's test window, classic Win32 (31) | 28 | 31 | 20 | 28 | 31 |
| Explorer, a folder of four items (4) | 4 | 0 | 0 | 4 | 4 |
| TUR's form page in Edge (7) | 7 | 0 | 0 | 7 | 7 |
| TUR's form page in Chrome (7) | 7 | 0 | 0 | 7 | 7 |
| TUR's form page in Firefox (7), run 43552339780 | 0 | 0 | 0 | 0 | 0 |
| TUR's form page in Firefox (7), run 43755897776 | 7 | 0 | 0 | 7 | 7 |

**Windows without a list.** The pictures of Microsoft's applications were
kept for one day and had expired before anyone read run 43552339780, so
nobody counted by eye what a person sees in them; the cells are what each
tier reported on screen (Server 2025 / Server 2022 / Windows 11 ARM), not a
score.

| Window | UI Automation | window tree | MSAA | bridge's UI Automation |
|---|---|---|---|---|
| Notepad | 14 / 14 / 16 | 6 / 6 / 3 | 1 / 1 / 3 | 14 / 14 / 16 |
| Notepad, File menu open | 8 / 8 / – | 0 / 0 / – | 9 / 9 / – | 9 / 9 / – |
| Settings, first page (XAML) | 24 / 14 / 25 | 0 / 0 / 0 | 1 / 0 / 1 | 26 / 15 / 27 |
| Settings, Apps page | 25 / 19 / 27 | 0 / 0 / 0 | 1 / 1 / 1 | 27 / 21 / 29 |
| WordPad's ribbon (Server 2022 only) | 57 | 6 | 9 | 59 |
| WordPad, View tab | 26 | 4 | 9 | 26 |
| Inno Setup's two first prompts | 5, 5 | 3, 3 | 3, 4 | 5, 5 |

WordPad is not part of Server 2025 or of this Windows 11: its ribbon was
read on Server 2022 only.

**Microsoft's applications, counted by eye (run 43755897776).** The
one-day pictures of run 43755897776 were read within their day. Counted:
every control a person can operate in the window as pictured (buttons,
fields, menus, tabs, list rows, links, scroll bars, the window's own
caption buttons), set against the outline UI Automation drew for each
control it reported. On the ARM runner the pictures were again of Windows'
first-sign-in window (section 10.10), so none is counted there.

| Window | Runner | Controls a person sees | Outlined by UI Automation |
|---|---|---|---|
| Notepad | Server 2022 | 12 | 12 |
| Notepad, File menu | Server 2022 | 8 | 8 |
| Settings, first page | Server 2022 | 14 | 14 |
| Settings, Apps page | Server 2022 | 18 | 18 |
| Settings, first page (Windows 11 design) | Server 2025 | 23 | 23 |
| WordPad with its ribbon | Server 2022 | 48 | 48 |
| **Together** | | **123** | **123** |

The window tree outlined 6 of Notepad's 12 and none of Settings'; MSAA one
or none in Settings. UI Automation's extra elements beyond these counts
are parts of one control (a split button's arrow, a combo box's text) and
headings.

**What the numbers say.**

1. *The applications measured under Wine*: UI Automation and the window
   tree together found every listed control, 151 of 151. UI Automation
   alone missed one (an unnamed text field of Notepad++); the window tree
   alone missed none. The registered bar was what the window tree reached
   under Wine (163 of 172, 95 %); on real Windows the same builds lose
   nothing.
2. *What Wine could not show*: Settings (XAML) gave UI Automation 14 to 27
   controls a page and the window tree none; TUR's page in Edge and Chrome
   gave UI Automation 7 of 7 and the window tree and MSAA none; Explorer's
   items are invisible to the window tree. The window tree alone is blind
   to modern applications, to Explorer's content and to the web; UI
   Automation is the tier for them.
3. *The window tree is still needed*: the three toolbar buttons of TUR's
   test window that carry only an icon reach UI Automation without a name;
   the window tree names them from the toolbar's own text. The agent reads
   both and merges, as the `UI Automation or window tree` column does.
4. *Browsers build their page tree on the first request*: Edge and Chrome
   answered the first read with 0 of the 7 page controls and a read two
   seconds later with 7 of 7, on every runner. The agent must re-read, or
   wait for the structure-changed event, before it reports a page empty.
5. *Firefox's 0 of 7 is not Firefox's accessibility*: its first-run
   welcome window was open over the page (it is modal: the page beneath is
   hidden from accessibility, as it is from a person). Started with the
   preferences that skip it, run 43755897776 found 7 of 7 page controls in
   Firefox on every runner (29 controls in all), after the same empty first
   read as Chromium's.
6. *The bridge's own UI Automation tier* reports what native UI Automation
   reports, within two controls, at two to ten times the cost (it adds the
   pipe and its own tree walk). On real Windows the product agent should
   call UI Automation directly; the bridge's value is that the same
   program and protocol serve Wine and Windows.
7. *Cost*: one UI Automation read of a window took 4 to 392 ms on the x64
   runners (median about 60 ms; Explorer was the slowest) and up to 1.2 s
   on ARM (TUR's test window 1232 ms, Explorer 1153 ms).
8. *Microsoft's own applications*: 123 of 123 controls a person sees,
   counted by eye, were reported by UI Automation (table above).

**H1: holds.** Kill: under 80 % of winsee's listed controls found by UI
Automation or the window tree in the four applications (measured: 100 %
on every runner), or under half of the page's seven controls in every
browser (measured: 7 of 7 in Edge, Chrome and, in run 43755897776,
Firefox).

### 3.2 Whether its actions arrive (H2)

Every action was sent by UI Automation's control patterns (and one by the
bridge's window-tree tier), and each was checked by a route that does not
ask the tier that acted.

| Kind | Application: action | S25 | S22 | A11 | Checked by |
|---|---|---|---|---|---|
| Classic Win32 | TUR's test window: press "Save draft" | yes | yes | yes | the application's own log |
| | TUR's test window: tick "Remember me" | yes | yes | yes | `BM_GETCHECK` |
| | TUR's test window: choose "Large" | yes | yes | yes | `BM_GETCHECK` |
| | TUR's test window: open the "Advanced" tab | yes | yes | yes | the application's own log |
| | TUR's test window: type into "Name" | yes | yes | yes | `WM_GETTEXT` |
| | TUR's test window: toolbar "Refresh", by the bridge's window-tree tier | yes | yes | yes | the application's own log |
| | Notepad: type into the editor | yes | yes | yes | `WM_GETTEXT` of its Edit control |
| | Notepad: open the File menu | yes | yes | **no** | a menu window appears |
| | Notepad: answer the save prompt with "Don't Save" | yes | yes | yes | the window is gone |
| | PuTTY: choose "Serial" | yes | yes | yes | `BM_GETCHECK` |
| | PuTTY: type into the host field | yes | yes | yes | `WM_GETTEXT` |
| | PuTTY: select the tree row "Colours" | yes | yes | yes | the panel's controls change (same tier: weak) |
| | Notepad++: Settings, Preferences | yes | yes | **no** | the Preferences window opens |
| | 7-Zip: select beta.txt | no¹ | no¹ | no¹ | (run 43552339780 looked in the wrong place) |
| Explorer | select beta.txt | yes² | yes² | yes² | the item says it is selected (same tier: weak) |
| | open the folder "inner" | yes | yes | yes | the window's title |
| Modern (XAML) | Settings: open the "Apps" page | yes | yes | **no**³ | the picture changes |
| Web page | Edge: fill the form and send it | yes | yes | yes | the page's own script sets the title to "sent:Ada" |
| | Chrome: the same | yes | yes | yes | the same |
| | Firefox: the same | no⁴ | no⁴ | no⁴ | — |
| Installer | Inno Setup: "Install for all users", "OK" | yes | yes | yes | the page changes |
| | Inno Setup: accept the licence | yes | yes | yes | `IsWindowEnabled` of "Next" |
| Ribbon | WordPad: switch to View | – | yes | – | the picture changes |
| | WordPad: type into the document | – | yes⁵ | – | not checked independently |
| | **Arrived** | **21 of 23** | **23 of 25** | **18 of 22** | |

¹ The check looked for "1 object selected" in the name of 7-Zip's status
bar, which UI Automation does not give; whether the row was selected is
unknown. Run 43755897776 asked the list view itself (`LVM_GETITEMSTATE`): arrived.
² Same tier only. Run 43755897776 asked the shell's own model of the folder window
(`Shell.Application`), which does not go through UI Automation.
³ The picture of the screen did not change because Windows 11's first
sign-in window covered the whole screen on the ARM runner for the entire
run (section 10.10); UI Automation saw 15 new names on the page, so the page
probably opened. Run 43755897776 also compared the
window's own picture.
⁴ No action was sent: the page's controls were behind Firefox's welcome
window (section 3.1).
⁵ Counted as arrived in run 43552339780's total although nothing checked
it; the honest count on Server 2022 is 22 of 24 checked.

**Run 43755897776, with the checks of section 3.4.** Every action is now
checked by a route other than the one that acted.

| Runner | Arrived | Not arrived |
|---|---|---|
| Server 2025 | **22 of 22** | — (Notepad++'s Settings menu: both ExpandCollapse and the default action refused with `0x80131509`, so no action was sent; not counted) |
| Server 2022 | **25 of 25** | — |
| Windows 11 ARM | **21 of 23** | Notepad's File menu and Notepad++'s Settings menu: ExpandCollapse and the default action both refused (`COMError 0x80131509`) |

What changed from the first run: 7-Zip's selection arrived on all three
(`LVM_GETITEMSTATE`: row 1 selected, the others not); Explorer's selection
is confirmed by the shell's own record of the folder window (1 selected,
"beta"); Firefox's form arrived (the page's script set the title); PuTTY's
tree row is confirmed by the window tree (8 visible child windows of the
Colours panel); WordPad's typing is confirmed by `WM_GETTEXT` of its
rich-edit control; Settings on ARM is confirmed by the window's own picture
(26.7 % changed) while the screen showed only the first-sign-in window.

**Menus are the one weak spot.** Opening a classic menu bar's menu through
UI Automation failed on Windows 11 ARM in both runs, with the first-sign-in
window hidden or not, and once on Server 2025 (Notepad++, run 43755897776;
the same action arrived there in run 43552339780). The product agent opens
classic menus by the window tree instead (the winsee bridge's
`WM_COMMAND` route arrived on every runner) or by the keyboard, and falls
back between them.

**H2: holds.** Kill: a kind of application in which no action arrives. In
both runs, in every kind (classic Win32, Explorer, modern, web page in
every browser, installer, ribbon), actions arrived and were checked
independently on every runner where the kind exists.

### 3.3 The agent below the program's rights

Run 43755897776: a Notepad started with the job's administrator rights
(high integrity), and the agent's `lower.py` started twice against it.
Identical on the three runners except the one cell marked.

| What the agent tried | At medium integrity (`CreateProcessAsUser`, the level of an ordinary program on a person's desktop) | Through `runas /trustlevel:0x20000` |
|---|---|---|
| Its own rights, as recorded | integrity **medium**, Administrators enabled | integrity **high**, Administrators deny-only |
| Read the window's title | yes | yes |
| Enumerate its child windows | yes | yes |
| UI Automation read | **2 elements** (the frame only) | 27 elements (30 on ARM) |
| UI Automation `SetValue` into the text | — (nothing to act on) | arrived |
| `WM_SETTEXT` into the editor | **refused** (error 5, access denied) | arrived |
| Keys typed (`keybd_event`) | **did not arrive** | arrived on Server 2022 only (the foreground was not granted on the others) |
| Post `WM_CLOSE` | **refused** | arrived |

So User Interface Privilege Isolation does what Microsoft documents: an
agent at the person's level sees an elevated window's title and nothing
inside it, and can neither type into it nor close it. (The `runas
/trustlevel` column is not a lower agent at all: it keeps high integrity
and only loses the Administrators group, and UIPI goes by integrity.)

**Consequence for the product.** The session part of the agent can read
and operate everything the person runs normally, and nothing they started
"as administrator", unless it holds Windows' "UI access" right, which
Windows grants only to a program that is signed and installed in a
protected folder (`Program Files` or `System32`). Without a code-signing
identity (section 11, decision 2), P@x must say plainly that it cannot see
an elevated window, and show the guest's own screen for it.

### 3.4 What reading run 43552339780 changed in the experiment

Fixed in commits `83c8528` and `0531d1f` and measured again by run
43755897776:

| Found in the first run | Fix |
|---|---|
| The token probe asked `GetTokenInformation` for `TokenElevation` with a 256-byte buffer; Windows wants exactly four bytes and refused every call, which also emptied section 3.3 | Exact sizes; the lower agent no longer stops when that probe fails |
| Pictures and picture checks were of whatever lay on top: windows were placed with `SWP_NOZORDER` and `SetForegroundWindow` alone, and on ARM Windows 11's first-sign-in window covered the screen | Windows raised with the topmost / not-topmost pair; every picture checked with `WindowFromPoint`; the first-sign-in window hidden first; picture checks also compare the window's own picture (`PrintWindow`) |
| Firefox's welcome window hid the page | The preferences a managed installation sets to skip it, and an answer through UI Automation if it comes anyway |
| 7-Zip's selection looked for in a status-bar name UI Automation does not give; Explorer's checked by the tier that acted; PuTTY's tree row likewise; WordPad's typing counted with no check | `LVM_GETITEMSTATE`; `Shell.Application`'s own record of the folder window's selection; PuTTY's child windows; `WM_GETTEXT` of WordPad's rich-edit control |
| ARM refused `ExpandCollapse` on two Win32 menus | Falls back to the element's default action, and says so |
| The covered-window measurement (section 4.3) was void | Rebuilt as above; pictures kept one day |
| `WScript.Shell`'s view of the shortcuts failed on `Get-Content`'s decorated strings (section 7.3) | `ReadAllLines` |
| The taskbar order reader caught a stray character before each name | Names looked up in the value, order by position |
| Three answers called "read" told nothing (section 6.1) | "empty" and "read (differs)" |
| No route to the disk layout without administrator rights was tried beyond PowerShell's | Four more (section 6.1) |
| Icon positions were read as bytes only | `layout.py` decodes them; an icon dragged with the real pointer tests a non-default layout (section 7.3) |
| Only the per-user registry file was compared | SYSTEM and SOFTWARE saved and compared value by value; drive letters byte for byte |

## 4. The window list a seamless desktop needs (H3)

`agent/desk.py`, run 43552339780 (sections 4.1 and 4.2; section 4.3 was
void in that run). The window whose changes
were timed belongs to another process (TUR's `tw-controls.exe`); the
listener is an out-of-context WinEvent hook, as an agent would hold it.

### 4.1 One full listing

Every visible top-level window with rectangle, visible frame, title
length, class, program, z-order, cloaking, minimised and maximised state,
owner and window styles; "with icons" adds each window's icon handle and
its size. Forty listings each.

| | Server 2025 | Server 2022 | Windows 11 ARM |
|---|---|---|---|
| Visible top-level windows | 15 | 13 | 15 |
| Of which a taskbar would show | 4 | 4 | 5 |
| Cloaked by the compositor (never show) | 7 | 6 | 7 |
| Visible frame differs from the window rectangle | 5 | 6 | 6 |
| Listing without icons, median / 95th / max | 0.58 / 0.64 / 0.70 ms | 0.75 / 0.82 / 0.90 ms | 0.94 / 0.96 / 0.97 ms |
| Listing with icons, median / 95th / max | 2.08 / 2.30 / 2.68 ms | 1.75 / 2.27 / 2.29 ms | 2.84 / 2.97 / 2.98 ms |

Icons: every window that had one gave a 32 × 32, 32-bit icon (WM_GETICON
or the class icon); TUR's test window has none of its own. Three
observations a host must act on: *cloaked* windows (Windows' own hidden
shell surfaces, and on Windows 11 suspended Store windows) are half of the
"visible" list; the window rectangle includes an invisible resize border
of 7 pixels on each side and 7 below, so the host must place by the frame
(`DWMWA_EXTENDED_FRAME_BOUNDS`), not the rectangle; and "on the taskbar"
has to be computed (unowned, not a tool window, not cloaked, titled), as
here, because Windows exposes no such flag.

### 4.2 Events against polling

Twenty-five rounds of four changes made to the other process's window,
each timed from the call to the arrival of Windows' own event.

| Change (event) | Server 2025 median / 95th / max | Server 2022 | Windows 11 ARM |
|---|---|---|---|
| Moved (`EVENT_OBJECT_LOCATIONCHANGE`) | 0.69 / 0.74 / 0.75 ms | 0.57 / 0.63 / 0.64 ms | 0.64 / 1.58 / 1.59 ms |
| New title (`EVENT_OBJECT_NAMECHANGE`) | 0.31 / 0.38 / 0.41 ms | 0.28 / 0.29 / 0.29 ms | 0.29 / 0.34 / 0.42 ms |
| Minimised (`EVENT_SYSTEM_MINIMIZESTART`) | 3.56 / 3.85 / 4.47 ms | 1.70 / 1.81 / 1.85 ms | 5.31 / 5.82 / 6.09 ms |
| Restored (`EVENT_SYSTEM_MINIMIZEEND`) | 5.36 / 5.47 / 5.48 ms | 3.06 / 3.95 / 4.33 ms | 4.14 / 4.30 / 4.45 ms |
| Closed (`EVENT_OBJECT_DESTROY`) | 9.4 ms | 5.0 ms | 7.3 ms |
| Events in three idle seconds, whole desktop | 5 | 3 | 4 |
| A drag of 120 steps in 1.5 s | 120 events, all location, all for the window | 123 (3 title changes of another window) | 120 |

Every one of the 100 changes per runner brought its event (no change was
missed). Polling, for comparison: to see a move within 50 ms a host would
list every 50 ms, which costs 0.6–0.9 ms each time (1–2 % of one processor
core, continuously, idle or not) and finds the change 25 ms late on
average; the events arrive in under 1 ms for moves and titles and under
6 ms for minimising, and cost nothing while nothing happens. A drag
produces one event per step (80 a second here): the agent should coalesce
moves to one per frame before sending them (section 5).

**H3: holds** on all three runners, by a factor of over 100 against the
registered bounds (50 ms median, 200 ms at the 95th percentile, 250 ms for a
listing). The window list does not have to come from the remote-display
protocol's window channel.

### 4.3 A covered window's content, asked of the window (for dualrun's H5b)

Run 43755897776. Each kind of window was placed at 760 × 560 and raised;
the screen was pictured as the reference and the window was asked for its
own picture (`PrintWindow` with `PW_RENDERFULLCONTENT`); then Notepad was
raised over it and the window was asked again, eight times. "On top" and
"covered" were checked with `WindowFromPoint` at five points.

| Window | Server 2022: own picture covered vs the screen's reference | Server 2025: own picture covered vs own picture in front | ARM: the same | Screen while covered differs from the reference | One capture, median / max |
|---|---|---|---|---|---|
| TUR's test window (classic Win32) | 0.0 % | 0.0 % | 0.0 % | 76–77 % | 32–47 / 63 ms |
| Settings (XAML) | 0.0 % | 0.0 % | 0.0 % | 3–97 % | 16 / 32 ms |
| Edge (Chromium, composited) | 0.0 % | 3.1 % | 3.1 % | 7 % | 16–32 / 33 ms |
| Explorer | 5.3 %¹ | 0.0 % | 0.0 % | 22–27 % | 30–47 / 48 ms |

Only Server 2022's placements passed every check (on top for the
reference: yes; covered by the cover: yes), so only there is the covered
picture compared with the screen itself, and it is **identical** for three
kinds of window. On Server 2025 a Windows notification ("pin this
application?") stayed above every window in one corner, and on ARM
Windows' first-sign-in window stayed above everything (hiding its proxy
window did not remove it): the checks failed there, but the window's own
picture taken under the cover equals its own picture taken in front, and
the pictures (kept one day) show the whole window under the notification
and under the first-sign-in window. ¹ Explorer's 5.3 % on Server 2022 is
the same between its two own pictures: Explorer changed between captures
(it is not hidden content). Edge's 3.1 % is a change between its own two
pictures, not examined.

**A minimised window gives no content**: `PrintWindow` returns only its
160 × 28 caption, on all three runners.

**For dualrun's H5(b): its kill criterion is not met.** The content of a
covered window was obtained from inside Windows, without Remote Desktop,
for a classic window, a XAML application, Chromium and Explorer, on
Windows Server 2022, Server 2025 and Windows 11 on ARM, at 16 to 63 ms per
760 × 560 capture by polling. A minimised window must be restored (off
screen, or behind) to be captured. Not measured: the Windows Graphics
Capture API (frames pushed, not polled), a GPU-accelerated game or video,
a Home edition.

## 5. The interface between TUR and the guest agent (proposal; not built)

For the `dualrun` engineer, who owns the design of the session. Revised
after the measurements of sections 3 and 4; the changes from the version
written before any run are marked **(changed)**.

**Where things sit.** P@x's service, its policy engine, the model and its
keys stay on TUR, outside the virtual machine. Inside Windows there is only
an agent that holds no authority: it answers requests and performs what TUR
already decided, as the winsee bridge does under Wine.

**Two parts inside Windows.**

- A *service part* that owns the channel to TUR, starts with Windows, and
  stays idle unless the host answers its handshake (dualrun's rule: idle
  when Windows runs by itself).
- A *session part* that runs on the person's desktop with the person's
  rights. It reads windows, performs actions and reports events.
  **(changed)** Section 3.3 measured it: at the person's level the agent
  sees an elevated window's title and nothing else, and cannot act on it.
  Operating elevated windows needs Windows' "UI access" right (a signed
  program in a protected folder); without it the session part reports such
  a window as "elevated: not visible to P@x" and TUR shows the guest's
  screen for it.

**Transport (changed).** dualrun starts an unmodified Windows, which has no
virtio drivers, so the first channel is the one dualrun already uses: a
16550 serial port (driver built into Windows) for messages, and the private
virtual network link for anything large. A virtio serial port becomes an
option after the person approves adding a driver (section 11, decision 5).
What the numbers say about the serial line:

| Message | Measured size (sections 3, 4) | At a 16550's nominal 115 200 baud (11.5 KB/s) |
|---|---|---|
| One window read by UI Automation, this experiment's node model | 2–15 KB for the windows measured (10–73 nodes; about 160–200 bytes a node); 43 KB for a whole Chromium window through the bridge's UI Automation tier | 0.2–1.3 s; 3.8 s |
| One window read by the window tree, everything including hidden controls | up to 125 KB (Notepad++: 625 windows) | 11 s: never on the serial line; ask for visible controls only |
| One window event (`window.moved` and similar) | about 60 bytes | 190 events a second |
| A window dragged across the screen | 80 location events a second for one window (section 4.2) | about 5 KB/s: fits, but coalesce to one event per frame |
| A window's pixels | 1.7 MB for 760 × 560 at 32 bits | never on the serial line: the private link, or shared memory |

Whether QEMU paces its emulated 16550 at the nominal rate was not measured
here; dualrun's round trips can. The node model of this experiment is
verbose (names, kinds, rectangles, states, actions, class, automation
identifier, framework, handle); a product encoding with short keys and
rectangles relative to the window roughly halves it. **Proposal:** messages
on the serial line; `window.read` replies above 8 KB and all pixels on the
private link, named by the message that asked for them.

**Messages.** One JSON object a line, as the winsee bridge speaks, each
with a number; the agent's events carry a sequence number so that a lost
one is noticed.

| Direction | Message | Carries |
|---|---|---|
| TUR to agent | `hello` | protocol version; the agent answers with its version, Windows' build, the session's rights (section 6.1's token facts: elevated, integrity level) |
| | `windows.list` | every top-level window: an identifier, rectangle and visible frame (they differ by the invisible resize border on 5–6 of 13–15 windows, section 4.1), title, class, program, z-order, whether it is on the taskbar, minimised, cloaked by the compositor (6–7 of 13–15 windows are cloaked: never show those) |
| | `windows.watch` | start sending the events below |
| | `window.read` | one window's controls in tur-pax-a11y's node model, **(changed)** by UI Automation first and the window tree for what UI Automation leaves unnamed (section 3.1: toolbar buttons with only an icon), with a node budget, visible controls only unless asked; **re-read once after about two seconds when a browser window answers with an empty page** (section 3.1: Chromium builds its page tree on the first request) |
| | `element.act` | an action on a node, **with what TUR believes the node is** (name, kind, rectangle); the agent re-reads the node and refuses if it is no longer that |
| | `point.inspect` | what is at a screen point (winsee's rule for anything proposed by vision) |
| | `window.content` | the pixels of one window, also when it is covered (section 4.3: identical to the window in front; 16–63 ms per 760 × 560 capture), on the private link; a minimised window has none |
| | `icon.get` | a window's or an application's icon as raw pixels with a size (32 × 32 at 32 bits from every window that had one, section 4.1) |
| | `apps.list`, `open` | the Start menu's entries (section 7: 172–217 on the runners); start one; open a file with its default application |
| | `desktop.read` **(new)** | the carry-over manifest of section 7.4, read by the agent from the running Windows, for what the disk cannot give (Start pins on Windows 11) |
| Agent to TUR | `window.created`, `.destroyed`, `.moved`, `.titled`, `.minimised`, `.restored`, `.foreground`, `.reordered`, `.cloaked`, `.uncloaked` | the window's identifier and the new fact; `.moved` coalesced to one per 16 ms |
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

### 6.1 What a checker can read, and with which rights (H4)

`checker/check.py` through `rights.py`, on each runner three times. The
levels, as run 43755897776 recorded them from each process's token:

- *elevated*: the job's administrator, full token, **high** integrity;
- *restricted*: the same account through `runas /trustlevel:0x20000`:
  **still high integrity**, with the Administrators group deny-only. It is
  not what the report first called it (a program started from a person's
  desktop without a consent prompt runs at medium integrity);
- *standard*: a new local account that is not an administrator, started
  with `CreateProcessWithLogonW`: **medium** integrity. This is the level
  of an ordinary program on a person's desktop, and the one H4 is about;
  its per-user facts describe a new, empty profile.

Run 43755897776: 39 facts (34 as in run 43552339780, which gave the same
answers, and 5 new routes). Each cell: *read*; *read (differs)*: answered,
but not what the elevated run read; *empty*: answered without refusing,
with nothing in the answer; *refused*: access denied; *absent*: the value
does not exist on that machine; *failed*: another error. Where the three
runners agree one cell is shown.

| Fact | Route | Elevated | Restricted | Standard account |
|---|---|---|---|---|
| windows | `HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion` | read | read | read |
| architecture | `environment` | read | read | read |
| memory | `GlobalMemoryStatusEx` | read | read | read |
| firmware-type | `GetFirmwareType` | read | read | read |
| secure-boot (registry) | `HKLM\SYSTEM\CurrentControlSet\Control\SecureBoot\State` | read | read | read |
| secure-boot (cmdlet) | `Confirm-SecureBootUEFI` | read | refused | refused |
| virtualisation (processor feature) | `IsProcessorFeaturePresent 21, 20` | read | read | read |
| virtualisation (CIM) | `Win32_Processor` | read | read | read |
| hypervisor-present | `Win32_ComputerSystem` | read | read | read |
| system-drive-space | `GetDiskFreeSpaceExW` | read | read | read |
| volumes | `Get-Volume (Storage module)` | read | read | read |
| disks | `Get-Disk` | read | read | read |
| partitions | `Get-Partition` | read | empty | read |
| partitions (WMI) | `Win32_DiskPartition` | read | read | read |
| volume-on-disk (IOCTL) | `IOCTL_VOLUME_GET_VOLUME_DISK_EXTENTS on the volume, opened with no access` | read | read | read |
| partitions (IOCTL) | `IOCTL_DISK_GET_DRIVE_LAYOUT_EX on the disk, opened with no access` | read | read | read |
| shrink-room | `Get-PartitionSupportedSize` | read | failed | failed |
| shrink-room (NTFS volume data) | `FSCTL_GET_NTFS_VOLUME_DATA on the volume, opened with no access` | failed | failed | failed |
| bitlocker (shell property) | `System.Volume.BitLockerProtection` | read | read | read (differs) |
| bitlocker (cmdlet) | `Get-BitLockerVolume` | read | refused | refused |
| bitlocker (manage-bde) | `manage-bde -status` | read | refused | refused |
| bitlocker (WMI) | `Win32_EncryptableVolume` | read | refused | refused |
| fast-startup (registry) | `HKLM\SYSTEM\CurrentControlSet\Control\Session Manager\Power` | read | read | read |
| hibernation (registry) | `HKLM\SYSTEM\CurrentControlSet\Control\Power` | absent | absent | absent |
| hibernation-file | `the file itself` | read | read | read |
| sleep-states (powercfg) | `powercfg /a` | S25 read; S22 failed; A11 read | S25 read; S22 failed; A11 read | S25 read; S22 failed; A11 read |
| volume-dirty (fsutil) | `fsutil dirty query` | read | refused | refused |
| volume-dirty (FSCTL) | `FSCTL_IS_VOLUME_DIRTY on the volume, opened with no access` | failed | failed | failed |
| recovery-environment (reagentc) | `reagentc /info` | read | refused | refused |
| firmware-boot-entries (bcdedit) | `bcdedit /enum firmware` | read | refused | refused |
| tpm | `Get-Tpm` | read | empty | empty |
| power | `GetSystemPowerStatus` | read | read | read |
| pending-restart | `registry keys under HKLM` | read | read | read |
| storage-controllers | `Win32_SCSIController` | read | failed | read |
| usb-drives | `Get-Disk` | read | read | read |
| windows-licence-channel | `SoftwareLicensingProduct (no key is read)` | read | read | read |
| firmware-holds-a-windows-key | `SoftwareLicensingService (presence only)` | read | read | read |
| clock-is-utc | `HKLM\SYSTEM\...\TimeZoneInformation` | read | read | read |
| device-encryption-ready | `Win32_Tpm` | read | refused | refused |

**The new routes.** `IOCTL_DISK_GET_DRIVE_LAYOUT_EX` on the disk opened with
no access rights gave the whole partition table at every level, on every
runner, in under half a millisecond: the style, each partition's offset,
length, type GUID, partition GUID and attributes (Server 2025: Microsoft
reserved, EFI system, C:; Server 2022 and ARM: reserved, Windows recovery,
EFI system, C:). `IOCTL_VOLUME_GET_VOLUME_DISK_EXTENTS` names the disk and
offset of C: likewise, and `Win32_DiskPartition` answers every level. The
two file-system controls (`FSCTL_IS_VOLUME_DIRTY`,
`FSCTL_GET_NTFS_VOLUME_DATA`) failed at every level, the administrator's
included, with "invalid function": a volume opened with no access rights is
not a volume handle the file system answers. Windows' own shrink estimate
took 47 to 57 s and answers only an administrator.

**What the checker would say.** Server 2025: only 31.0 GiB free on C:,
under the installer's 41 GiB. Server 2022: nothing stands in the way.
Windows 11 ARM: Fast Startup is on.

**H4: holds for the disk layout; killed for BitLocker at medium
integrity.** Kill: BitLocker's state or the disk layout readable only with
administrator rights.

- *The disk layout* is readable by a medium-integrity program that is not
  an administrator, by three routes (`Get-Disk`, `Win32_DiskPartition`, the
  drive-layout control), the last asking the disk itself. `Get-Partition`
  is not one to rely on: it answered the medium account but gave the
  high-integrity restricted token nothing at all.
- *BitLocker's state*: the shell's property answered 2 ("off") to both
  high-integrity levels and **0 ("unknown") to the medium account**, in
  both runs, on all three runners; `Get-BitLockerVolume`, `manage-bde` and
  `Win32_EncryptableVolume` refused everything below the administrator.
  No route gave a program at the person's own level BitLocker's state. One
  doubt remains: the medium account was a new account started outside the
  interactive logon, and the 0 may come from that session rather than from
  its rights; a program started from a person's desktop is NOT RUN. An
  encrypted volume (the property reading 1) is not on any runner: NOT RUN.
- *Read only with administrator rights*, in both runs: Secure Boot by the
  cmdlet (the registry route answers everyone), the dirty bit by `fsutil`,
  Windows RE's state, firmware boot entries, the TPM's state, Windows'
  shrink estimate, the display configuration.

**What it changes in "get TUR" (section 6.2).** The first, read-only step
can state everything that decides except BitLocker without a consent
prompt. For BitLocker it says "not known yet" unless the shell answers 1 or
2, and the one consent prompt of step 3 (which Windows needs anyway to
suspend BitLocker) comes before that sentence. The person is never told
"not encrypted" from a 0.

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

### 7.1 What was read on the runners, and with which rights (H5)

`carry/carry.py` through `rights.py`, as for the checker, after a fixture
of TUR's own was put on each runner's desktop (a test wallpaper with fit
"6", two `.lnk` shortcuts and one `.url`). Run 43755897776 (run
43552339780 gave the same answers for the probes it had); the standard
account's per-user answers describe its own new, empty profile, so its
"read (differs)" and "absent" are expected there.

| Fact | Route | Elevated | Restricted | Standard account |
|---|---|---|---|---|
| wallpaper | `HKCU\Control Panel\Desktop: WallPaper, WallpaperStyle, TileWallpaper; %AppData%\Microsoft\Windows\Themes\TranscodedWallpaper` | read | read | read (differs) |
| wallpaper-per-monitor | `HKCU\Control Panel\Desktop: TranscodedImageCache, TranscodedImageCache_000...` | read | read | empty |
| wallpaper-kind | `HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\Wallpapers: BackgroundType` | read | read | absent |
| slideshow | `%AppData%\Microsoft\Windows\Themes\slideshow.ini; HKCU\Control Panel\Personalization\Desktop Slideshow` | read | read | absent |
| background-colour | `HKCU\Control Panel\Colors: Background` | read | read | S25 read (differs); S22 read (differs); A11 read |
| accent-colour | `HKCU\Software\Microsoft\Windows\DWM: AccentColor (ABGR), ColorizationColor; HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\Accent: AccentPalette` | read | read | absent |
| light-dark | `HKCU\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize: AppsUseLightTheme, SystemUsesLightTheme` | read | read | read (differs) |
| theme-file | `HKCU\Software\Microsoft\Windows\CurrentVersion\Themes: CurrentTheme` | read | read | absent |
| lock-screen (user settings) | `HKCU\Software\Microsoft\Windows\CurrentVersion\Lock Screen` | read | read | read |
| lock-screen (machine record) | `HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\SystemProtectedUserData\<SID>\AnyoneRead\LockScreen` | absent | absent | absent |
| lock-screen (picture files) | `C:\ProgramData\Microsoft\Windows\SystemData\<SID>\ReadOnly\LockScreen_*` | absent | absent | absent |
| desktop-shortcuts | `the Desktop folder and Public\Desktop: .lnk ([MS-SHLLINK]) and .url (text)` | read | read | read (differs) |
| desktop-shortcuts-as-windows-reads-them | `WScript.Shell on the same files` | read | read | read (differs) |
| desktop-icons-live | `the desktop's list view, through UI Automation` | read | read | read |
| desktop-icon-layout | `HKCU\Software\Microsoft\Windows\Shell\Bags\1\Desktop: IconLayouts, ItemPos<width>x<height>x<dpi>(<n>), FFlags, IconSize` | read | read | absent |
| desktop-icon-layout (decoded) | `HKCU\Software\Microsoft\Windows\Shell\Bags\1\Desktop: IconLayouts, decoded by layout.py and set beside the desktop's own positions` | read | read | absent |
| desktop-system-icons | `HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\HideDesktopIcons\NewStartPanel` | absent | absent | absent |
| taskbar-pinned-shortcuts | `%AppData%\Microsoft\Internet Explorer\Quick Launch\User Pinned\TaskBar\*.lnk` | S25 read; S22 read; A11 empty | S25 read; S22 read; A11 empty | empty |
| taskbar-order | `HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\Taskband: Favorites (order), the pinned folder (the shortcuts)` | S25 read; S22 read; A11 absent | S25 read; S22 read; A11 absent | absent |
| start-pins (Windows' export) | `Export-StartLayout` | read | read | failed |
| start-pins (files) | `%LocalAppData%\Packages\Microsoft.Windows.StartMenuExperienceHost_cw5n1h2txyewy\LocalState\start*.bin; HKCU\...\CloudStore` | read | read | read (differs) |
| start-menu-shortcuts | `%ProgramData%\Microsoft\Windows\Start Menu\Programs and %AppData%\...\Start Menu\Programs` | read | read | read (differs) |
| start-menu-shortcuts-as-windows-reads-them | `WScript.Shell on the same files` | read | read | S25 read; S22 read (differs); A11 read |
| start-apps | `Get-StartApps (shell:AppsFolder)` | read | read | read (differs) |
| packaged-applications | `Get-AppxPackage; on disk: Program Files\WindowsApps\*\AppxManifest.xml` | read | read | read (differs) |
| drive-letters | `HKLM\SYSTEM\MountedDevices: \DosDevices\X:` | read | read | read |
| volume-labels | `the label is in the file system itself; GetVolumeInformationW` | read | read | read |
| partitions | `Get-Partition` | read | empty | read |
| drive-label-overrides | `HKLM\SOFTWARE\...\Explorer\DriveIcons\<letter>\DefaultLabel` | empty | empty | empty |
| letters-against-volumes | `MountedDevices set beside the volumes and partitions present` | read | read (differs) | read |
| hidden-drives-policy | `HKCU\Software\Microsoft\Windows\CurrentVersion\Policies\Explorer: NoDrives` | absent | absent | absent |
| known-folders (registry) | `HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\User Shell Folders` | read | read | read |
| known-folders (Windows) | `the shell` | read | read | read (differs) |
| onedrive | `HKCU\Software\Microsoft\OneDrive\Accounts\Personal: UserFolder` | absent | absent | absent |
| associations | `HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\FileExts\<.ext>\UserChoice: ProgId; HKCU\Software\Microsoft\Windows\Shell\Associations\UrlAssociations\<protocol>\UserChoice; HKCR` | read | read | S25 read; S22 read; A11 read (differs) |
| installed-applications | `HKLM and HKCU ...\CurrentVersion\Uninstall\*; HKLM\...\App Paths` | read | read | read |
| accounts-with-profiles | `HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion\ProfileList (the file SOFTWARE); Users\<name>\NTUSER.DAT` | read | read (differs) | read (differs) |
| account-name | `HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Authentication\LogonUI: LastLoggedOnDisplayName` | absent | absent | absent |
| account-kind | `Get-LocalUser: PrincipalSource` | read | read | read |
| account-microsoft-identity | `HKCU\Software\Microsoft\IdentityCRL\UserExtendedProperties` | absent | absent | absent |
| account-picture (registry) | `HKLM\SOFTWARE\...\AccountPicture\Users\<SID>: Image32...Image1080` | absent | absent | absent |
| account-picture (files) | `C:\Users\Public\AccountPictures\<SID>` | absent | absent | absent |
| pin-in-use | `HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Authentication\LogonUI\NgcPin\Credentials\<SID> (existence)` | absent | absent | absent |
| hello-store | `C:\Windows\ServiceProfiles\LocalService\AppData\Local\Microsoft\Ngc (existence)` | S25 refused; S22 absent; A11 refused | S25 refused; S22 absent; A11 refused | S25 refused; S22 absent; A11 refused |
| biometrics-enrolled | `HKLM\SOFTWARE\...\WinBio\AccountInfo\<SID>: EnrolledFactors` | absent | absent | absent |
| account-database (live) | `HKLM\SAM (the file Windows\System32\config\SAM)` | refused | refused | refused |
| regional | `HKCU\Control Panel\International` | read | read | read |
| languages | `HKCU\Control Panel\International\User Profile: Languages` | read | read | read |
| keyboard-layouts | `HKCU\Keyboard Layout\Preload, Substitutes` | read | read | read |
| time-zone | `HKLM\SYSTEM\CurrentControlSet\Control\TimeZoneInformation: TimeZoneKeyName` | read | read | read |
| displays (live) | `EnumDisplayMonitors, GetDpiForMonitor` | read | read | read |
| display-scaling (registry) | `HKCU\Control Panel\Desktop\PerMonitorSettings\<monitor>: DpiValue; ...\WindowMetrics: AppliedDPI` | absent | absent | absent |
| display-layout (registry) | `HKLM\SYSTEM\CurrentControlSet\Control\GraphicsDrivers\Configuration` | read | refused | refused |
| registry-file-holds-the-same | `reg save HKCU, reg load, compare` | read | refused | refused |
| machine-registry-files-hold-the-same | `reg save HKLM\SYSTEM and HKLM\SOFTWARE, read by regf.py, compare` | read | refused | refused |

**The registry file against the running Windows.** The person's registry
(`reg save HKCU`, administrator only while Windows runs) was read back two
ways: loaded by Windows as a stranger's hive, and read by `regf.py`, which
asks Windows nothing. Both agreed with the running registry on every value
compared: 14 of 14 listed values on the x64 runners and 13 of 13 on ARM
(where the taskbar's order value does not exist), and, through `regf.py`,
every value of six whole keys (`Control Panel\Desktop`,
`Control Panel\International`, `User Shell Folders`, `Explorer\Advanced`,
`Environment`, `Control Panel\Colors`): 168 of 168, 163 of 163 and 163 of
163. Run 43755897776 compared 25 listed values (24 on ARM), now including
four default-application choices, two more known folders, the wallpaper
history and the colourisation colour: 25 of 25, 25 of 25 and 24 of 24 both
ways, and again every value of the six keys.

**The machine's files.** Run 43755897776 saved SYSTEM (10–18 MiB, 0.1 s) and
SOFTWARE (174–203 MiB, 1.8–2.5 s), read them with `regf.py` (5–90 ms) and
compared: on every runner `Select\Current`, the time zone, the UTC clock
flag, Fast Startup and the computer name (SYSTEM, through
`ControlSet00N`), and Windows' build, edition and version and the program
folder (SOFTWARE) were all the same, 9 of 9; every drive letter in
`MountedDevices` was byte-identical (3 of 3); the profile list held the
same accounts (5 of 5, 6 of 6, 6 of 6) and the uninstall list the same
number of entries (265, 257, 263). Saving a machine file needs an
administrator while Windows runs; TUR's installer reads the files from the
disk when Windows is not running and needs no one's rights. The registered
kill for this addition (any listed value differs or cannot be read) did not
happen.

**Drive letters.** `MountedDevices` recorded C: by partition GUID (the GUID
`Get-Partition` reports for C:), D: by MBR signature and offset, and E: by
a device path, on all three runners. On ARM there was no D: volume at all,
and E: was absent everywhere: the record keeps letters of volumes that are
gone (section 7.4, rule 1). The GPT letter's partition GUID was found among
the partitions present on every runner.

### 7.2 Where each thing lives on the disk (for `dualrun` and the installer)

TUR's installer sees the disk before Windows ever runs under TUR. Nearly
everything is in three registry files and a few folders:

- `Users\<name>\NTUSER.DAT`: the person's own settings (what a running
  Windows shows as HKCU);
- `Windows\System32\config\SOFTWARE` and `...\SYSTEM`: the machine's
  (HKLM\SOFTWARE and HKLM\SYSTEM). On the disk there is no
  `CurrentControlSet`: `SYSTEM\Select\Current` names the `ControlSet00N`
  to read;
- `Windows\System32\config\SAM`: the accounts' secrets. Not needed: the
  profile list in SOFTWARE says which people use the PC, and TUR has no
  business reading password hashes.

"Seen" says on which runners the value was present and read (S25 Windows
Server 2025, S22 Windows Server 2022, A11 Windows 11 on ARM), "doc" that it
is described from documentation only. "Disk" says how the value was read
from the file without Windows (section 7.1).

| What | Where on the disk | Format | Seen | Disk |
|---|---|---|---|---|
| Wallpaper: the picture | NTUSER.DAT `Control Panel\Desktop\WallPaper` names the file; Windows' own scaled copy is `AppData\Roaming\Microsoft\Windows\Themes\TranscodedWallpaper` | a path (REG_SZ); a JPEG file | S25 S22 A11 | regf.py |
| Wallpaper: fit | same key, `WallpaperStyle` (0 centre, 2 stretch, 6 fit, 10 fill, 22 span) and `TileWallpaper` | REG_SZ digits | S25 S22 A11 | regf.py |
| Wallpaper history | `...\Explorer\Wallpapers\BackgroundHistoryPath0..4` | paths | S25 S22 A11 | regf.py |
| Slideshow, per-monitor wallpaper | `AppData\...\Themes\slideshow.ini`; `TranscodedImageCache_00N` | ini text; binary with a UTF-16 path at byte 24 | doc (no slideshow and one monitor on the runners) | — |
| Background colour | `Control Panel\Colors\Background` | "R G B" text | S25 S22 A11 | regf.py |
| Accent colour | `Software\Microsoft\Windows\DWM\AccentColor` (ABGR) and `ColorizationColor` (ARGB); `...\Explorer\Accent\AccentPalette` (8 × RGBA) | DWORD; 32 bytes | S25 S22 A11 | regf.py |
| Light or dark | `...\Themes\Personalize\AppsUseLightTheme`, `SystemUsesLightTheme` | DWORD 0/1 | S25 S22 A11 | regf.py |
| Desktop shortcuts | `Users\<name>\Desktop\*.lnk, *.url` and `Users\Public\Desktop` (or wherever `User Shell Folders\Desktop` points) | [MS-SHLLINK]; INI text | S25 S22 A11 | lnk.py (7.3) |
| Desktop icon positions | NTUSER.DAT `Software\Microsoft\Windows\Shell\Bags\1\Desktop\IconLayouts` | undocumented; decoded by layout.py (7.3) | S25 S22 A11 | layout.py |
| Desktop view (grid, size, auto-arrange) | same key, `FFlags`, `IconSize`, `Mode`, `LogicalViewMode` | DWORDs | S25 S22 A11 | regf.py |
| Taskbar pins | `AppData\Roaming\Microsoft\Internet Explorer\Quick Launch\User Pinned\TaskBar\*.lnk` | [MS-SHLLINK] | S25 S22 (none on A11) | lnk.py |
| Taskbar pin order | NTUSER.DAT `...\Explorer\Taskband\Favorites` | undocumented list of shell items; order recovered by name (7.3) | S25 S22 (absent on A11) | regf.py + name search |
| Start pins, Windows 11 | `AppData\Local\Packages\Microsoft.Windows.StartMenuExperienceHost_cw5n1h2txyewy\LocalState\start2.bin` | **undocumented binary, not readable by TUR** | S25 A11 (2.4 KB, 22 KB) | no |
| Start pins, Windows 10 generation | NTUSER.DAT `...\CloudStore\Store\Cache\DefaultAccount\*start.tilegrid*` | undocumented binary | S22 | no |
| Start pins, from a running Windows | `Export-StartLayout` | JSON (`pinnedList` of `desktopAppLink` paths and `packagedAppId`s) on Windows 11 and Server 2025; XML on Server 2022 | S25 S22 A11, administrator and restricted; refused to the new standard account | the agent |
| Start menu entries | `ProgramData\Microsoft\Windows\Start Menu\Programs` and `AppData\Roaming\...\Start Menu\Programs` | [MS-SHLLINK] | S25 S22 A11 | lnk.py |
| Packaged (Store) applications | `Program Files\WindowsApps\*\AppxManifest.xml`; per user in `ProgramData\Microsoft\Windows\AppRepository` | XML; a database | live count only (51 on S25) | NOT RUN |
| Installed desktop applications | SOFTWARE `Microsoft\Windows\CurrentVersion\Uninstall\*` (and `WOW6432Node`), NTUSER.DAT the same for per-user installs | keys with DisplayName, DisplayIcon, InstallLocation | S25 S22 A11 | regf.py (count) |
| Drive letters | SYSTEM `MountedDevices`, values named `\DosDevices\X:` | GPT: "DMIO:ID:" + partition GUID (24 bytes); MBR: disk signature + byte offset (12 bytes); otherwise a device path (removable and virtual drives) | S25 S22 A11 | regf.py (byte for byte) |
| Volume labels | inside each file system (NTFS: the `$Volume` file); not in the registry | NTFS attribute | live only | NOT RUN (blkid reads it on Linux) |
| Label overrides | SOFTWARE `...\Explorer\DriveIcons\<letter>\DefaultLabel` | REG_SZ | key present, empty, on all three | regf.py |
| Known folders | NTUSER.DAT `...\Explorer\User Shell Folders` | REG_EXPAND_SZ with `%USERPROFILE%`; the GUID-named values are the newer folders (Downloads `{374DE290-…}`) | S25 S22 A11 | regf.py |
| OneDrive folders | NTUSER.DAT `Software\Microsoft\OneDrive\Accounts\Personal\UserFolder`; redirected known folders point into it | paths | doc (no OneDrive on the runners) | — |
| Default applications | NTUSER.DAT `...\Explorer\FileExts\<.ext>\UserChoice\ProgId`, `Shell\Associations\UrlAssociations\<scheme>\UserChoice\ProgId`; the ProgId's command in SOFTWARE `Classes` | strings; Windows also keeps a hash of the choice, which TUR never writes | S25 S22 A11 | regf.py |
| Regional settings, keyboard layouts | NTUSER.DAT `Control Panel\International`, `Keyboard Layout\Preload` | strings | S25 S22 A11 | regf.py |
| Time zone, clock in UTC | SYSTEM `ControlSet00N\Control\TimeZoneInformation\TimeZoneKeyName`, `RealTimeIsUniversal` | string, DWORD | S25 S22 A11 | regf.py |
| Fast Startup | SYSTEM `ControlSet00N\Control\Session Manager\Power\HiberbootEnabled` | DWORD | S25 S22 A11 (on only on A11) | regf.py |
| The people who use the PC | SOFTWARE `Microsoft\Windows NT\CurrentVersion\ProfileList\<SID>\ProfileImagePath`; their `NTUSER.DAT` | paths | S25 S22 A11 | regf.py (keys) |
| Account picture, Microsoft account, PIN, Hello | SOFTWARE `...\AccountPicture\Users\<SID>`; NTUSER.DAT `Software\Microsoft\IdentityCRL`; `...\LogonUI\NgcPin`; `Windows\ServiceProfiles\LocalService\...\Ngc` | — | doc (absent on the runners; the Ngc folder refused even to the administrator) | — |
| Display layout and scaling | SYSTEM `Control\GraphicsDrivers\Configuration` (administrator only while Windows runs); NTUSER.DAT `Control Panel\Desktop\PerMonitorSettings` | binary and DWORDs | S25 S22 A11 (one monitor, 96 dpi) | — |

### 7.3 Shortcuts, icon positions, pins: what the measurement found

**Shortcuts.** `lnk.py` read every `.lnk` on the desktops, in the pinned
folder and in both Start menu folders without error, on all three
runners, with targets, arguments, working folders and icons (the test
shortcut's `notepad.exe`, its argument, `C:\Windows` and `shell32.dll,23`
came back exactly). Run 43755897776 set each `.lnk` beside what Windows
itself says of the same file (`WScript.Shell`):

| | Server 2025 | Server 2022 | Windows 11 ARM |
|---|---|---|---|
| Desktop: same target | 7 of 7 | 7 of 8 | 6 of 7 |
| Start menu: same target, or neither has one | 156 of 163 | 192 of 209 | 132 of 139 |
| **Together** | **163 of 170 (95.9 %)** | **199 of 217 (91.7 %)** | **138 of 146 (94.5 %)** |
| Same arguments / same working folder | 170 / 170 | 217 / 217 | 146 / 146 |

The differences are of two kinds, both in the reader and both fixable: a
target given only as a shell item list, which Windows resolves to a path
and `lnk.py` does not decode (5, 16 and 8 shortcuts: the ID list's item
format is not part of [MS-SHLLINK]), and a target taken from the
environment block with the quotation marks it was stored with (2, 2 and
0). Advertised shortcuts are agreed on as "no path" (Windows reports the
installer's icon). What the first run showed without Windows' side:
Start menu shortcuts (173, 219 and 145 on Server 2025, Server 2022 and
ARM, all read without an error) come in four kinds: a path (108, 141, 93),
a path with variables (`%windir%\system32\…`, from the environment block:
46, 48, 36, Windows' own tools), a shell item with no path (6, 12, 10,
such as File Explorer's pin) and an installer's "advertised" shortcut whose
target only Windows Installer knows (3, 8, 0); plus `.url` files (10, 10,
6). The last two kinds of `.lnk` are carried by name and resolved by the
agent (section 7.4, rule 3).

**Icon positions.** `IconLayouts` holds the desktop's items by name and a
table of (column, row) cells; its structure was worked out from the three
runners' values (`layout.py` documents it) and, decoded, puts every icon in
the cell the desktop showed it in: 9 of 9 on Server 2025, 10 of 10 on
Server 2022, 9 of 9 on ARM (the Recycle Bin is named by its class
identifier in the value and by a translated name on the screen; it is
paired as the one shell item left over). That was Windows' default arrangement. Moving icons with
`LVM_SETITEMPOSITION` changed the screen but not Explorer's record: after a
refresh they were back. In run 43755897776 one icon was dragged with the
real pointer, five cells right and three down: on Server 2025 it stayed
there after a refresh, and the decoded value put it in column 6, row 3,
the cell the desktop showed (9 of 9 icons in their cells); on Server 2022
column 6, row 4, likewise (10 of 10). On ARM the drag did not reach the
desktop (Windows' first-sign-in window lay over it) and nothing moved.
**Icon positions: read from the disk, the default arrangement and a moved
icon alike.**

**Taskbar pins.** The shortcuts are files (two on Server 2025, three on Server 2022, none on
ARM, which pins by policy) and read by `lnk.py`. Their order lies in
`Taskband\Favorites`, a value nobody outside Microsoft documents; it names
each pin's shortcut file in UTF-16, so the order is recovered by where the
names lie (the first run's reader, which matched names by pattern, caught a
stray character before each). In run 43755897776 every pinned file was
named in the value: Server 2025 File Explorer, then Edge; Server 2022 Edge,
File Explorer, Chrome. That order was not checked against the taskbar
itself (NOT RUN).

**Start pins.** Not readable from the disk: Windows 11 and Server 2025 keep
them in `start2.bin`, an undocumented binary (2.4 KB and 22 KB here), and
Server 2022 in undocumented CloudStore blobs. A running Windows exports
them (`Export-StartLayout`): 8 pins on Server 2025 and 12 on Windows 11
ARM as JSON (`desktopAppLink` paths and `packagedAppId`s), 10 tiles in one
group as XML on Server 2022; for the administrator and the restricted
token, not for a new account that has never opened Start. **Start pins are
carried by the agent at Windows' first start under TUR**, which is H5's
named fallback.

**Wallpaper.** The test wallpaper's path, fit and file (by hash) were read
on all three runners, and Windows' scaled copy (`TranscodedWallpaper`)
exists beside it. One oddity: on Windows 11 the fit read 6 ("fit")
right after the picture was set and 10 ("fill") a few minutes later, in
both runs; something on Windows 11 rewrote it in between (not
identified). The reader carries what the file says when it reads.

**H5: holds.** Kill: the `.lnk` reader agrees with Windows on under 90 % of
shortcut targets (measured: 91.7 to 95.9 %); or a listed item is found only
in a form nobody outside Microsoft can read. One item is: **Start pins**
(`start2.bin`, CloudStore), which are therefore carried by the agent at
Windows' first start under TUR (`Export-StartLayout`), as registered.
Everything else listed (wallpaper and fit, colours, desktop shortcuts with
targets, icon positions, taskbar pins in order, known folders, drive
letters, default applications, regional settings, keyboard layouts, time
zone) was read from the files, and the registry files were shown to hold
what the running Windows reports, value by value, by a reader that asks
Windows nothing. Not measured: volume labels read from the file system on
Linux (they are in each volume's own file system), and the taskbar order
against the taskbar itself.

### 7.4 The carry-over manifest (for `dualrun` and the installer)

What TUR's reader of the Windows installation would hand to the first-run
set-up and to dualrun's launcher, built from section 7.2's sources. It is
the `carry` field of section 6.2's handover file when the Windows program
wrote it, and the same structure when TUR read it from the disk (the disk
wins; section 11, decision 7). A proposal for dualrun to adopt or change;
not a frozen interface, and not yet a schema under `schemas/`.

```json
{
  "carry": 1,
  "read_from": "disk",
  "windows": {"build": "26100", "edition": "Professional", "partition_guid": "51199064-…"},
  "person": {
    "profile": "Users/ann", "sid_known": true,
    "locale": "en-GB", "short_date": "dd/MM/yyyy", "keyboard_layouts": ["00000809"],
    "time_zone": "GMT Standard Time", "clock_in_utc": false
  },
  "look": {
    "wallpaper": {"path": "C:/Users/ann/Pictures/lake.jpg", "exists": true, "sha256": "…",
                  "fit": "fill", "windows_copy": "Users/ann/AppData/Roaming/Microsoft/Windows/Themes/TranscodedWallpaper",
                  "ships_with_windows": false},
    "background_rgb": [0, 0, 0], "accent_rgb": [0, 120, 215], "apps_light": true, "system_light": false
  },
  "desktop": {
    "folder": "C:/Users/ann/Desktop", "public_folder": "C:/Users/Public/Desktop",
    "grid": {"auto_arrange": false, "snap_to_grid": true, "icon_size": 48},
    "items": [
      {"name": "Budget.xlsx", "kind": "file", "column": 0, "row": 1},
      {"name": "Notes.lnk", "kind": "lnk", "target": "C:/Program Files/Notes/notes.exe", "arguments": "",
       "working_dir": "C:/Program Files/Notes", "icon": "C:/Program Files/Notes/notes.exe", "icon_index": 0,
       "column": 0, "row": 2},
      {"name": "::{645FF040-5081-101B-9F08-00AA002F954E}", "kind": "shell", "role": "recycle-bin", "column": 0, "row": 0}
    ]
  },
  "taskbar": [
    {"order": 0, "lnk": "AppData/Roaming/Microsoft/Internet Explorer/Quick Launch/User Pinned/TaskBar/File Explorer.lnk", "role": "file-manager"},
    {"order": 1, "lnk": "…/TaskBar/Notes.lnk", "target": "C:/Program Files/Notes/notes.exe"},
    {"order": 2, "packaged_app": "Microsoft.WindowsCalculator_8wekyb3d8bbwe!App"}
  ],
  "start": {"source": "agent", "pins": [{"desktopAppLink": "%APPDATA%/…/Notes.lnk"}, {"packagedAppId": "…!App"}]},
  "folders": {"Desktop": "%USERPROFILE%/Desktop", "Documents": "%USERPROFILE%/Documents", "Downloads": "%USERPROFILE%/Downloads",
              "Pictures": "%USERPROFILE%/Pictures", "Music": "%USERPROFILE%/Music", "Videos": "%USERPROFILE%/Videos"},
  "defaults": {".pdf": {"progid": "MSEdgePDF", "program": "C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe"},
               "https": {"progid": "ChromeHTML", "program": "C:/Program Files/Google/Chrome/Application/chrome.exe"}},
  "drives": [
    {"letter": "C:", "label": "Windows", "kind": "gpt", "partition_guid": "51199064-…", "present": true},
    {"letter": "D:", "label": "Data", "kind": "mbr", "disk_signature": "23a77acc", "offset": 1048576, "present": true},
    {"letter": "E:", "kind": "device", "present": false}
  ],
  "people": {"with_a_profile": 1}
}
```

Rules the measurements give for using it:

1. **Letters only for volumes that are there.** `MountedDevices` keeps a
   letter for every volume Windows has ever seen; on the ARM runner D: was
   recorded and absent (section 7.1). Match each GPT letter by partition
   GUID and each MBR letter by disk signature and offset against the
   partitions present, and carry only those.
2. **Icon positions are grid cells, not pixels.** The layout stores a
   column and a row per item; TUR places them on its own grid. When TUR's
   grid has fewer rows (another resolution, another scale), fill
   column-wise as Windows does.
3. **Shortcuts are carried as what they point to.** A target that is a
   Windows program becomes a launcher entry that starts it in the guest; a
   target that is a file or folder on a Windows volume becomes TUR's own
   link to the same path; shortcuts with no path (a shell item only, an
   installer's advertised shortcut) are carried by name and resolved by the
   agent at first start.
4. **Default applications are shown, not written.** Windows protects each
   choice with a hash TUR must not forge; TUR uses them to decide which
   side opens a file type, and never writes them back.
5. **Nothing of Microsoft's is copied.** A wallpaper that ships with
   Windows (`ships_with_windows`: under `Windows\Web`) is shown from the
   Windows volume in place; the person's own picture likewise; neither is
   copied into TUR's image or sent anywhere.

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
second password. (Desktop icon positions are no longer a candidate for
cutting: they are grid cells in a value decoded in section 7.3.) Start
pins read from the disk: the agent reads them instead (7.3). Android and Mac applications *inside* the Windows
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
   and are gone. One exception, found and removed: item 10.
10. **The ARM runner's screen was covered.** On `windows-11-arm`,
    Windows 11's first-sign-in privacy window lay over the whole screen for
    the entire run 43552339780: every picture taken there, and every check
    that compared pictures, saw that window. The counts of section 3.1 are
    not affected (they come from the reads, not the pictures). Pictures of
    it had gone into the artifact meant for open-source applications only;
    that artifact (`inwin-see-windows-11-arm` of run 43552339780) was
    deleted once its counts had been copied, because it held Microsoft's
    text. Run 43755897776 hid the window's proxy, which did not remove it:
    its pictures on ARM again show that window, but they went only to the
    one-day folder, because each picture is now kept as an open-source one
    only when the window pictured is the one on top.
11. **Run 43552339780's rights were not recorded.** Its probe of the
    process token failed at every level (a buffer of the wrong size), so
    that run does not itself show which rights each level had; that the
    lower levels were lower shows in what they were refused. Run
    43755897776 records them; it also showed that `runas /trustlevel` keeps
    high integrity (section 6.1).
12. **Microsoft's applications were not counted by eye in run
    43552339780.** Their pictures were kept one day and had expired before
    the run was read. Run 43755897776's were counted within their day
    (section 3.1), on the x64 runners only.

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
> public accessibility interface (UI Automation) and the window tree,
> performs only what TUR sent, and reports windows and events through
> Windows' own event hooks. Everything it sends is data from the less
> trusted side: it is never an instruction, never the basis of a decision
> about TUR itself, and never decides where TUR's own confirmation surfaces
> are drawn.
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
> applied.* Wallpaper and its fit, colours, light or dark, desktop
> shortcuts and where they lie, taskbar pins in their order, drive letters
> and labels, known folders, default applications and regional settings
> are read from the Windows installation's own files, stay on the PC, and
> are listed for the person to accept, item by item, before anything is
> applied. What the files do not give (Start pins on Windows 11) is read
> by the agent at Windows' first start under TUR and shown the same way. No
> Microsoft artwork is copied into TUR's image or sent anywhere; the
> person's own files are referenced where they lie.

**Defaults proposed under the owner's "I trust your choices"** (each is
what TUR does unless the owner says otherwise; one stays the owner's).

| # | Decision | Default proposed | Why (measured where marked) |
|---|---|---|---|
| 1 | The text above | Adopt it | H1 to H5 hold on real Windows (sections 3, 4, 6.1, 7) |
| 2 | **Code-signing identity for Windows** | **Owner action, not defaulted.** Until then: the "get TUR" program and the agent are unsigned previews; P@x cannot see inside or operate a window the person started as administrator (section 3.3, measured) | A certificate binds a legal name to TUR and costs money every year; only the owner can choose the name |
| 3 | What the Windows program may do after consent | The checker only reads, with no consent prompt, until the person agrees; on BitLocker it says "not known yet" rather than guess (section 6.1). After one consent prompt it does, through Windows' own tools, the steps the person was shown: Fast Startup off, Windows' own shrink, BitLocker suspended for one restart. Where anything is unusual (BitLocker on without the recovery key saved, a volume that needs checking, under 41 GiB free) it only explains and stops | The reading needs no rights for everything that decides (section 6.1); the doing is what Windows' tools do best themselves |
| 4 | Staging without a USB stick | Not in the first version: a USB stick | It changes the EFI partition from Windows; it needs its own experiment |
| 5 | A driver added to the person's Windows | None at first: messages on the serial port dualrun already uses, pixels on the private link; virtio serial offered later, as an option the person approves | dualrun section 4.2; section 5's sizes fit a serial line for messages, not for pixels |
| 6 | What is carried over in the first version | Wallpaper and fit, background and accent colours, light or dark, desktop shortcuts and their grid positions, taskbar pins and their order, known folders, drive letters and labels, default applications (shown, applied only for file types that have a TUR or Windows-guest handler), regional settings, keyboard layouts, time zone. Start pins on Windows 11 by the agent at first start. Not carried: lock screen, sounds, cursors, theme files, Microsoft's own wallpapers as copies (shown in place from the Windows volume) | Every item in the first group was read from the files without Windows (section 7) |
| 7 | Who reads the carry-over data | The disk, by TUR's installer and again by TUR at each start, wins; the agent adds only what the disk does not give | The disk reading needs no trust in anything that ran on Windows |
| 8 | The licence question | Stays with the owner's lawyer (dualrun H3) | — |
