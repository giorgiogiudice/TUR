# ADR-0028: P@x's policy store is a root-owned directory written through polkit

- **Status:** Proposed
- **Date:** 2026-11-13
- **Specification sections:** AI control layer (addendum A) sections 5.3, 5.6, 17 item 5, 18 item 1, 19 item 2, 22 item 2, 23 item 1
- **Related:** [ADR-0012](0012-assistant-policy-model.md) (inputs that grant authority are owned by a separate component)
- **Design:** [pax-policy-store.md](../architecture/pax-policy-store.md)

## Context

ADR-0012 put every input that grants P@x authority (the person's policy,
standing approvals, the repair-session record, the trusted-path state) in a
separate system component that P@x's process can read and cannot write.
Addendum A section 18, item 1, adds that schedules with standing approvals
must not live anywhere P@x's process can write. The desktop design calls that
component `tur-pax-guard` and gives it the trusted screen (task D-16) and the
approval sheet (D-18), neither of which exists. Until now nothing at all
existed: the policy file was read from `~/.config/tur/pax/policy.json`, which
every program of the person, P@x's process included, can rewrite, and
standing approvals were held only by a stand-in in tests (issues 30, 43).

P@x runs as the person. Anything the person's processes can write, P@x's
process can write. So the store needs another owner: root, or a system
user. And a change to it needs a path that P@x's process cannot complete by
itself.

## Options considered

1. **Wait for `tur-pax-guard` and the trusted screen.** Nothing changes until
   D-16 is built and proven. Correct in the end, but the policy file stays
   writable by P@x's process for as long as that takes.
2. **A system service on the system bus** (`org.tur.PaxGuard1`, its own
   system user, its own D-Bus policy). The eventual shape of `tur-pax-guard`.
   A daemon, a bus interface, a system user and a review of each, for a store
   that today holds two small files and is written a few times in its life.
3. **A root-owned directory, written by a small program started through
   `pkexec`.** `/var/lib/tur-pax-policy/<uid>/`, readable by the person,
   writable by root only. `tur-pax-policy` runs as the person to read and
   starts itself through `pkexec` to write; polkit asks for an
   administrator's password for every change that could loosen P@x and for
   nothing when standing approvals are taken away. No daemon, no bus
   interface; `pkexec` names the person (`PKEXEC_UID`), so root writes that
   person's directory and no other.

## Decision

Option 3, as the store of the eventual policy owner. The person's policy and
the standing approvals of their schedules live in
`/var/lib/tur-pax-policy/<uid>/`, owned by root, readable by the person's
group. P@x applies the store's policy, made stricter by the person's own file
and never less strict. Standing approvals are bound to a schedule's
identifier and the exact text of its task, and are **not yet read by the task
loop**: the engine refuses every unattended class 2 action until a proven
trusted session exists, so reading them would change nothing, and wiring them
in is the reviewed step that comes with the trusted screen.

## Consequences

- No file that P@x's process can write can make P@x less strict than the
  store says. The person's own file still works, for tightening only.
- The polkit prompt is drawn on the person's desktop. It is **not** the
  trusted screen of addendum A section 19, item 2: other software in the
  session could show something that looks like it. What it gives is that a
  change needs an administrator's password, which P@x does not have and
  cannot type into a polkit agent (its commands have no bus and run with
  `no_new_privs`; its screen tier refuses the authentication agent, an
  excluded application). When `tur-pax-guard` and the trusted screen exist,
  they replace the polkit prompt; the files and their format stay.
- Settings that loosen P@x but are not policy (switching it on, the provider,
  the key, the day's limit; issue 66) stay in the session service for now:
  they grant no authority above class 1, and moving them would change the
  desktop's P@x window (D-17). Their move is an owner decision, listed in the
  design.
- A new program runs as root. It links only `serde` and `serde_json`, takes
  its document on standard input (never a path chosen by the caller), refuses
  `--root` when it runs as root, writes with `O_CREAT|O_EXCL` and a rename in
  a directory only root can write, and refuses a store directory that is a
  link, belongs to someone else, or can be written by others.

## Verification

- Unit and command-line tests of `services/tur-pax-policy` (the CI `rust`
  workflow), run as the person against a store of the test's own.
- `services/tur-pax` tests: the store's policy can only be tightened by the
  own file, in every combination of modes; an unusable file leaves
  observation only.
- On the booted image (the `boot` workflow): the store directory exists and
  belongs to root; the person can read their store and cannot write it;
  `pkexec`'s path, simulated with `sudo` and `PKEXEC_UID`, writes it with
  the right owner and modes, and the person's `tur-pax-policy show` reads
  it back.
- Not verified: the polkit prompt itself (it needs a person at the
  keyboard), and its behaviour with KDE's authentication agent.
