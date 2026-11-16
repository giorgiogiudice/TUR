<!-- SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0 -->
# reports-e2e-1 — "Report this problem", end to end

The owner's request (16 November 2026): *"The agent can report issues on
request; make sure this mechanism really works so it's easier to ship
updates and fixes."*

**Nothing in this report has run.** GitHub Actions is disabled for the
owner's account (support ticket open), so no workflow ran on this branch:
not the `rust` or `reports` workflows that compile and test the Rust code,
not the new `reports-deployed` workflow. Nothing was compiled, built or
started on the development machine (AGENTS.md). The Rust code was written
without a compiler; the first CI run will find what review did not. The
only things executed locally were Python's syntax check of the two new test
scripts, `bash -n` on `install.sh`, and `scripts/check-fixes.py` (a reader
of one JSON file) against the shipped list and a list with planted errors.

```text
TUR DEVELOPMENT REPORT

Task ID:          reports-e2e-1 (ADR-0024; issue 75; follows the problem
                  reports slice of pull request 70 and self-healing, 108)
Subsystem:        tur-diagnostics (tur-diag report; P@x tools),
                  server/report-receiver (deploy, CI), schemas, docs
Build ID:         none: nothing was built (GitHub Actions disabled)
Source revision:  feat/reports-end-to-end, from main at d686e7e

SUMMARY
- What changed:
  1. The person's routes. `tur-diag report submit`: one command that
     prepares a report, shows all of it, and sends it only after "yes" is
     typed on a terminal (it has no --yes); `--from-heal CODE` makes it from
     a fault the healing service offers a report for. `report show` prints
     a saved report exactly as it would be sent, or one of its files.
     `--include-pax-activity` adds a summary of P@x's audit trail (when,
     which task and action, class, decision, outcome, codes; never what was
     said, shown or run).
  2. P@x never sends. The tool `diag_report_send` is declared with class 2
     and has no adapter: never offered, never performed. P@x's preview
     carries a hand-over: the person's `submit` command with the report's
     redacted fields. `send` and `submit` refuse to run in P@x's read-only
     sandbox (TUR-DIAG-REPORT-013), a second barrier behind its missing
     network. P@x's own policy engine is held to all of it by a new test.
  3. The receiver installs with one command: deploy/install.sh (packages,
     SSH keys only, firewall, image checked against its .sha256, key,
     token asked on the terminal or read from a file, configuration,
     systemd unit, Caddy with Let's Encrypt or the server's own
     certificate, local and HTTPS checks; safe to run again; an update is
     the same command with a new image).
  4. The loop back: when a report is sent, its origin (code, signature,
     version, build) is kept beside its record; `tur-diag report fixed`
     says "The problem you reported is fixed" for every sent report whose
     code the running image's fixes.json lists and that was made on
     another build, and for every report the receiver marked fixed.
     scripts/check-fixes.py checks fixes.json in the reports workflow.
  5. A dispatch-only workflow, reports-deployed, that runs the whole loop
     on one runner: install.sh, TUR's pinned base image as the client, P@x's
     preview, the person's "no" and "yes" on a terminal, a send from P@x's
     sandbox refused, redaction on both sides, the filed summary against a
     stand-in for GitHub, mark-fixed, an update that lists the code, two
     reports near the limits at once with memory and CPU measured, the
     logs, and installing again.
  6. A maintainer's guide from a filed report to the update that fixes it;
     the deployment guide rewritten around the one command, with the
     owner's checklist and costs.
- Why it changed: the mechanism existed in parts (ADR-0024, ADR-0027) but
  could not be used end to end: P@x could not hand a report to the person,
  the person had two commands and a path to copy, the receiver needed a
  page of manual steps, and a fix reached the person only for faults of the
  healing catalogue.

FILES
- Added: services/tur-diagnostics/src/report/handoff.rs, activity.rs,
  fixed.rs; src/report_cli.rs; tests/report_handoff.rs;
  schemas/report-origin.schema.json, report-fixed.schema.json;
  server/report-receiver/deploy/install.sh; server/report-receiver/ci/
  deployed_e2e.py, answer.py; scripts/check-fixes.py;
  .github/workflows/reports-deployed.yml;
  docs/guides/fix-a-reported-problem.md; this report
- Modified: services/tur-diagnostics: report.rs (prepare_with, the
  hand-over in the preview, preview_of_archive, member), main.rs (submit,
  show, fixed; the report's fields shared; send keeps the origin and
  refuses in P@x's sandbox), error.rs (TUR-DIAG-REPORT-013),
  heal_cli.rs (report_request visible to the crate: one word),
  agent-tools.json (diag_report_send; prepare's description),
  tests/agent_tools.rs, src/report/tests.rs;
  schemas/report-preview.schema.json (handoff);
  server/report-receiver: deploy/Caddyfile (a line install.sh fills),
  examples/planted_bundle.rs (large.tar.zst, for the load measurement);
  .github/workflows/reports.yml (job `fixes`: fixes.json and shellcheck;
  paths); docs/architecture/problem-reports.md (sections 2, 2.2, 2.3 new,
  2.4 new, 9, 10), diagnostics.md (one code); docs/guides/
  deploy-report-receiver.md (rewritten)
- Not touched: gate.yml, docs/adr/README.md, the image (no
  problem-reports.conf is installed), services/tur-pax, the receiver's
  program, self-healing.md and the heal engine (pull request 117's)
- Removed: none

BUILD
- Command: CI only.
- Result: NOT RUN (GitHub Actions disabled).
- Warnings: unknown. rustfmt has not seen the new code: the first `rust`
  or `reports` run may report formatting, which `scripts/dev/ci.py fixes`
  applies.

TESTS
- Unit tests: written, NOT RUN. tur-diagnostics: the hand-over (only P@x's
  previews get one; its argv; shell quoting; the sandbox host name), P@x's
  activity summary (fields kept and dropped, the newest 200, redacted in a
  real bundle), origins and `report fixed` (by update, by the receiver,
  not on the same build, catalogue codes marked), show (reads back exactly
  what prepare showed), schemas against the Rust types (preview with
  handoff, origin, fixed), the command line (submit has no --yes; the
  hand-over parses as submit), the new error code.
- Policy tests (tests/report_handoff.rs, P@x's engine): NOT RUN. The send
  tool is class 2, unavailable, not offered, not proposable; never allowed
  in any of 48 contexts (3 modes x attended/unattended x locked x model
  flag x trusted session), refused in every one without a trusted
  session; pressing Send in Settings or a notification, or typing in a
  terminal, by accessibility or screen: class 2, refused; a read-only
  command naming `report send` is class 0 and its sandbox has no network
  and the host name `tur-diag` refuses.
- Integration tests: reports-deployed workflow, NOT RUN (dispatch-only, and
  Actions disabled). The existing reports workflow (receiver tests and the
  container end to end) is unchanged except for the new `fixes` job: NOT
  RUN on this branch.
- Boot tests: none needed: nothing in the image, boot or update path
  changed.
- Hardware tests: NOT RUN (none apply).
- Performance tests: NOT RUN. The deployed workflow measures the
  receiver's peak memory (cgroup memory.peak) and CPU time for two 56 MiB
  reports at once; the numbers in the deployment guide are estimates from
  the design until it runs.

DIAGNOSTICS
- Artifacts: reports-deployed (results.json, summary.md with the
  measurements, install logs, the journal); none exist yet.
- Relevant error IDs: TUR-DIAG-REPORT-013 (new); TUR-DIAG-REPORT-002, -004;
  TUR-DIAG-RECEIVER-010, -011.

RECOVERY
- Rollback procedure: revert the pull request. Nothing in the image
  changes. On a computer, the new files are `origins/` beside `sent/` in
  the person's reports directory; an older tur-diag ignores them and still
  reads every record (the record format is unchanged on purpose). On the
  server: `systemctl disable --now tur-report-receiver caddy`; the files are
  in /etc/tur-report-receiver and /var/lib/tur-report-receiver.
- Recovery tested: NOT RUN (the deployed workflow runs install.sh twice
  and checks that key, token and reports are kept).

KNOWN LIMITATIONS
- See "Not done" below.

NEXT SAFE ACTION
- When Actions is back: run rust, reports and reports-deployed on this
  branch, apply CI's formatting fixes, fix what fails, then review.
- The owner: the checklist below.
```

