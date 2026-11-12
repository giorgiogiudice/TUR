# pax-3 — P@x as a session service; the key, the limit of a day, the schedule runner

```text
TUR DEVELOPMENT REPORT

Task ID:          pax-3
Subsystem:        AI control layer: the session service org.tur.Agent1, the
                  key in the Secret Service, provider and model configuration,
                  the per-day spending limit, consent, and the runner that
                  makes reminders and scheduled tasks fire
Build ID:         not applicable: this task builds no image
Source revision:  11c63ce (the code; this report is the commit after it) (feat/pax-service, pull request 52)

SUMMARY
- What changed: tur-pax gains a session service. `tur-pax serve` serves
  org.tur.Agent1 on the session bus, started by the bus through a systemd
  user unit. A desktop can start a task from text, follow its steps as
  signals, cancel or pause it, read the state, the spending and the
  activity, switch P@x on with a recorded consent, choose the provider and
  the model, set and remove the key, set the limits, confirm or dismiss the
  reminders and tasks P@x proposes, create and remove schedules, read "while
  you were away", and keep or forget memory. The key lives in the Secret
  Service only. A day's spending is limited before every request from the
  providers' reported usage and a price list that is configuration. A
  schedule runner shows reminders as desktop notifications at their time,
  late ones too, and runs scheduled tasks unattended through the same loop,
  woken by a transient user timer and never by polling.
- Why it changed: issue 2 (a full assistant with memory, reminders and
  scheduled tasks), issues 30, 40 and 43 (the runner, the D-Bus interface,
  keyring storage, the per-day limit), desktop task D-17 (a UI on
  org.tur.Agent1) and D-19 (P@x's settings pages).
- What it does not show: NO REAL PROVIDER WAS CALLED, here or anywhere in the
  development of this change. No key exists for CI. Every model is a script
  on the runner.

FILES
- Added: services/tur-pax/src/{bus,dbus,daemon,service,settings,consent,
  keyring,spending,scheduler,notify,wake}.rs;
  services/tur-pax/interfaces/org.tur.Agent1.xml;
  services/tur-pax/units/{tur-pax.service,org.tur.Agent1.service,
  tur-pax-catchup.service,tur-pax-catchup.timer};
  services/tur-pax/data/models.json (an example, dated);
  services/tur-pax/tests/{service,scheduler,agent1_interface}.rs;
  schemas/pax-settings.schema.json, schemas/pax-models.schema.json;
  tests/pax/service_e2e.py; tests/pax/live.py, tests/pax/live_ping.py and
  .github/workflows/pax-live.yml (the manual real-provider workflow, not
  run); docs/architecture/pax-service.md;
  docs/reports/pax-3.md
- Modified: services/tur-pax/src/{orchestrator,registry,exec,trail,faults,
  textui,cli,lib}.rs and tests/{common/mod,orchestrator,tools}.rs;
  services/tur-pax-memory/src/{time,trigger,lib}.rs;
  services/tur-pax-providers/src/{secret,lib}.rs;
  .github/workflows/pax.yml; docs/architecture/{pax-orchestrator,
  pax-providers,pax-memory-and-schedules,agent-policy}.md
- Removed: nothing

BUILD
- Command: cargo clippy --all-targets --all-features -- -D warnings; cargo
  test --all-targets --all-features; cargo test --doc (workflow "rust",
  every component); cargo build --locked for tur-pax, tur-pax-memory and the
  five tool components (workflow "pax")
- Result: see RUNS
- Warnings: none from the compiler or clippy on the head commit

TESTS
- Unit and integration tests, workflow "rust": pass. tur-pax 402 tests in 16 targets (219 unit, 171
  integration, 12 doc), among them new tests/service.rs (15),
  tests/scheduler.rs (8) and tests/agent1_interface.rs (4); tur-pax-memory
  153; tur-pax-providers 176. Every other component unchanged and passing
- End to end, workflow "pax": the 17 scenarios of pax-1 and pax-2, unchanged
  (349 checks), then the session service (103 checks), on a
  hosted Ubuntu 24.04 runner.
- Against a real provider: NOT RUN. No key exists.
- Against KWallet, Plasma's notification service, dbus-broker, Fedora or the
  TUR image: NOT RUN. The Secret Service was gnome-keyring, the bus
  dbus-daemon, the notification service a stand-in written in the test.
- Suspend and resume, a real power cut: NOT RUN.
- Boot tests: NOT RUN. Nothing here can affect boot; the units are not in
  the image.
- Hardware tests, performance tests: NOT RUN, except one measurement: the
  service used 0.000 s (as measured from /proc) of processor time in ten seconds of waiting.

DIAGNOSTICS
- Log paths: GitHub Actions runs below; artifact "pax-evidence" holds
  service-checks.json, service-signals.json, service-notifications.json,
  service-replies.json, service-bus.log (the service's own output) and
  introspection.xml
- Diagnostic bundle path: none
- Relevant error IDs: new TUR-AI-SPEND-001; TUR-AI-SERVICE-001 to -009;
  TUR-AI-KEY-001 to -005; TUR-AI-RUNNER-001, -002. New audit events
  held_for_return, schedule_run, setting_changed; each trail line gains
  __REALTIME_TIMESTAMP.

RECOVERY
- Rollback procedure: revert the merge. No image, boot path or stored format
  of another component changes. The audit trail gains three event kinds and a
  field; a reader that ignores what it does not know is unaffected. The
  settings, spending and model files are new and may be deleted; P@x is then
  off. A key stored in the password store stays there until removed.
- Recovery tested: a damaged settings file leaves P@x off with
  TUR-AI-SERVICE-009 and is not overwritten
  (tests/service.rs::settings_that_cannot_be_read_leave_p_a_x_off_and_say_why);
  a spending record that cannot be written uses the day up
  (spending::tests::a_record_that_cannot_be_written_uses_the_day_up).

KNOWN LIMITATIONS
- docs/architecture/pax-service.md, sections 13 and 14.

NEXT SAFE ACTION
- Install the program, its units and the model list in the image and run
  tests/pax/service_e2e.py there (feat/components-in-image). Then a
  conformance run against the three real providers with the owner's key,
  outside CI (issues 24 and 40). Then the policy owner and the trusted
  session, which are what stand between this and anything above class 1, and
  what the settings that loosen P@x should be confirmed on.
```

