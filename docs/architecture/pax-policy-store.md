# P@x policy store

**Component:** `services/tur-pax-policy` (program `tur-pax-policy`)
**Decision:** [ADR-0028](../adr/0028-pax-policy-store.md)
**Design:** [AI control layer](ai-control-layer.md), sections 17 item 5, 18
item 1, 22 item 2, 23 item 1; [agent policy](agent-policy.md), sections 2.3,
2.4 and 7

The place where the inputs that loosen P@x are kept, out of reach of P@x's
own process: the person's policy and the standing approvals of their
schedules. It is the store of the policy owner the desktop design calls
`tur-pax-guard`; the trusted screen and the approval sheet of that owner do
not exist yet (desktop tasks D-16, D-18).

## 1. Layout

```text
/var/lib/tur-pax-policy/            root:root 0755   (systemd-tmpfiles, image/rootfs/usr/lib/tmpfiles.d/tur-pax-policy.conf)
    <uid>/                          root:<group> 0750
        policy.json                 root:<group> 0640
        approvals.json              root:<group> 0640
```

`<group>` is the person's primary group, from `getent passwd`. The person can
read their own directory; nobody else can, and only root can write.

## 2. Writing

| Command | polkit action | Asks |
|---|---|---|
| `tur-pax-policy set-policy < policy.json` | `org.tur.pax.policy.change` | an administrator's password, every time |
| `tur-pax-policy set-approvals < approvals.json` | `org.tur.pax.policy.change` | an administrator's password, every time |
| `tur-pax-policy clear-approvals --schedule ID` | `org.tur.pax.policy.tighten` | nothing: it only takes approvals away |

Run by the person, the program starts itself again through
`/usr/bin/pkexec /usr/bin/tur-pax-policy …`. Run as root, it writes only for
the person `pkexec` names in `PKEXEC_UID`, and refuses to run as root
otherwise. It reads the document from standard input, never from a path the
caller names. `--root DIR` (a store elsewhere, written as the person) is for
tests and is refused as root.

A write checks the document (section 3), makes the person's directory if it
is missing, refuses a directory that is a link, belongs to someone else or
can be written by others, writes a new file with `O_CREAT|O_EXCL` and mode
0640, gives it to root and the person's group, and renames it into place.

Reading (`show`, `approvals-for`) is the person's own act and needs nothing.

**Why this is out of P@x's reach.** P@x runs as the person, so the
directory's owner is root. Its commands run in the read-only sandbox with no
session or system bus and with `no_new_privs`, so they can start neither
`pkexec` nor polkit's agent. Its screen and accessibility tiers refuse the
authentication agent (an excluded application) and do not know the password.
What the polkit prompt does not give is the trusted screen's guarantee that
the prompt is real: it is drawn on the desktop (ADR-0028, consequences).

## 3. The documents

**`policy.json`**: the policy file of `tur-pax` ([agent-policy.md](agent-policy.md),
section 7). The store checks its outline: an object without a repeated key,
`schema_version` 1, a known mode, no other fields than the format's, lists
where lists go. `tur-pax` reads it completely.

**`approvals.json`**:

```json
{
  "schema_version": 1,
  "schedules": [
    {
      "schedule": "s-00000001",
      "task": "Send the weekly report to the team",
      "approvals": [
        { "service": "org.tur.Mail1", "tool": "mail_send", "arguments": { "to": "team@example.org" } }
      ]
    }
  ]
}
```

No repeated key at any depth, no unknown field, at most 64 schedules and 32
approvals each, a schedule identifier of letters, digits, `-` and `_`. A set
of approvals applies to a run of its schedule only while that schedule's task
is exactly the stored `task` (`Approvals::for_schedule`): the schedule itself
stays in the person's own store (`tur-pax-memory`), which P@x's process can
write, so a changed task loses its approvals.

## 4. How P@x uses it

**The policy.** `tur-pax serve` and `tur-pax run` apply
`UserPolicy::in_force(store, own)`: the store's policy, made stricter by the
person's own file (`~/.config/tur/pax/policy.json`, or `--policy`) and never
less strict. The own file can choose a stricter mode and add exclusions and
sensitive applications; its relaxations are ignored. A file that is there and
cannot be used, in either place, leaves P@x observing only. With no store
file, the default policy is the base. `tur-pax policy check` is a what-if
tool, given its inputs on the command line, and does not read the store.

**Standing approvals: not read yet.** The policy engine refuses every
unattended class 2 action until a proven trusted session exists
([agent-policy.md](agent-policy.md), rule 5), so honouring an approval would
change no decision. Reading `approvals.json` into the engine is done by
implementing `tur_pax::PolicyStore` over this store, in the same review that
lets the trusted session be set; the test
`no_shipped_code_can_hand_the_engine_a_standing_approval` marks the spot.

## 5. What is not here

- **The settings that loosen P@x without being policy** (issue 66):
  switching it on, the provider, the key, raising the day's limit. They grant
  no authority above class 1 and stay in the session service, open to any
  program of the person. Moving them here would make each one ask for an
  administrator's password and change the P@x window (D-17). Owner decision:
  move them here now, or with the trusted screen.
- The repair-session record and the trusted-path state: they come with the
  trusted screen.
- A settings page that writes the store: the desktop's (D-19).

## 6. Tests

| What | Where |
|---|---|
| Outline of a policy; strict reading of approvals; approvals bound to schedule and task | `services/tur-pax-policy/src/lib.rs` |
| Who writes where: root only through `pkexec`, for `PKEXEC_UID`; `--root` refused as root | `services/tur-pax-policy/src/main.rs` |
| The program as the person on a store of its own: modes, refusal changes nothing, approvals per schedule and task, a link in place of the directory | `services/tur-pax-policy/tests/store.rs` |
| The own file only tightens, every pair of modes; an unusable file means observe-only | `services/tur-pax/src/policy.rs`, `the_person_s_own_file_can_only_make_the_store_s_policy_stricter` |
| On the booted image: owner and modes, the person cannot write, root through `PKEXEC_UID` can, `show` reads back | `boot` workflow, check `components.pax_policy_store` |
