# pax-issues-1 — P@x: grants through the system's links, the policy lists from the image, and the real provider's generation settings

```text
TUR DEVELOPMENT REPORT

Task ID:          pax-issues-1 (issues 56, 58, 24, 40, 92)
Subsystem:        AI control layer: the read-only sandbox's grants, the
                  policy engine's built-in lists (services/tur-pax); the
                  manual workflow pax-live; the boot test
Build ID:         not applicable: no image of its own; the image and boot
                  workflows run on the pull request
Source revision:  pull request 110 (fix/pax-grants-lists-live); commits and
                  runs under "Runs"

SUMMARY
- What changed:
  1. (#56) A folder granted as a person writes it, /home/<name>/Documents,
     is accepted on Fedora Atomic, where /home is a link to var/home. Only
     the system's own links are followed (/home, /mnt, /opt, /srv, /root,
     /usr/local; root-owned, in a root-owned directory, with a target
     SystemLink::new accepts), only at the start of the path; the path is
     judged under the name a person writes, so both names of a folder get
     the same answer and /var/roothome is /root. The sandbox shows the
     folder at its real place and makes the same link, so a command can
     name it either way.
  2. (#58) The built-in lists held against the image (identifiers.md of
     image run 43479278136), each finding judged: KWallet's secret prompter
     and kdesu excluded; the desktop's scripting console and the login
     screen's settings sensitive; 22 unit prefixes, one file prefix and two
     packages added; the 16 prefixes that match nothing kept, with the
     reason; three duplicates removed, and a test refuses duplicates.
  3. (#24, #40, #92) pax-live sends two of its scenarios with Anthropic
     generation settings (effort; effort with stop sequences) and checks
     that a setting the family refuses (temperature) is refused before any
     request. Its caps are unchanged.
- Why: the issues named; #56 was a real failure on the booted image.

FILES
- Added: tests/pax/scenarios/20-grant-through-system-link.json, this report
- Modified: services/tur-pax/src/{sandbox,policy,action}.rs,
  tests/pax/{e2e,live}.py, tests/boot/guest/.../component-checks,
  tests/boot/report.py, docs/architecture/{pax-orchestrator,agent-policy}.md
- Removed: nothing

BUILD
- Command: CI only (rust, pax, image, boot workflows); nothing was compiled
  on the development machine
- Result: see "Runs"
- Warnings: see "Runs"

TESTS
- Unit tests: sandbox.rs (four new: same answer under either name; links
  that could widen a grant are not taken; the sandbox's link; admission
  through a link on a real file system), policy.rs
  (what_the_image_showed_is_covered), action.rs (image units covered; no
  duplicate; drop-in directories)
- Integration tests: pax scenario grant-through-system-link (in the pax
  workflow, where /home is not a link, and on the booted image, where it is)
- Boot tests: components.pax_grant_through_home_link (now required),
  components.pax_grant_through_home_link_still_refuses (new)
- Hardware tests: NOT RUN (none needed)
- Performance tests: NOT RUN (none needed)
- Real provider: pax-live against Anthropic (see "Runs"); DeepSeek NOT RUN
  (no DEEPSEEK_KEY)

DIAGNOSTICS
- Log paths: boot workflow artifact components/pax-grant-*.out/.err and
  components/pax/grant-through-system-link.*; pax-live artifact
  pax-live-evidence
- Diagnostic bundle path: not applicable
- Relevant error IDs: TUR-AI-SANDBOX-002

RECOVERY
- Rollback procedure: revert the merge. Grants named through /home are then
  refused again on the image; nothing is stored.
- Recovery tested: not applicable

KNOWN LIMITATIONS
- Removable media (/run/media, and /media, a link to it on Fedora Atomic)
  still cannot be granted: /media is not among the system's links.
- The check-then-start window on a granted folder is unchanged (#40).
- The fourteen unverified applications are not on the image and stay
  unverified.

NEXT SAFE ACTION
- Merge; then the DeepSeek run once DEEPSEEK_KEY exists.
```

## Runs

First head, commit `66cb502`:

| Workflow | Run | Result |
|---|---|---|
| image | 43495903731 | pass |
| boot (the installed system, `/home → var/home`: scenario `grant-through-system-link`, checks `pax_grant_through_home_link`, `pax_grant_through_home_link_still_refuses`) | 43495904098 | pass |
| pax | 43495903747 | pass |
| iso, desktop, rollback | 43495907183, 43495903880, 43495903777 | pass |
| rust (services/tur-pax) | 43495903886 | failed at "Fail if fixes were needed": rustfmt only; the patch is applied in the next commit |
| pax-live (Anthropic, claude-haiku-5-5) | 43501213798 | failed in the test harness: live.py lost the line that sets the request budget (`requests_left`). The local temperature check passed; no scenario reached the provider apart from the workflow's one-line key check. Fixed in the next commit |

Second head (rustfmt applied, live.py fixed): the runs of the head commit, and
pax-live dispatched once more on it with the same caps, are listed in the pull
request.

## The issues

**#56** — fixed as described; scenario on the booted image:
`grant-through-system-link` in the P@x end-to-end test (run by the boot
workflow on the installed system, `/home → var/home`), and the boot checks
`pax_grant_through_home_link` (required now) and
`pax_grant_through_home_link_still_refuses`. `/mnt`, `/opt`, `/srv`, `/root`
and `/usr/local` were checked: the first three are grantable under their
names, `/root` and `/usr/local` stay refused under either name. `/media` was
left out (removable media: a decision of its own).

**#58** — every item judged; the table is in agent-policy.md §2.5 ("On the TUR
image").

**#24** — "Test against real providers": Anthropic, by pax-live (manual,
capped, never on pull requests). OpenAI, Google: no key, NOT RUN. The
unverified wire details of pax-providers.md were not targeted by these
scenarios.

**#40** — "Run against a real provider": the three scenarios ran against
claude-haiku-5-5 (run 43299605588 before this change, and the run below);
nothing above class 1 was decided or performed; the key appeared nowhere.

**#92** — "Verify the generation-setting mappings against the real
services": Anthropic's effort (`output_config.effort`) and stop sequences
(`stop_sequences`) accepted by the real API (run below); temperature refused
locally as the mapping says. DeepSeek: NOT RUN (DEEPSEEK_KEY does not exist).