## How a person asks, and who sends

| Route | Today |
|---|---|
| Command line | `tur-diag report submit`: shows everything, sends after "yes" |
| P@x ("report this", "send this to TUR") | P@x previews (class 0) and hands over the `submit` command; the person runs it |
| A healing notice (notification, companion's bubble) | the notice shows the command as text; the person runs it (`submit --from-heal CODE` is the one-step form; the notice still names `heal report`, see "Not done") |
| Settings | not built; the design needs no new rule (problem-reports.md, 2.3) |

Why P@x cannot send, on KWin as it is (no surface ignores emulated input):
no tool sends; its commands have no network, no session bus, and a host
name `send` refuses; a "Send" control or a terminal is class 2 and refused
until the approval sheet (D-18) exists, after which it would be the
person's approval on that sheet. The companion's `Announce` carries an
identifier only and the companion runs nothing; a Send button in its bubble
is not proposed.

## The owner's checklist

Do none of this before Actions is back and `reports-deployed` and
`reports` are green on this branch, and the privacy statement
(problem-reports.md, section 9) has been reviewed. Nothing here was created
or changed by this work.

| # | What | Cost | Where it goes |
|---|---|---|---|
| 1 | A Linode **Nanode 1 GB** (1 shared CPU, 25 GB), Debian 13 or Ubuntu 24.04, your SSH key, **no** Linode Backups | about US$5 a month (check the price when ordering) | Enough by design: idle a few tens of MiB; worst case about 150–200 MiB per upload, at most two at once, capped at 768 MiB; 120 uploads an hour in all. The workflow measures it. |
| 2 | A **subdomain**, e.g. `reports.<your-domain>`: `A` (and `AAAA`) to the server; behind Cloudflare **DNS only** | nothing with an existing domain; about US$10–15 a year for a new one | DNS |
| 3 | A **private repository**, e.g. `tur-problem-reports`, with the label `problem-report` | nothing (GitHub Free) | GitHub |
| 4 | A **fine-grained token**: only that repository, **Issues: Read and write**, nothing else, expiring in at most 366 days | nothing | Pasted once into `install.sh` on the server; it lives only in `/etc/tur-report-receiver/github-token` (0400, uid 10001). Never in the repository, a CI secret, a chat or a log. |
| 5 | Run `install.sh` on the server (docs/guides/deploy-report-receiver.md, step 5) with the image of a green `reports` run on main | | |
| 6 | Then a change to the image installs `/usr/lib/tur/problem-reports.conf` with `RECEIVER_URL=https://reports.<your-domain>` (not part of this work) | | |

Total: about US$5 a month, plus a domain if there is none.

## Proposed decision record

ADR-0024 stands; this records how its consent rule is met by every route.
Text for an ADR (number to be given by the owner; docs/adr/README.md not
edited):

> **ADR-00xx: Only the person sends a problem report; P@x hands it over**
>
> *Status:* Proposed. *Related:* ADR-0024, ADR-0027, ADR-0012;
> ai-control-layer.md sections 5, 17, 19, 23.
>
> *Context.* ADR-0024 makes sending a report class 2 when P@x proposes it
> and requires consent every time. P@x performs nothing above class 1 until
> the approval sheet exists, and on KWin no desktop surface ignores
> emulated input. The owner wants reports to be easy to send on request,
> from P@x, notices, Settings and the command line, without P@x ever
> sending one.
>
> *Decision.* (1) Sending is never a P@x action: the tool is declared, with
> class 2, and has no adapter, so it is never offered or performed; a test
> holds P@x's policy engine to this in every context. (2) P@x's preview
> carries a hand-over: the person's own command, which shows the whole
> report again and sends only after the person types yes. (3) `tur-diag`
> refuses to send from P@x's sandbox, behind the sandbox's missing network.
> (4) Every other route ends in the same `send`, run by the person:
> `submit` on a terminal, a Settings page's Send (a TUR program the person
> runs; P@x pressing it is class 2). (5) No one-press Send in a
> notification or the companion until the approval sheet exists and has
> passed the trusted-path tests. (6) A sent report's origin is kept beside
> its record, not in it, so the record format stays readable by older
> versions after a rollback; with the image's list of fixed codes it tells
> the person that their problem is fixed.
>
> *Consequences.* P@x is useful for reports (it explains and prepares)
> without holding the consent. The person runs one command; until
> Settings has a page, that command is the interface. A report without a
> code is known as fixed only through the receiver.

