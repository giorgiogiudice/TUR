# pax-issues-2 — P@x's policy store, and the engine's two levels of confirmation

```text
TUR DEVELOPMENT REPORT

Task ID:          pax-issues-2 (issues 30, 33, 43, 66: the safety gaps)
Subsystem:        AI control layer: the policy store (new component
                  services/tur-pax-policy), the policy engine
                  (services/tur-pax), the image
Build ID:         not applicable: no image of its own; the image and boot
                  workflows run on the pull request
Source revision:  pull request 113 (feat/pax-policy-store); commits and runs
                  under "Runs"

SUMMARY
- What changed:
  1. A policy store P@x's own process cannot write (ADR-0028):
     /var/lib/tur-pax-policy/<uid>/{policy.json, approvals.json}, owned by
     root, readable by the person's group. The new program tur-pax-policy
     reads it as the person and writes it as root through pkexec; polkit
     asks for an administrator's password for every change that could
     loosen P@x, and for nothing to take standing approvals away.
  2. P@x applies the store's policy, made stricter by the person's own file
     and never less strict, in `tur-pax serve` and `tur-pax run`. No file
     P@x's process can write can loosen it any more.
  3. Standing approvals are kept there, bound to a schedule's identifier and
     the exact text of its task. They are not read by the task loop, and so
     not honoured: the engine refuses every unattended class 2 action until
     a proven trusted session exists.
  4. The policy engine names the two levels of addendum A section 23, item 1:
     a new input, approval_sheet_proven, lets an attended class 2 action
     that is not a privileged request be asked, and nothing else. Nothing
     sets it or the trusted session: class 2 stays refused.
- Why: issues 30 and 43 ("the policy store does not exist"; "move standing
  approvals and schedules out of a location P@x's own process can write"),
  33 (align the engine with the design's latest revisions), 66 ("settings
  that loosen P@x"), and the owner's decision of 12 November 2026.

FILES
- Added: services/tur-pax-policy/ (Cargo.toml, src/lib.rs, src/main.rs,
  tests/store.rs; Cargo.lock from CI), docs/adr/0028-pax-policy-store.md,
  docs/architecture/pax-policy-store.md,
  image/rootfs/usr/share/polkit-1/actions/org.tur.pax.policy.policy,
  image/rootfs/usr/lib/tmpfiles.d/tur-pax-policy.conf, this report
- Modified: services/tur-pax/src/{context,engine,cli,orchestrator,policy,
  service,settings,textui}.rs, services/tur-pax/tests/{invariants,cli,
  orchestrator,agent1_interface}.rs, image/{Containerfile,
  Containerfile.containerignore,programs.tsv}, tests/image/smoke.sh (twelve
  programs), tests/boot/guest/.../component-checks, tests/boot/report.py,
  docs/architecture/{agent-policy,ai-control-layer,pax-service}.md
- Removed: nothing

BUILD
- Command: CI only (rust, pax, image, boot workflows); nothing was compiled
  on the development machine
- Result: see "Runs"
- Warnings: see "Runs"

TESTS
- Unit tests: tur-pax-policy (policy outline, strict approvals, approvals
  per schedule and task, account line, who writes where); tur-pax
  policy.rs (the own file only tightens, every pair of modes)
- Integration tests: tur-pax-policy tests/store.rs (the program as the
  person on its own store); tur-pax invariants.rs
  (the_approval_sheet_lets_an_attended_class_2_action_be_asked_and_nothing_more,
  over the whole input space), cli.rs (--approval-sheet-proven)
- Boot tests: components.pax_policy_store (store owner and modes; the
  person cannot write; root through PKEXEC_UID can; show reads back)
- Hardware tests: NOT RUN (none needed)
- Performance tests: NOT RUN (none needed)

DIAGNOSTICS
- Log paths: boot workflow artifact, components/policy-store-*.out/.err
- Diagnostic bundle path: not applicable
- Relevant error IDs: TUR-AI-POLICY-006 (trusted_path_unavailable), now for
  both levels

RECOVERY
- Rollback procedure: revert the merge. The store directory stays on
  installed systems and is ignored by the older tur-pax; P@x then reads
  ~/.config/tur/pax/policy.json alone, as before.
- Recovery tested: not applicable

KNOWN LIMITATIONS
- The polkit prompt is not the trusted screen: it is drawn on the desktop.
- Standing approvals are stored, not honoured (by design until D-16).
- The settings that loosen P@x but are not policy (on, provider, key, day's
  limit) stay in the session service: owner decision.
- The polkit prompt and KDE's agent were not exercised (need a person).

NEXT SAFE ACTION
- Owner decisions below; then the trusted screen (D-16), which replaces the
  polkit prompt and is the step that may read approvals into the engine.
```

