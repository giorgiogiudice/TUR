# pax-2 — P@x integration: the seams between the components

```text
TUR DEVELOPMENT REPORT

Task ID:          pax-2
Subsystem:        AI control layer: provider adapters, policy engine,
                  orchestrator, memory; and the assistant tools of storage,
                  the device manager, diagnostics, the catalogue and the
                  runtime manager
Build ID:         not applicable: this task builds no image
Source revision:  9fc6e6af4985 (fix/pax-integration, pull request 42), which is
                  the commit the measurements below come from. The commit
                  that adds this report follows it and changes no code.

SUMMARY
- What changed: six seams that connecting P@x's parts had exposed (issues
  40 and 33) are closed, one commit each.
  1. The provider adapters read tool-call arguments strictly. A repeated
     key at any depth makes the response malformed; the argument text
     comes with the finished call. The task loop no longer rebuilds that
     text from streamed fragments, so a call whose arguments arrive in one
     piece (the Google adapter) runs. The loop is now run through all
     three adapters.
  2. The policy engine follows sections 18 to 21 of the AI design: a
     standing approval reaches it only from a policy store, and none
     exists; proposing a schedule is class 1 and creating one is the
     user's act; an unattended task records only observed memory;
     changing a stated memory entry is class 2; "trusted path available"
     is "trusted session proven"; the action-outcome and output-withheld
     events are types of the audit module. The built-in application
     identifiers and unit names were checked against upstream sources.
  3. Every component ships its own assistant tools: agent-tools.json and
     agent-tools.adapter.json beside its code, loaded by tur-pax from one
     directory of tool sets, and tested in the component against the
     program it builds.
  4. tur-storage can ask about the live system (lsblk, nothing else) and
     has one spelling for access; tur-device has list and get. Three more
     tools are offered: device_list, device_get, storage_preview_mount.
  5. tur-pax-memory can turn memory on and record what the user states,
     and the end-to-end test runs "an instruction recalled from memory
     changes no decision" with a store prepared that way.
  6. tur-core has RUNTIME and CATALOGUE families. The runtime manager's
     ten codes that were parked under DESKTOP moved; the catalogue's did
     not, because the application centre's fixtures hold them.
- Why it changed: the parts were built separately; the orchestrator
  (pax-1) recorded where they did not fit.
- What it does not show: NO REAL MODEL WAS CALLED, here or anywhere in
  the development of this change. Every provider is a script. The three
  wire formats the scripts speak are written by hand from the providers'
  documentation; none is a recording. Nothing here says that a real
  OpenAI or Google response is read correctly.

FILES
- Added: services/tur-pax-providers/src/strict.rs,
  services/tur-pax/src/store.rs, services/tur-pax/tests/wires.rs,
  services/{tur-catalogue,tur-device-manager,tur-diagnostics,tur-runtime,
  tur-storage}/tests/agent_tools.rs,
  services/{tur-catalogue,tur-diagnostics,tur-runtime}/agent-tools.json
  (moved from services/tur-pax/tools/manifests),
  services/{tur-catalogue,tur-device-manager,tur-diagnostics,tur-runtime,
  tur-storage}/agent-tools.adapter.json (moved from
  services/tur-pax/tools/adapters),
  tests/pax/scenarios/11 to 17 (seven scenarios),
  docs/reports/pax-2.md
- Modified: services/tur-pax-providers (toolcalls, provider, model,
  anthropic, google, lib, conformance scripts and tests);
  services/tur-pax (strict, orchestrator, context, engine, action,
  decision, policy, audit, trail, cli, registry, adapter, textui, lib and
  their tests); services/tur-pax-memory (cli, main, tests);
  services/tur-storage (probe, cli, lib, tests);
  services/tur-device-manager (main, tests); services/tur-runtime (codes,
  the code strings in sources and tests); services/tur-catalogue (the note
  in codes.rs, one test); crates/tur-core (two families);
  the Cargo.toml and Cargo.lock of the five tool components (a development
  dependency on tur-pax); schemas/pax-tool-adapter.schema.json;
  tests/pax/{e2e.py,fake_provider.py}, scenarios 02 and 07;
  .github/workflows/pax.yml; docs/architecture/{pax-providers,
  pax-orchestrator,agent-policy,pax-memory-and-schedules,storage,
  device-manager,runtime-manager,catalogue-and-routing}.md
- Removed: services/tur-pax/tools/

BUILD
- Command: cargo clippy --all-targets --all-features -- -D warnings; cargo
  test --all-targets --all-features; cargo test --doc (workflow "rust",
  every component); cargo build --locked for tur-pax, tur-pax-memory and
  the five tool components (workflow "pax"); cargo build --locked for
  tur-runtime (workflow "windows-runtime")
- Result: see RUNS below
- Warnings: none from the compiler or clippy

TESTS
- Unit and integration tests, workflow "rust", all components: pass.
  services/tur-pax: 169 unit tests and 144 integration tests in nine
  files, among them the invariant sweep (41 tests over 738 432 decisions
  each) and the new tests/wires.rs (5 tests, each over three wire formats
  and two ways of delivering arguments).
  services/tur-pax-providers: the conformance suite is 22 tests for each
  of the three adapters.
  The five tool components: tests/agent_tools.rs in each.
- End to end: workflow "pax", 349 checks in 17 scenarios, on a
  hosted Ubuntu 24.04 runner with bubblewrap 0.9.0. Pass. Detail below.
- Against a real provider: NOT RUN. No key exists.
- The live probe of tur-storage on real hardware: NOT RUN. It ran on the
  CI runner, inside P@x's sandbox, where lsblk lists the runner's disks.
- tur-device list and get on real hardware: NOT RUN (the same runner).
- On the TUR image: NOT RUN. The runner is Ubuntu.
- Boot tests: NOT RUN. Nothing here can affect boot.
- The desktop workflow: NOT RUN by this change; no file it watches
  changed. The catalogue's JSON output is unchanged, and
  services/tur-catalogue/tests/desktop_fixtures.rs passes.
- Hardware tests, performance tests: NOT RUN.

DIAGNOSTICS
- Log paths: GitHub Actions runs below; artifact "pax-evidence"
- Diagnostic bundle path: none
- Relevant error IDs: new: TUR-DEVICE-INPUT-002. Renamed:
  TUR-DESKTOP-RUNTIME-001 to -006 are TUR-RUNTIME-MANAGER-001 to -006;
  TUR-DESKTOP-PROFILE-001 to -004 are TUR-RUNTIME-PROFILE-001 to -004.
  New audit event: output_withheld. New decision reason:
  unattended_memory_not_observed.

RECOVERY
- Rollback procedure: revert the merge. No image, no boot path and no
  stored data format is touched. The audit file gains an event kind; a
  reader of the old format that ignores unknown events is unaffected.
- Recovery tested: not applicable.

KNOWN LIMITATIONS
- See "What is not done" below, and the known gaps of
  docs/architecture/pax-orchestrator.md (section 9) and
  docs/architecture/agent-policy.md (section 11).

NEXT SAFE ACTION
- A conformance run of the loop against the three real providers, with a
  key, outside CI (issues 24 and 40). Then the policy store and the
  trusted session, which are what stand between this and anything above
  class 1.
```

