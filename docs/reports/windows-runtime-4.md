# windows-runtime-4 — WinSCP on TUR's Windows runtime, measured

```text
TUR DEVELOPMENT REPORT

Task ID:          windows-runtime-4
Subsystem:        Universal Runtime Manager (services/tur-runtime), tested
                  through tests/windows-runtime
Build ID:         not applicable: this task builds no image
Source revision:  f37001f (feat/windows-runtime-winscp, pull request 121),
                  the commit the measurements below come from. The commit
                  that adds this report changes only the window-title
                  pattern (see "Runs") and adds this file.

SUMMARY
- What changed: WinSCP 6.7.1 is a sixth real application of the
  windows-runtime workflow, as two packages: its Inno Setup installer and
  its portable zip. Each is installed with `tur-runtime app install`,
  started from its generated launcher entry, used for a real SFTP file
  transfer through WinSCP's console program, and removed.
- Why it changed: the owner asked whether WinSCP can be installed on TUR.

FILES
- Added: docs/reports/windows-runtime-4.md
- Modified: tests/windows-runtime/apps.json (two entries),
  tests/windows-runtime/apps.py (the check "winscp-sftp"),
  .github/workflows/windows-runtime.yml (openssh-server, openssh-client and
  putty-tools in the two jobs that run apps.json)
- Removed: none

BUILD
- Command: cargo build --locked (workflow "windows-runtime"); no Rust
  source changed
- Result: pass
- Warnings: none

TESTS
- Unit tests: none changed.
- Integration tests: job "Install and run real applications under Wine",
  ten installations, all four stages of each passed; job "Real
  applications with and without case-insensitive prefixes", the same ten
  four times each, all passed. Run 43535838684.
- Compatibility autopilot: NOT RUN on WinSCP. Nothing failed, and the
  autopilot is for failures.
- Boot tests: NOT RUN. Nothing here can affect boot.
- Hardware tests: NOT RUN. No graphics card, no real display.
- Performance tests: NOT RUN. Times are wall-clock on a shared runner.
- On the TUR image: NOT RUN. Wine is not in the image.
- Real remote servers: NOT RUN. The SFTP server was OpenSSH on the
  runner's loopback interface.
- WinSCP's graphical file transfer, including drag and drop: NOT RUN. The
  window was opened and closed; the transfer used the console program.

DIAGNOSTICS
- Log paths: artifact "windows-runtime-applications" of run 43535838684,
  apps/winscp-inno/ and apps/winscp-zip/: identify.json, install.json,
  application.json, profile.json, launcher.desktop, launch.png, launch.txt,
  winscp-output.txt (the console program's output), winscp.log (WinSCP's
  session log), sshd.log, uninstall.json, logs/. Artifact
  "windows-runtime-casefold" for the four further passes.
- Diagnostic bundle path: none
- Relevant error IDs: none produced

RECOVERY
- Rollback procedure: revert the merge. Only the test, its data and the
  workflow change.
- Recovery tested: removal checked to leave nothing, for both packages, in
  five runs each.

KNOWN LIMITATIONS
- See "What was not tested".

NEXT SAFE ACTION
- Run the same two packages on the TUR image once it carries Wine.
```

## Answer

**Yes, on Ubuntu's Wine 9.0 inside tur-runtime's sandbox, both WinSCP
packages install unattended, open their window, transfer files over SFTP
and uninstall without leaving anything.** That was measured five times for
each package, on one runner. It is not a measurement on the TUR image,
which has no Wine yet.

## Runs

| Commit | Workflow | Run | Result |
|---|---|---|---|
| `f37001f` | windows-runtime | 43535838684 | Passed, all four jobs. Applications job (job 132264499101): ten installations, 40 of 40 stages. Case-folding job (job 132264498891): the same ten in four passes, all passed. Autopilot job: passed (it does not include WinSCP). **This report is written from this run** |
| `f37001f` | gate | 43535838655 | Passed |

