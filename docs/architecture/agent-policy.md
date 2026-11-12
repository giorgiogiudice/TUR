# Agent policy engine

**Milestone A0 of the [AI control layer](ai-control-layer.md)**
**Component:** `services/uos-agent`
**Related decisions:** [ADR-0004](../adr/0004-ai-provider-model.md), [ADR-0005](../adr/0005-languages-and-workspace.md)

This document describes the policy engine as implemented: what it takes as input,
what it decides, which properties are guaranteed and by which test, the two file
formats, and what the engine cannot guarantee by itself.

The engine implements sections 5, 6 and 11 of the AI control layer design
("addendum A") and the revision 2 rules for commands, repair sessions, unattended
tasks and memory. Where those leave a case undefined, the stricter behaviour was
chosen. Every such choice is listed in [section 10](#10-interpretations).

A0 contains no model, no network code and no D-Bus code. Nothing here executes an
action. The engine decides; other components act on the decision.

---

## 1. What the engine is

One pure function:

```text
decide(action, context, user policy, now) -> decision
```

It performs no I/O and reads no clock; the time is a parameter. The same inputs
always give the same decision, and nothing is remembered between calls. In
particular there is no input that says "the user approved": an `ask` or an
`authenticate` is resolved outside the engine, for that one action, and the next
call for the same action gives `ask` or `authenticate` again.

| Input | Source | May come from the model |
|---|---|---|
| Action | The proposal | Yes: it is a proposal and carries no authority |
| Class of a tool | The service's validated manifest | No |
| Sandbox and operation type of a command | The orchestrator and the privileged helper | See [section 11](#11-known-limits) |
| Application and window of a UI action | The compositor or the accessibility bus | No |
| Context | Session manager, compositor, repair-session record, schedule store | Only the "consequential" hint |
| User policy | The policy file, written by the user on the trusted path | No |
| Time | The caller's clock | No |

The assistant's memory is not an input. Nothing recalled from memory can reach the
engine except as part of a proposed action, where it is data.

## 2. Inputs

### 2.1 Classes

| Class | Name | Rule in normal mode, attended, trusted path available |
|---|---|---|
| 0 | Observe | Allow |
| 1 | Reversible | Allow |
| 2 | Consequential | Ask |
| 3 | System-critical | Authenticate on the trusted path, every time |
| 4 | Prohibited | Refuse |

### 2.2 Actions and how each is classified

| Action | Class |
|---|---|
| **Tool call** | The class the service declared in its manifest (0 to 3). A tool call can only be built from a validated manifest, so the service, the tool name and the class never come from the model. |
| **Accessibility or screen action, observing** | 0 |
| **Accessibility or screen action, acting** | 1. Raised to 2 if the control name or the window title matches the pattern table, or if the control has no name. |
| **Any accessibility or screen action in a sensitive application** | 2 |
| **Command, read-only sandbox** | 0 |
| **Command, user sandbox, inside a task workspace the system can undo** | 1 |
| **Command, user sandbox, otherwise** | 2 |
| **Command, privileged, operation type a repair session may cover** | 2, with its own rule (rule 7 below) |
| **Command, privileged, system-critical or unclassified operation type** | 3 |
| **Agent operation: memory read** | 0 |
| **Agent operation: memory write** | 1 |
| **Agent operation: schedule write** | 2 |
| **Agent operation: read a keyring secret; change policy, permissions or standing approvals; disable the indicator or the audit log; operate the trusted path; enable APFS write** | 4 |

A command is never classified by parsing its text. The class comes from the sandbox
alone. An accessibility or screen action can never exceed class 2.

After this, if the model flagged the action as consequential, the class is raised
to 2 if it was lower. Nothing lowers a class.

**The pattern table** is `CONSEQUENTIAL_PATTERNS` in `src/patterns.rs`, the only
place the patterns are defined. The text is lower-cased and split into words at
every character that is not a letter or a digit; a word matches if it begins with
a stem in the table. The categories are send, pay, purchase, delete, install and
submit.

**Privileged operation types.** A privileged command is a request to the
privileged helper. The request has a typed operation field, which selects the
confinement the helper applies.

| A repair session may cover | A repair session never covers (class 3) |
|---|---|
| diagnostics, service control, system configuration, network configuration, software management | partitioning, formatting, encryption, bootloader, firmware, OS update and rollback, device hand-off, security policy, and anything unclassified |

The list is `PrivilegedOperation::repair_session_may_cover`. It is a match with no
wildcard arm, so a new operation type cannot be added without placing it on one
side.

### 2.3 Context

| Field | Meaning |
|---|---|
| Presence | Attended (the user is present and asked for the task) or unattended (a scheduled task or reminder firing) |
| Session locked | The session is locked |
| Trusted path available | All five compositor guarantees of addendum A section 6 hold |
| Repair session | Purpose and expiry, if the user opened one |
| Standing approvals | The approvals the user attached to the schedule that started the task |
| Model hint | The model flagged the action as consequential |

A repair session is open if it has a stated purpose, the time is before its
expiry, and it has at most 24 hours left. At the expiry instant it is closed. The
24 hours are a sanity bound against a record with no real time limit, not the
intended length of a session. The engine treats a session as closed while the
session is locked or the task is unattended, whatever the record says.

A standing approval names a service, a tool and a complete set of arguments. It
matches a call only if all three are equal. JSON numbers compare as written, so
`1` and `1.0` differ. The order of keys does not matter.

### 2.4 User policy

| Setting | Effect |
|---|---|
| Mode `normal` | The rules as listed |
| Mode `ask_every_action` | Every action of every class is asked first; an unattended task is deferred |
| Mode `observe_only` | Only class 0 is available |
| Excluded applications | No accessibility or screen action, observation included |
| Sensitive applications | Every accessibility or screen action is class 2 at least |
| Relaxation, class 2 | Class 2 accessibility and screen actions in that application are allowed without asking, in normal mode, attended |
| Relaxation, class 1 | Accepted; changes no decision |

## 3. Decisions

| Decision | Meaning | Carries |
|---|---|---|
| `allow` | The action may run now | Reason |
| `ask` | The user must approve this one action | Reason, the text to show |
| `authenticate` | A person must authenticate on the trusted path | Reason, the text to show |
| `defer` | Nobody can be asked now; held for the user's return | Reason |
| `refuse` | The action does not run | Reason, `UOS-AI-*` error code |

The decisions have a total strictness order, used by the tests:
`allow < ask < authenticate < defer < refuse`. The order is by what must happen
before the action can run: nothing; a confirmation; an authentication; the user's
return; never.

Every decision also carries the class the action was treated as.

The prompt text states the exact action. A command is shown argument by argument
and is never shortened. Every character that is not printable ASCII is shown as an
escape, so a prompt cannot hide characters or reverse its reading direction.

## 4. Decision table

The rules are evaluated in this order. The first that applies decides.

| # | Condition | Decision | Reason |
|---|---|---|---|
| 1 | The action is incomplete: no tool name, no application, empty command | refuse | `malformed_action` |
| 2 | Class 4 | refuse | `prohibited` |
| 3a | Accessibility or screen action in an excluded application | refuse | `excluded_application` |
| 3b | Accessibility or screen action while the session is locked | refuse | `session_locked` |
| 4 | Mode is observe-only and the class is above 0 | refuse | `observe_only` |
| 5 | The trusted path is not available and the class is 2 or 3 | refuse | `trusted_path_unavailable` |
| 6a | Class 3, proposed as a command | refuse | `system_critical_as_command` |
| 6b | Class 3, unattended or locked | refuse | `system_critical_unattended` |
| 6c | Class 3, otherwise | authenticate | `system_critical_needs_authentication` |
| 7a | Privileged command, unattended or locked | refuse | `privileged_unattended` |
| 7b | Privileged command, no open repair session | authenticate | `repair_session_required` |
| 7c | Privileged command, open repair session, mode ask-every-action | ask | `ask_every_action_mode` |
| 7d | Privileged command, open repair session | allow | `repair_session_open` |
| 8 | Attended task, session locked | defer | `paused_while_locked` |
| 9a | Unattended, mode ask-every-action | defer | `unattended_ask_every_action_mode` |
| 9b | Unattended, class 0 or 1 | allow | `observe_permitted`, `reversible_permitted` |
| 9c | Unattended, class 2, a tool call that exactly matches a standing approval | allow | `standing_approval_matched` |
| 9d | Unattended, class 2, otherwise | defer | `unattended_without_standing_approval` |
| 10a | Attended, mode ask-every-action | ask | `ask_every_action_mode` |
| 10b | Attended, class 0 or 1 | allow | `observe_permitted`, `reversible_permitted` |
| 10c | Attended, class 2, accessibility or screen action in an application relaxed to class 2 and not sensitive | allow | `relaxed_by_user_policy` |
| 10d | Attended, class 2, otherwise | ask | `consequential_needs_confirmation` |

Consequences worth stating:

- There is no rule that allows class 3. The engine never releases a class 3
  action. It says `authenticate`; the authentication and the release happen in the
  owning service, through polkit, on the trusted path.
- A repair session is consulted only in rule 7, which class 3 never reaches.
- A standing approval is consulted only in rule 9c: unattended, class 2, tool
  calls. It does nothing when the user is present, does nothing for an attended
  task in a locked session, does not replace the trusted path, and cannot cover a
  command, a UI action or an agent operation.
- A relaxation is consulted only in rule 10c.
- An unattended task runs whether or not the session is locked, but never through
  the accessibility tree or the screen while it is locked (rule 3b). An attended
  task does nothing while the session is locked (rules 3b, 6b, 7a and 8).

The test `the_documented_decision_table_holds` in `tests/invariants.rs` asserts at
least one row for each of rules 2 to 10. Rule 1 is tested by
`malformed_actions_are_refused` in `src/engine.rs`.

## 5. Invariants and the tests that enforce them

`tests/invariants.rs` builds every combination of:

- action: tool calls of class 0 to 3 with two argument sets; accessibility and
  screen actions, observing and acting, on four labels, in six applications
  (plain, excluded, sensitive, relaxed to 1, relaxed to 2, sensitive and relaxed);
  commands in every sandbox, working context and privileged operation type; every
  agent operation;
- context: attended or unattended; locked or not; trusted path or not; repair
  session absent, open, expired, expiring now, without a purpose, or with more
  than a day left; no standing approvals, exact ones, or near misses; model hint
  or not;
- user policy: each mode, with no per-application settings, with all of them, and
  with every application relaxed.

That is 147 actions, 288 contexts and 9 policies: 381 024 decisions for each
sweep. Each generated action carries the class its source gives it, stated in the
test independently of the engine.

| Invariant | Test |
|---|---|
| No input yields `allow` for class 3 or class 4 | `class_3_and_class_4_are_never_allowed`; for a tool call that claims class 4, which cannot be built outside the crate, `a_tool_call_claiming_class_3_or_4_is_never_allowed` in `src/engine.rs` |
| Class 4 is always `refuse` | `class_4_is_always_refused` |
| The class is never below the class of the action's source | `the_class_is_never_below_the_class_of_the_source` |
| The model hint never lowers the outcome: for every input, the decision with the hint is at least as strict | `the_model_hint_never_lowers_the_outcome` |
| A stricter mode is never more permissive than normal mode | `a_stricter_mode_is_never_more_permissive_than_normal_mode` |
| Excluding an application or marking it sensitive is never more permissive | `excluding_or_marking_an_application_sensitive_is_never_more_permissive` |
| A relaxation changes exactly one thing: an attended class 2 UI `ask` in normal mode becomes `allow`. It never affects classes 3 and 4 | `a_relaxation_only_turns_an_attended_class_2_ui_ask_into_allow` |
| A class 1 relaxation changes no decision | `a_class_1_relaxation_changes_no_decision` |
| Without the trusted path nothing above class 1 runs, and nothing asks for authentication | `without_the_trusted_path_nothing_above_class_1_runs_and_nothing_authenticates` |
| Unattended or locked: never `ask`, never `authenticate` | `nobody_is_asked_when_nobody_is_present` |
| Unattended class 2 is `allow` only with an exactly matching standing approval, and only in a task that really is unattended | `unattended_class_2_runs_only_with_an_exactly_matching_standing_approval`; near misses in `standing_approval_matches_only_the_exact_call` (`src/context.rs`) and in `tests/cli.rs` |
| Unattended class 3 is always `refuse` | `unattended_class_3_is_always_refused` |
| An expired or absent repair session never permits a privileged command; nor does one without a purpose or without a time limit | `a_privileged_command_runs_only_inside_an_open_repair_session`; the boundaries in `src/context.rs` |
| An open repair session never permits a system-critical operation; such an operation cannot be reached as a command | `a_system_critical_operation_never_runs_as_a_command`; the operation list in `repair_session_never_covers_the_system_critical_operations` (`src/action.rs`) |
| Locked session: no accessibility or screen actions | `a_locked_session_has_no_accessibility_or_screen_actions` |
| Locking the session is never more permissive, and an attended task does nothing while it is locked | `locking_the_session_is_never_more_permissive` |
| Losing the trusted path is never more permissive | `losing_the_trusted_path_is_never_more_permissive` |
| A repair session changes the decision for privileged commands only | `a_repair_session_affects_only_privileged_commands` |
| A standing approval changes exactly one thing: an unattended class 2 tool call that would be deferred is allowed | `a_standing_approval_affects_only_unattended_class_2_tool_calls` |
| Excluded application: never observed or acted on | `an_excluded_application_is_never_observed_or_acted_on` |
| Observe-only mode: only class 0 is allowed; nothing is asked | `observe_only_mode_allows_only_class_0` |
| Ask-every-action mode: nothing is allowed without asking | `ask_every_action_mode_allows_nothing_by_itself` |
| A refusal, and only a refusal, carries an error code | `a_refusal_and_only_a_refusal_carries_an_error_code` |
| The engine is deterministic and remembers no approval | `the_engine_is_deterministic_and_remembers_nothing` |
| Arguments carry no authority: a claim inside the arguments changes nothing | `arguments_carry_no_authority` |
| An action whose audit record cannot be written is refused | `an_action_that_cannot_be_recorded_is_refused` (`src/audit.rs`) |
| Secrets do not reach the audit record | `secrets_never_reach_the_record` (`src/audit.rs`) |

The A0 exit criterion, "a scripted caller exercises every class against a test
service; class 3 fails without authentication", is `tests/cli.rs`. It runs the
built binary against `tests/data/test-service.manifest.json`, which declares one
tool of each class 0 to 3, and asserts each outcome. One test runs class 3 through
every combination of context flags and policy files, 192 runs, and asserts that
none of them is `allow`.

What that test proves is that the engine does not release class 3. It does not
prove that a service refuses a class 3 call made directly; there is no service
yet. See [section 11](#11-known-limits).

## 6. Tool manifest

Schema: [`schemas/agent-tool-manifest.schema.json`](../../schemas/agent-tool-manifest.schema.json).

```json
{
  "schema_version": 1,
  "service": "org.uos.Storage1",
  "tools": [
    {
      "name": "storage_unmount",
      "description": "Unmount a volume by its UOS volume identifier.",
      "class": 1,
      "method": "Unmount",
      "input_schema": {
        "type": "object",
        "properties": { "volume_id": { "type": "string" } },
        "required": ["volume_id"],
        "additionalProperties": false
      }
    }
  ]
}
```

The loader rejects a manifest unless all of these hold. It reports every problem
it finds, each with an error code.

| Rule | Detail |
|---|---|
| No unknown fields | In the manifest and in each tool |
| `schema_version` | 1 |
| `service` | A D-Bus well-known name: at least two elements of letters, digits and underscores, not starting with a digit |
| `tools` | At least one |
| `name` | `^[a-z][a-z0-9_]{0,63}$`, unique within the manifest |
| `description` | 1 to 1024 characters, not blank, no control characters |
| `class` | An integer 0 to 3. Class 4 is rejected with its own code: a prohibited operation is not offered as a tool |
| `method` | A D-Bus member name |
| `input_schema` | An object schema with `"additionalProperties": false`, in the subset below |

**The input schema subset.** Every schema, at every depth, must have a `type` that
is one string. The keywords are:

| Keyword | Allowed on |
|---|---|
| `type`, `title`, `description`, `enum` | Every type |
| `properties`, `required`, `additionalProperties` | `object`. `additionalProperties` must be `false`, at every depth. Property names match `^[A-Za-z_][A-Za-z0-9_]{0,63}$`. Each `required` entry must be a declared property. |
| `items` | `array`, where it is mandatory |
| `minLength`, `maxLength` | `string`; counted in characters |
| `minimum`, `maximum` | `integer`, `number` |

Any other keyword is rejected, including `anyOf`, `oneOf`, `$ref`, `pattern`,
`format` and `default`. The reason is that the agent checks the arguments of every
call against the schema, and it enforces exactly this subset. A manifest cannot
state a constraint the agent would ignore.

`ToolManifest::tool_call` builds a tool call: it looks the tool up, takes the class
from the manifest and checks the arguments.

## 7. User policy file

Schema: [`schemas/agent-policy.schema.json`](../../schemas/agent-policy.schema.json).

```json
{
  "schema_version": 1,
  "mode": "normal",
  "excluded_applications": ["org.keepassxc.KeePassXC"],
  "sensitive_applications": ["org.mozilla.firefox"],
  "relaxations": [
    { "application": "org.kde.kmail2", "class": 2 }
  ]
}
```

| Field | Rule |
|---|---|
| `schema_version` | 1; required |
| `mode` | `normal`, `ask_every_action` or `observe_only`; required, so that a truncated file is not read as normal mode |
| `excluded_applications`, `sensitive_applications` | Optional lists of application identifiers. Matched without regard to ASCII case or surrounding whitespace, so that a near-identical identifier is still caught. |
| `relaxations` | Optional. Each names an application, matched exactly, and a class, 1 or 2. Any other class is an error. An application may appear once. |

Unknown fields are an error. There is no field that relaxes class 3 or class 4,
and the type that holds a relaxation has no value for them.

If an application is both sensitive and relaxed, it is sensitive.

If the policy file cannot be read or is invalid, the caller must use
`UserPolicy::fail_closed()`, which is observe-only. The default policy, normal
mode with empty lists, is for the case where the user has no policy file.

The file is written by the user on the trusted path. Changing it is a class 4
operation for the assistant (`AgentOperation::PolicyWrite`).

## 8. Audit record

One record for each proposed action and its decision, rendered to journal fields:

```text
MESSAGE=uos-agent: refused org.uos.Storage1:storage_format (class 3, tier 1): trusted_path_unavailable; arguments volume_id=string
UOS_AI_TASK=task-0001
UOS_AI_ACTION=org.uos.Storage1:storage_format
UOS_AI_CLASS=3
UOS_AI_DECISION=refused
UOS_AI_TIER=1
UOS_AI_PROVIDER=anthropic
UOS_ERROR_CODE=UOS-AI-POLICY-006
```

| Field | Values |
|---|---|
| `UOS_AI_ACTION` | `<service>:<tool>`; `accessibility:observe`, `accessibility:act`; `screen:observe`, `screen:act`; `command:read_only`, `command:user`, `command:privileged`; `agent:<operation>` |
| `UOS_AI_CLASS` | The class the action was treated as, 0 to 4 |
| `UOS_AI_DECISION` | `allowed`, `confirmation_required`, `authentication_required`, `deferred`, `refused`; and, for the user's answer to a prompt, `confirmed`, `denied` |
| `UOS_AI_TIER` | 1 tool, 2 accessibility, 3 screen and input, 4 command, 0 an operation of the agent itself |
| `UOS_ERROR_CODE` | Present only for a refusal |

What a record may contain is restricted by construction:

- Argument values are never written. Tool arguments are summarised as the property
  names with the kind of each value (`string`, `number`, and so on). At most 16
  names are listed.
- If a property name resembles a secret, the kind is withheld too. If a name is
  not a plain identifier, or looks like an opaque token, the name is not written.
- A command is summarised as its program name, the number of arguments and, for a
  privileged command, the operation type. The arguments are never written. The
  program name is withheld if it resembles a secret.
- A UI action is summarised as the application and the pattern category that
  matched. Window titles and control names are never written.
- The prompt text is never written.
- Task, provider, service, tool and application identifiers are written only if
  they consist of ASCII letters, digits, dots, underscores and hyphens; otherwise
  `<invalid>` is written.

"Resembles a secret" means: contains one of the markers in `SECRET_MARKERS`
(`password`, `token`, `secret` and others), or contains a run of 20 or more
characters from the base64 and hexadecimal alphabets with both a letter and a
digit.

Records are written through the `AuditSink` trait. `MemorySink` is provided for
tests. This crate has no journald dependency; the journal sink belongs to the
service. `decide_and_record` writes the record before returning the decision, and
refuses the action if the record cannot be written.

## 9. Command line and error codes

```text
uos-agent manifest validate <file>
uos-agent policy check --manifest <file> --tool <name> [--args <json>]
                       [--policy <file>] [--unattended] [--locked] [--trusted-path]
                       [--repair-session-until <seconds> --repair-session-purpose <text>]
                       [--standing-approval <json>]... [--model-flags-consequential]
                       [--now <seconds>] [--task <id>] [--json]
```

`policy check` prints the decision, and with `--json` also the audit record. The
defaults are the strict ones: attended, unlocked, **trusted path not available**.
Without `--trusted-path`, classes 2 and 3 are refused.

| Exit status | Meaning |
|---|---|
| 0 | `allow`; or the manifest is valid |
| 1 | An input was unreadable or invalid; nothing was decided |
| 2 | Usage error |
| 10 | `ask` |
| 11 | `authenticate` |
| 12 | `defer` |
| 13 | `refuse` |

Only `allow` exits with 0, so a script that tests the exit status fails closed.
Without `--now` the command reads the system clock; if the clock cannot be read
it uses the latest possible time, at which every repair session has expired. This
command is the only place in the crate that reads a file or the clock.

The error codes are declared once, in the registry table in `src/codes.rs`. A unit
test checks that every code parses as a `UOS-AI-*` code and that codes and titles
are unique.

| Code | Meaning |
|---|---|
| `UOS-AI-MANIFEST-001` to `-010` | Tool manifest: parse, version, service, no tools, tool name, duplicate tool, description or method, class range, class 4 declared, input schema |
| `UOS-AI-TOOL-001`, `-002` | Unknown tool; arguments do not match the input schema |
| `UOS-AI-POLICYFILE-001` to `-004` | Policy file: parse, version, application, relaxation |
| `UOS-AI-POLICY-001` | Prohibited (class 4) |
| `UOS-AI-POLICY-002` | Malformed action |
| `UOS-AI-POLICY-003` | Excluded application |
| `UOS-AI-POLICY-004` | Session locked |
| `UOS-AI-POLICY-005` | Observe-only mode |
| `UOS-AI-POLICY-006` | Trusted path not available |
| `UOS-AI-POLICY-007` | System-critical action with nobody present |
| `UOS-AI-POLICY-008` | System-critical operation proposed as a command |
| `UOS-AI-POLICY-009` | Privileged command with nobody present |
| `UOS-AI-AUDIT-001` | Audit record could not be written |
| `UOS-AI-INPUT-001`, `-002` | Unreadable file; invalid command-line value |

## 10. Interpretations

Addendum A and the revision 2 rules leave the following cases undefined or
ambiguous. In each, the stricter reading was implemented.

| # | Case | Choice |
|---|---|---|
| 1 | Class of a privileged command that is not system-critical | Class 2, with its own rule: an open repair session instead of a per-action confirmation. It cannot be class 3, because class 3 is never allowed and a repair session allows these. |
| 2 | A system-critical operation requested as a privileged command | Refused outright, with no offer to authenticate. The operation exists only as a class 3 tool of the owning service, where polkit enforces it. |
| 3 | A privileged operation the helper cannot classify | Treated as system-critical. |
| 4 | Privileged command in an unattended task | Refused, not deferred, even if a repair session is recorded as open. An unattended task never runs anything as root. |
| 5 | Locked session with a task marked attended | The task is paused: every action that is not refused is deferred, classes 0 and 1 included (addendum A section 7: tasks pause when the session locks). Class 3 and privileged commands are refused. A repair session is treated as closed, whatever the record says. Standing approvals are not honoured: they belong to a schedule, and this task has none. |
| 6 | Repair session with no stated purpose | Never open. |
| 7 | Repair session at the exact expiry instant | Closed. |
| 7a | Repair session with no effective time limit | A session with more than 24 hours left is closed. Revision 2 says a session has a time limit but not what it is; the bound is a sanity check and should be replaced by the stated maximum. |
| 8 | Open repair session while the trusted path is unavailable | Permits nothing: rule 5 comes first. |
| 9 | "Ask before every action" and class 0 | Class 0 is asked too. "Every" is read literally. |
| 10 | "Ask before every action" in an unattended task | Everything is deferred, including a class 2 call with a matching standing approval, and including classes 0 and 1. |
| 11 | "Ask before every action" inside an open repair session | Each privileged command is asked, showing the exact command. |
| 12 | "Ask before every action" or observe-only, and a relaxation | The global mode wins. A relaxation applies only in normal mode. |
| 13 | What a class 1 relaxation does | Nothing. Class 1 is already allowed in normal mode and choice 12 removes the only other case. The value is accepted so that the file format matches addendum A section 5.3. |
| 14 | Which actions a relaxation covers | Accessibility and screen actions only. A tool call or a command has no target application known to the engine. |
| 15 | A relaxation in an unattended task | Does not apply. Unattended class 2 needs a standing approval. |
| 16 | A relaxation without the trusted path | Does not apply. Rule 5 comes first. |
| 17 | An application that is both sensitive and relaxed | Sensitive. |
| 18 | Observing a sensitive application | Class 2: asked, like acting in it. Addendum A names only "acting in" a sensitive application. |
| 19 | Matching of application identifiers | Exclusion and sensitivity ignore ASCII case and surrounding whitespace. Relaxation is exact. |
| 20 | Acting on a control with no name | Class 2: when in doubt, ask. |
| 21 | Which actions a standing approval covers | Tool calls only. A command, a UI action or an agent operation of class 2 in an unattended task is deferred. |
| 22 | A standing approval when the user is present | Ignored. The user is asked. |
| 23 | A standing approval without the trusted path | Does not apply. Rule 5 comes first, and the decision is refuse, not defer. |
| 24 | Standing approval and JSON numbers | `1` and `1.0` do not match. |
| 25 | The model hint on a class 2 action that a standing approval, a relaxation or a repair session already allows | No effect. The hint raises the class to 2 and does nothing else. A hint that revoked these would make every scheduled class 2 task defer whenever the model, correctly, called its own action consequential. |
| 26 | Class of creating or changing a reminder or schedule | Class 2. A schedule causes later unattended actions. Granting a standing approval with it is not something the assistant can do: that is class 4. |
| 27 | Class of removing a memory entry | Class 1, the same operation as writing. |
| 28 | Unattended class 2 without the trusted path | Refused, not deferred. |
| 29 | Class 2 confirmation without the trusted path | Refused. Addendum A section 6 limits the assistant to classes 0 and 1 until all five guarantees hold, although section 5.2 lets class 2 be approved on screen or by voice. |
| 30 | `UOS_AI_DECISION` for a decision that is not final | `confirmation_required`, `authentication_required` and `deferred` were added to the four values of addendum A. |
| 31 | `UOS_AI_TIER` for a command and for an agent operation | 4 and 0. Addendum A defines 1 to 3. |
| 32 | An audit record that cannot be written | The action is refused. |
| 33 | Logging of privileged commands | The record holds the program name, the argument count and the operation type, not the arguments. See limit 9. |
| 34 | An incomplete action | Refused. |
| 35 | `mode` missing from a policy file | The file is invalid. |
| 36 | An empty tool list in a manifest | The manifest is invalid. |
| 37 | Nested object schemas in a manifest | Must also set `additionalProperties: false`. |
| 38 | Schema keywords the agent does not enforce | Rejected. |

## 11. Known limits

What this engine cannot guarantee by itself.

1. **The engine decides; it does not enforce.** A decision has effect only if every
   action passes through `decide` and the caller obeys it. Enforcement of class 3
   also lives in each service, through polkit: a class 3 operation must fail
   without human authentication even when called directly, bypassing the agent
   (addendum A sections 5.1 and 12, "policy tests"). That is a property of each
   service and is tested there, from milestone A1. Nothing in A0 tests it.

2. **The trusted path is a compositor feature.** The engine takes "trusted path
   available" as an input. It cannot check any of the five guarantees. Until
   milestone A3 delivers them, the input must be false, and the engine then
   refuses everything above class 1. Setting it to true without the guarantees
   removes the protection the rest of the design assumes.

3. **Confirmation is not bound to the action here.** The engine returns `ask` or
   `authenticate` with the text to show. Showing it, collecting the answer, and
   making sure the approved action is the one that runs are the orchestrator's
   and the compositor's work. An `ask` in ask-every-action mode for a class 0 or 1
   action, without the trusted path, is an ordinary prompt the assistant could
   in principle answer itself; it adds friction, not a guarantee.

4. **The text of a privileged command cannot be fully classified.** The engine
   does not try. It applies the repair session's deny list to the operation type
   of the helper request, not to the command. `rm -rf /boot` requested with the
   type "service control" passes the engine. What must stop it is the helper: each
   operation type has to run in a confinement that makes its type true, for
   example no access to block devices, the EFI variables, `/boot`, or the polkit,
   sudo, PAM and crypttab configuration, for every type a repair session may
   cover. "System configuration" is the widest of these and needs the most care.
   The helper does not exist yet. Until it does, the statement "a repair session
   never covers system-critical operations" is true of the engine's decisions and
   unproven for the system.

5. **The same holds for the other sandboxes.** A command in the read-only sandbox
   is class 0 because the sandbox is supposed to make it harmless. The engine
   trusts the sandbox kind it is given. A read-only view that includes secrets,
   or the data of excluded applications, would let a class 0 command read them and
   pass them to the provider. The sandbox profiles must exclude them.

6. **Exclusions and sensitivity cover accessibility and screen actions only.** A
   tool call or a command that touches an excluded application's data is not
   recognised as such.

7. **The UI rule set is a heuristic.** The pattern table is English only. A
   control with a misleading label, a label in another language, a look-alike
   character, an icon with a harmless name, or a key press in a window whose
   title says nothing is not recognised. For screen-and-input actions the control
   description is only as trustworthy as its source: if it comes from the model, a
   compromised model can describe a "Pay" button as "Cancel". The description
   should come from the accessibility tree or the compositor. The real bound on UI
   actions is addendum A's first rule: privileged prompts reject emulated input.
   Within class 1 and class 2, an assistant misled by screen content can still do
   unwanted things, as addendum A section 7 says.

8. **A manifest is trusted.** The class of a tool is what the service declares. A
   service that declares a destructive operation as class 1 is not detected.
   Manifests are part of each service's reviewed interface.

9. **The audit record of a command is a summary.** Revision 2 says each privileged
   command is logged; addendum A says arguments are never logged in full. The
   record follows the second rule, so it does not say what the command was. A
   full record of privileged commands, readable only by the user, is a question
   for the revision. The secret heuristics are heuristics: a short secret used as
   a program name or as a property name is not recognised. Argument values are
   never written, whatever they are.

10. **Memory is kept out by construction, not by inspection.** The engine has no
    memory input, and nothing in a context can be filled from memory if the
    orchestrator follows the table in section 1. The engine cannot detect an
    orchestrator that builds a context from recalled text.

11. **Standing approvals are taken as given.** The engine cannot tell whether an
    approval was granted by the user on the trusted path or written by something
    else. The schedule store must not be writable by the agent's own process.

12. **Numbers and repeated keys.** `minimum` and `maximum` in an input schema are
    compared as 64-bit floating-point values, exact up to 2^53. If a JSON object
    inside an input schema or inside the arguments repeats a key, the last value
    is the one the agent reads and enforces; a service that parses the same text
    differently could read the first. A repeated field of the manifest, of a tool
    or of the policy file is an error.

13. **The JSON schemas are descriptive.** No test validates documents against
    them. `tests/schemas.rs` checks the points where a schema and a loader state
    the same fact. The loaders are authoritative.

14. **Default exclusions are not shipped.** Addendum A section 8 excludes password
    managers, the keyring, the lock screen and private browser windows by default.
    The default policy here has empty lists; the default list needs application
    identifiers that are not fixed yet.

## 12. Not in A0

The orchestrator, the tool registry over D-Bus, provider adapters, the journald
sink, the privileged helper, the sandboxes, the schedule store, memory storage,
the trusted path, voice, and undo.
