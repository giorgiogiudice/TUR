# Agent policy engine

**Milestone A0 of the [AI control layer](ai-control-layer.md)**
**Component:** `services/tur-pax`
**Related decisions:** [ADR-0004](../adr/0004-ai-provider-model.md), [ADR-0005](../adr/0005-languages-and-workspace.md), [ADR-0012](../adr/0012-assistant-policy-model.md)
**Reviews:** [first independent review](../reviews/policy-engine-review-1.md)

This document describes the policy engine as implemented: what it takes as input,
what it decides, which properties are guaranteed and by which test, the two file
formats, and what the engine cannot guarantee by itself.

The engine implements sections 5, 6, 11 and 17 to 21 of the AI control layer
design ("addendum A") as of **revision 8**. Where those leave a case undefined,
the stricter behaviour was chosen. Every such choice is listed in
[section 10](#10-interpretations). What sections 18 to 21 changed in the engine,
and what is left, is in [section 11](#11-known-limits), limit 16.

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
| Kind and target of a typed operation | The proposal; checked against the table of typed operations | Yes: the table decides what is accepted |
| "Secret" mark on the content of a typed operation | The orchestrator, for a value the user entered on a protected surface | No |
| Application, window and control of a UI action | The compositor or the accessibility bus | No |
| Context | Session manager, compositor, and the system component that owns the repair-session record and the standing approvals | Only the "consequential" hint |
| User policy | The policy file, owned by that component and written through system settings on the trusted path | No |
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
| **Accessibility or screen action, acting** | 1. Raised to 2 if the name of the control or the title of the window matches the pattern tables, if the name of the control cannot be read, or if the compositor did not say whose process the window belongs to. |
| **Any accessibility or screen action in a sensitive application** | 2 |
| **Command, read-only sandbox** | 0 |
| **Command, user sandbox, inside a task workspace the system can undo** | 1 |
| **Command, user sandbox, otherwise** | 2 |
| **Privileged request, operation type a repair session may cover** | 2, with its own rule (rule 7 below) |
| **Privileged request, system-critical or unclassified operation type** | 3 |
| **Agent operation: memory read** | 0 |
| **Agent operation: record an observed memory entry; change or remove an observed entry** | 1 |
| **Agent operation: record a memory entry on the user's word** (stated by the user, a procedure the user accepted, content waiting for confirmation) | 1; deferred in an unattended task (rule 9b2; addendum A section 18, item 4) |
| **Agent operation: change or remove a memory entry the user stated or confirmed, or a saved procedure** | 2 |
| **Agent operation: propose a reminder or a schedule** | 1: nothing takes effect until the user confirms the proposal (addendum A section 18, item 3) |
| **Agent operation: create, change or remove a schedule; confirm a pending memory entry** | 4: the user's acts, never performed by P@x (addendum A section 18, items 1 to 3) |
| **Agent operation: read a keyring secret; change policy, permissions or standing approvals; disable the indicator or the audit log; operate the trusted path; enable APFS write** | 4 |

A command is never classified by parsing its text. The class comes from the sandbox
alone. An accessibility or screen action can never exceed class 2.

After this, if the model flagged the action as consequential, the class is raised
to 2 if it was lower. Nothing lowers a class.

**Sensitive applications.** An application is sensitive if the user's policy lists
it, if it is on the built-in list, or if its identifier is not a plain one
(section 2.4). The built-in list, `DEFAULT_SENSITIVE_APPLICATIONS` in
`src/policy.rs`, holds the surfaces that execute what is typed into them:
terminal emulators, run dialogs and launchers on KDE and GNOME, the common
third-party terminals, and the consoles of the Windows runtime (addendum A
section 17, item 1). It applies whatever the user policy says, an empty or missing
policy included.

**The pattern tables** are `CONSEQUENTIAL_PATTERNS` and `CONSEQUENTIAL_WORDS` in
`src/patterns.rs`, the only place the patterns are defined. The text is lower-cased
and split into words at every character that is not a letter or a digit. A word
matches if it begins with a stem in the first table, or is a word in the second.
The second table holds what is too short to be a stem and the generic affirmations
of a dialog: `ok`, `okay`, `yes`, `apply`, `done`, `run` and others. The categories
are send, pay, purchase, delete, install, submit and execute.

**A name that cannot be read.** The tables are ASCII words. The name of a control
can be read against them only if it has at least one ASCII letter or digit and
consists of printable ASCII and a short list of typographic marks (the no-break
space, dashes, typographic quotation marks, the ellipsis). Everything else is
treated as class 2: an empty name, a name that is only a symbol or an icon, a name
in another script, and a name with a look-alike letter, an accent, or a character
that does not show. The rule is `is_legible` in `src/patterns.rs`.

**Privileged requests.** A privileged request is anything passed to the privileged
helper. There are two kinds (addendum A section 5.4):

- a **typed operation**: one of a closed set, acting on one named thing;
- a **free-text command**: a command in the privileged sandbox, with an operation
  type the request declares.

Every privileged request has an operation type:

| A repair session may cover | A repair session never covers (class 3) |
|---|---|
| diagnostics, service control, system configuration, network configuration, software management | partitioning, formatting, encryption, bootloader, firmware, OS update and rollback, device hand-off, security policy, and anything unclassified |

The list is `PrivilegedOperation::repair_session_may_cover`. It is a match with no
wildcard arm, so a new operation type cannot be added without placing it on one
side.

**Typed operations.** The set is `TYPED_OPERATIONS` in `src/action.rs`, the only
place it is defined. Each entry names the operation type, the rule its target must
satisfy, whether it writes a content, and what the helper must confine it to.

| Typed operation | Operation type | Target | Confined to |
|---|---|---|---|
| `service_start`, `service_stop`, `service_restart` | service control | a unit | The one named unit, through the service manager. No other unit, no unit file, no other argument. |
| `service_enable`, `service_disable` | service control | a unit | The one named unit, which must already be installed. No unit file is created, changed or removed. |
| `configuration_file_change` | system configuration | a file under `/etc`, with the new content | The one named regular file. The previous content is kept so that the change can be reversed. No symbolic link is followed, no directory created, no owner or mode set, nothing run. |
| `package_layer_install`, `package_layer_remove` | software management | a package | The one named package, from the configured, signed repositories. No repository is added, no local file installed, nothing of the base image removed. |
| `network_profile_reset` | network configuration | a connection profile | The one named profile, reset to the defaults of its type. No other profile, no firewall rule, no global setting. |

| Target | Rule |
|---|---|
| Unit | 1 to 255 of the ASCII letters, digits and `:` `_` `.` `@` `-`; no leading hyphen; ends in `.service`, `.socket`, `.timer` or `.path` |
| File | An absolute path below `/etc/` of at most 4096 bytes; every component is one or more of the ASCII letters, digits and `.` `_` `-` `+` `@` `:`, and is neither `.` nor `..` |
| Package | An ASCII letter or digit, then up to 254 of the ASCII letters, digits and `.` `_` `+` `-` |
| Network profile | 1 to 255 characters, each printable ASCII other than `/`, or a letter or digit of another script; no space at either end; no leading hyphen |

A typed operation whose target does not satisfy its rule, or which has a content
where its kind takes none (or none where it takes one), is incomplete and is
refused (rule 1).

**Protected targets.** `PROTECTED_TARGETS` in `src/action.rs` lists the units,
files and packages a repair session never covers: authentication and authorisation
(polkit, sudo, PAM, accounts), the firewall, the audit trail and the journal, the
project's own services, storage and encryption, the bootloader, firmware and the
OS image, and the files and units that are run as root rather than read as
configuration (unit files, udev rules, cron, shell profiles). A typed operation on
a protected target has the system-critical operation type recorded in the list. It
is class 3 and is refused (rule 6a). An entry covers every name that begins with
it, which errs towards refusing. The list is a deny list and is incomplete by
nature; see [section 11](#11-known-limits), limit 4.

### 2.3 Context

| Field | Meaning |
|---|---|
| Presence | Attended (the user is present and asked for the task) or unattended (a scheduled task or reminder firing) |
| Session locked | The session is locked |
| Trusted session proven (`trusted_session_proven`) | A separate trusted session exists and has been proven (addendum A section 19, item 2). See below. |
| Repair session | Purpose, opening time, expiry and an "ended" mark, if the user opened one |
| Standing approvals (`HeldApprovals`) | The approvals of the schedule that started the task, as the policy store holds them. Empty unless read from a policy store, and none exists yet. See below. |
| Model hint | The model flagged the action as consequential |
| Owner of the target window | For an accessibility or screen action: the user, a process with administrator rights, or unknown |

A repair session is open at a given time only if all of these hold: it was not
ended; it has a stated purpose; the time is not before its opening; the time is
before its expiry; and from opening to expiry it lasts at most 60 minutes. At the
expiry instant it is closed. A record that claims more than 60 minutes is closed
for its whole length. The default length is 15 minutes
(`DEFAULT_REPAIR_SESSION_SECONDS`); the constructors `RepairSession::open` and
`RepairSession::with_length` give no session for a blank purpose or a length
outside 1 second to 60 minutes. The engine treats a session as closed while the
session is locked or the task is unattended, whatever the record says. The owner
of the record sets the "ended" mark when the user ends the session, when the task
finishes and when the session locks, so that a session does not reopen on unlock.

**Trusted session proven.** Until the engine followed revision 8 of addendum A the field was
called "trusted path available" and meant the five compositor guarantees of
addendum A section 6. Section 19 of the addendum found that the compositor in
the image provides none of them: it has no surface that ignores emulated input
or is kept out of screen capture, so a prompt drawn on the user's desktop is
one the assistant could read and operate. The field is renamed so that no
caller can take such a prompt for it. It is true only when all of these hold:

1. class 3 confirmation, opening a repair session and loosening policy take
   place in a session of their own, outside the user's desktop, which P@x's
   processes cannot reach;
2. that session shows the user's personal phrase and picture, so that it cannot
   be imitated;
3. a prototype of it has passed the trusted-path tests of addendum A
   section 12 on the image P@x runs on.

"A confirmation dialog can be shown" is not among them. No shipped code sets
the field: the task loop builds its context with it false, and
`tur-pax policy check --trusted-session-proven` sets it only to answer what
the engine would decide, and performs nothing. The refusal that results from it
being false keeps its name and code (`trusted_path_unavailable`,
`TUR-AI-POLICY-006`).

**Standing approvals and the policy store.** Addendum A section 18, item 1:
standing approvals are held by the separate system component that owns the
policy file, and until that component exists they are not honoured. In the
engine this is structural (`src/store.rs`):

- `PolicyStore` is the interface of that component as far as the engine needs
  it: the standing approvals it holds for a schedule.
- A context carries `HeldApprovals`, whose content is private. It is either
  empty (`HeldApprovals::none()`) or read from a policy store
  (`HeldApprovals::read`). There is no conversion from a list, no
  deserialisation and no public field; two `compile_fail` examples in the
  module's documentation show the attempts that do not compile.
- Nothing in the crate implements `PolicyStore`, outside its tests. So no
  shipped code path can hand the engine an approval: not the task loop, not a
  file, not `tur-pax policy check --standing-approval`, which checks the form
  of what it is given and then ignores it. An unattended class 2 action is
  therefore always deferred.

The tests use a stand-in store to show what the engine will do once a real one
exists. When the component is built, its client implements `PolicyStore`, and
the test `no_shipped_code_can_hand_the_engine_a_standing_approval` has to be
changed in the same review.

A standing approval names a service, a tool and a complete set of arguments. It
matches a call only if all three are equal. The order of keys does not matter. An
integer and a number written with a fraction or an exponent are different, so `1`
does not match `1.0`; two numbers with a fraction or an exponent are equal if they
denote the same 64-bit floating-point value, so `1.0` matches `1.00`. Read from
JSON, an approval must not repeat a key at any depth.

The owner of the target window has no effect on any action that is not an
accessibility or screen action. `Context::attended()` and `Context::unattended()`
set it to unknown.

### 2.4 User policy

| Setting | Effect |
|---|---|
| Mode `normal` | The rules as listed |
| Mode `ask_every_action` | Every action of every class is asked first; an unattended task is deferred |
| Mode `observe_only` | Only class 0 is available |
| Excluded applications | No accessibility or screen action, observation included. Added to the built-in list. |
| Sensitive applications | Every accessibility or screen action is class 2 at least. Added to the built-in list. |
| Relaxation, class 2 | Class 2 accessibility and screen actions in that application are allowed without asking, under the conditions of rule 10c |

Two lists are built in and apply to every policy, the default and the fail-closed
one included. Nothing in the policy file removes an entry.

- `DEFAULT_SENSITIVE_APPLICATIONS`: see section 2.2.
- `DEFAULT_EXCLUDED_APPLICATIONS`: password managers, keyrings, one-time-password
  authenticators, and the prompts that ask for a password (addendum A section 8).

**Matching of application identifiers.** Exclusion and sensitivity compare without
regard to ASCII case and surrounding whitespace, and an identifier also matches
its own form with `.desktop` appended. A relaxation compares exactly.

**Plain identifiers.** An identifier is plain if it is 1 to 255 printable ASCII
characters with no space at either end. The policy file accepts plain identifiers
only. An application whose reported identifier is not plain is sensitive: such an
identifier may look like a listed one without being equal to it.

### 2.5 Where the built-in names were checked

A wrong name in a built-in list fails open: the terminal is treated as an
ordinary application, the password manager is observed, the unit is restarted.
The lists were first written from memory. On 2026-11-12 every entry was checked
against a published source: the application's own repository (the name of its
desktop file, a `StartupWMClass` line, an identifier set in its code), the file
list of a Fedora 44 or Debian package, or its Flathub identifier. Nothing was
checked on the TUR image, which is still the place that decides.

**Applications.** The source of each entry is in the code, beside the entry:
`SENSITIVE_APPLICATION_SOURCES` and `EXCLUDED_APPLICATION_SOURCES` in
`src/policy.rs` are generated from the same rows as the two lists and state
for each identifier what kind of name it is (`DesktopFile`, `WindowClass`,
`Program`, `Unverified`) and where it was seen. What the check changed:

| Finding | Before | Now |
|---|---|---|
| 1Password | `com.1password.1Password`, found in no source | `com.onepassword.OnePassword` added (its Flathub identifier and the desktop file in the vendor's archive). The old entry is kept, marked unverified. |
| KWallet's daemon | `org.kde.kwalletd5`, `org.kde.kwalletd6` | `org.kde.kwalletd` added: the name from KDE Frameworks 6.30, which the older two do not match |
| Seahorse | `org.gnome.seahorse.Application` | `org.gnome.Seahorse` added: upstream's new name, in no release yet |
| pinentry | program names only | `org.gnupg.pinentry-qt` and `org.gnupg.pinentry-qt5` added: the desktop file names the Qt pinentry sets for its windows |
| Contour | `contour`, which is the program | `org.contourterminal.Contour` added: the desktop file |
| Other prompts and managers | – | `org.gnome.keyring.PrivatePrompter`, four more `ssh-askpass` programs, two more polkit agents, `kwalletmanager5-kwalletd`, `keepass` (Fedora), `secrets`, `authenticator`, `Proton Pass` |
| Other terminals and launchers | – | development and preview builds of Ptyxis, GNOME Console, Ghostty and Warp; `foot-server`; `rxvt-unicode`; Debian's `debian-xterm`; `qterminal-drop`; `xfce4-run`; `tofi-run`, `tofi-drun`; `com.rioterm.Rio` |
| Letter case | `Alacritty` (the program is `alacritty`), `org.ijhack.QtPass` (upstream writes `org.ijhack.qtpass`), `rio` (the window class is `Rio`) | Unchanged: identifiers are compared without regard to ASCII case |

Fourteen entries could not be confirmed and are marked `Unverified` in the
tables: `org.codeberg.dnkl.foot` (an AppStream identifier only), `hyper`, the
six Windows console names (not checked against a Wine session),
`org.keepass.KeePass`, `com.1password.1Password`, `1password`, `proton-pass`,
`com.enpass.Enpass` and `org.qtpass.QtPass`. They are kept: an entry too many
costs a question; it protects nothing until it is confirmed. The test
`every_built_in_identifier_has_a_source` names them, so an entry cannot be
added without a source unnoticed.

Konsole (`org.kde.konsole`), Yakuake (`org.kde.yakuake`), KRunner
(`org.kde.krunner`), GNOME Terminal (`org.gnome.Terminal`), Ptyxis
(`org.gnome.Ptyxis`; `app.devsuite.Ptyxis` on Flathub), KeePassXC
(`org.keepassxc.KeePassXC`), Bitwarden (`com.bitwarden.desktop`), KWallet's
manager (`org.kde.kwalletmanager` from KDE Gear 25.04, `org.kde.kwalletmanager5`
before) and Seahorse were correct as written.

**Protected units.** The prefixes in `PROTECTED_TARGETS` were checked against
the unit files that Fedora 44 and 43 packages ship
(`packages.fedoraproject.org`, file lists of the packages named):

| Units seen | Package | Covered by the prefix |
|---|---|---|
| `polkit.service`, `polkit-agent-helper@.service` | polkit | `polkit` |
| `firewalld.service` | firewalld | `firewalld` |
| `nftables.service` | nftables-services | `nftables` |
| `iptables.service`, `ip6tables.service`, `ebtables.service`, `arptables.service` | iptables-services | `iptables`, `ip6tables`, `ebtables`, **`arptables`** (added) |
| `auditd.service`, `audit-rules.service` | audit, audit-rules | `auditd`, `audit-` |
| `systemd-journald.service` and its sockets, `systemd-logind.service`, `systemd-userdbd.service` | systemd | `systemd-journal`, `systemd-logind`, **`systemd-userdbd`** (added) |
| `systemd-homed.service`, `systemd-boot-update.service`, `systemd-pcrphase*.service`, `systemd-pcrextend@.service`, `systemd-pcrlock*.service` | systemd-udev | `systemd-homed`, `systemd-boot`, **`systemd-pcr`** (added) |
| `sshd.service`, `sshd.socket`, `sshd@.service` | openssh-server | `sshd` |
| `sssd.service` and its responders | sssd-common | `sssd` |
| `fprintd.service` | fprintd | `fprintd` |
| `selinux-autorelabel.service`, `selinux-autorelabel-mark.service` | policycoreutils | `selinux` |
| `usbguard.service`, `usbguard-dbus.service` | usbguard, usbguard-dbus | `usbguard` |
| `fwupd.service`, `fwupd-refresh.service` | fwupd | `fwupd` |
| `rpm-ostreed.service`, `rpm-ostreed-automatic.service` | rpm-ostree | `rpm-ostree` |
| `bootc-fetch-apply-updates.service`, `bootc-finalize-staged.service` | bootc | `bootc` |
| `clevis-luks-askpass.service`, `.path` | clevis-systemd | `clevis` |
| `tpm2-abrmd.service` | tpm2-abrmd | **`tpm2-abrmd`** (added) |
| `dbus-broker.service` | dbus-broker | `dbus` |
| `sddm.service`, `greetd.service`, `plasmalogin.service` (Fedora 44 only) | sddm, greetd, plasma-login-manager | **`sddm`**, **`greetd`**, **`plasmalogin`** (added) |

`apparmor.service` was seen in Debian's package only. `systemd-cryptsetup@` has
no unit file: its instances are generated, and the name was seen in systemd's
sources. The test `the_real_names_of_the_protected_units_are_covered` holds
these names against the table. The prefixes for files and packages were not
checked in this pass; the units `fapolicyd`, `systemd-repart`, `systemd-growfs`,
`systemd-makefs`, `systemd-mkswap`, `systemd-fsck`, `lvm2`, `mdadm`, `mdmon`,
`multipathd`, `udisks`, `bootupd`, `bootloader-update`, `grub` and `ostree`
were not looked up either.

## 3. Decisions

| Decision | Meaning | Carries |
|---|---|---|
| `allow` | The action may run now | Reason |
| `ask` | The user must approve this one action | Reason, the text to show |
| `authenticate` | A person must authenticate on the trusted path | Reason, the text to show |
| `defer` | Nobody can be asked now; held for the user's return | Reason |
| `refuse` | The action does not run | Reason, `TUR-AI-*` error code |

The decisions have a total strictness order, used by the tests:
`allow < ask < authenticate < defer < refuse`. The order is by what must happen
before the action can run: nothing; a confirmation; an authentication; the user's
return; never.

Every decision also carries the class the action was treated as.

The prompt text states the exact action. A command is shown argument by argument
and is never shortened. A typed operation is shown with its kind, its target, its
content and the operation type it is treated as. Every character that is not
printable ASCII is shown as an escape, so a prompt cannot hide characters or
reverse its reading direction. A content marked secret is shown as `<secret>`.

## 4. Decision table

The rules are evaluated in this order. The first that applies decides.

| # | Condition | Decision | Reason |
|---|---|---|---|
| 1 | The action is incomplete: no tool name, no application, empty command, or a typed operation outside the shape of its type | refuse | `malformed_action` |
| 2 | Class 4 | refuse | `prohibited` |
| 3a | Accessibility or screen action in an excluded application | refuse | `excluded_application` |
| 3b | Accessibility or screen action on a window of a process with administrator rights | refuse | `privileged_window` |
| 3c | Accessibility or screen action while the session is locked | refuse | `session_locked` |
| 4 | Mode is observe-only and the class is above 0 | refuse | `observe_only` |
| 5 | The trusted path is not available and the class is 2 or 3 | refuse | `trusted_path_unavailable` |
| 6a | Class 3, proposed as a privileged request | refuse | `system_critical_as_command` |
| 6b | Class 3, unattended or locked | refuse | `system_critical_unattended` |
| 6c | Class 3, otherwise | authenticate | `system_critical_needs_authentication` |
| 7a | Privileged request, unattended or locked | refuse | `privileged_unattended` |
| 7b | Privileged request, no open repair session | authenticate | `repair_session_required` |
| 7c | Privileged request, open repair session, mode ask-every-action | ask | `ask_every_action_mode` |
| 7d | Free-text privileged command, open repair session | ask | `privileged_command_needs_approval` |
| 7e | Typed operation, open repair session, flagged by the model | ask | `consequential_needs_confirmation` |
| 7f | Typed operation, open repair session | allow | `repair_session_open` |
| 8 | Attended task, session locked | defer | `paused_while_locked` |
| 9a | Unattended, mode ask-every-action | defer | `unattended_ask_every_action_mode` |
| 9b | Unattended, session locked, a command outside the read-only sandbox | defer | `unattended_command_while_locked` |
| 9b2 | Unattended, recording a memory entry on the user's word | defer | `unattended_memory_not_observed` |
| 9c | Unattended, class 0 or 1 | allow | `observe_permitted`, `reversible_permitted` |
| 9d | Unattended, class 2, a tool call that exactly matches a standing approval held by the policy store. No store exists, so no shipped caller reaches this rule. | allow | `standing_approval_matched` |
| 9e | Unattended, class 2, otherwise | defer | `unattended_without_standing_approval` |
| 10a | Attended, mode ask-every-action | ask | `ask_every_action_mode` |
| 10b | Attended, class 0 or 1 | allow | `observe_permitted`, `reversible_permitted` |
| 10c | Attended, class 2, accessibility or screen action in an application relaxed to class 2; the application is not sensitive, the window is known to belong to the user, and the model did not flag the action | allow | `relaxed_by_user_policy` |
| 10d | Attended, class 2, otherwise | ask | `consequential_needs_confirmation` |

Consequences worth stating:

- There is no rule that allows class 3. The engine never releases a class 3
  action. It says `authenticate`; the authentication and the release happen in the
  owning service, through polkit, on the trusted path.
- A repair session is consulted only in rule 7, which class 3 never reaches.
  Inside one, only a typed operation runs without a further approval (rule 7f).
  A free-text privileged command is asked every time (rule 7d).
- A standing approval is consulted only in rule 9d: unattended, class 2, tool
  calls. It does nothing when the user is present, does nothing for an attended
  task in a locked session, does not replace the trusted path, and cannot cover a
  command, a typed operation, a UI action or an agent operation.
- A relaxation is consulted only in rule 10c.
- The model's hint raises the class to 2. Where a relaxation or a repair session
  would then allow, it adds the confirmation (rules 7e and 10c). An action the
  model flagged is allowed only by rule 9d, where the user approved that exact
  call in advance and nobody can be asked.
- An unattended task runs whether or not the session is locked. While it is
  locked it uses tools, agent operations and read-only commands only (rules 3c,
  7a and 9b). An attended task does nothing while the session is locked (rules
  3c, 6b, 7a and 8).

The test `the_documented_decision_table_holds` in `tests/invariants.rs` asserts at
least one row for every rule of the table.

## 5. Invariants and the tests that enforce them

`tests/invariants.rs` enumerates:

- **255 actions.** Tool calls of class 0 to 3 with two argument sets.
  Accessibility and screen actions in eleven applications (plain; excluded by the
  user, and the same in another letter case with `.desktop` appended; sensitive;
  relaxed; sensitive and relaxed; a terminal on the built-in list, and the same
  in another form; a run dialog on the built-in list that the user also relaxed;
  a password manager on the built-in list; an identifier with an invisible
  character), observing, and acting on seven labels (ordinary; a consequential
  control; a consequential window title; no name; "OK"; an icon; a look-alike
  letter). Commands in every sandbox, working context and operation type. Typed
  operations of every kind, six of them on protected targets, one with a secret
  content, and seven that do not have the shape of their type. Every agent
  operation.
- **Contexts.** Attended or unattended; locked or not; trusted path or not; model
  hint or not; and three dimensions that each bear on one kind of action:
  - the repair session, in fifteen records (absent; open in the middle, opened
    this second, and at 60 minutes one second before the end; expiring now;
    expired; opened in the future; longer than 60 minutes by a second, by a day
    and without end; no purpose; a blank purpose; ended; no length; expiry before
    opening): all fifteen for privileged requests, absent and open for the rest;
  - the standing approvals, in five sets (none; exact; another letter case;
    another service; another tool): all five for tool calls, none and exact for
    the rest;
  - the owner of the target window: all three for accessibility and screen
    actions, the user and an administrator for the rest.

  That is 320 contexts for a tool call, 192 for a UI action, 960 for a privileged
  request and 128 for anything else. Tests assert that each of the three
  dimensions changes nothing outside the kind it bears on.
- **9 user policies**: each mode, with no per-application settings, with all of
  them, and with every application relaxed.

That is 738 432 decisions for each sweep. The test
`the_generators_cover_what_they_claim` asserts these numbers and that every
variant of every enumerated type appears. Everything a test compares a decision
with is stated in the test file and not taken from the engine: the class each
action's source gives it, which sessions are open, which applications are excluded
or sensitive, which approvals match.

Three further tests put hostile text (empty, very long, control characters,
direction overrides, invisible characters, look-alike letters, shell and format
syntax, text that claims an approval) into every field an action has: the
arguments of a tool call and of a command, the name of a control, the title of a
window, the identifier of an application and the target of a typed operation.

| Invariant | Test |
|---|---|
| No input yields `allow` for class 3 or class 4 | `class_3_and_class_4_are_never_allowed`; for a tool call that claims class 4, which cannot be built outside the crate, `a_tool_call_claiming_class_3_or_4_is_never_allowed` in `src/engine.rs` |
| Class 4 is always `refuse` | `class_4_is_always_refused` |
| The class is never below the class of the action's source | `the_class_is_never_below_the_class_of_the_source` |
| The class is exactly the class of the source, raised only by sensitivity, an unknown window owner and the model hint | `the_class_is_the_class_of_the_source_raised_only_by_the_stated_rules` |
| An incomplete action is refused as such, and nothing else is | `a_malformed_action_is_always_refused_as_such` |
| The model hint never lowers the outcome: for every input, the decision with the hint is at least as strict | `the_model_hint_never_lowers_the_outcome` |
| An action the model flagged is allowed only on an exactly matching standing approval | `an_action_the_model_flagged_runs_only_on_an_exactly_matching_standing_approval` |
| A stricter mode is never more permissive than normal mode | `a_stricter_mode_is_never_more_permissive_than_normal_mode` |
| Excluding an application or marking it sensitive is never more permissive | `excluding_or_marking_an_application_sensitive_is_never_more_permissive` |
| Excluded application, by the user or by the built-in list: never observed or acted on | `an_excluded_application_is_never_observed_or_acted_on` |
| Sensitive application, by the user, by the built-in list or because it cannot be identified: class 2 at least, never allowed, never relaxed, and asked when the user is present | `a_sensitive_application_is_never_observed_or_acted_on_without_asking` |
| A window of a process with administrator rights is never observed or acted on; acting in a window whose owner is unknown is never allowed | `a_window_of_an_administrator_process_is_never_observed_or_acted_on` |
| The owner of the window changes nothing for other kinds of action | `the_owner_of_the_window_matters_only_to_accessibility_and_screen_actions` |
| A relaxation changes exactly one thing: an attended class 2 UI `ask` in normal mode, in a window of the user, not flagged by the model, becomes `allow`. It never affects classes 3 and 4 | `a_relaxation_only_turns_an_attended_class_2_ui_ask_into_allow` |
| Without the trusted path nothing above class 1 runs, and nothing asks for authentication | `without_the_trusted_path_nothing_above_class_1_runs_and_nothing_authenticates` |
| Unattended or locked: never `ask`, never `authenticate` | `nobody_is_asked_when_nobody_is_present` |
| Unattended class 2 is `allow` only with an exactly matching standing approval, and only in a task that really is unattended | `unattended_class_2_runs_only_with_an_exactly_matching_standing_approval`; near misses in `standing_approval_matches_only_the_exact_call` and `standing_approval_compares_numbers_by_kind_and_value` (`src/context.rs`) and in `tests/cli.rs` |
| Unattended class 3 is always `refuse` | `unattended_class_3_is_always_refused` |
| A privileged request is allowed only if it is a typed operation, inside an open repair session, with the user present. A free-text privileged command is never allowed: inside a session it is asked. Without an open session nothing is allowed or asked | `a_privileged_request_runs_only_inside_an_open_repair_session`; the boundaries in `src/context.rs` |
| An open repair session never permits a system-critical operation; such an operation cannot be reached as a privileged request, a typed operation on a protected target included | `a_system_critical_operation_never_runs_as_a_privileged_request`; the lists in `repair_session_never_covers_the_system_critical_operations`, `every_protected_target_is_system_critical` and `a_protected_target_makes_a_typed_operation_system_critical` (`src/action.rs`) |
| Locked session: no accessibility or screen actions | `a_locked_session_has_no_accessibility_or_screen_actions` |
| Locked and unattended: tools and read-only commands only | `a_locked_unattended_task_runs_tools_and_read_only_commands_only` |
| Locking the session is never more permissive, and an attended task does nothing while it is locked | `locking_the_session_is_never_more_permissive` |
| Losing the trusted path is never more permissive | `losing_the_trusted_path_is_never_more_permissive` |
| A repair session changes the decision for privileged requests only | `a_repair_session_affects_only_privileged_requests` |
| A standing approval changes exactly one thing: an unattended class 2 tool call that would be deferred is allowed | `a_standing_approval_affects_only_unattended_class_2_tool_calls` |
| With the approvals every shipped caller has (none), an unattended class 2 action never runs and no decision is `standing_approval_matched` (addendum A section 18, item 1) | `without_a_policy_store_an_unattended_class_2_action_never_runs` |
| No source file of the crate implements the policy store or reads approvals from one, outside its tests; the content of `HeldApprovals` is private | `no_shipped_code_can_hand_the_engine_a_standing_approval`; the `compile_fail` examples of `src/store.rs`; unit tests in `src/store.rs` |
| An unattended task records only observed memory entries: a record on the user's word is deferred; an observed one runs, locked or not (section 18, item 4) | `an_unattended_task_records_only_observed_memory_entries` |
| Creating a schedule, changing a standing approval and confirming a pending memory entry are always refused as prohibited; changing a stated memory entry is never allowed without asking; proposing a schedule is class 1 (section 18, items 2 and 3) | `the_users_acts_on_memory_and_schedules_are_never_performed`; `agent_operation_classes_are_as_documented` (`src/action.rs`) |
| The built-in application identifiers each have a source, and the identifiers upstream really uses are covered | `every_built_in_identifier_has_a_source`, `the_identifiers_upstream_really_uses_are_covered` (`src/policy.rs`) |
| The real names of the protected units are covered by the deny list | `the_real_names_of_the_protected_units_are_covered` (`src/action.rs`) |
| Observe-only mode: only class 0 is allowed; nothing is asked | `observe_only_mode_allows_only_class_0` |
| Ask-every-action mode: nothing is allowed without asking | `ask_every_action_mode_allows_nothing_by_itself` |
| A refusal, and only a refusal, carries an error code; a prompt is printable ASCII | `a_refusal_and_only_a_refusal_carries_an_error_code` |
| The engine is deterministic and remembers no approval | `the_engine_is_deterministic_and_remembers_nothing` |
| The time changes nothing except for a privileged request, and no session is open at the first or the last moment that can be expressed | `the_time_matters_only_to_a_privileged_request_and_never_opens_a_session` |
| Arguments carry no authority: a claim inside the arguments changes nothing | `arguments_carry_no_authority` |
| Hostile text in a tool call or a command changes no decision | `hostile_text_in_a_tool_call_or_a_command_changes_no_decision` |
| Hostile text as a control name, a window title or an application identifier never lowers a decision | `hostile_text_on_the_screen_never_lowers_a_decision` |
| Hostile text as the target of a typed operation is refused, or names a network profile and is decided like any other | `hostile_text_as_the_target_of_a_typed_operation_is_refused_or_confined` |
| An action whose audit record cannot be written is refused | `an_action_that_cannot_be_recorded_is_refused` (`src/audit.rs`) |
| Secrets do not reach the audit record of an ordinary action; a value marked secret does not reach the record of a privileged request | `secrets_never_reach_the_record`, `a_value_marked_secret_is_replaced_in_the_record_of_a_privileged_request` (`src/audit.rs`) |

`tests/review.rs` holds one regression test for each finding of the first
independent review that can be stated as an input and a decision.

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
  "service": "org.tur.Storage1",
  "tools": [
    {
      "name": "storage_unmount",
      "description": "Unmount a volume by its TUR volume identifier.",
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
| No repeated field or key | In the manifest, in each tool, and inside an input schema at any depth |
| `schema_version` | 1 |
| `service` | A D-Bus well-known name: at least two elements of letters, digits and underscores, not starting with a digit |
| `tools` | At least one |
| `name` | `^[a-z][a-z0-9_]{0,63}$`, unique within the manifest |
| `description` | 1 to 1024 characters, not blank, no control characters |
| `class` | An integer 0 to 3. Class 4 is rejected with its own code: a prohibited operation is not offered as a tool |
| `method` | A D-Bus member name. Two tools may call one method only if they declare the same class |
| `input_schema` | An object schema with `"additionalProperties": false`, in the subset below |

**The input schema subset.** Every schema, at every depth, must have a `type` that
is one string. The keywords are:

| Keyword | Allowed on |
|---|---|
| `type`, `title`, `description`, `enum` | Every type |
| `properties`, `required`, `additionalProperties` | `object`. `additionalProperties` must be `false`, at every depth. Property names match `^[A-Za-z_][A-Za-z0-9_]{0,63}$`. Each `required` entry must be a declared property. |
| `items` | `array`, where it is mandatory |
| `minLength`, `maxLength` | `string`; counted in characters |
| `minimum`, `maximum` | `integer`, `number`. Two integers are compared exactly; if either side has a fraction or an exponent, both are compared as 64-bit floating-point values |

Any other keyword is rejected, including `anyOf`, `oneOf`, `$ref`, `pattern`,
`format` and `default`. The reason is that the agent checks the arguments of every
call against the schema, and it enforces exactly this subset. A manifest cannot
state a constraint the agent would ignore.

`ToolManifest::tool_call` builds a tool call: it looks the tool up by its exact
name, takes the class from the manifest and checks the arguments. Arguments that
arrive as text must be parsed with `strict::object_from_str`, which refuses a
repeated key. What is sent to the service must be that parsed value, written out
again, and never the text the model produced.

## 7. User policy file

Schema: [`schemas/agent-policy.schema.json`](../../schemas/agent-policy.schema.json).

```json
{
  "schema_version": 1,
  "mode": "normal",
  "excluded_applications": ["org.example.Vault"],
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
| `excluded_applications`, `sensitive_applications` | Optional lists of plain application identifiers, added to the built-in lists |
| `relaxations` | Optional. Each names a plain application identifier, matched exactly, and the class 2. Any other class is an error. An application may appear once, ignoring letter case and a trailing `.desktop`. |

Unknown fields and repeated fields are errors. There is no field that relaxes
class 3 or class 4, and the type that holds a relaxation has no value for them.
There is no field that removes an entry from a built-in list.

If an application is both sensitive and relaxed, it is sensitive. That includes an
application on the built-in list.

If the policy file cannot be read or is invalid, the caller must use
`UserPolicy::fail_closed()`, which is observe-only. The default policy, normal
mode with empty lists, is for the case where the user has no policy file.

The file is owned by a separate system component and changes only through system
settings on the trusted path (addendum A section 17, item 5). Changing it is a
class 4 operation for the assistant (`AgentOperation::PolicyWrite`).

`tests/schemas.rs` validates example documents against both schemas with a
validator of its own, and requires the schema and the loader to give the same
answer on each. The rules a schema cannot express are listed in that test, each
with a document: uniqueness of tool names and of relaxed applications, one class
for a method, `required` entries naming declared properties, an integer written
with a fraction, and repeated keys.

## 8. Audit record

One record for each proposed action and its decision, rendered to journal fields:

```text
MESSAGE=tur-pax: refused org.tur.Storage1:storage_format (class 3, tier 1): trusted_path_unavailable; arguments volume_id=string
TUR_AI_TASK=task-0001
TUR_AI_ACTION=org.tur.Storage1:storage_format
TUR_AI_CLASS=3
TUR_AI_DECISION=refused
TUR_AI_TIER=1
TUR_AI_ATTENDED=yes
TUR_AI_PROVIDER=anthropic
TUR_ERROR_CODE=TUR-AI-POLICY-006
```

| Field | Values |
|---|---|
| `TUR_AI_ACTION` | `<service>:<tool>`; `accessibility:observe`, `accessibility:act`; `screen:observe`, `screen:act`; `command:read_only`, `command:user`, `command:privileged`; `operation:<typed operation>`; `agent:<operation>` |
| `TUR_AI_CLASS` | The class the action was treated as, 0 to 4 |
| `TUR_AI_DECISION` | `allowed`, `confirmation_required`, `authentication_required`, `deferred`, `refused`; and, for the user's answer to a prompt, `confirmed`, `denied` |
| `TUR_AI_TIER` | 1 tool, 2 accessibility, 3 screen and input, 4 command or typed operation, 0 an operation of the agent itself |
| `TUR_AI_SANDBOX` | Present for a command: `read-only`, `workspace`, `user` or `privileged`; and for a typed operation: `privileged` |
| `TUR_AI_ATTENDED` | `yes` or `no`: the presence of the task. A locked session does not make an attended task unattended. |
| `TUR_AI_REQUEST` | Present for a privileged request: the full statement of the request |
| `TUR_ERROR_CODE` | Present only for a refusal |

What a record may contain is restricted by construction:

- The argument values of a tool call are never written. Tool arguments are
  summarised as the property names with the kind of each value (`string`,
  `number`, and so on). At most 16 names are listed.
- If a property name resembles a secret, the kind is withheld too. If a name is
  not a plain identifier, or looks like an opaque token, the name is not written.
- A command outside the privileged sandbox is summarised as the path of its
  program, as it was given, and the number of arguments. The arguments are never
  written. The path is withheld if it is not a plain path of at most 255 bytes or
  if a part of it resembles a secret.
- **A privileged request is written in full** (addendum A section 17, item 9): a
  free-text command with every argument, a typed operation with its kind, target,
  content and operation type. The text is the one shown to the user, with the same
  escaping. A content marked secret is written as `<secret>`. Nothing else is
  withheld: a rule that withheld what merely looks like a secret would let the
  model keep part of a command out of the record.
- A UI action is summarised as the application and the pattern category that
  matched. Window titles and control names are never written.
- The prompt text of any other action is never written.
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

**Two further events** (addendum A section 21, item 7). `TUR_AI_DECISION` is
about the decision; it cannot say what became of an action that was allowed.
The audit module defines the two events that do, beside the record
(`src/audit.rs`); the orchestrator writes them to the trail of a task.

| Event | `TUR_AI_EVENT` | Type | Fields |
|---|---|---|---|
| Action outcome | `executed` | `ActionOutcome` | `TUR_AI_ACTION`; `TUR_AI_RESULT` (`ok`, `failed`, `timed_out`, `not_started`: `ActionResult`); `TUR_AI_EXIT`; `TUR_AI_OUTPUT_BYTES`; `TUR_AI_TRUNCATED`; `TUR_AI_WITHHELD` (a count); `TUR_ERROR_CODE` if the action did not end well |
| Output withheld | `output_withheld` | `OutputWithheld` | `TUR_AI_ACTION`; `TUR_AI_WITHHELD` (the number of pieces, never zero); `TUR_AI_WITHHELD_KINDS` (sorted, each once); `TUR_ERROR_CODE` = `TUR-AI-OUTPUT-001` |

Neither type has a field for output. `OutputWithheld` takes the kind of each
withheld piece as a name fixed in the program, so nothing read from the output
can be put into it. An output-withheld event is written only when something
was withheld, after the outcome of its action and before the result is passed
on; if either cannot be written the result is not passed on and the task stops.

## 9. Command line and error codes

```text
tur-pax manifest validate <file>
tur-pax policy check --manifest <file> --tool <name> [--args <json>]
                       [--policy <file>] [--unattended] [--locked] [--trusted-session-proven]
                       [--repair-session-until <seconds> --repair-session-purpose <text>
                        [--repair-session-opened <seconds>]]
                       [--standing-approval <json>]... [--model-flags-consequential]
                       [--now <seconds>] [--task <id>] [--json]
```

`policy check` decides tool calls only. It prints the decision, and with `--json`
also the audit record. The defaults are the strict ones: attended, unlocked,
**no proven trusted session**. Without `--trusted-session-proven`, classes 2 and
3 are refused; the flag supposes a trusted session that does not exist, to show
what the engine would decide, and performs nothing. `--trusted-path` is its
former name and is still accepted. Without `--repair-session-opened` a repair
session is taken to have the default length of 15 minutes. `--args` and
`--standing-approval` must not repeat a key.

**`--standing-approval` is checked and not honoured.** Only the policy store can
hand the engine an approval (section 2.3), and this program is not one. The
approval's form is validated; the decision is made without it; the output
states how many were ignored (`standing_approvals_ignored` with `--json`).
`tur-pax policy check --unattended --trusted-session-proven --standing-approval
<exact match>` therefore prints `defer`, where it used to print `allow`.

`tur-pax run` and `tur-pax chat`, the text interface of the orchestrator, are
described in [pax-orchestrator.md](pax-orchestrator.md).

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

The error codes are declared once, in the registry table in `src/codes.rs`. Each
entry has the fields specification section 29.3 requires: title, severity,
detection criteria, user message, technical explanation and recovery action
(`RegistryEntry`). Each code is checked when the crate is compiled, through
`ErrorCode::from_static`. Unit tests check that every entry is complete, that
codes and titles are unique, and that a user message uses no term of the design.

| Severity | Meaning |
|---|---|
| `notice` | The assistant did not do something because of a setting or of its situation. Nothing is wrong. |
| `warning` | The assistant proposed something it must never do, or in a form that is not accepted. It did not run. |
| `error` | A file or an input is invalid. Nothing was decided. |
| `critical` | A safeguard the assistant depends on is not working. |

| Code | Severity | Meaning |
|---|---|---|
| `TUR-AI-MANIFEST-001` to `-011` | error | Tool manifest: parse, version, service, no tools, tool name, duplicate tool, description or method, class range, class 4 declared, input schema, a method declared with two classes |
| `TUR-AI-TOOL-001`, `-002` | warning | Unknown tool; arguments do not match the input schema |
| `TUR-AI-POLICYFILE-001` to `-004` | error | Policy file: parse, version, application, relaxation |
| `TUR-AI-POLICY-001` | warning | Prohibited (class 4) |
| `TUR-AI-POLICY-002` | warning | Malformed action |
| `TUR-AI-POLICY-003` | notice | Excluded application |
| `TUR-AI-POLICY-004` | notice | Session locked |
| `TUR-AI-POLICY-005` | notice | Observe-only mode |
| `TUR-AI-POLICY-006` | notice | Trusted path not available |
| `TUR-AI-POLICY-007` | notice | System-critical action with nobody present |
| `TUR-AI-POLICY-008` | warning | System-critical operation proposed as a privileged request |
| `TUR-AI-POLICY-009` | notice | Privileged request with nobody present |
| `TUR-AI-POLICY-010` | warning | Target window belongs to a process with administrator rights |
| `TUR-AI-AUDIT-001` | critical | Audit record could not be written |
| `TUR-AI-INPUT-001`, `-002` | error | Unreadable file; invalid command-line value |

## 10. Interpretations

Addendum A leaves the following cases undefined or ambiguous. In each, the stricter
reading was implemented. Choices 1 to 38 are those of the engine's author. The
first independent review went through each; the column on the right records what
it changed. Choices 39 onward were made while aligning the engine with revision 4.

| # | Case | Choice | Review |
|---|---|---|---|
| 1 | Class of a privileged request that is not system-critical | Class 2, with its own rule. It cannot be class 3, because class 3 is never allowed and a repair session allows these. | Kept; confirmed by addendum A section 17, item 10. The session stands in for the confirmation only for a typed operation (rule 7f). |
| 2 | A system-critical operation requested as a privileged command | Refused outright, with no offer to authenticate. The operation exists only as a class 3 tool of the owning service, where polkit enforces it. | Kept; extended to typed operations on protected targets. |
| 3 | A privileged operation the helper cannot classify | Treated as system-critical. | Kept. |
| 4 | Privileged request in an unattended task | Refused, not deferred, even if a repair session is recorded as open. An unattended task never runs anything as root. | Kept. |
| 5 | Locked session with a task marked attended | The task is paused: every action that is not refused is deferred, classes 0 and 1 included. Class 3 and privileged requests are refused. A repair session is treated as closed, whatever the record says. Standing approvals are not honoured: they belong to a schedule, and this task has none. | Kept; confirmed by section 17, item 8. |
| 6 | Repair session with no stated purpose | Never open. | Kept. |
| 7 | Repair session at the exact expiry instant | Closed. | Kept. |
| 7a | Repair session with no effective time limit | A session with more than 24 hours left is closed. | **Overruled** by section 17, item 10: 60 minutes at most, measured from the opening. See 51 and 52. |
| 8 | Open repair session while the trusted path is unavailable | Permits nothing: rule 5 comes first. | Kept. |
| 9 | "Ask before every action" and class 0 | Class 0 is asked too. | Kept; confirmed by section 17, item 10. |
| 10 | "Ask before every action" in an unattended task | Everything is deferred, including a class 2 call with a matching standing approval, and including classes 0 and 1. | Kept. |
| 11 | "Ask before every action" inside an open repair session | Each privileged request is asked, showing the exact request. | Kept. |
| 12 | "Ask before every action" or observe-only, and a relaxation | The global mode wins. A relaxation applies only in normal mode. | Kept. |
| 13 | What a class 1 relaxation does | Nothing; the value is accepted in the file. | **Overruled** by section 17, item 7: removed. The file accepts class 2 only. |
| 14 | Which actions a relaxation covers | Accessibility and screen actions only. | Kept; confirmed by section 5.3. |
| 15 | A relaxation in an unattended task | Does not apply. | Kept. |
| 16 | A relaxation without the trusted path | Does not apply. Rule 5 comes first. | Kept. |
| 17 | An application that is both sensitive and relaxed | Sensitive. | Kept; holds for the built-in list too (46). |
| 18 | Observing a sensitive application | Class 2: asked, like acting in it. | Kept. It is the stricter reading: what is observed is sent to the provider. |
| 19 | Matching of application identifiers | Exclusion and sensitivity ignore ASCII case and surrounding whitespace. Relaxation is exact. | Kept; exclusion and sensitivity also match the `.desktop` form. |
| 20 | Acting on a control with no name | Class 2: when in doubt, ask. | Kept; extended to every name that cannot be read (48). |
| 21 | Which actions a standing approval covers | Tool calls only. | Kept. |
| 22 | A standing approval when the user is present | Ignored. The user is asked. | Kept. |
| 23 | A standing approval without the trusted path | Does not apply; the decision is refuse, not defer. | Kept. |
| 24 | Standing approval and JSON numbers | `1` and `1.0` do not match. | Kept. The earlier text said numbers "compare as written"; they compare as parsed (section 2.3). |
| 25 | The model hint on a class 2 action that a standing approval, a relaxation or a repair session already allows | No effect. | **Overruled in part.** Addendum A section 5.1 says the hint can "add a confirmation". It now does for a relaxation and for a typed operation in a repair session, where the user is present to be asked. For a standing approval it still has no effect: nobody can be asked, and the user approved that exact call in advance. |
| 26 | Class of creating or changing a reminder or schedule | Class 2. | **Replaced** (addendum A section 18, item 3). Proposing a schedule is class 1: nothing takes effect until the user confirms it. Creating, changing or removing one is the user's act and is class 4 for P@x, like changing a standing approval. |
| 27 | Class of removing a memory entry | Class 1, the same operation as writing. | **Replaced.** The operation is split. Recording, and changing or removing an observed entry, are class 1. Changing or removing an entry the user stated or confirmed, or a saved procedure, is **class 2**: the stricter of the two readings that keep it possible at all. It is asked on screen, so until a trusted session is proven it is refused, and unattended it is deferred. Confirming a pending entry is class 4 (section 18, item 2). |
| 27a | An unattended task that proposes a record on the user's word | Deferred (`unattended_memory_not_observed`), not refused: the entry is offered to the user on return, as a deferred action is. | New (section 18, item 4). |
| 27b | A standing approval given to `policy check` | Checked for its form, then ignored. | New (section 18, item 1). |
| 28 | Unattended class 2 without the trusted path | Refused, not deferred. | Kept. |
| 29 | Class 2 confirmation without the trusted path | Refused. | Kept. |
| 30 | `TUR_AI_DECISION` for a decision that is not final | `confirmation_required`, `authentication_required`, `deferred`. | Kept; now the design (section 17, item 11). |
| 31 | `TUR_AI_TIER` for a command and for an agent operation | 4 and 0. | Kept; now the design. A typed operation is tier 4 too (57). |
| 32 | An audit record that cannot be written | The action is refused. | Kept. |
| 33 | Logging of privileged commands | Program name, argument count and operation type only. | **Overruled** by section 17, item 9: recorded in full. |
| 34 | An incomplete action | Refused. | Kept. |
| 35 | `mode` missing from a policy file | The file is invalid. | Kept. |
| 36 | An empty tool list in a manifest | The manifest is invalid. | Kept. |
| 37 | Nested object schemas in a manifest | Must also set `additionalProperties: false`. | Kept. |
| 38 | Schema keywords the agent does not enforce | Rejected. | Kept. |
| 39 | The compositor does not say whose process a window belongs to | Acting in it is class 2. Observing it is not raised. A relaxation does not apply. | New. A field that defaulted to "the user's" would allow whenever a caller forgot to set it. |
| 40 | Observing a window of a process with administrator rights | Refused, like acting in it. Section 17, item 2, makes such a window a protected surface, and protected surfaces are not captured. | New. |
| 41 | A typed operation on a protected target | System-critical: refused outright, as in 2. | New. |
| 42 | A typed operation whose target does not have the shape of its type | Incomplete: refused, not asked. There would be nothing definite to show. | New. |
| 43 | The model hint on a typed operation in an open repair session | Asked. | New; see 25. |
| 44 | Which values can be marked secret | The content of a typed operation, by the orchestrator, for a value the user entered on a protected surface. A free-text command has nothing that can be marked and is recorded whole; a secret must not be put on a command line. | New. Section 17, item 9, does not say who marks a value. |
| 45 | What is withheld from the record of a privileged request because it resembles a secret | Nothing. | New. A heuristic would let the model shape an argument so that it is left out. |
| 46 | The built-in lists and the user policy | The user adds to them. Nothing removes an entry, and a relaxation of a built-in sensitive application has no effect. | New. Section 17, item 1, says "sensitive by default"; a default the file could switch off would be a way to restore the gap by editing one file. |
| 47 | An application identifier that is not plain printable ASCII | In the policy file: an error. Reported for a window: the application is sensitive. | New. |
| 48 | A control whose name is a symbol, in another script, or holds a look-alike or invisible character | Class 2. | New; extends 20. |
| 49 | A control that only affirms: "OK", "Yes", "Apply", "Done", "Continue" | Class 2. What it confirms cannot be read from the button. | New. |
| 50 | An unattended task, session locked, a command outside the read-only sandbox | Deferred, class 1 included. Section 5.6 allows "tools and read-only commands only". Deferred, not refused, so that it is offered on return. | New. |
| 51 | A repair-session record that claims more than 60 minutes | Closed for its whole length, not only after the sixtieth minute. A record like that was not written by the component that owns it. | New. |
| 52 | A time before the opening of a repair session | Closed. This catches a clock set back past the opening. | New. |
| 53 | A JSON object that repeats a key | An error: in tool arguments, in a standing approval, in an input schema. | New. |
| 54 | One method declared with two classes | An error, also when the second declaration is the stricter one. | New. |
| 55 | `TUR_AI_ATTENDED` for an attended task in a locked session | `yes`. The field records what the task is, not who is at the screen. | New. |
| 56 | `TUR_AI_SANDBOX` for a typed operation | `privileged`. | New. |
| 57 | `TUR_AI_TIER` for a typed operation | 4, as for a command. | New. |

## 11. Known limits

What this engine cannot guarantee by itself.

1. **The engine decides; it does not enforce.** A decision has effect only if every
   action passes through `decide` and the caller obeys it. `decide` is public, so a
   caller can also decide without recording; the record is written only by
   `decide_and_record`. Enforcement of class 3 also lives in each service, through
   polkit: a class 3 operation must fail without human authentication even when
   called directly, bypassing the agent (addendum A sections 5.1 and 12, "policy
   tests"). That is a property of each service and is tested there, from milestone
   A1. Nothing in A0 tests it.

2. **The trusted session is not the engine's to check.** The engine takes
   `trusted_session_proven` as an input (section 2.3). It cannot check any of
   the three conditions. Until the trusted session of addendum A section 19,
   item 2, exists and has passed the trusted-path tests, the input must be
   false, and the engine then refuses everything above class 1. Setting it to
   true without that proof removes the protection the rest of the design
   assumes. The name of the field says what has to be true; nothing in the
   type system stops a caller from lying.

3. **Confirmation is not bound to the action here.** The engine returns `ask` or
   `authenticate` with the text to show. Showing it, collecting the answer, and
   making sure the approved action is the one that runs are the orchestrator's
   and the compositor's work. An `ask` in ask-every-action mode for a class 0 or 1
   action, without the trusted path, is an ordinary prompt the assistant could
   in principle answer itself; it adds friction, not a guarantee.

4. **A privileged request is only as confined as the helper makes it.** The engine
   checks the shape of a typed operation and refuses the targets on its deny list.
   That list cannot be complete: a unit or a file it does not name passes. What
   must hold is the helper's confinement of each type, as the table in section 2.2
   states it. For a free-text command the engine checks nothing but the declared
   operation type; `rm -rf /boot` declared as "service control" reaches the user
   as a prompt, and what stops it after an approval is the helper: no access to
   block devices, the EFI variables, `/boot`, or the polkit, sudo, PAM and crypttab
   configuration, for every type a repair session may cover. The helper does not
   exist yet. Until it does, "a repair session never covers system-critical
   operations" is true of the engine's decisions and unproven for the system.
   The unit, file and package names in the deny list were written for the Fedora
   base and have not been checked against the image.

5. **The same holds for the other sandboxes.** A command in the read-only sandbox
   is class 0 because the sandbox is supposed to make it harmless. The engine
   trusts the sandbox kind it is given. A read-only view that includes secrets,
   or the data of excluded applications, would let a class 0 command read them and
   pass them to the provider (addendum A section 17, item 3). The sandbox profiles
   must exclude them.

6. **Exclusions and sensitivity cover accessibility and screen actions only.** A
   tool call or a command that touches an excluded application's data is not
   recognised as such.

7. **The UI rule set is a heuristic.** The pattern tables are English only. A
   control with a misleading label ("Next" on a button that pays), a label in
   another language written in plain ASCII letters ("Enviar", "Senden"), a label
   obfuscated in ASCII ("S e n d", "De1ete"), or a key press in a window whose
   title says nothing is not recognised. For screen-and-input actions the control
   description is only as trustworthy as its source: if it comes from the model, a
   compromised model can describe a "Pay" button as "Cancel". The description
   should come from the accessibility tree or the compositor. The real bound on UI
   actions is addendum A's first rule: privileged prompts reject emulated input.
   Within class 1 and class 2, an assistant misled by screen content can still do
   unwanted things, as addendum A section 7 says.

8. **The built-in lists are lists.** A terminal or a password manager that is not
   on them is an ordinary application until the user adds it. An application that
   embeds a terminal, such as an editor with a terminal panel, is not on the list;
   the pattern tables catch the usual names of such panels in a window title or a
   control name, and nothing else does. The application identifiers were written
   from the upstream desktop-file names as known to the author of the list and
   have not been checked against the packages the image ships. Private browser
   windows cannot be told from ordinary ones by an identifier and are the
   compositor's to exclude.

9. **A manifest is trusted.** The class of a tool is what the service declares. A
   service that declares a destructive operation as class 1 is not detected.
   Manifests are part of each service's reviewed interface. Two manifests that
   declare the same service are not compared with each other here; the tool
   registry must refuse the second.

10. **The audit record of a privileged request holds what the request held.** A
    secret typed into a free-text privileged command is recorded. That is the
    design's choice (section 17, item 9): the command is shown to the user anyway.
    The journal sink must therefore keep these records readable by the user and
    administrators only. For other actions the secret heuristics are heuristics: a
    short secret used as a program name or as a property name is not recognised.
    Argument values of tool calls are never written, whatever they are.

11. **Memory is kept out by construction, not by inspection.** The engine has no
    memory input, and nothing in a context can be filled from memory if the
    orchestrator follows the table in section 1. The engine cannot detect an
    orchestrator that builds a context from recalled text.

12. **Every grant in the context is taken as given.** The engine cannot tell
    whether a standing approval, a repair-session record, the user policy, "the
    trusted path is available" or "this window belongs to the user" came from the
    component that owns it or was written by something else. That component is
    part of the security boundary ([ADR-0012](../adr/0012-assistant-policy-model.md));
    it does not exist yet.

13. **The engine has no clock and no memory, so some rules belong to the owner of
    the state.** If the clock is set back to a moment inside a repair session that
    had expired, the session reads as open again; the engine catches only a clock
    set back past the opening. A session that was locked reads as open again after
    unlock unless its record was marked ended. The owner of the record must mark a
    session ended when it expires, when it is locked, when the user ends it and
    when the task finishes, and should take its times from a clock that cannot be
    set back.

14. **Numbers.** An integer outside the range of 64-bit integers is read as a
    floating-point number and is then not an `integer` for an input schema. Bounds
    that involve a fraction or an exponent are compared as 64-bit floating-point
    values, exact up to 2^53.

15. **The JSON schemas describe; the loaders decide.** `tests/schemas.rs` holds
    the two together on about 230 example documents and lists the rules a schema
    cannot express. The validator in that test implements only what the two
    schemas use. It is not a general JSON Schema validator and has not been
    compared with one.

16. **Revisions 5 to 8 of addendum A: implemented, with what is left.** Section
    18, items 1, 3 and 4, and section 19, item 2, are implemented as sections
    2.2, 2.3 and 4 describe. What is left: (a) the policy store itself does not
    exist, so standing approvals cannot be honoured at all, and the stand-in
    store of the tests is the only evidence of what the engine does with one;
    (b) the engine is told which kind of memory entry an operation concerns by
    whoever builds the action. Nothing writes to memory through the engine
    yet, so nothing shows that a caller tells it the truth; (c) the kinds of
    the memory layer (`tur-pax-memory`: stated, observed, procedure, from
    content) are mapped onto the engine's operations in this document only,
    not in code; (d) the built-in names were checked against upstream sources
    and distribution packages, not against the image (section 2.5).

## 12. Not in A0

The orchestrator, the tool registry over D-Bus, provider adapters, the journald
sink, the privileged helper, the sandboxes, the component that owns the policy
file, the standing approvals and the repair-session record, the schedule store,
memory storage, the trusted path, voice, and undo.