The window-title pattern in `apps.json` was `Login|WinSCP` for that run,
because the title was not known beforehand. The window it matched was
titled exactly `Login`, so the commit that adds this report tightens it to
`^Login$`. That commit's runs are named in the pull request.

## Where this ran

```text
runner image: ubuntu24 20261111.370.1
os: Ubuntu 24.04.5 LTS
kernel: 7.0.0-1017-azure
wine: wine-9.0 (Ubuntu 9.0~repack-4build3)
bubblewrap: bubblewrap 0.9.0
openssh: OpenSSH_9.6p1 Ubuntu-3ubuntu13.20, OpenSSL 3.0.13 30 Jan 2024
puttygen: Release 0.81 (putty-tools 0.81-1)
```

**This is Ubuntu's Wine on a GitHub-hosted runner, not the TUR image.** No
graphics card: an X server without a screen (Xvfb, 1280 by 800) with
Openbox, drawn in software. No Microsoft component, redistributable
runtime or winetricks verb was installed.

## The application, its licence and where it came from

Read on 14 November 2026 on WinSCP's own site.

| | |
|---|---|
| Version | 6.7.1, published 2026-11-11 (`https://winscp.net/eng/downloads.php`) |
| Licence | GPL-3.0-or-later (`https://winscp.net/eng/docs/license`, and `license.txt` in the zip). The icon set is not under the GPL: it may not be reused or redistributed apart from WinSCP |
| Installer | `WinSCP-6.7.1-Setup.exe`, 13,176,776 bytes, SHA-256 `95ea85587c30ac6dd0c4b8148e16971f7854a6eee9e68298bf007566ebbe7603` |
| Portable | `WinSCP-6.7.1-Portable.zip`, 8,833,106 bytes, SHA-256 `e6d2f8e1249e41f3bf8638319bb9bd15a541a64720df0805cb0bebdedb5eda83`; holds `WinSCP.exe`, `WinSCP.com`, `license.txt`, `readme.txt` |
| Hashes from | The publisher's release notes, `https://winscp.net/download/WinSCP-6.7.1-ReadMe.txt` |
| Downloaded from | `https://downloads.sourceforge.net/project/winscp/WinSCP/6.7.1/<file>`, the project's SourceForge files |

The publisher's download page links to its own CDN with a token that
expires, so that link cannot be pinned. The same page offers the
project's SourceForge files as the alternative, and the test uses those.
The workflow refuses a file whose size or SHA-256 is not the pinned one.
Nothing is committed or redistributed.

## The measurements

From run 43535838684, job "Install and run real applications under Wine":

| Package | Installer kind | Install | Launch | Check | Uninstall |
|---|---|---|---|---|---|
| WinSCP 6.7.1 | inno-setup (a 32-bit program) | pass (17.4 s, 9.4 of it creating the prefix) | pass (window after 5.1 s) | pass (2.8 s) | pass (5.8 s): its own uninstaller ran and succeeded |
| WinSCP 6.7.1, portable | portable-zip | pass (10.2 s) | pass (window after 5.1 s) | pass (2.6 s) | pass (0.1 s): no uninstaller, the prefix was deleted |

The same two packages in the case-folding job, four passes (case-folded,
not, not, case-folded), all stages passed:

| Package | Mode | Install (s) | First window (s) | Check | Uninstall (s) | Left behind |
|---|---|---|---|---|---|---|
| Installer | case-folded | 20.7 / 20.2 | 3.8 / 3.8 | pass / pass | 4.9 / 5.5 | 0 / 0 |
| Installer | not case-folded | 18.2 / 17.6 | 4.1 / 3.8 | pass / pass | 5.4 / 4.9 | 0 / 0 |
| Portable | case-folded | 10.7 / 14.6 | 3.8 / 4.1 | pass / pass | 0.1 / 0.1 | 0 / 0 |
| Portable | not case-folded | 13.7 / 11.4 | 4.4 / 3.8 | pass / pass | 0.1 / 0.1 | 0 / 0 |

What each stage required is in windows-runtime-2, "The compatibility
table". In short:

