# P@x session service

**Part of the [AI control layer](ai-control-layer.md)**, sections 3, 5.6, 7, 8, 10, 11, 16.2, 17 to 22
**Component:** `services/tur-pax` (modules `service`, `dbus`, `bus`, `daemon`, `settings`, `consent`, `keyring`, `spending`, `scheduler`, `notify`, `wake`)
**Builds on:** [pax-orchestrator.md](pax-orchestrator.md), [pax-providers.md](pax-providers.md), [pax-memory-and-schedules.md](pax-memory-and-schedules.md), [agent-policy.md](agent-policy.md)
**Serves:** the P@x chapters of [desktop-experience.md](desktop-experience.md) (section 9, tasks D-17 and D-19)
**Report:** [pax-3](../reports/pax-3.md)

This document describes the session service as built: `org.tur.Agent1` on the
session bus, the files it keeps, where the key lives, how a day's spending is
limited, and the runner that makes reminders and scheduled tasks fire.

**No real model has been called.** Every task in every test goes to a scripted
provider on the test machine. No key for Anthropic, OpenAI or Google exists in
CI. Nothing here says how well a real model does a task.

**Nothing above class 1 can run**, by any path through the service: a task a
person asks for, a scheduled task, or any method of the interface
([section 4](#4-authority-classes-0-and-1-by-every-path)).

**Not on the image yet.** The image installs neither the program, nor its
units, nor the model list (`image/**` belongs to other work). What it must
install is in [section 6](#6-units-start-and-stop).

---

## 1. What it is

```text
 desktop (tur-pax-ui, tur-settings, tur-notifier)        a timer of systemd --user
             │  org.tur.Agent1, session bus                       │ tur-pax schedule wake
             ▼                                                    ▼
 tur-pax serve ── dbus.rs: who is calling, which method ──▶ service.rs: Core
                                                              │
     settings.json, spending.json, audit.jsonl, the store ◀───┤
     Secret Service (the key)  ◀──────────────────────────────┤
     org.freedesktop.Notifications  ◀─────────────────────────┤
     the one task loop (orchestrator.rs) ◀────────────────────┘
          │ classes 0 and 1 only
          ▼
     tools and read-only commands, in the sandbox
```

One process per signed-in person, running as that person. It answers on the
session bus, runs one task at a time on a thread of its own, and has one more
thread that looks at the schedules when it is woken. It does not poll: with
nothing to do it waits on the bus and on a condition variable. The `pax`
workflow measures it using less than 0.05 s of processor time in ten seconds
of waiting.

| Module | Holds |
|---|---|
| `service` | `Core`: every operation of the interface as a plain function, the only place a task loop is built, P@x's own operations (proposals, notes), the schedule runner's side of a run |
| `dbus` | The interface: the method tables, the caller rule, the dispatch to `Core`, events as signals |
| `bus` | A small D-Bus implementation: wire format, messages, a connection ([section 10](#10-why-not-a-d-bus-crate)) |
| `daemon` | `tur-pax serve`, `tur-pax schedule wake`, `tur-pax key` |
| `settings` | The settings file, the model list, the paths |
| `consent` | The consent text and its version |
| `keyring` | The key in the Secret Service |
| `spending` | The spending record and the limit of a day |
| `scheduler` | What is due, what is done about it, when to look again |
| `notify` | The words and the call of a desktop notification |
| `wake` | The transient timer that wakes the runner |

## 2. The interface as built

`org.tur.Agent1` at `/org/tur/Agent1` on the session bus. The introspection
data is committed as
[`services/tur-pax/interfaces/org.tur.Agent1.xml`](../../services/tur-pax/interfaces/org.tur.Agent1.xml);
`Introspect` returns that file byte for byte, and a test holds it and the
dispatch tables together. Interface version 1.

Structured values are JSON text in a string. A method that fails replies with
the D-Bus error `org.tur.Agent1.Error.<CODE>`, where `<CODE>` is the
`TUR-AI-*` code with its hyphens as underscores, and the message
`<code>: <sentence>`. The sentence never holds a key and never repeats what a
model wrote.

### 2.1 Methods

| Method | In | Out | What it does | Refused with |
|---|---|---|---|---|
| `StartTask` | `s request`, `s options` | `s task` | Starts a task for a person who is there, on a thread of its own; returns at once. `options` is `""` or a JSON object: `grant_read` (folders or files the task's commands may read), `continue` (go on from the last conversation) | Off `SERVICE-001`; consent not current `-002`; nothing configured `-003`; a task running `-004`; a bad request or option `-006`; session locked `-008`; a file unreadable `-009`; no key `KEY-002`; no password store `KEY-001`; a folder that cannot be granted `SANDBOX-002` |
| `CancelTask` | `s task` (`""`: whatever runs) | | Stops the task at once: it is reported cancelled now, its sandboxed program is killed, a response still on its way is discarded | `SERVICE-007` |
| `Pause`, `Resume` | | | Pauses the running task at its next step; lets it go on. A task paused because the session locked stays paused until it is unlocked | `SERVICE-007` |
| `GetTask` | `s task` | `s details` | One of the last 20 tasks: request, steps, texts, outcome, usage, cost | `SERVICE-007` |
| `ListActivity` | `u limit` | `s activity` | The audit trail as a list of tasks, newest first, each with every entry: event, time, action, class, tier, decision, sandbox, result, code | `SERVICE-009` |
| `GetStatus` | | `s status` | Everything a panel header or a settings page shows ([2.4](#24-the-json-documents)) | never |
| `GetConsentText` | `s provider` | `s version`, `s text` | The text a person reads before switching P@x on, for the provider with that identifier | `SERVICE-006` |
| `Enable` | `s consent_version` | | Switches P@x on and records the acceptance of that version of the text, for the provider in use | `-003`; another version `-002`; `-009` |
| `Disable` | | | Switches P@x off: a running task is cancelled and no timer is left set | `-009` |
| `SetProvider` | `s provider`, `s model` | | Chooses a provider by its identifier ([section 9](#9-consent-provider-model-and-the-limit-of-a-day)) and a model name. Another provider withdraws the consent and switches P@x off | `-006`, `-009` |
| `SetLimits` | `s limits` | | JSON with any of `task_cost`, `day_cost`, `task_tokens`, `day_tokens`; `null` restores a default | `-006`, `-009` |
| `ListModels` | | `s models` | Every provider by identifier: its adapter family, name, origin (`listed`, `by_hand`, `implicit`), address, keyring item, `key_required`, declared capabilities, `data_terms` as TUR read them (`null` when not reviewed or added by hand), `data_terms_reviewed`, `runs_locally`, terms and key pages, models with their prices, declared capabilities, `can_act`, `can_see` and `notices`; the list's date and status | never |
| `SetKey` | `s provider`, `s key` | | Stores the key under the provider's keyring item (its identifier unless its entry names another), in place of any key there. An identifier no entry has yet is accepted | `SERVICE-006`, `KEY-001`, `-003`, `-004`, `-005` |
| `RemoveKey` | `s provider` | `b removed` | Removes it | as above |
| `HasKey` | `s provider` | `b set` | Whether one is stored. Never makes the store prompt | as above |
| `ListProposals` | | `s proposals` | Reminders and scheduled tasks P@x proposed, waiting for the person | never |
| `ConfirmProposal` | `s proposal` | `s schedule` | Creates the schedule exactly as proposed | `SERVICE-007`; `SCHEDULE-*` (its time has passed, too many schedules) |
| `DismissProposal` | `s proposal` | | Drops it | `SERVICE-007` |
| `CreateSchedule` | `s definition` | `s schedule` | Creates a schedule the person wrote: the proposal part of `schemas/pax-schedule.schema.json`. With a standing approval in it: refused | `SERVICE-006`, `SCHEDULE-*` |
| `RemoveSchedule` | `s schedule` | | Removes a schedule and its runs | `-006`, `SCHEDULE-001` |
| `ListSchedules` | | `s schedules` | Each schedule with its next occurrence, its last run, its count of standing approvals | `SCHEDULE-*` |
| `GetAway` | | `s runs` | "While you were away": runs of scheduled tasks the person has not seen, with what was done and what waits for them and why. A reminder that was simply shown is not in it | `SCHEDULE-*` |
| `MarkAwaySeen` | `as runs` | | Records that the person has seen them | `-006`, `SCHEDULE-001` |
| `Wake` | | | Looks at the schedules now. What the timers call | never |
| `ListMemory` | | `s memory` | Whether memory is on; every entry with its kind, state, origin and dates | `MEMORY-*` |
| `SetMemoryEnabled` | `b enabled` | | Turns memory on or off | `MEMORY-*` |
| `ConfirmMemory` | `s entry` | | Keeps an entry that waits for the person. It stays marked as taken from content | `-006`, `MEMORY-*` |
| `ForgetMemory` | `s entry` | | Forgets an entry | `-006`, `MEMORY-*` |

Also answered: `org.freedesktop.DBus.Introspectable.Introspect`,
`org.freedesktop.DBus.Peer.Ping` and `GetMachineId`, and
`org.freedesktop.DBus.Properties.Get` and `GetAll`. `Set` is refused with
`PropertyReadOnly`. A call with the wrong arguments is refused with
`InvalidArgs`, an unknown method with `UnknownMethod`.

### 2.2 Signals

| Signal | Arguments | When |
|---|---|---|
| `TaskStarted` | `s task`, `s details` | A task began (a person's or a schedule's) |
| `TextAdded` | `s task`, `s kind`, `s text` | `interim`: the model's words between steps; `answer`: its answer; `notice`: a sentence of the system's, such as a limit that was reached. The UI keeps them apart (desktop design 9.2) |
| `ActionProposed` | `s task`, `s step` | A step was decided and the decision is in the trail; if allowed, it is about to run |
| `ActionCompleted` | `s task`, `s step` | The same step, ended: done, failed, refused, rejected |
| `TaskFinished` | `s task`, `s result` | The task ended |
| `ScheduleProposed` | `s proposal`, `s details` | P@x proposed a reminder or a scheduled task. Nothing is scheduled |
| `SchedulesChanged` | | Schedules, runs or proposals changed |
| `MemoryChanged` | | Memory changed |
| `StateChanged` | `s state` | The state changed. `org.freedesktop.DBus.Properties.PropertiesChanged` is sent with it |

### 2.3 Properties

| Property | Type | Value |
|---|---|---|
| `State` | `s` | `off`, `idle`, `working`, `paused` |
| `Enabled` | `b` | Whether P@x is switched on |
| `InterfaceVersion` | `u` | 1 |

### 2.4 The JSON documents

**A step** (`ActionProposed`, `ActionCompleted`, and `steps` of a task):
`index`, `tool`, `service`, `title` (the first sentence of the tool's
description, in plain words), `detail` (the exact action, escaped; a command
in full), `class`, `tier` (0 P@x's own, 1 tool, 4 command), `decision`
(`allowed`, `refused`, `confirmation_required`, `authentication_required`,
`deferred`, `rejected`), `reason`, `state` (`running`, `done`, `failed`,
`refused`, `needs_you`, `rejected`), `result`, `exit_code`, `output_bytes`,
`truncated`, `withheld`, `error_code`, `message`, `audit_id`
(`<task>#<n>`: the n-th record the task's trail had when the step was
decided).

**A task** (`GetTask`, `TaskStarted`, `TaskFinished`): `task`, `request`,
`attended`, `schedule`, `state` (`working`, `paused`, `finished`),
`started_at`, `finished_at`, `outcome`, `error_code`, `message`, `answer`,
`texts`, `steps`, `usage` (`input_tokens`, `output_tokens`,
`cached_input_tokens`), `cost`, `currency`.

**The status** (`GetStatus`): `interface_version`, `enabled`, `state`,
`ready` (switched on, consent current, a key set), `problem` (`error_code`,
`message`: why a task cannot start, if it cannot), `consent` (`current`,
`text_version`, `accepted_version`, `accepted_for`, `accepted_at`), `provider`
(the identifier), `provider_name`, `adapter`, `provider_origin`, `model`, `model_listed`, `model_tested`, `price_known`, `key`
(`set`, `not_set`, `unknown`, `not_needed` for a server that takes no key),
`abilities` (`act`, `see`: what the model's entry declares, `null` when
nothing is; `notices`: the plain sentences that say what P@x will not
attempt with it), `session_locked`, `running`, `spending`
(`currency`, `today` and `month` each with `cost`, `input_tokens`,
`output_tokens`, `responses`; `day`; `day_limit` and `task_limit` each with
`cost` and `tokens`; `day_limit_reached`; `prices_as_of`; `prices_status`),
`proposals` (a count).

The others are listed member by member in `service.rs`, beside the function
that builds each.

## 3. Who is served

**The rule.** A call is answered only if the message bus reports, through
`GetConnectionUnixUser`, that the connection it came from runs under the same
Unix user as the service. A call from another user, from a connection the bus
cannot place, or to a service that cannot read its own user, is refused with
`TUR-AI-SERVICE-005`. Tests: `dbus::tests::only_a_caller_of_the_same_user_is_served`;
in the `pax` workflow, a client run as the user `nobody` on a bus that lets
other users connect is refused, and the same call from the service's own user
is answered.

**What it is worth.** On today's desktop every program the person runs is that
user, and any program in the session can reach the session bus (addendum A
section 19, item 1). Every one of them can call every method above: start a
task, switch P@x on, set or remove the key, raise a limit, create a schedule,
keep a note. The rule keeps out other accounts and nothing else. The design
wants the loosening acts (switching on, raising a limit) confirmed on a
trusted screen by a separate component; neither exists
([section 13](#13-where-the-desktop-design-and-the-ai-design-were-silent-or-differ)).

**What keeps the model out** is not the bus rule:

- The commands and tool programs P@x runs have no session bus: the read-only
  sandbox binds no `/run/user`, has a network namespace of its own, and is
  started with an empty environment (pax-orchestrator.md, section 5; proved in
  the `pax` workflow with a listener on a stand-in bus socket).
- No tool calls this interface. P@x's own tools are three, all class 1, and
  none takes effect without the person ([section 4](#4-authority-classes-0-and-1-by-every-path)).
- Whatever reaches the interface and runs a task runs it through the one loop
  that performs classes 0 and 1 only.

## 4. Authority: classes 0 and 1, by every path

There is one place a task loop is built: `Core::run_job`. A task a person asks
for (`StartTask`) and a scheduled task (the runner) both end there. The only
thing it tells the loop about the circumstances is a `Situation`: attended,
or unattended with the session's lock state. The loop builds the engine's
context from that with no trusted session, no repair session and no standing
approval (pax-orchestrator.md, section 3). The service has no field, option,
file or method that could add any of them.

| Entry point | What it can cause | Above class 1? |
|---|---|---|
| `StartTask` | A task, attended | No: the loop |
| `Wake`, the timers, a schedule that comes due | A reminder shown; a scheduled task, unattended | No: the loop, unattended |
| `CreateSchedule`, `ConfirmProposal` | A schedule exists | No: a schedule only says when a task runs. A standing approval is refused, so no schedule can hold one |
| `Enable`, `SetProvider`, `SetLimits`, `SetKey`, … | Settings change | No action is performed |
| `ConfirmMemory`, `ForgetMemory`, `SetMemoryEnabled` | Memory changes | No action is performed; memory is data, never authority |

**P@x's own tools.** The service offers the model three tools of its own, all
declared under `org.tur.Pax1` and turned by the registry into the engine's own
operations, whose class the engine fixes:

| Tool | Engine operation | Class | What it does |
|---|---|---|---|
| `propose_reminder` | `ScheduleProposal` | 1 | Records a proposal. Nothing is scheduled until the person confirms it |
| `propose_scheduled_task` | `ScheduleProposal` | 1 | The same, for a task. A proposal cannot hold a standing approval |
| `keep_note` | `MemoryRecordStated` (an entry waiting for the person) | 1; deferred unattended | Records an entry of kind *from content*: pending, untrusted, never recalled until the person keeps it. Offered only while memory is on |

A model cannot record that the person stated something: the kind is fixed by
the service, not taken from the model. `tur-pax run` and `chat` offer none of
these tools: nothing there could show a proposal to anyone.

**Tests** (all in CI):

| What | Test |
|---|---|
| Every tool, attended and unattended through the service: no class 2 or 3 decision allowed, no class 2 or 3 program run | `tests/service.rs::nothing_above_class_one_runs_through_the_service_attended_or_scheduled`; `pax` `scheduled-task` |
| P@x's own tools have the engine's classes; a manifest cannot take their names | `tests/service.rs::p_a_x_s_own_tools_have_the_class_the_engine_gives_them` |
| One task loop, built with nothing that lifts authority; the service's situations are only "a person asked" and "nobody is there"; the bus layer reaches nothing that runs | `tests/agent1_interface.rs::every_way_into_the_service_ends_in_one_task_loop_that_grants_nothing` |
| No method gives back a key; none approves, confirms an action or opens a repair session | `tests/agent1_interface.rs::no_method_gives_back_a_key_and_none_asks_for_an_approval` |
| The existing sweep over the whole tool set | `tests/orchestrator.rs::nothing_above_class_one_runs_whatever_the_policy_and_the_situation` (unchanged) |
| No standing approval can be given through the interface | `tests/service.rs::a_standing_approval_cannot_be_given_through_the_service` |
| Planted text cannot create a schedule or a memory P@x recalls | `tests/service.rs::text_planted_in_a_tool_result_creates_no_schedule_and_no_memory`; `pax` `planted` |

## 5. The key

The key is kept in one place: an item of the person's password store, reached
through the freedesktop Secret Service API on the session bus. On Plasma that
is KWallet. **It is never written to a file by P@x**: not to the settings, the
spending record, the audit trail, a log or a diagnostic bundle. It is never in
a reply or a signal of `org.tur.Agent1`.

| | |
|---|---|
| The item | In the collection the alias `default` names (or `login` if `default` is not set); label `P@x: key for <item>`; attributes `xdg:schema=org.tur.Pax.ProviderKey`, `application=org.tur.Pax`, `provider=<item>`, where the item is the provider's keyring item: its identifier unless its entry names another |
| Set, replace | `SetKey`, or `tur-pax key set <provider>` with the key on standard input (never on the command line, where every program of the person could read it in the process list). `CreateItem` with `replace` |
| Remove | `RemoveKey`, `tur-pax key remove <provider>` |
| Is one set | `HasKey`, `tur-pax key status <provider>`, `GetStatus` (`key`). Asked of the store as a whole, which lists locked items too, so the question never makes the store prompt |
| Read | Only by the service, for a task: into the provider layer's `ApiKey`, which cannot be printed, cloned or serialised and is overwritten when dropped |
| No password store | `TUR-AI-KEY-001`, "This computer's password store is not available, so P@x cannot keep or read your key." Nothing falls back to a file |
| Locked, prompt dismissed | `TUR-AI-KEY-003`. The store asks the person; P@x waits up to two minutes and cannot answer the prompt |
| Not a key | `TUR-AI-KEY-004` (empty, a space, a character a header cannot hold). The text is not repeated |

**What is not protected.** The secret goes to and from the store in the
`plain` transfer of the Secret Service: unencrypted on the bus. The encrypted
transfer would hide it only from a program that watches the bus, and every
program of the person can ask the store for the item directly, exactly as P@x
does. On today's desktop the store, not the transfer, decides who reads it,
and it lets the person's programs read it. The buffers a message passes
through are not overwritten. `SetKey` carries the key in a method call, which
any program of the person that monitors the bus can see.

**Tested** against gnome-keyring 46.1 (its `secrets` component), started on the
`pax` workflow's private bus with a new login keyring: set through the command
line, found by the service, refused when it cannot be a key, removed, set
through the interface, read for a task and sent to the scripted provider in
its header. Then every reply, signal, notification and program output of the
run, and every file under the work directory, the evidence directory and the
runtime directory (116 files in the first green run), is searched for the key:
it is in none. KWallet was not run.

## 6. Units: start and stop

| File | Installed as | Says |
|---|---|---|
| `units/tur-pax.service` | `/usr/lib/systemd/user/tur-pax.service` | `Type=dbus`, `BusName=org.tur.Agent1`, `ExecStart=/usr/bin/tur-pax serve`, `Restart=on-failure`, `NoNewPrivileges=yes`. Not enabled, not wanted by anything |
| `units/org.tur.Agent1.service` | `/usr/share/dbus-1/services/org.tur.Agent1.service` | D-Bus activation: the bus starts the user unit when the name is first called |
| `units/tur-pax-catchup.service` | `/usr/lib/systemd/user/` | `ExecStart=/usr/bin/tur-pax schedule wake`, one shot |
| `units/tur-pax-catchup.timer` | `/usr/lib/systemd/user/`, enabled for every user (`systemctl --global enable`) | `OnStartupSec=30s`, `OnClockChange=true`, `OnTimezoneChange=true`. No period |

**With P@x off** nothing runs: nothing calls the name, and `tur-pax schedule
wake` reads the settings file and ends without touching the bus. That is the
one trace of P@x in an idle session of a person who never turned it on: a
one-shot command after sign-in.

**The wake timer.** After every look at the schedules, the service sets one
transient timer of the person's service manager for the next occurrence of
any schedule (`systemd-run --user --on-calendar=<UTC instant>`, after stopping
the one before) that runs `tur-pax schedule wake`. The instant is absolute and
in UTC: the recurrence rules of `tur-pax-memory` stay the only ones. The unit
is given the session bus and the configuration directory of the service, and
nothing of the schedule. The `pax` workflow starts a user service manager on
its runner (`loginctl enable-linger`) and the reminder and the scheduled task
of the end-to-end test were woken by this timer.

**Not verified:** that a calendar timer fires on resume from suspend after
its instant passed (taken from `systemd.timer(5)`; no suspend in CI), and the
catch-up timer itself (it was not loaded in any test; its command was run by
the test in its place where needed).

## 7. Files

| File | Holds | Written by | Mode |
|---|---|---|---|
| `$XDG_CONFIG_HOME/tur/pax/settings.json` | Switch, consent, provider identifier, model, gateway, output limit, limits, providers the person added. Schema `schemas/pax-settings.schema.json`. Never a key | The service, on a call; whole or not at all (written beside, flushed, renamed) | 0600, directory 0700 |
| `$XDG_CONFIG_HOME/tur/pax/policy.json` | The person's policy (agent-policy.md). Read, never written. Unusable: observe only | Not the service | |
| `$XDG_STATE_HOME/tur/pax/spending.json` | Per day: responses, tokens, cost, currency. 62 days. No words, no key | The service, after every response | 0600 |
| `$XDG_STATE_HOME/tur/pax/audit.jsonl` | The audit trail of pax-orchestrator.md section 7, plus three events ([section 8](#8-audit)); each line now says when it was written (`__REALTIME_TIMESTAMP`, microseconds, the journal's name for it) | The service | as created |
| `$XDG_DATA_HOME/tur/pax/` | Memory, schedules, runs (pax-memory-and-schedules.md) | The service | 0600 / 0700 |
| `/etc/tur/pax/models.json`, else `/usr/share/tur/pax/models.json` | The model list. Schema `schemas/pax-models.schema.json`. The repository's `services/tur-pax/data/models.json` is an **example** read on 2026-11-12 | Never written | |

A settings file that cannot be read leaves P@x off and says why
(`TUR-AI-SERVICE-009`); nothing overwrites it. A spending record that cannot
be read stops every task: a limit that cannot be kept is treated as reached.

## 8. Audit

Three events are new (`trail.rs`):

| `TUR_AI_EVENT` | When | Fields |
|---|---|---|
| `held_for_return` | An unattended task proposed an action it may not take alone; it was not performed and is kept for the person | `TUR_AI_ACTION`, `TUR_AI_CLASS`, `TUR_AI_REASON` |
| `schedule_run` | The runner handled a schedule that was due | `TUR_AI_SCHEDULE`, `TUR_AI_KIND`, `TUR_AI_OUTCOME` (`completed`, `deferred`, `failed`, `skipped_missed`, `still_due`), `TUR_AI_LATE`, `TUR_AI_HELD`, `TUR_AI_SCHEDULED_FOR`, `TUR_AI_ATTENDED=no`, `TUR_ERROR_CODE` |
| `setting_changed` | The person changed the switch, the provider, the key, a limit, a schedule or a memory entry | `TUR_AI_SETTING`, `TUR_AI_VALUE` (`on`, `set`, `removed`, …: never a value that is a secret, never a key) |

A scheduled task's own task records carry `TUR_AI_ATTENDED=no`. The arguments
of an action held for the person go to the run record they read on return,
never to the trail.

## 9. Consent, provider, model and the limit of a day

**Consent.** P@x is off until switched on. `Enable` takes the version of the
consent text the person read; it must be the current one
(`consent::CONSENT_TEXT_VERSION`, now `2`: version 2 added what each
provider's terms say). The acceptance is recorded with
that version, the provider the text named and the time. A task is refused
while P@x is off (`SERVICE-001`), when the text has a newer version or the
provider changed (`SERVICE-002`), or when nothing is chosen (`SERVICE-003`).
The text is the desktop design's consent screen (9.12, step 3) with what
addendum A sections 18.5, 19.1 and 21.6 require it to say, one paragraph of
this build's own ("P@x cannot see or use the screen yet"), and the stop key
sentence left out because there is no stop key.

**What each provider keeps** is said per provider (ADR-0004, amended), from
the `data_terms` of its entry: whether it keeps what it is sent, for how
long, whether it trains on it, and where it processes it, "as TUR read them
on" the date, with a line that its own terms govern. A statement the pages
did not make reads "not stated in what TUR read". An entry without reviewed
terms (Anthropic's, OpenAI's and Google's in the example list among them)
reads "<name>'s terms are not reviewed by TUR"; so does **every provider the
person added by hand, whatever its entry claims**, and a listed provider
reached through a gateway address of the settings, because the review was
of the entry's own address. A preset that runs on this computer at a
loopback address says so instead: nothing goes to a company. The
OpenAI-compatible family's paragraph on storing says that its interface has
no setting that asks a service not to store a conversation. Consent is
recorded per provider identifier: choosing another provider withdraws it.

**Provider and model.** A provider is an identifier with an entry
(ADR-0004, amended 12 November 2026): the adapter family that speaks its
interface (`anthropic`, `openai`, `google`, or `openai-chat` for any
OpenAI-compatible service), its name, its address if it is not the family's
public one (`openai-chat` has none and needs one), the password store item of
its key (its identifier unless named), whether it needs a key at all
(`key_required: false` for a server that takes none, `openai-chat` only),
whether the model's reasoning is sent back (`send_back_reasoning`,
`openai-chat` only, for DeepSeek), declared capabilities for the provider and
for each model (tool calling, streamed tool calls, image input, context
size), its data terms, its terms and key pages, and its models with their
prices. **Capabilities are acted on:** a model that declares no tool calling
is offered no tools or commands, and the task says why
([pax-providers.md](pax-providers.md), section 14.4). **Keyring items**
may be any provider identifier, or 1 to 64 lower-case letters, digits, `.`,
`-` and `_`, starting with a letter or a digit, with no two dots in a row.
The presets of the OpenAI-compatible family are in the model list
([pax-providers.md](pax-providers.md), section 14.8). Entries come from
the model list, or from the person's own `providers` in the settings file
(added by editing the file in this build; the consent text then says TUR has
reviewed nothing of the service). The three companies the families are
named after are providers even when no list names them (`implicit`), at their
public addresses with no prices. The settings, the consent record, the key,
the status and the limits are all keyed by identifier, so a fourth adapter
family adds entries, not code here ([pax-3](../reports/pax-3.md) lists what
it would still touch). Chosen with `SetProvider`. No model name is compiled
in; a name typed by hand is accepted, with no known price. A gateway address
for the provider in use (`base_url` at the top of the settings file) can be
set only by editing the file, because whoever it names is sent the key; it is
removed when the provider changes.

**Prices** are configuration, from the model list. The example file was
written from the providers' own pricing pages on 2026-11-12 and says so in its
`status` (`example`) and `prices_as_of`. Where a provider's price depends on
the length of the prompt, the example gives the shortest tier, or no price
when no single figure is true (`claude-haiku-5-5`), or when the price is
announced to change within weeks (`gemini-4-carbon`, from 2026-12-01). No
model in it has been run with TUR.

**Limits.** A task stops at its limit (the provider layer's meter); a day
stops at the day's limit (`spending.rs`). Defaults, from the setup flow of the
desktop design: 1.00 a task and 5.00 a day in the currency of the prices; for
a model whose price is not known, 400 000 tokens a task and 2 000 000 a day.

- Before every request to a provider the loop asks whether the day's limit is
  reached; if it is, nothing is sent and the task stops with
  `TUR-AI-SPEND-001`, "P@x stopped: today's spending limit was reached."
- After every response, what the provider reported is added to the day and
  written at once. A cut, cancelled or crashed task is counted up to its last
  response.
- The limit acts between responses: a day can pass it by the cost of one
  response, which the output limit bounds (`max_output_tokens`, 8192 in the
  example list).
- A day is the calendar day in the computer's time zone when the request is
  made. A changed clock or zone moves no use between days.
- If the record cannot be written, the day counts as used up.
- Amounts are estimates from reported tokens and listed prices. The
  provider's invoice is the authority; the status says the prices' date.

## 10. Why not a D-Bus crate

The brief preferred `zbus` if a crate was needed. zbus 6.2.1 (MIT, read on
2026-11-12) was the candidate. It was not used:

- Every component that ships tools for P@x has `tur-pax` as a development
  dependency, so a dependency of `tur-pax` lands in five more lockfiles, and
  the `pax` and `windows-runtime` workflows build them `--locked`. pax-2
  recorded that cost.
- zbus 6 is asynchronous and brings its own runtime (`zruntime`) or Tokio,
  plus some twenty crates; the rest of `tur-pax` is blocking threads with no
  async runtime (pax-providers.md, section 1).
- What P@x needs is small: strings, booleans, numbers, `a{sv}`, one struct
  for a secret; serve one object; call two services; listen for two signals.

`bus.rs` is that much: the wire format (every type but file descriptors),
message encoding and decoding with every length and alignment checked,
`EXTERNAL` authentication over a Unix socket, and a connection that matches
replies to calls on a reader thread. About 1400 lines and 340 of tests,
standard library only, no unsafe code. Its tests check the `Hello` call against the
bytes the specification gives, a big-endian message written by hand, every
value at every alignment, every truncation of a message and three corruptions
of every byte (no panic). It talks to dbus-daemon 1.14.10 and to libdbus clients
(python3-dbus 1.3.2) in the `pax` workflow; it has not been run against dbus-broker, which Fedora
uses. **An owner may prefer zbus**; the change would be confined to
`bus.rs` and `dbus.rs`.

## 11. The schedule runner

One look at the schedules (`scheduler::run_due`):

| The schedule | What happens |
|---|---|
| A reminder that is due | Shown as a notification. Once the notification service has taken it, a run record says so. If it could not be shown, nothing is recorded, it stays due, and the runner asks to be woken again in 60 s |
| A scheduled task that is due | Run unattended through the one loop. A run record keeps what it did and what it left for the person; then the person is told by a notification. If a task is running, it waits for it to end |
| Occurrences missed, and the schedule says to skip them | One run record says they were passed over. Nothing is shown or run |
| Anything else | Nothing |

Then the timer is set for the next occurrence of any schedule.

**When it looks:** when the service starts, when the timer elapses, when a
schedule is created, confirmed or removed, when P@x is switched on, when a
task ends (a scheduled task that waited has its turn), on `Wake`, after
sign-in and after a change of clock or time zone (the catch-up timer).

**Late.** An occurrence found more than 120 s after its time is late: the
computer was off, asleep, or nobody was signed in. With `run_once` (what
proposals use) it is shown or run once, however many were missed, and a
reminder says "This was due at 16:00 on 2026-11-12. The computer was off or
asleep then." With `skip` the missed ones are passed over.

**Time**, tested with a clock moved by hand (`tests/scheduler.rs`):

| Case | Behaviour | Test |
|---|---|---|
| Spring forward (Rome, 29 March 2026) | A daily 02:30 fires at 03:00 CEST that day, once | `a_daily_reminder_fires_once_a_day_across_the_spring_daylight_saving_change` |
| Fall back (Rome, 25 October 2026) | A daily 02:30 fires at the first 02:30, once | `a_daily_reminder_fires_once_on_the_night_the_clocks_go_back` |
| The computer changes time zone | Nothing moves: a schedule names its zone | `a_change_of_the_computers_time_zone_moves_no_occurrence` |
| The clock is set back two days | Nothing fires twice; the next occurrence after the last run is waited for | `a_clock_set_backwards_fires_nothing_twice` |
| Off for three and a half days | One reminder, for the latest occurrence, marked late | `what_came_due_while_the_computer_was_off_is_shown_once_and_late` |
| `skip` | One record of the passing-over; nothing shown | `a_schedule_that_skips_missed_occurrences_records_that_and_shows_nothing` |
| Notification service down | Stays due; tried again 60 s later; then finished | `a_reminder_that_could_not_be_shown_stays_due_and_is_tried_again_soon` |
| A task holds the slot | Stays due; runs when it is free | `a_scheduled_task_that_cannot_start_stays_due` |

**What an unattended task may do** is decided by the engine, as for any
task, with nobody present (addendum A sections 5.6, 18, 20.5 and 22): classes 0
and 1 run; class 2 is refused because no trusted session exists, and is kept
for the person; class 3 is refused and not kept (nobody could approve it on
return either); a memory record on the person's word is deferred and kept;
with the session's lock state unknown it is treated as locked, so commands
outside the read-only sandbox would be deferred (none exist). Standing
approvals are not honoured. P@x offers no tool that records an observed
entry, so an unattended task records no memory at all, which satisfies
"observed only".

**Not delivered:** event triggers. A schedule on "a device was connected" is
stored and never fires: nothing sends the runner events.

## 12. Notifications

To `org.freedesktop.Notifications.Notify`, as application `P@x`, with the hint
`desktop-entry=org.tur.Pax` (so the person can silence P@x in the desktop's
settings), urgency normal, staying until dismissed. A reminder's title is its
own message and its body "Reminder: <name>", with the late sentence if late.
After a scheduled task: "P@x ran “<name>”" (or "… and waited for you", or
"P@x could not finish “<name>”"), the counts of what it did and what waits,
the system's reason if it failed, and "See Settings ▸ P@x ▸ Schedules". The
body is escaped, because a notification service may read it as markup. No
text a model wrote during a run goes into a notification. P@x sends a
notification only for something the person created.

## 13. Where the desktop design and the AI design were silent or differ

What the desktop's P@x programs need (desktop-experience.md, sections 9 and
14.1), against what is built:

| Needed | Built |
|---|---|
| Activity state as a property with change notification | `State` and `PropertiesChanged`. Values `off`, `idle`, `working`, `paused`; `listening`, `observing`, `acting`, `waiting` do not occur in this build (no voice, no screen, no approvals) |
| Steps with plain text, tier, class, audit identifier | Yes ([2.4](#24-the-json-documents)) |
| Cost so far for the task and the day | The day: in `GetStatus`, updated after every response. The task: in its result when it ends. **A running task's cost is not streamed**; a UI can show the day's change |
| Guide mode per task; private mode | Not built: there is no screen control to show or to suspend |
| "While you were away" as a query | `GetAway`, `MarkAwaySeen` |
| Setup 9.12: consent with version, a model list with "not tested with TUR", limits | Yes. **"Check the key" (one small billed request) is not built**: it would call a real provider |
| Spending page: today, this month, the limits, estimates | Yes, in `GetStatus` |
| "Allow another 1.00 USD for this task" | Not built: raising a limit is to be confirmed on the trusted screen |
| Schedules page: list, next run, last result, approvals, "Run now", "Edit", "Approve now" | List, next, last, approval count, create, remove. **Not built:** run now, edit in place (remove and create), approve a deferred action (it is class 2) |
| Memory page: list, keep, dismiss, forget, edit, forget everything, "has it been sent" | List, keep, forget. **Not built:** edit (correct), forget everything in one call, whether an entry was sent |
| Activity page | `ListActivity` |
| Approvals and Privacy pages | Not on this interface: they belong to the policy owner (`tur-pax-guard`), which does not exist |
| Notifications: "task finished or stopped by a limit" (low priority) | Sent for scheduled tasks only; an attended task reports through the panel |

Decisions taken where the designs were silent (the owner may reverse any):

1. **Settings through the interface.** Switching on, choosing the provider,
   raising a limit and setting the key are methods any program of the person
   can call. The design's trusted-screen confirmation for loosening does not
   exist. The alternative, refusing them until it does, would leave P@x
   unusable.
2. **No standing approval through the interface**, ever, until the policy
   owner exists: an approval given over the session bus would be one any
   program in the session can give.
3. **A model cannot state anything on the person's behalf.** `keep_note`
   records content waiting for the person, attended or not.
4. **Unattended tasks may propose** reminders and tasks; the proposal waits.
5. **A changed provider withdraws the consent** and switches P@x off.
6. **Cancellation frees the slot at once**, while the cancelled task's thread
   may still be waiting for its last response, which is then discarded and
   counted.
7. **An unknown lock state** counts as locked for a scheduled task and as
   unlocked for a person's task (the person called).
8. **A day** is a calendar day in the computer's zone; the record keeps 62.
9. **The proposals** use `run_once` for missed occurrences.
10. **The key transfer** is `plain`.
11. **The audit trail** gains `__REALTIME_TIMESTAMP` on each line.
12. **An own D-Bus implementation** instead of zbus ([section 10](#10-why-not-a-d-bus-crate)).

## 14. Known gaps

1. No real provider, no real model. The manual workflow `pax-live` exists
   (pax-3 report) and has not been run.
2. Not on the image; not run on Fedora, Plasma, KWallet or dbus-broker.
3. Any program of the person can operate every method ([section 3](#3-who-is-served)).
4. "Check the key", a running task's live cost, and the pages' missing actions
   listed in [section 13](#13-where-the-desktop-design-and-the-ai-design-were-silent-or-differ).
5. Event triggers are not delivered.
6. The catch-up timer was not loaded in a test; suspend and resume were not
   tested.
7. Two sessions of one person would each set and stop the same transient
   timers.
8. The trail and the settings file are files of the person's, not
   append-only and not owned by another component.
9. A proposal lives in the service's memory: a restart loses proposals the
   person has not confirmed.
10. The service keeps the last 20 tasks in memory only; the trail is the
    lasting record.