## What was built

| Part | Where |
|---|---|
| The store: layout, strict reading, atomic root-owned writes, approvals bound to schedule and task | `services/tur-pax-policy/src/lib.rs` |
| The program: `show`, `approvals-for`, `set-policy`, `set-approvals`, `clear-approvals`; re-exec through `pkexec`; root writes only for `PKEXEC_UID` | `services/tur-pax-policy/src/main.rs` |
| polkit actions `org.tur.pax.policy.change` (auth_admin) and `org.tur.pax.policy.tighten` (yes) | `image/rootfs/usr/share/polkit-1/actions/` |
| The store's directory at boot | `image/rootfs/usr/lib/tmpfiles.d/tur-pax-policy.conf` |
| `UserPolicy::in_force`, `tightened_by`, `PolicySource`; `Paths::policy_store`; serve and run read the store | `services/tur-pax/src/{policy,settings,service,textui}.rs` |
| `Context::approval_sheet_proven`; rule 5 in two levels; rule 10c needs the trusted session | `services/tur-pax/src/{context,engine}.rs` |
| Design, decision | `docs/architecture/pax-policy-store.md`, `docs/adr/0028-pax-policy-store.md`; agent-policy.md (§2.3, §4 rules 5 and 10c, interpretations 29 amended, 66 to 68, §7, limit 16); ai-control-layer.md §6 (the amendment section 23 asks for) |

## Runs

Head `1412666` (the runs of the first head, `b40233f`, were cancelled by it):

| Workflow | Run | Result |
|---|---|---|
| rust, services/tur-pax-policy | 43501224679 (job 132172536104) | clippy and every test passed on CI's corrected tree (unit 5 + 2, store.rs 5, doc test 1); failed only at "Fail if fixes were needed": the new Cargo.lock and rustfmt, applied in the next commit |
| rust, services/tur-pax | 43501224679 (job 132172536028) | tests passed on the corrected tree; rustfmt only, applied in the next commit |
| pax, desktop | see the pull request | pass |
| image, boot, rollback, iso | see the pull request | failed at "Build image": `cargo fetch --locked` had no Cargo.lock for services/tur-pax-policy. Fixed in the next commit |

Next head (Cargo.lock and rustfmt applied): its runs are listed in the pull
request.

## The issues

**#43** — "The policy store does not exist": built; holds the policy (applied)
and standing approvals (stored, not honoured; see #30). "Nothing writes memory
through the policy engine": since #52 the session service's `keep_note` does
(class 1, an entry waiting for the person); recording *observed* entries needs
the orchestrator to set the kind from what happened (pax-memory-and-schedules.md
§2), which is a design question, not done. The other items of #43 (storage live
probe, wire formats, duplicates, invariant job, error codes, identifiers) are
outside this change; duplicates and identifiers are in pull request 110.

**#30** — "Move standing approvals and schedules out of a location P@x's own
process can write": standing approvals, with the identity of the schedule that
carries them (identifier and exact task), now live in the root-owned store;
schedules without approvals stay in the person's store, which the design
allows (a schedule only says when a task runs). "Until then standing
approvals must not be honoured for class 2": still true, by rule 5.

**#33** — Engine aligned with revisions 9 and 10 (§22, §23 item 1): two
levels; interpretation 29 amended; agent-policy.md says revision 10. Still
open: a second reviewer, a real JSON Schema validator, fuzzing.

**#66** — "Settings that loosen P@x": the policy part (mode, exclusions,
relaxations) is now out of the session service's reach. The service settings
(on, provider, key, day's limit) stay: owner decision.

## Owner decisions

1. **The polkit prompt as the interim path for loosening.** Accept an
   administrator's password through polkit as the way to change the store
   until the trusted screen exists (ADR-0028, status Proposed)?
2. **Service settings that loosen P@x** (issue 66): move switching on, the
   provider, the key and raising the day's limit into the store now (each then
   asks for a password, and the P@x window D-17 changes), or leave them until
   the trusted screen?
3. **Observe-only versus ask-every-action** when the store and the own file
   disagree: observe-only is taken as stricter (interpretation 68).