- **Install**: `tur-runtime app install <file>` alone, unattended (for
  Inno Setup: `/VERYSILENT /SUPPRESSMSGBOXES /NORESTART /SP-`), in the
  sandbox. The main program had to be `WinSCP.exe`, and it was, by rule:
  "the only program a shortcut points to ("WinSCP")" for the installer,
  "the only program that was installed" for the zip. The installer put it
  in `C:\Program Files (x86)\WinSCP\`.
- **Launch**: the generated launcher entry's command, until a window of
  class `winscp.exe` with the expected title is viewable.
- **Check**: below.
- **Uninstall**: `tur-runtime app uninstall`. For the installer, WinSCP's
  own `unins000.exe` ran with exit status 0 and left no program and no
  uninstall entry in the prefix. Then nothing of either package was left
  under the runtime's data or state, no Wine server ran and no window
  stayed open. Nothing appeared in the runner's home directory in the
  places Wine or an installer would write without a sandbox.

### The check: a real SFTP transfer

1. **The server.** Outside the sandbox, the test started OpenSSH's `sshd`
   as the runner's own user on 127.0.0.1, on a free port, with a throwaway
   Ed25519 host key and a throwaway client key. It accepted public-key
   login only and served a temporary directory. PuTTY's `puttygen`
   converted the client key to the `.ppk` format WinSCP reads. The key was
   in the granted directory so that the sandboxed program could read it,
   and it was deleted afterwards. It is not in the artifact.
2. **A control.** OpenSSH's own `sftp` client listed the directory first,
   so that a failure afterwards would be WinSCP's.
3. **WinSCP.** `WinSCP.com /ini=nul /nointeractiveinput /log=… /script=…`
   ran in the sandbox through `tur-runtime launch`. The script opened
   `sftp://runner@127.0.0.1:<port>/`, pinned the host key by its SHA-256
   fingerprint and used the key file. It uploaded a 1,035,000-byte file
   from the granted directory, downloaded it back under another name, and
   downloaded a 300,000-byte file the test had made on the server.
4. **Verified outside WinSCP.** The test computed SHA-256 on the runner.
   The uploaded copy on the server, the copy downloaded back and the
   server file downloaded all matched their originals. `sshd` had to log
   two public-key logins (the control and WinSCP). It logged exactly two,
   each followed by `Starting session: subsystem 'sftp'`.

WinSCP's own output, from `apps/winscp-inno/winscp-output.txt`:

```text
Authenticating with public key "tur-test-client".
Authenticated.
Starting the session...
Session started.
Active session: [1] runner@127.0.0.1
Z:\home\runner\work\_temp\tur-apps\run\shared\winscp-inno\upload.bin |        1010 KB | 51514.4 KB/s | binary | 100%
uploaded.bin              |        1010 KB | 40781.6 KB/s | binary | 100%
from-server.bin           |         292 KB | 43497.4 KB/s | binary | 100%
-rw-r--r--   1 runner   runner     1035000 Nov 14  1:54:10 2026 uploaded.bin
-rw-r--r--   1 runner   runner      300000 Nov 14  1:54:10 2026 from-server.bin
```

The portable package's output is the same, apart from the port and the
times.

**The network.** Nothing was granted for this test. tur-runtime shares the
host's network with a prefix by default
([runtime-manager.md](../architecture/runtime-manager.md), "The network";
`--no-network` removes it). The check read each prefix's `metadata.json`,
and it recorded `"network": true` for both packages. Only the loopback
interface was used on purpose.

## The screenshot

`apps/winscp-inno/launch.png` and `apps/winscp-zip/launch.png` in the
artifact "windows-runtime-applications" of run 43535838684. Both were
looked at, and they show the same thing:

- WinSCP's main window, titled "runner – WinSCP", in its Commander layout:
  menus, toolbars, a local tab "runner – runner", a "New Tab" tab and two
  file panels.
- In front of it, the "Login" dialog: "New Site", the file protocol set to
  SFTP, empty host name, user name and password fields, port 22, and the
  Save, Advanced, Tools, Manage, Login, Close and Help buttons. Everything
  is drawn whole and is readable.
