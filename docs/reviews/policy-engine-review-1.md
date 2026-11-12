# Policy engine: independent review 1

- **Subject:** `services/uos-agent` as merged in pull request #6 (source revision
  `e9b3675` on `main`), with `docs/architecture/agent-policy.md` as its author's
  account
- **Reviewed against:** AI control layer (addendum A) **revision 4**, sections 5,
  6, 7 and 17
- **Date:** 2026-11-12
- **Reviewer:** an automated development agent that did not write the engine
- **Pull request:** #18 (`review/policy-engine`); closes issue #10
- **Result:** no input was found that yields `allow` for a class 3 or class 4
  action. Three findings of high severity, twelve of medium, ten of low. All but
  the ones marked "accepted" are fixed in the same pull request, each with a test.

This document is the record of the review. What the engine does now is described
in [agent-policy.md](../architecture/agent-policy.md); the decision the engine
embodies is [ADR-0012](../adr/0012-assistant-policy-model.md).

---

## 1. How the review was done

**Reading.** All of `services/uos-agent` (eleven source files, three test files,
five fixtures), both JSON schemas, `agent-policy.md` and addendum A revision 4,
read as someone who controls everything the model says and everything on screen.

**No local compiler.** Nothing is compiled on a development machine
([ADR-0002](../adr/0002-ci-and-build-infrastructure.md)). Every claim below that
something fails or passes was established by pushing a commit and reading the
GitHub Actions run; the run is named. Where a claim rests on reading alone, it
says so.