## Not done

- **Nothing has run** (see the top). In particular `install.sh` has never
  been executed, and the reports-deployed workflow has never run.
- **Desktop delivery of "fixed" for reports outside the healing
  catalogue.** `tur-diag report fixed` is the interface; `tur-diag heal
  notify`, which pull request 117 rewrites, should show its entries that
  are not `rechecked_by_healing`, once each. Left to that pull request's
  component to avoid a conflict.
- **The healing service's report notice** names `tur-diag heal report
  CODE` (two steps with `send`); it should name `tur-diag report submit
  --from-heal CODE`. The same component; not changed here.
- **Settings page** for reports, and a button for the hand-over in P@x's
  window (tur-pax-ui): not built.
- **The image's `problem-reports.conf`**: not installed; nothing can be
  sent from TUR until the receiver exists.
- **Issue 75** stays open: the owner's steps above.
- **reports-1**: problem-reports.md linked a report of the first slice
  that was never committed; the link now points here.
- **ADR-0024 item 6** (a failed update proposed at the next start) and
  item 5 (public issues without personal data) are unchanged.

## Follow-ups

1. When Actions is back: run and fix (rust, reports, reports-deployed).
2. #117's component: add `report fixed` entries to `heal notify`; change the
   report notice's command to `report submit --from-heal`.
3. A Settings page "Problem reports" (list, show, send, status, fixed).
4. The approval sheet (D-18), then reconsider a one-press Send.
5. `tur-update` showing which of the person's reports an update fixes.
6. An update test in which the second image lists a fixed code (heal-1's
   open point), now for `report fixed` too.