- The local panel shows `Z:\home\runner\` with one folder, `work`, which is
  how the sandbox shows the home directory. What else the sandbox shows of
  it was not examined here.

Both windows have class `winscp.exe`: the dialog is 652 by 396 pixels, the
main window 1082 by 666. Both closed when asked.

**The screenshots are not committed.** WinSCP's licence page says the
images distributed with WinSCP are not under the GPL, and may not be
reused or redistributed apart from WinSCP. A screenshot of its window
contains them. They stay in the run's artifact, which GitHub keeps for the
repository's retention period.

## What the runs showed

1. **Both packages worked at every stage, in all five runs.** Results did
   not change between runs, and case folding made no difference.
2. **WinSCP opens two windows on start**: its main window and the Login
   dialog over it. The test measured the dialog, and the window class
   matched the generated launcher entry.
3. **The installer is a 32-bit program, and it installs the 32-bit
   WinSCP.** Wine's 32-bit half (`wine32:i386`) was installed in the job,
   as it is for the other 32-bit installers. The portable zip holds the
   same 32-bit program.
4. **The installer made two shortcuts and installed four programs.** The
   main program was chosen by rule, without a hint. The names in the
   records came from the files: "WinSCP 6.7.1" from the installer,
   "WinSCP" (version 6.7.1.0) from the zip. Both icons are the same
   256-pixel PNG; the test kept only its digest.
5. **`tur-runtime installer identify` counts only `.exe` members in a
   portable zip.** It reported "1 member named like a Windows program" and
   did not count `WinSCP.com`. The console program was still unpacked
   beside `WinSCP.exe`, and the check used it.
6. **Wine printed errors that did not matter here**: that it could not run
   its menu builder (TUR switches it off on purpose, as windows-runtime-2
   notes), and that `ntlm_auth` is missing (NTLM authentication, which
   SFTP with keys does not use).

## The compatibility autopilot

NOT RUN on WinSCP. No stage failed, and the autopilot exists to find fixes
for failures. The autopilot job in the same run passed, but WinSCP is not
in its list of real applications.

## What was not tested

| Not tested | Why |
|---|---|
| The TUR image | Wine is not in the image. Ubuntu 24.04 with Ubuntu's Wine 9.0 |
| A graphics card | The runner has none; everything was drawn in software |
| Real remote servers | Only OpenSSH on the runner's own loopback interface. Not tested: other servers, the internet, SCP, FTP, FTPS, WebDAV, S3, host-key prompts, password and keyboard-interactive login, Pageant, proxies |
| The graphical interface: drag and drop, the panels, the Login dialog used to connect | The window was opened and closed. All transfers used the console program `WinSCP.com` |
| Drag and drop out of WinSCP into a desktop, and its shell extension | Not tried. The installer installs the extension, and nothing exercised it |
| WinSCP's settings kept between runs | The check ran with `/ini=nul`, so no configuration was read or written |
| Paths outside ASCII, large files, many files, resuming, synchronisation | One file of about 1 MB each way, and one of 300 kB |
| The 64-bit "Experimental" portable package, and the MSI | Not chosen. The task's two packages were the installer and the portable zip |

## Catalogue

No catalogue entry was added. Of the five applications already measured,
four have one and Inno Setup does not, so the condition for adding one
("only if the existing five have one") is not met. An entry would also
change the application centre's recorded fixtures
(`desktop/apps/tur-centre/fixtures/*/validate.json`). If the owner wants
one, it would take the form of PuTTY's entry: rating `unknown`,
`reported_by = "tur-test"`, `test_report` this file, because
`catalogue/README.md` still says TUR has rated nothing.

## Decisions the owner may want to reverse

- **The download comes from SourceForge**, not from winscp.net's own CDN,
  because the CDN's links expire. The digest still comes from winscp.net.
- **The screenshots are not committed**, because of the icon-set terms. A
  reader of this report has to open the artifact to see them.
- **The check pins a host key and uses a key file**, as a script would.
  The interactive path (the Login dialog, accepting a new host key) was
  not driven.