**Demonstration before repair.** For each defect that can be stated as an input
and a decision, a test that asserts the behaviour the design requires was pushed
first, in `tests/review.rs`, against the unchanged engine. Run
[43157368292](https://github.com/giorgiogiudice/TUR/actions/runs/43157368292)
is that push: 22 tests, 22 failures. The fixes followed. Run
[43179599822](https://github.com/giorgiogiudice/TUR/actions/runs/43179599822)
is the first run with the fixes: the same tests, and `f04`, pass. That run is red
only at the formatting check; run
[43180389657](https://github.com/giorgiogiudice/TUR/actions/runs/43180389657)
is the first green one.

**Mutation.** To learn whether the invariant tests would notice a broken
invariant, the engine was broken on purpose, one line at a time, on scratch
commits that were then reverted (section 4).

**What "the engine as reviewed" disagreed with.** Revision 4 of the design was
written after the engine, to close gaps the engine's own author had found and
listed. Several findings below are therefore not mistakes of the author: they are
places where the merged code had not yet caught up with the design in force.
They are findings all the same, because the code decided otherwise than the
design says.

## 2. Findings

Severity: **critical** — a class 3 or 4 action is allowed; **high** — something
the design says must be asked, authenticated or refused is allowed, in a way an
attacker can reach; **medium** — the same under narrower conditions, or a silent
weakening of policy, of the record or of the tests; **low** — an inconsistency
with small effect; **note** — recorded so that it is not rediscovered.

| ID | Severity | Finding | Status |
|---|---|---|---|
| F-01 | high | Typing into a terminal or a run dialog through the screen is class 1 | Fixed |
| F-02 | high | A free-text privileged command runs without approval inside a repair session | Fixed |
| F-03 | medium | A repair session may last 24 hours, cannot be marked ended, and has no opening time | Fixed |
| F-04 | high | A window of a process with administrator rights is an ordinary window | Fixed |
| F-05 | medium | A control named only by an icon or a symbol is class 1 | Fixed |
| F-06 | medium | A look-alike letter, an invisible character or another language defeats the pattern table | Fixed for what cannot be read; the rest accepted (N-5) |
| F-07 | medium | "OK", "Yes", "Continue" and "Apply" are class 1 | Fixed |
| F-08 | medium | The pattern table lacks destructive and granting words | Fixed |
| F-09 | medium | A locked unattended task runs commands outside the read-only sandbox | Fixed |
| F-10 | low | The audit record lacks `UOS_AI_ATTENDED` and `UOS_AI_SANDBOX` | Fixed |
| F-11 | medium | A privileged command is recorded as a program name and a count | Fixed |
| F-12 | low | The record names a program by its last path component | Fixed |
| F-13 | medium | One method can be declared with two classes | Fixed |
| F-14 | low | A repeated key inside an input schema is accepted; the last value wins | Fixed |
| F-15 | low | Integer bounds are compared as floating-point numbers | Fixed |
| F-16 | medium | The policy file accepts an application identifier that can never match | Fixed |
| F-17 | low | An exclusion does not cover the `.desktop` form of an identifier | Fixed |
| F-18 | medium | No application is excluded by default | Fixed |
| F-19 | low | The model's hint adds no confirmation where a relaxation or a repair session allows | Fixed |
| F-20 | low | A class 1 relaxation is still part of the file format | Fixed |
| F-21 | medium | A repeated key inside a standing approval or in tool arguments is accepted | Fixed |
| F-22 | low | Two relaxations that differ only in letter case are accepted | Fixed |
| F-23 | low | The command line accepts arguments with a repeated key | Fixed |
| F-24 | medium | Breaking sensitivity is noticed by one hand-written row and by no invariant | Fixed |
| F-25 | low | The sweep's generators cover less than "everything", and one helper copies the engine | Fixed |

### F-01 (high) — Typing into a terminal through the screen is class 1

Design: section 17, item 1. Acting in a terminal emulator, a run dialog or
another surface that executes what is typed is class 2.

Input: a screen action, acting, on the control `Text` in the window
`user@host: ~` of the application `org.kde.konsole`; attended, unlocked, trusted
path available; the default user policy.

Decision as reviewed: `allow`, `reversible_permitted`. The assistant could run
any command as the user with no sandbox and no approval, which the command tier
exists to prevent. The engine could mark an application sensitive only if the
user had listed it.

Evidence: `f01_acting_in_a_terminal_or_a_run_dialog_is_asked_with_an_empty_user_policy`,
failing in run 43157368292.

Fix: `DEFAULT_SENSITIVE_APPLICATIONS` in `src/policy.rs`, applied to every
policy; a new pattern category for the usual names of terminals and consoles in a
window title. Limit: the list is a list (agent-policy.md, limit 8).

### F-02 (high) — A free-text privileged command runs unasked inside a repair session

Design: section 5.4 and section 17, item 4. Inside a repair session a free-text
privileged command is still shown and approved one at a time; only a typed
operation runs without further approval.

Input: the command `systemctl restart cups` in the privileged sandbox with the
operation type "service control"; attended; a repair session open. The text is
irrelevant: `rm -rf /boot` with the same declared type decides the same.

Decision as reviewed: `allow`, `repair_session_open`. The engine had one kind of
privileged request. The declared operation type, which is all it checked, is
chosen by whoever builds the request.

Evidence: `f02_a_free_text_privileged_command_is_asked_inside_a_repair_session`,
failing in run 43157368292.

Fix: two kinds of privileged request. A typed operation is one of a closed set
defined as data (`TYPED_OPERATIONS`), with the shape of its target checked and a
deny list of protected targets; it is allowed inside a session. A free-text
command is asked (`privileged_command_needs_approval`).

### F-03 (medium) — The repair-session record

Design: section 5.5 and section 17, item 10. 15 minutes by default, 60 at most;
ends early on lock, when the user ends it and when the task finishes.

Inputs and decisions as reviewed:

- A session expiring 24 hours from now is open.
- The record holds a purpose and an expiry only. It cannot say "ended". After a
  lock and an unlock the same record is open again, although the design ends the
  session at the lock.
- Without an opening time, "at most 60 minutes" cannot be checked: only the time
  left can, and a record renewed every hour would pass for ever.

Evidence: `f03_a_repair_session_of_more_than_an_hour_is_closed`, failing in run
43157368292. The second and third points were established by reading.

Fix: the record gains `opened_at` and `ended`. A session is closed before its
opening, at its expiry, when ended, and when it lasts more than 60 minutes from
opening to expiry. `RepairSession::open` gives the default length. What a pure
function cannot do is in agent-policy.md, limit 13.

### F-04 (high) — A window of an administrator process is an ordinary window

Design: section 17, item 2. A window that belongs to a process with administrator
rights is a protected surface; screen control of it is refused.

Input: an accessibility action on the control `Next` in a partition editor that
runs as root.

Decision as reviewed: `allow`, class 1. The context had no way to say whose
process a window belongs to, so "screen control cannot exceed class 2" rested on a
prompt that an already-authorised tool never shows.

Evidence: by reading; no failing test could be written before the input existed.
`f04_a_window_of_a_process_with_administrator_rights_is_refused` passes from run
43179599822.

Fix: the context names the owner of the target window. An administrator's window
is refused, observation included. If the compositor does not say, acting is asked
and a relaxation does not apply.

### F-05, F-06, F-07, F-08 (medium) — The pattern table fails open

Design: section 5.1. "The rules are conservative: when in doubt, ask."

The table matched English stems in the name of a control and the title of a
window, and asked for a control with an empty name. Everything else was class 1.
Acting on each of these, in an ordinary window, was allowed without asking:

| Finding | Control name | Why it matters |
|---|---|---|
| F-05 | a wastebasket icon; `→`; `✓`; `…`; `?` | An icon button has no text. The name was not empty, so it was not "in doubt". |
| F-06 | `Send` with a Cyrillic first letter (U+0405); `Send` with a zero-width space inside (U+200B); `Send` in full-width letters; `Delete` with a soft hyphen inside (U+00AD); `Löschen`; `Удалить`; `删除` | A page controls the accessible names of its controls. One invisible character was enough. |
| F-07 | `OK`, `Yes`, `Continue`, `Proceed`, `Apply`, `Done` | What a dialog's button confirms is in the dialog, not on the button. |
| F-08 | `Format`, `Overwrite`, `Reset to defaults`, `Restore`, `Encrypt`, `Approve`, `Authorize`, `Grant access`, `Manage subscription`, `Run`, `Execute` | Missing from the table. `subscrib` did not match "subscription". |

Evidence: `f05_…`, `f06_…`, `f07_…` and `f08_…` in `tests/review.rs`, failing in
run 43157368292.

Fix: a control whose name cannot be read against an ASCII table (`is_legible`) is
class 2. That is an allow list of characters, so it does not depend on a table of
look-alikes. A second table of whole words holds the affirmations and what is too
short to be a stem. The stems gained the missing words. Accepted: N-5.

### F-09 (medium) — A locked unattended task runs commands outside the read-only sandbox

Design: section 5.6. "While the session is locked it uses tools and read-only
commands only."

Input: a command in the user sandbox, in an undoable workspace (class 1);
unattended; session locked.

Decision as reviewed: `allow`, `reversible_permitted`.

Evidence: `f09_…`, failing in run 43157368292.

Fix: rule 9b defers it (`unattended_command_while_locked`).

### F-10, F-11, F-12 — The audit record

- **F-10 (low).** Section 11 of the design lists `UOS_AI_SANDBOX` and
  `UOS_AI_ATTENDED`. Neither was written. A record could not tell an unattended
  action from an attended one, or a workspace command from one on the user's
  files.
- **F-11 (medium).** Section 17, item 9: a privileged request is logged in full.
  `systemctl restart cups.service` as root was recorded as
  `program systemctl, 2 arguments, operation service_control`. Nobody could
  review what a repair session did.
- **F-12 (low).** `/tmp/x/systemctl` was recorded as `systemctl`. A record that
  names a planted program like the genuine one misleads.

Evidence: `f10_…`, `f11_…`, `f12_…`, failing in run 43157368292.

Fix: both fields are written. A privileged request is recorded with the exact
text shown to the user, in `MESSAGE` and in `UOS_AI_REQUEST`; a content marked
secret is replaced, and nothing else is (interpretations 44 and 45). A command is
named by the path it was given.

### F-13 (medium) — One method under two classes

Input, a manifest with two tools:

```json
{ "name": "storage_format", "class": 3, "method": "Format", "...": "..." },
{ "name": "storage_tidy",   "class": 1, "method": "Format", "...": "..." }
```

Result as reviewed: the manifest loads. A call to `storage_tidy` is class 1 and is
allowed; what it calls is `Format`. The author's limit "a manifest is trusted"
covers a service that lies about a class. This is different: the file contradicts
itself and the loader took the lower statement without a word. A reviewer of a
manifest looks for the dangerous method's name and finds it marked class 3.

Evidence: `f13_a_method_cannot_be_declared_with_two_classes`, failing in run
43157368292.

Fix: `UOS-AI-MANIFEST-011`. The service's own check through polkit remains the
enforcement; this closes the silent case.

### F-14, F-21, F-23 — Repeated keys

JSON does not say what `{"to": "a", "to": "b"}` means. The parser in use keeps the
last value; other parsers keep the first; a person may read either.

- **F-21 (medium).** A standing approval
  `{"service": …, "tool": "mail_send", "arguments": {"to": "a@example.org", "to": "b@example.org"}}`
  loaded and matched calls to `b@example.org`. A settings page that renders the
  first value shows the user an approval for `a@example.org`. The same holds for
  the arguments of a call: what is decided and shown is one reading, and what the
  service receives may be the other.
- **F-14 (low).** Inside an input schema, `"maximum": 10, "maximum": 1000000` and
  `"additionalProperties": true, "additionalProperties": false` loaded with the
  last value.
- **F-23 (low).** `--args '{"value":"a","value":"b"}'` was decided as `b`.

The author listed the first two as a known limit. Fields of the manifest and of
the policy file were already rejected when repeated; that was checked and is now
tested.

Evidence: `f14_…`, `f21_…`, `f23_…`, failing in run 43157368292.

Fix: `src/strict.rs`, a JSON reader that refuses a repeated key at any depth, used
for input schemas, standing approvals and arguments.

### F-15 (low) — Integer bounds as floating-point numbers

Input: a property `{"type": "integer", "maximum": 9007199254740992}` and the
argument `9007199254740993`.

Result as reviewed: accepted; as 64-bit floating-point values the two are equal.
Listed by the author as a known limit.

Evidence: `f15_…`, failing in run 43157368292. Fix: two integers are compared
exactly.

### F-16 (medium), F-17 (low), F-22 (low) — Application identifiers

- **F-16.** `"excluded_applications": ["org.keepassxc.KeePassXC\u200b"]` loaded.
  With the zero-width space the entry equals no identifier a compositor reports,
  so the exclusion the user sees in the file never applies. The same with a
  Cyrillic letter or a byte-order mark. The loader rejected control characters
  and surrounding whitespace only.
- **F-17.** An excluded `org.example.Vault` did not cover a window reported as
  `org.example.Vault.desktop`, a form some toolkits use.
- **F-22.** `org.example.Mail` and `org.example.mail` could each have a
  relaxation. Harmless, since a relaxation matches exactly, but the file then says
  two things about what the rest of the engine treats as one application.

Evidence: `f16_…`, `f17_…`, `f22_…`, failing in run 43157368292.

Fix: the policy file accepts printable ASCII identifiers only; an application
whose reported identifier is not plain is sensitive; exclusion and sensitivity
match the `.desktop` form; repeated relaxations are detected without regard to
case.

### F-18 (medium) — No application is excluded by default

Design: section 8. Password managers, the keyring and the lock screen are excluded
by default.

Input: a screen observation of `org.keepassxc.KeePassXC` with the default policy.
Decision as reviewed: `allow`, class 0; the capture goes to the provider. The
author listed this as a known limit, for want of identifiers.

Evidence: `f18_…`, failing in run 43157368292.

Fix: `DEFAULT_EXCLUDED_APPLICATIONS`. The identifiers are unverified
(section 6).

### F-19 (low) — The model's hint adds no confirmation

Design: section 5.1. The hint "can only add a confirmation; it can never remove
one."

Input: acting on `Send` in an application the user relaxed, with the hint set.
Decision as reviewed: `allow`, `relaxed_by_user_policy`, the same as without the
hint (the author's interpretation 25). The hint lowered nothing, so the invariant
held; but a model that says "this one matters" was not heard exactly where the
user had switched the question off.

Evidence: `f19_…`, failing in run 43157368292.

Fix: the hint adds the confirmation where a relaxation or a repair session would
allow and the user is present. For a standing approval it still has no effect,
for the author's reason.

### F-20 (low) — The class 1 relaxation

Section 17, item 7: removed. The file format and `RelaxationLevel` still had it.
Evidence: `f20_…`, failing in run 43157368292. Fix: removed.

### F-24 (medium) — Sensitivity was guarded by one row

Mutation 6 (section 4) made `ui_class` ignore sensitivity. Of the sweep's 26
tests, 25 passed. Only the hand-written decision table noticed, through two rows.
The documented invariant "excluding an application or marking it sensitive is
never more permissive" holds trivially when sensitivity does nothing, and no test
stated what sensitivity must do.

Evidence: run
[43157022561](https://github.com/giorgiogiudice/TUR/actions/runs/43157022561).

Fix: `a_sensitive_application_is_never_observed_or_acted_on_without_asking`, and
`the_class_is_the_class_of_the_source_raised_only_by_the_stated_rules`, which
pins the class of every input from both sides.

### F-25 (low) — What the sweep called everything

The sweep was honest about its size: 147 actions, 288 contexts and 9 policies are
what its generators produced. What they did not produce:

- a label outside ASCII, or any hostile string, in any field;
- an application identifier in another letter case, so the loose matching of
  exclusion and sensitivity ran only in a unit test;
- a time other than one fixed instant;
- a repair session near the upper bound;
- a standing approval for another service or another tool.

And `repair_session_is_open`, the test's "independent" statement of which sessions
are open, was the engine's own expression copied, the 24-hour bound included. Had
the bound been wrong in the engine, the test would have agreed with it.

Evidence: by reading `tests/invariants.rs`.

Fix: the generators are extended (agent-policy.md, section 5), a test asserts
what they cover, three tests put hostile text in every field, one varies the time,
and every helper the sweep compares with is written from the design text.

### Notes

- **N-1. A clock set back.** A pure function cannot know that the time it is
  given is earlier than the time it was given before. The engine now catches a
  clock set back past the opening of a session; inside the session it cannot. The
  owner of the record must mark an expired session ended. Accepted
  (agent-policy.md, limit 13).
- **N-2. "Numbers compare as written."** The account said a standing approval
  compares JSON numbers as written. It compares them as parsed: `1` and `1.0`
  differ, `1.0` and `1.00` are equal. The behaviour is sound; the sentence was
  wrong and is corrected.
- **N-3. `schedule_write`.** Section 17, item 10: P@x proposes a schedule and the
  user creates it. The engine has a class 2 operation "create, change or remove a
  schedule", which P@x would perform after an `ask`. It is stricter than a
  proposal, and it names an act the design gives to the user. Not changed: the
  scheduler is being built against this type. Reported for that work.
- **N-4. Removing a memory entry is class 1.** See interpretation 27 in section 5.
- **N-5. Labels that mislead in plain ASCII.** `S e n d`, `De1ete`, `Enviar`,
  `Senden`, and a button named `Next` that pays. No table catches these, and the
  author said so. Accepted: the bound on screen actions is that protected surfaces
  reject emulated input, not the label.
- **N-6. `decide` is public.** A caller can decide without recording. The record
  is written only by `decide_and_record`. The orchestrator must use it. Accepted
  as a caller obligation; stated in limit 1.
- **N-7. "No policy file" and "unreadable policy file".** The first is the default
  policy, normal mode; the second must be `UserPolicy::fail_closed()`. The engine
  provides both and cannot tell which case the caller is in.
- **N-8. Two manifests for one service.** The loader sees one manifest at a time.
  The registry must refuse a second manifest that declares a service already
  loaded, or F-13 returns across files.

## 3. What was tried and held

So that the next reviewer does not repeat it.

**An `allow` for class 3 or 4.** Every `Decision::allow` in `decide` was traced
back. Class 4 returns at rule 2 and class 3 at rule 6, before any of them. The
class of a tool call comes from `ToolCall::declared_class`, whose fields are
private to the crate; outside it a tool call can be built only from a validated
manifest, and a manifest cannot declare class 4. `Decision` has no public
constructor. The command-line test runs class 3 through 192 combinations of
flags. Mutation 1 confirms that the tests notice a hole here.

**Model-supplied data lowering a decision.** The arguments of a tool call are
read in one place, the equality test of a standing approval. The text of a
command is never read by `decide`. A tool name is looked up exactly: `Test_observe`
is not `test_observe` (now tested). The hint goes through `max`. Claims inside
arguments ("approved": true, "the user has confirmed") change nothing; the test
for that was already there and is sound.

**Forging a grant.** A standing approval, a repair session, the trusted-path flag
and the user policy are plain inputs. Whoever builds the context can set them.
Inside the engine there is nothing to attack; the boundary is the component that
supplies them, which does not exist yet (ADR-0012). The declared class of a tool
cannot be forged from outside the crate.

**Standing approvals.** Near misses tried: a trailing space, another letter case,
a zero-width space, a look-alike letter, `1` against `1.0`, `-1`, `"1"`, `true`,
`null`, a nested list, a missing key, an extra key, a key in another case, another
tool, the same tool in another case, another service, and integers that differ
only above 2^53. None matches. Key order does not matter, as documented.

**Time.** The expiry instant is closed. `expires_at - now` cannot underflow: the
comparison before it short-circuits. The largest time closes everything. The
command line uses the largest time when the clock cannot be read. Mutation 4
confirms that the boundary is tested.

**The manifest loader.** Rejected, and tested: unknown fields; a missing class
(there is no default); a class given as a string, a fraction, null or a boolean;
class 4; classes below 0 and above 4; an open object schema at any depth;
`additionalProperties` given as a schema; a `type` given as a list; every
unsupported keyword tried (`anyOf`, `oneOf`, `$ref`, `patternProperties`,
`pattern`, `format`, `default`, `const`, `minItems`, `minProperties`); a repeated
field of the manifest or of a tool. Nesting 200 levels deep is refused by the
parser's own limit and does not exhaust the stack.

**The policy loader.** `mode` is required. An unknown mode, a mode in another
case, an unknown field, a repeated field and a version other than 1 are errors.
No field relaxes class 3 or 4. The fail-closed policy is observe-only.

**The audit path.** `decide_and_record` writes before it returns and turns a
failed write into a refusal. Identifiers outside a narrow character set are
replaced, so a task name cannot inject a journal field. Argument values of
ordinary tool calls never reach the record, whatever they are.

**The prompt.** Everything outside printable ASCII is escaped; quotes and
backslashes are escaped; a command of ten thousand characters is shown whole (now
tested).

**The command line.** Only `allow` exits with 0. A usage error is 2 and an input
error is 1, never the status of a decision.

## 4. Mutation checks

Each mutation is one line. Each was pushed as a scratch commit, the run was
recorded, and the next commit undid it. Commit `7a7ef2a` restores the tree
exactly: `git diff d5da899 7a7ef2a` is empty. To see what the sweep alone
notices, the unit tests and the command-line tests were compiled out for runs 1c
to 6.

| # | Invariant | The one-line change | Noticed by | Run |
|---|---|---|---|---|
| 1 | No input yields `allow` for class 3 | Rule 6: `if class == SystemCritical` becomes `if class == SystemCritical && present`, so an unattended class 3 call falls through to a standing approval | The unit test `a_tool_call_claiming_class_3_or_4_is_never_allowed`. With unit tests out: five sweep tests, among them `class_3_and_class_4_are_never_allowed` and `unattended_class_3_is_always_refused` | 43155949036; sweep alone 43156377820 |
| 2 | The model hint never lowers | `base.max(Consequential)` becomes `Consequential` | Nine sweep tests, among them `the_model_hint_never_lowers_the_outcome`, `the_class_is_never_below_the_class_of_the_source` and `class_4_is_always_refused` | 43156504308 |
| 3 | Unattended class 2 runs only on an exactly matching standing approval | `StandingApproval::matches` no longer compares the arguments | `unattended_class_2_runs_only_with_an_exactly_matching_standing_approval` and the decision table | 43156629149 |
| 4 | An expired repair session permits nothing | `now < expires_at` becomes `now <= expires_at` | `a_privileged_command_runs_only_inside_an_open_repair_session` and the decision table | 43156753030 |
| 5 | Without the trusted path nothing above class 1 runs | Rule 5: `class >= Consequential` becomes `class > Consequential` | Six sweep tests, among them `without_the_trusted_path_nothing_above_class_1_runs_and_nothing_authenticates` | 43156885903 |
| 6 | A sensitive application is class 2 | `ui_class` asks whether the empty string is sensitive | **The decision table only.** No invariant. Finding F-24 | 43157022561 |

All five of the most important invariants are guarded: each mutation turned the
run red, through a test that names the invariant. The sixth was a probe of a
suspected gap and found one.

Two of the scratch runs proved nothing and are listed for completeness. Run
43156117221 tried to switch the unit tests off in `Cargo.toml`, which has no
effect under `--all-targets`; its result is that of run 43155949036. Run
43156247756 failed at the lint step because the reviewer's change removed a
crate's documentation; no test ran.

**The new rules.** After the fixes, three of the defects were put back, one line
each, to see whether the extended tests notice. The scratch commits are undone by
commit `62e670e`: `git diff 9a75cf3 62e670e -- services` is empty. The unit
tests were compiled out for runs 7b, 8 and 9. A test run stops at the first test
program that fails, so `tests/review.rs`, which holds a test for each of these
defects, was not reached in any of them.

| # | Rule | The one-line change | Noticed by | Run |
|---|---|---|---|---|
| 7 | A free-text privileged command is asked inside a repair session (F-02) | Rule 7d returns `allow` | The unit test `inside_a_repair_session_a_typed_operation_runs_and_a_command_is_asked`. With unit tests out: `a_privileged_request_runs_only_inside_an_open_repair_session`, `an_action_the_model_flagged_runs_only_on_an_exactly_matching_standing_approval` and the decision table | 43180926479; sweep alone 43181129568 |
| 8 | The built-in sensitive list applies (F-01) | `is_sensitive` compares the built-in list with the empty string | `a_sensitive_application_is_never_observed_or_acted_on_without_asking`, `the_class_is_the_class_of_the_source_raised_only_by_the_stated_rules`, `a_relaxation_only_turns_an_attended_class_2_ui_ask_into_allow` and the decision table | 43181464847 |
| 9 | A window of an administrator process is refused (F-04) | Rule 3b applies only while the session is locked | `a_window_of_an_administrator_process_is_never_observed_or_acted_on` and the decision table | 43181785490 |

Mutation 8 is mutation 6 repeated against the new tests: what one row noticed
before, three invariants notice now.

## 5. The author's 38 interpretations

Each was checked against revision 4 and against the question "is this the
stricter reading?". The verdict on every one is in the right-hand column of the
table in [agent-policy.md, section 10](../architecture/agent-policy.md#10-interpretations).
In summary:

- **Overruled: 7a, 13, 33** — by revision 4 itself (60 minutes; no class 1
  relaxation; privileged requests logged in full).
- **Overruled in part: 25** — the hint now adds a confirmation where the user is
  present to give one (F-19). Kept for standing approvals.
- **Narrowed: 1** — the repair session replaces the per-action confirmation for a
  typed operation only (F-02).
- **Would overrule, not changed: 26 and 27.** Both concern the operations of the
  agent itself, which the memory and scheduler work is being built against.
  26: "schedule write" should be a proposal (N-3). 27: removing or changing what
  the user stated is not the stricter reading at class 1; it should be asked.
- **Kept: the other 31.** 5, 9, 30 and 31 are now the design's own text. 18
  (observing a sensitive application is asked) and 23, 28, 29 (refuse rather than
  defer without the trusted path) were looked at twice and are the stricter
  readings.

## 6. What was not reviewed and not tested

- **Nothing was run locally.** No fuzzing, no property testing beyond the
  enumerated sweep and the hand-picked hostile strings, no coverage measurement,
  no Miri. CI has none of these. The statement "the sweep is exhaustive" means
  exhaustive over its generators, which a test now pins; it does not mean every
  string.
- **The callers.** No orchestrator, privileged helper, sandbox, compositor
  support or policy-owning component exists. Every obligation the engine places
  on them (ADR-0012, consequences; agent-policy.md, limits 1 to 5, 12 and 13) is
  unverified.
- **The names in the built-in lists.** The application identifiers in
  `DEFAULT_SENSITIVE_APPLICATIONS` and `DEFAULT_EXCLUDED_APPLICATIONS`, and the
  unit, file and package names in `PROTECTED_TARGETS`, were written from the
  reviewer's knowledge of upstream names. They were not checked against current
  upstream packages or against the image, which AGENTS.md requires before
  anything depends on them. A wrong identifier fails open for that application.
- **The fixes themselves.** The changes in this pull request were written by the
  reviewer and reviewed by nobody else. The new sweep found no disagreement with
  them, and section 4 shows three of the new rules are guarded; that is evidence,
  not a second review.
- **Revision 5 of the design.** It was merged during the review. The engine was
  not reviewed against it. Three items of its section 18 bear on the engine and
  are listed in agent-policy.md, limit 16.
- **Other components.** `crates/uos-core` was changed only to add the constant
  constructor issue #8 asks for. No other component was read beyond checking that
  none depends on this engine's types (none does at the reviewed revision of
  `main`; the storage service's manifest was checked against the stricter loader
  and loads).
- **The validator in `tests/schemas.rs`** was written for this review and has not
  been compared with an established JSON Schema implementation.
- **Runtime cost.** The sweep takes about two minutes in CI. It was not profiled.

## 7. Changes proposed to documents outside this pull request

- `docs/adr/README.md`: add the row for ADR-0012.
- `docs/architecture/ai-control-layer.md`:
  - section 11, audit log: add `UOS_AI_REQUEST` (the full statement of a
    privileged request) and say that `UOS_AI_SANDBOX` is `privileged` for a typed
    operation;
  - section 17, item 9: say who may mark a value secret (interpretation 44);
  - section 17, item 2, or section 6: say that the compositor reports, for a
    target window, whether its process has administrator rights, and that an
    unknown owner is treated as class 2;
  - section 5.6 or 17, item 8: say whether a class 1 workspace command of a
    locked unattended task is deferred or refused (the engine defers);
  - section 5.5: say that the record of a repair session carries its opening
    time and is marked ended by its owner on lock;
  - section 5.1: say that a control whose name cannot be read is treated as
    consequential, and that generic affirmations are;
  - section 17, item 1: say whether the built-in sensitive list can be reduced
    by the user (the engine says no);
  - section 5.3: the sentence "relax class 2 handling" could add that a
    relaxation does not apply to an action the model flagged.