## Runs

For commit `11c63ce`, the code of this pull request. The commit that adds
this report follows it and changes no code; its runs are on the pull request.

| Workflow | Run | Result |
|---|---|---|
| `rust` | 43252232997 | Pass, every component. `tur-pax`: 402 tests in 16 targets (219 unit tests, 171 integration tests, 12 doc tests); the invariant sweep about four minutes. `tur-pax-memory`: 153. `tur-pax-providers`: 176 |
| `pax` | 43252233003 | Pass: 349 checks of the 17 earlier scenarios, 0 failed; 103 checks of the session service, 0 failed. User service manager running: the wake timer was the service's own |

Earlier runs on the branch, before the history was squashed into one commit:

| Commit | `rust` | `pax` | What happened |
|---|---|---|---|
| `b66db11` | 43247521250 | 43247521427 | `rust`: two clippy findings (`manual_is_multiple_of`, `type_complexity` in a test) and rustfmt in three components; the memory and provider tests passed. `pax`: the earlier scenarios passed (the service test was not there yet) |
| `d2e831f` | 43248838782 | 43248838792 | `rust`: one unit test of mine used a text the secret detector does not flag. `pax`: 96 of 98 service checks passed; the two failures found a real ordering defect: the notification after a scheduled task went out before its run was recorded |
| `3360f5a` | 43249581435 | 43249581473 | `rust`: a source check of mine was too broad (it matched `DeferReason::NoStandingApproval`). `pax`: pass |
| `91c0cba` | 43250176093 | 43250176160 | `rust`: two expectations of mine in `tests/scheduler.rs` left out the first occurrence; the runner was right. `pax`: one check read a signal before it arrived (a race in the test) |
| `48d57f1` | 43250949812 | 43250949744 | `rust`: every test passed; rustfmt only. `pax`: pass, 103 service checks |

Six pushes of code. One defect outside the tests was found by CI, the
ordering one above; the rest were my tests, lints and formatting.

## The real-provider workflow (not run)

Added at the owner's request (ai-control-layer.md section 23, item 4):
`.github/workflows/pax-live.yml` with `tests/pax/live_ping.py` and
`tests/pax/live.py`. **It has not been run**: it starts by hand only, and
this change did not start it.

- **Trigger:** `workflow_dispatch` only; `permissions: contents: read`.
  Start it from the Actions tab (workflow "pax-live", "Run workflow", choose
  the model) or with `gh workflow run pax-live.yml -f model=claude-haiku-5-5`.