## Runs

For commit `9fc6e6af4985`:

| Workflow | Run | Result |
|---|---|---|
| `rust` | 43216264535 | Pass: every component |
| `pax` | 43216264604 | Pass: 349 checks, 0 failed |
| `windows-runtime` | 43216264546 | Pass (it builds `tur-runtime`, whose lockfile and code strings changed) |

Earlier runs on the branch, for the record:

| Commit | `rust` | `pax` | `windows-runtime` | What happened |
|---|---|---|---|---|
| `c69c0dc` fix 1 | 43208533180 | 43208533308 | – | `rust`: compiled, every test passed, failed only at formatting. `pax`: two checks of the new scenarios failed, both a mistake in the test (it expected the untrusted-data notice on a refusal, which is P@x's own text). |
| `40f399f` | 43208939085 | 43208939036 | – | Both pass. Fix 1 green. |
| `c436cac` fix 2 | 43211125120 | 43211125098 | – | `rust`: one type error in a test. `pax` passes. |
| `4407601` | 43211330268 | 43211330063 | – | `rust`: one clippy finding in a new test. |
| `e2d9d76` fix 3 | 43212109250 | 43212109263 | 43212109343 | The five components' lockfiles lacked the new development dependency, so the two workflows that build with `--locked` stopped. `rust`: two wrong expectations in new unit tests of `tur-pax`. The five components' own tool tests passed on their first run. |
| `81af277` | 43212561804 | 43212561849 | 43212561813 | `rust`: one of those two expectations again. `pax` and `windows-runtime` pass. |
| `8c5b62c` fix 4 | 43213786682 | 43213787468 | 43213786662 | `rust`: formatting, and `tur-storage`'s source check that no file starts a program, which the live probe now does on purpose; the check was rewritten to say exactly what holds. `pax` passes, with the new command-line forms run in the sandbox. |
| `0a83b50` fix 5 | 43214751976 | 43214752043 | 43214751963 | `rust`: formatting only. `pax` passes, with the memory scenario. |
| `ed61fb6` fix 6 | 43215444535 | 43215444457 | 43215444432 | `rust`: formatting only, in `tur-core` and `tur-catalogue`. The other two pass. |

Eleven pushes in all, the one that adds this report included. Every failed
run needed the lockfile or the formatting that CI computes, because no
development machine has a toolchain; for three of them that was all. The
other failures were in new tests: a type error, a clippy finding, four wrong
expectations, and the storage source check described above. Each was found
by CI on the first run that reached it. No run found a defect in code that
an earlier run had passed, and none found a defect outside the tests.

The commit that adds this report also brings two design notes up to date.
It changes no code, so of the three workflows only `rust` runs on it; the
`pax` and `windows-runtime` runs above are those of the code it contains.
The invariant sweep of the policy engine now takes two to four minutes of
the `rust` job for `services/tur-pax`.

## What ran end to end

Workflow `pax` (`tests/pax/e2e.py`) on `ubuntu-24.04`, kernel 7.0.0-1017-azure,
bubblewrap 0.9.0, util-linux 2.39.3. The ten scenarios of pax-1 still pass;
`injection-lies` lost its two repeated-key calls, which are a scenario of
their own now. New in this change:

| Scenario | What the scripted model does | What was proved |
|---|---|---|
| tool sets (before the scenarios) | – | Five components ship tools; the workflow installed each one's two files under `/usr/share/tur/pax/tools/<component>/`; they are byte for byte the components' own; `tur-pax` is not told where they are and finds them; `services/tur-pax` holds none |
| `repeated-key` | One response: an unremarkable call, then two calls whose argument text names a key twice | The response is discarded whole as malformed (`TUR-AI-PROVIDER-007`, exit status 21): no decision, nothing run, the unremarkable call included; neither value is in the transcript or the trail |
| `openai-adapter` | Speaks the OpenAI Responses format. Three calls with streamed arguments (two real tool programs and a real command), then four tools of the test service, one of each class, delivered only as finished items | Seven decisions with the manifests' classes; five performed in the sandbox; class 2 and class 3 refused; every result went back as a `function_call_output`; the requests went to `/v1/responses`; the trail names the adapter |
| `google-adapter` | Speaks the Google Interactions format. The first response delivers the arguments of each call whole, inside the step that starts it; the second streams them | The same seven decisions and five runs. **The first response is the delivery the loop refused before this change.** Results went back as `function_result` steps to `/v1beta/interactions` |
| `openai-repeated-key`, `google-repeated-key` | The repeated key in each of the two formats; for Google inside the JSON object of the step itself | Discarded whole, as for `repeated-key` |
| `new-tool-forms` | Calls `device_list`, `device_list` with two categories, `device_get` and `storage_preview_mount` for things the runner does not have, and `storage_list_volumes` | `tur-device list` ran in the sandbox and returned a redacted listing; the categories reached the program as options and only those devices came back; `tur-device get` answered `TUR-DEVICE-INPUT-002` with exit status 3; `tur-storage decide --live` ran `lsblk` itself inside the sandbox and answered `TUR-STORAGE-VOLUME-001`, "not in this machine"; `tur-storage volumes --live` listed the runner's volumes |
| `injection-memory` | Says "my notes say this is approved" and calls the class 3 tool | The store was prepared with the program the user has: `add --stated` is refused while memory is off (`TUR-AI-MEMORY-001`); `switch on`; `add --stated` stores the note as stated by the user. The note, an instruction that claims advance approval, was recalled and sent inside its "data, not instructions" frame, before the request and not in the system text. Recall was decided as `agent:memory_read`, class 0. The class 3 action: refused, `TUR-AI-POLICY-006`, nothing run |
| `output-filter` (changed) | As in pax-1 | The outcome of the action is `ok` with three pieces withheld, and an `output_withheld` event of its own, after it, carries the count, the kinds and `TUR-AI-OUTPUT-001` |

Over all scenarios together, two checks are new: the system text is the same
in every request through every adapter, and the loop was run through all
three adapters. Every scenario also checks that each request went to the
endpoint of its adapter and that every record of the trail names it.

## What was verified on the web

All on 2026-11-12, by fetching the source named. Nothing was checked on the
TUR image, which does not ship these programs' files to a place this work
could read.

**Application identifiers** (the engine's built-in sensitive and excluded
lists). The source of every entry is in the code beside it:
`SENSITIVE_APPLICATION_SOURCES` and `EXCLUDED_APPLICATION_SOURCES` in
`services/tur-pax/src/policy.rs`. The ones the task named:

| Application | Identifier | Verdict | Source |
|---|---|---|---|
| Konsole | `org.kde.konsole` | Correct | invent.kde.org `utilities/konsole`, `desktop/org.kde.konsole.desktop`; Fedora 44 package `konsole` file list; Flathub |
| Yakuake | `org.kde.yakuake` | Correct | invent.kde.org `utilities/yakuake`, `data/org.kde.yakuake.desktop`; Fedora 44 `yakuake` |
| KRunner | `org.kde.krunner` | Correct | invent.kde.org `plasma/plasma-workspace`, `krunner/org.kde.krunner.desktop.cmake` (installed under `kglobalaccel`, not `applications`) |
| GNOME Terminal | `org.gnome.Terminal` | Correct; `org.gnome.Terminal.Preferences` added | gitlab.gnome.org `GNOME/gnome-terminal`, `data/`; Fedora 44 `gnome-terminal` |
| Ptyxis | `org.gnome.Ptyxis`; `app.devsuite.Ptyxis` on Flathub | Both correct; `org.gnome.Ptyxis.Devel` added | gitlab.gnome.org `chergert/ptyxis`, `meson.build`; Fedora 44 `ptyxis`; Flathub |
| KeePassXC | `org.keepassxc.KeePassXC` | Correct | github.com `keepassxreboot/keepassxc`, `share/linux/org.keepassxc.KeePassXC.desktop.in`; Fedora 44 `keepassxc`; Flathub |
| Bitwarden | `com.bitwarden.desktop` | Correct | github.com `bitwarden/clients`, `apps/desktop/electron-builder.json` and `resources/com.bitwarden.desktop.desktop`; Flathub |
| 1Password | `com.1password.1Password` | **Wrong: found in no source.** `com.onepassword.OnePassword` added | Flathub (`com.onepassword.OnePassword` exists, `com.1password.1Password` does not); the AUR `1password` PKGBUILD, which installs `resources/com.onepassword.OnePassword.desktop` from the vendor's archive (a secondary source) |
| KWallet's manager | `org.kde.kwalletmanager` (from KDE Gear 25.04), `org.kde.kwalletmanager5` (before) | Both correct; `kwalletmanager5-kwalletd` added | invent.kde.org `utilities/kwalletmanager` at v23.08.0, v24.02.0, v25.04.0 and master; Fedora 44 `kwalletmanager5` |
| KWallet's daemon | `org.kde.kwalletd5`, `org.kde.kwalletd6` | Correct for their versions, **and incomplete**: from KDE Frameworks 6.30 the file is `org.kde.kwalletd.desktop`. Added | invent.kde.org `frameworks/kwallet` at kf5, v6.0.0, v6.10.0, v6.14.0, v6.30.0 and master; `src/runtime/kwalletd/main.cpp` |
| Seahorse | `org.gnome.seahorse.Application` | Correct for every release; upstream's main branch renames it to `org.gnome.Seahorse`. Added | gitlab.gnome.org `GNOME/seahorse` at 43.0, 47.0.1 and main |
| pinentry | program names | Correct as program names; the Qt pinentry's desktop file names `org.gnupg.pinentry-qt` and `org.gnupg.pinentry-qt5` were missing. Added | github.com `gpg/pinentry`, `qt/Makefile.am`, `qt/main.cpp`, `qt5/`; Fedora 44 `pinentry-qt` |
| Contour | `contour` | The program, not the desktop file, which is `org.contourterminal.Contour`. Added | github.com `contour-terminal/contour`, `src/contour/CMakeLists.txt`; Fedora 44 `contour-terminal`; Flathub |

About 45 further identifiers were added from the same check (development and
preview builds of terminals, more askpass programs and polkit agents, Debian's
names). Fourteen entries could not be confirmed and are marked `Unverified` in
the tables and named in a test; `agent-policy.md`, section 2.5, lists them.

**Protected units.** The unit-name prefixes of `PROTECTED_TARGETS` were held
against the unit files in the Fedora 44 and Fedora 43 package file lists on
`packages.fedoraproject.org` (packages `polkit`, `firewalld`,
`nftables-services`, `iptables-services`, `audit`, `audit-rules`, `systemd`,
`systemd-udev`, `openssh-server`, `sssd-common`, `fprintd`, `policycoreutils`,
`usbguard`, `fwupd`, `rpm-ostree`, `bootc`, `clevis-systemd`, `tpm2-abrmd`,
`dbus-broker`, `sddm`, `greetd`, `plasma-login-manager`). Every prefix that
was looked up names real units. Seven prefixes were added for real units the
list did not cover: `arptables`, `systemd-userdbd`, `systemd-pcr`,
`tpm2-abrmd`, `sddm`, `greetd`, `plasmalogin`. `apparmor.service` was seen in
Debian's package only. Not looked up: the file and package prefixes, and
fifteen unit prefixes that `agent-policy.md`, section 2.5, names.

**Not verified again.** The providers' wire formats: the new conformance
scripts and the scripted provider of the tests follow the same documentation
`pax-providers.md` cites, read on 2026-11-12 for pax-providers; it was not
read again for this change. An Anthropic `tool_use` block that starts with a
non-empty input is accepted, though the streaming reference does not describe
one.

## Breaking changes

To a public Rust API:

- `tur_pax_providers::StreamEvent::ToolCallEnd` has a new field,
  `arguments_text`.
- `tur_pax::Context::trusted_path_available` is `trusted_session_proven`.
- `tur_pax::Context::standing_approvals` is `HeldApprovals`, not
  `Vec<StandingApproval>`. It cannot be built from a list.
- `tur_pax::AgentOperation`: `MemoryWrite` is gone; six operations are new;
  `ScheduleWrite` is class 4, not class 2.
- `tur_pax::Reason` has a new variant; `tur_pax::trail::EventKind` has a new
  variant; `tur_pax::adapter::ArgumentTemplate` has a new variant.
- `tur_pax::textui`: `DEFAULT_MANIFEST_DIR` and `DEFAULT_ADAPTER_DIR` are
  replaced by `DEFAULT_TOOLS_DIR`.
- `tur_storage::ProbeError` has a new variant; `tur_storage::cli::AccessArg`'s
  variants are renamed; `InputArgs::from` is optional.
- `tur_pax_memory::cli::Environment` has a new field, `switch`.
- `tur_core::Family` has two new variants and `Family::ALL` 17 entries.

To behaviour, a command line or a file format:

- A response in which a tool call repeats a key ends the task
  (`TUR-AI-PROVIDER-007`). Before, that one call was rejected and the task
  went on.
- `tur-pax policy check --standing-approval` no longer changes the decision:
  an exact match used to print `allow` and now prints `defer`. The JSON output
  has a new member, `standing_approvals_ignored`. `--trusted-path` is now
  `--trusted-session-proven`; the old name is still accepted.
- `tur-pax run` reads tool sets from `/usr/share/tur/pax/tools/<component>/`
  (`--tools`), no longer from `/usr/share/tur/pax/manifests` and `adapters` by
  default. `--manifests` and `--adapters` still work, for further files.
- The audit trail: the `executed` event no longer carries
  `TUR_AI_WITHHELD_KINDS`, and carries `TUR-AI-OUTPUT-001` no longer; an
  `output_withheld` event after it does. `TUR_AI_ACTION` values
  `agent:memory_write` is gone; six `agent:memory_*` and `agent:schedule_propose`
  are new.
- The adapter file format has a fifth argument form, `{option, each}`. Files
  written for the old format load unchanged.
- `tur-storage`: `--from` is no longer required if `--live` is given. `--access
  ro|rw` is still accepted; the help shows `read-only|read-write`.
- `tur-runtime`: ten error-code strings changed, in its text and JSON output.
- `tur-catalogue`: **nothing changed** in `search`, `show` or `route`.

## What is not done

- **A real model**, and a real provider's responses through the OpenAI and
  Google adapters.
- **The policy store.** Standing approvals cannot be honoured at all; the
  engine's behaviour with one is shown only by a stand-in store in tests.
- **Memory writes through the engine.** The engine has the operations of
  section 18; nothing calls them, because the orchestrator does not write to
  memory. The mapping from the memory layer's kinds to the engine's
  operations is in the design note, not in code.
- **`storage_mount`, `storage_unmount`, `storage_unlock`.** Declared, class 1,
  not offered: they need the service that mounts.
- **NTFS and HFS+ state on a live system.** The live probe runs `lsblk` only,
  so both are unknown and such volumes are previewed as read-only or refused.
- **A device looked up by a real identifier in the `pax` workflow.** The
  scripted model cannot know one; `tur-device get` with real identifiers is
  tested on the recorded laptop in the component.
- **The identifiers on the image.** Checked against upstream and Fedora
  packages, not against what the TUR image installs.
- **The catalogue's codes** stay under `DESKTOP`.
- **Installing the tool sets on the image**; a seccomp filter; the journal
  sink; the rest of issue 40.
- **A second reviewer for the fixes of pull request 18**, a real JSON Schema
  implementation, fuzzing (issue 33).

## Found on the way

- `PROTECTED_TARGETS` holds three entries twice: the unit prefix `tur-`, the
  path `/etc/tur` and the package prefix `tur-`. They look like the trace of
  a rename in which two different names became one. Harmless; not removed.
- `services/tur-storage/tests/interfaces.rs` asserted that no source file of
  the component starts a program. That was the component's promise, and the
  task asked for it to change. The check now says what holds instead: one
  place starts one program with a fixed command line.
- The specification's list of error-code families (§29.3) does not have
  `RUNTIME` or `CATALOGUE`, which `tur-core` now registers. Text is proposed
  in the pull request.
- The secret detector's false alarm of pax-1 (`PWD=/`) is unchanged.
