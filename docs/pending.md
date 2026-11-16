<!-- SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0 -->
# What is left to do

The single list of everything TUR still has to finish, as of 16 November 2026.
It is kept up to date as work lands: when an item is done, it moves to the
"Done" log at the end with the pull request that did it.

How to read it: **built** means the code is written; **proven** means it ran
and passed in CI or on hardware. Most items below are built but not yet
proven, because CI stopped (see "The blocker").

## The blocker

**GitHub Actions is disabled for the owner's account** since 16 November
2026, 01:38 UTC ("Actions has been disabled for this user", HTTP 422), most
likely an automated flag after the project ran very heavy CI (dozens of
parallel runs, long VM jobs, large downloads) in three days. Nothing can be
built or tested in CI until it is restored.

- [ ] Owner: get a human review at GitHub (reply to the closed ticket; the
      appeal form; a Community Discussions post; or one month of GitHub Pro,
      about $4, for real ticket support).
- [ ] When restored: run CI at a safe pace (one or two agents at a time,
      heavy jobs only on manual start, no long background experiments).
- Not chosen for cost: a rented build server (about €90–260/month) or a new
  laptop. ARM (Oracle's free tier) considered and deferred.

## Alpha 1: the build the owner installs (issue #134)

The owner will test only when it is done, on a **cloned disk** of his Windows
10 PC (Lenovo IdeaPad L340-17IRH: i5-9300H, 8 GB, GTX 1650). After he installs
it, the lead troubleshoots on that machine.

| Item | Pull request | State |
|---|---|---|
| Install TUR beside Windows on the same disk (free space), default system choice, Restart into Windows, uninstall, no drive letters for TUR's partitions | #112 | built; install job **proven** on a test disk; power-cut job found one window (fallback loader) — fix built, not proven |
| Offer shrinking Windows from the installer (refuses BitLocker, Fast Startup, dirty, too full) | #116 | built, never run |
| Desktop programs in the image (app centre, Settings, P@x window, P@x indicator) | #128 | image builds **proven**; boot pictures fix built, not proven |
| Windows applications on the installed system (Fedora's Wine, launcher entries) | #136 | image **proven** (+0.45 GB compressed); apps on the booted image not yet seen |
| P@x operates Windows applications (the bridge, tur-runtime, tur-pax-a11y; Qwen3-VL 4B as P@x's vision model) | #141 | built, never run |
| P@x companion on the bottom bar (the flame; never takes focus; steps aside over full screen) | #139 | 61 of 62 tests and KWin checks **proven**; 3 fixes built, not proven |
| P@x voice: push-to-talk, spoken answers, live conversation (local free tier first; OpenAI Realtime, Gemini Live) | #145 | built, never run |
| Self-healing: a notice for every action, history, hand-off to the companion | #117 | **proven** before main moved; rebased, rust not proven |
| P@x policy store and the two levels of confirmation | #113 | **proven** before main moved; rebased, not proven |
| Android runtime (tur-android, Waydroid, off until switched on) | #137 | first boot run: switch-on failed on an SELinux label — fix built, not proven |
| Re-hosting route (tur-rehost: Windows and Mac apps on their native runtimes) | #146 | built, never run |
| macOS route in Darling, installed by default and updated with the image | #147 | built, never run; first publish blocked until Darling's licences are read |
| Problem reports end to end ("report this", the person sends, one-command receiver, fixes come back) | #144 | built, never run |
| WinSCP launch fix in the Windows tests | #135 | built, never run |
| Your own Windows started inside TUR, protected, on one desktop | #131 (experiment) | 31 of 31 safety cases **proven**; real Windows never started yet (TPM fix built) |
| Identity carried over from Windows (wallpaper, icons, pins, folders, C: stays C:) | #132 (experiment) and #131 | reading **proven** on real Windows; carrying over not built |
| Signed updates published and received | main | release path **proven** in rehearsal; first real publication never happened (main never green at the right moment) |
| One commit, everything green; the installer built from it; known-limits page; trial procedure | — | not started |

## Breakthroughs

Confirmed (details in [breakthroughs.md](breakthroughs.md)):

1. Windows USB drivers, including KMDF, run as sandboxed programs, never in the kernel.
2. Driver synthesis: a Linux driver written blind from recordings passed 18 of 18.
3. P@x reads and operates Windows applications: 172 of 172 controls, 37 of 37 actions; vision fallback up to 97%.
4. Re-hosting: 20 of 24 Electron apps from Windows packages and 20 of 24 from macOS packages.

Still open (hypotheses on [frontier.md](frontier.md)):

| Idea | Where | State |
|---|---|---|
| Your own Windows inside TUR (F7/F8) | #131, #132 | in progress; see Alpha 1 |
| A Windows terminal judged by real Windows (F2) | #125 | harness fixed; scorecard never produced |
| A compatibility layer that repairs itself (F3) | #124 | first run failed in setup; never measured |
| Layered Windows (one shared base, a layer per app) | #133 | first run failed in setup; never measured |
| An AI-built Mac compatibility layer on Darling (F9) | merged plan (#143) | planned; nothing run; odds ~35% for a simple AppKit form app |
| Driver synthesis, harder round (stage C) | merged (#122) | proposed |
| Android by translation (ATL) | merged (#120) | measured: 13 of 60; kept as an optional route |
| A universal bus between ecosystems; speed against Windows | frontier "next in line" | not started |

## Decisions and actions waiting for the owner

- [ ] GitHub Actions (above).
- [ ] Problem-report receiver: a small server (Nanode, about $5/month), a subdomain, a private repo, a fine-grained token — only after CI is green and a privacy review (#144, #75).
- [ ] Make the update package `ghcr.io/giorgiogiudice/tur` public after the first publication; later the Darling package too.
- [ ] A code-signing identity for TUR's Windows helper (needed to operate elevated windows inside Windows).
- [ ] A Windows-only USB device for the real driver test (optional).
- [ ] Lawyer's list: licence (noncommercial), branding/credit line, Windows evaluation on hosted runners, running the person's Windows as a guest, re-hosting proprietary apps and Electron's codecs, Darling's APSL code, microG, reverse engineering for driver synthesis, problem-report privacy statement.
- [ ] `DEEPSEEK_KEY` secret (optional) to test P@x with DeepSeek.

## Smaller follow-ups

Tracked as GitHub issues: #12, #14, #16, #19, #21, #24, #28, #30, #32, #33,
#38, #40, #41, #43, #46, #48, #51, #57, #59, #62, #64, #66, #67, #72, #74, #75,
#76, #77, #91, #92, #95, #97, #98, #99, #103, #106, #130, and the vision issue
#2.

## Done log

Recent, newest first: #143 Mac layer plan · #142 re-hosting on the README ·
#123 re-hosting experiment · #140, #138 breakthroughs on the README · #126
P@x on Windows apps · #122 driver synthesis · #120 Android experiment · #121
WinSCP · #111 mirror every pinned image · #110 P@x grants and lists · #115
release never waits on a runner · #108 self-healing · #107 local AI and app
installs in the image · #105/#118 release key · #104 breakthroughs page ·
#102 P@x window · #96 KMDF drivers · #78 signed updates.