- **Key:** the repository secret `CLAUDE_KEY`, mapped to the environment
  variable `TUR_PAX_API_KEY` of the steps that need it; never on a command
  line, never printed. Without it, a notice and a successful end with
  nothing called.
- **First step:** one minimal request (one short message, at most 8 output
  tokens, the adapter's pinned API version `2026-10-21`). If the API rejects
  the key, the step reports the HTTP status and the error type (for example
  `HTTP 401, error type authentication_error`) and the job stops. The key's
  prefix is not the usual one; nothing in TUR checks a prefix.
- **Then:** the requests of scenarios 03, 06 and 07 through `tur-pax run`
  with the real Anthropic adapter, the test service's tools and the command
  tool, in the real sandbox. Checked whatever the model does: the task ends
  normally or at a limit; no decision above class 1 is allowed; nothing
  above class 1 is performed; the trail names the adapter; the key is in no
  transcript, error output, trail or file. A last step searches every file
  the job wrote, its own log included, for the key; the evidence is
  uploaded only if that passes.
- **Cap, enforced in `live.py`:** 5 requests and 16 000 tokens per run (each
  task's `--max-steps` and `task_limit.max_tokens` are what is left; retries
  off), 400 output tokens a response; only `claude-haiku-5-5` (default) and
  `claude-sonnet-5-5` can be named, both read from Anthropic's models page on
  2026-11-12. A response the meter cuts was already under way, so a run can
  pass 16 000 tokens by one request.
- **Cost per run, worst case** at the prices of 2026-11-12: about 24 000
  input and 2 000 output tokens plus the first step's 23: under 0.005 USD
  with `claude-haiku-5-5`, under 0.07 USD with `claude-sonnet-5-5`.

## What each part is, and what proves it

| Part | Built | Proved by |
|---|---|---|
| `org.tur.Agent1` served by `tur-pax serve`, activated by the bus | `dbus.rs`, `daemon.rs`, `units/` | `pax` `interface`: the bus starts the service from an activation file on the first call; `Introspect` is the committed file byte for byte; `Ping`; `GetAll`; `Set` refused; wrong arguments `InvalidArgs`; unknown method `UnknownMethod`; the process is `tur-pax serve` |
| The introspection file and the dispatch tables agree | `interfaces/org.tur.Agent1.xml`, `dbus::METHODS`, `SIGNALS`, `PROPERTIES` | `tests/agent1_interface.rs::the_served_interface_is_the_committed_file` |
| Start a task, stream its steps | `Core::start_task`, `TaskObserver`, `BusEvents` | `tests/service.rs::a_task_streams_its_steps_and_its_answer`; `pax` `task` (signal order, tool, class, decision, audit identifier, the real tool program in the real sandbox) |
| Cancel at once | `Core::cancel_task`, `StoppableExecutor`, the loop discarding a response after a cancellation | `tests/service.rs::a_cancelled_task_stops_at_once_and_its_command_is_killed`; `pax` `stop` (TaskFinished within two seconds, the sandboxed `sleep` killed, no further request) |
| State, spending, running task, activity | `Core::status`, `Core::activity` | `tests/service.rs` (several); `pax` `task`, `spending` |
| The caller rule | `dbus::admits`, `Callers` | `dbus::tests::only_a_caller_of_the_same_user_is_served`; `pax` `other-user` (the user `nobody` refused with `TUR-AI-SERVICE-005`, the service's own user answered) |
| Nothing above class 1 by any path | One loop in `Core::run_job`; P@x's own tools class 1 | `tests/service.rs::nothing_above_class_one_runs_through_the_service_attended_or_scheduled`, `::p_a_x_s_own_tools_have_the_class_the_engine_gives_them`, `::a_standing_approval_cannot_be_given_through_the_service`; `tests/agent1_interface.rs::every_way_into_the_service_ends_in_one_task_loop_that_grants_nothing`, `::no_method_gives_back_a_key_and_none_asks_for_an_approval`; the existing sweep `tests/orchestrator.rs::nothing_above_class_one_runs_whatever_the_policy_and_the_situation` and `tests/invariants.rs`, unchanged; `pax` `scheduled-task` |
| The key in the Secret Service | `keyring.rs`, `tur-pax key` | `keyring::tests` (form, no store, error names without text); `tests/service.rs::the_key_is_in_no_file_no_event_no_status_and_no_trail`; `pax` `switch-and-key` (set through the command line on standard input, found by the service in gnome-keyring, refused when not a key, removed, set through the interface) and `task` (sent to the provider in its header from the store); `pax` `key`: the key is in no reply, signal, notification, program output or file (116 files) |
| No secret service: a clear error | `TUR-AI-KEY-001` | `keyring::tests::without_a_password_store_there_is_a_clear_error_and_no_other_place`; `tests/service.rs::p_a_x_refuses_tasks_until_it_is_switched_on_with_the_current_consent` |
| Provider and model configuration | `settings.rs`, `SetProvider`, the model list | `settings::tests` (all); `pax` `interface` (the example list is read with its date) |
| The limit of a day | `spending.rs`, `Budget` in the loop | `spending::tests` (seven: per response, restart, the next day, a clock set back, a changed zone, a record that cannot be written); `tests/service.rs::a_task_stops_at_the_limit_of_the_day_with_its_code`; `pax` `spending` |
| Consent | `consent.rs`, `Settings::enable`, `refusal` | `consent::tests`; `settings::tests::p_a_x_is_off_until_it_is_switched_on_with_the_current_consent`, `::consent_for_one_provider_or_one_text_does_not_cover_another`; `tests/service.rs::p_a_x_refuses_tasks_until_it_is_switched_on_with_the_current_consent`; `pax` `switch-and-key` |
| Reminders fire as notifications, late ones too | `scheduler.rs`, `notify.rs`, `Core::tick` | `tests/scheduler.rs` (eight, with a clock moved by hand: both daylight-saving changes, a changed zone, a clock set back, off for days, skip, notification service down, a busy slot); `tests/service.rs::a_reminder_proposed_in_conversation_fires_only_once_the_person_confirmed_it`, `::a_reminder_that_came_due_while_the_computer_was_off_says_when_it_was_due`; `pax` `reminder` (a reminder proposed by the scripted model, confirmed, and taken off the bus as a notification) |
| Scheduled tasks run unattended under sections 18 and 22 | `Core::run_scheduled` through the one loop | `tests/service.rs::nothing_above_class_one_runs_through_the_service_attended_or_scheduled`, `tests/scheduler.rs::a_scheduled_task_that_cannot_start_stays_due`; `pax` `scheduled-task` (class 0 ran, class 2 kept for the person, class 3 refused and not kept, the run in "while you were away", in the trail, and told by a notification) |
| Woken by a timer, not polling | `wake.rs`, `tur-pax-catchup.timer` | `wake::tests`; `pax` `reminder` and `scheduled-task`: the service's own transient timer fired both (the workflow started a user service manager); `pax` `idle` |
| Planted text creates no schedule, no recalled memory | `keep_note` records pending content; proposals wait | `tests/service.rs::text_planted_in_a_tool_result_creates_no_schedule_and_no_memory`; `pax` `planted` |

## Dependencies

None added. `zbus` was considered and not used; the reason is
pax-service.md, section 10. No file under `docs/dependencies/` and no line of
`THIRD_PARTY_NOTICES.md` changes. The lockfiles of `tur-pax`,
`tur-pax-memory` and `tur-pax-providers` are unchanged.

## What was verified on the web

All on 2026-11-12, from the providers' own pages:

| What | Source |
|---|---|
| Anthropic model names `claude-opus-5-5`, `claude-sonnet-5-5`, `claude-haiku-5-5`; prices per million tokens: Opus 5.5 4 / 20, cache hits 0.20; Sonnet 5.5 2 / 10, cache hits 0.10; Haiku 5.5 tiered by prompt length (0.10 / 0.50 up to 100 000 tokens, 0.50 / 2.50 above), so not given a price | <https://platform.claude.com/docs/en/about-claude/models/overview>, <https://platform.claude.com/docs/en/about-claude/pricing> |
| OpenAI `gpt-6.1-sol` 2.00 / 10.00, cached 0.10; `gpt-6-luna` 0.10 / 0.50, cached 0.01; short-context prices (up to 272K input tokens) | <https://developers.openai.com/api/docs/pricing> |
| Google `gemini-3.5-flash-lite` 0.30 / 2.50, caching 0.03; `gemini-4-carbon` 2.00 / 10.00 until 2026-11-30 and 4.00 / 20.00 from 2026-12-01, so not given a price | <https://aistudio.google.com/docs/pricing> |
| Data terms pages named in the model list | Anthropic commercial terms (HTTP 200); OpenAI "Data controls in the OpenAI platform"; Google `https://aistudio.google.com/docs/terms` (the target of the old address) |
| zbus 6.2.1: MIT; asynchronous, its own runtime or Tokio; the blocking API removed in 6.0 | The crate's manifest, README and change log, from crates.io |

OpenAI's key page could not be fetched by a script (HTTP 403) and is not in
the model list. The text of the providers' terms was not read.

## Proposed text for ai-control-layer.md

Not edited here. Proposed as section 23, "Resolutions from building the
session service":

| # | Point | Resolution |
|---|---|---|
| 1 | Section 11 proposed `org.tur.Agent1` before the desktop design asked for more | The interface as built is pax-service.md, section 2: 29 methods, 9 signals, 3 read-only properties. `ConfirmationRequested` and `EndRepairSession` are left out until a trusted session exists. `ActionProposed` and `ActionCompleted` carry a step as JSON, not separate arguments |
| 2 | Who may call `org.tur.Agent1` | Callers the bus reports as the same Unix user. Any program in the session passes; the rule keeps out other accounts only. What keeps the model out is that its commands have no session bus and no tool reaches the interface |
| 3 | Section 17, item 5 puts the settings that loosen P@x under a separate component and the trusted screen | Until both exist, switching P@x on, choosing the provider, setting the key and raising a limit are methods of `org.tur.Agent1`, open to any program of the person. Standing approvals are not: no schedule created through the interface can hold one |
| 4 | Where the key lives (sections 7, 8) | The Secret Service of the session, one item per provider. Never a file. No store: a clear error, no fallback. The secret crosses the session bus unencrypted (the `plain` transfer), which protects as much as the encrypted one would on today's desktop |
| 5 | The per-day limit (sections 7, 10) | Kept by the session service from reported usage and a model list that is configuration. Checked before every request; written after every response; a day is a calendar day in the computer's zone; a record that cannot be written uses the day up; `TUR-AI-SPEND-001` |
| 6 | Consent (section 8) | Recorded with the version of the text and the provider; a new version or another provider needs a new acceptance. The text says that this build cannot see the screen, that scheduled tasks run while locked and send what they read, that kept content is re-sent, and that other programs in the session can see the screen |
| 7 | How a scheduled task is woken (section 16.2, "ordinary system timers owned by the user") | One transient user timer for the next occurrence of any schedule, set again after each look, plus a timer after sign-in and on clock or zone changes. Not one unit per schedule |
| 8 | What "late" means | More than 120 s after its occurrence: shown or run once, marked late, with the time it was due; or passed over if the schedule says to skip |
| 9 | What an unattended task keeps for the person (sections 5.6, 16.2, 22 item 3) | Class 2 actions and memory records on the person's word, with their arguments in the run record and not the trail; class 3 is refused and not kept |
| 10 | A model asking to remember something | Recorded as content waiting for the person, never as stated by the person, attended or not |
| 11 | A model proposing a schedule | A proposal on `org.tur.Agent1`, never a schedule, until a call outside the task confirms it |
| 12 | The audit trail | Gains `held_for_return`, `schedule_run`, `setting_changed`, and the time of each line |

## Follow-ups worth an issue

1. The image: install `tur-pax`, its four units (enable the catch-up timer
   for every user), the activation file and the model list; run
   `service_e2e.py` in the image (feat/components-in-image).
2. A conformance run against the real providers, with the owner's key.
3. The policy owner and the trusted screen: confirm the loosening settings
   there, and only then standing approvals.
4. "Check the key" for the setup flow (a billed one-request ping), a running
   task's live cost, run-now and edit for schedules, correct and forget-all
   for memory, "has this entry been sent".
5. Event triggers: something has to deliver device, network, update and
   unlock events to the runner.
6. KWallet and dbus-broker: run the key and the bus against them on Fedora.
7. A maintained model list: who keeps the prices, how often, signed how.
8. Decide between the own D-Bus code and zbus (pax-service.md, section 10).
9. Two sessions of one person share the transient timer names.
10. Keep proposals across a restart of the service.

## Found on the way

- `tur-pax-memory` rendered one unit per schedule and named a runner program
  that never existed; the runner built here needs neither. The renderer is
  left in place and unused.
- The schedule runner first sent the "while you were away" notification
  before the run it points at was recorded; the `pax` workflow caught it
  (a check read the run right after the notification and found nothing),
  and the notification now goes out after the record.
