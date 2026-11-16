<!-- SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0 -->
# From a reported problem to the update that fixes it

For maintainers, and for agents working on TUR. A person sent a problem
report ([problem-reports.md](../architecture/problem-reports.md)); the
receiver filed a summary in the private reports repository. This is the way
to a fix that the person's computer recognises.

## What you have

Each issue in the reports repository is one **signature**: one error code,
one component, one TUR version. Its title is
`[CODE] component version: title`; its body has the report ID (`R-…`), the
kind, who prepared it (the person or P@x), the build, the kernel, and
excerpts that were redacted twice. Later reports of the same signature are
comments, each with its own report ID. The bundle itself is never in the
tracker.

The two things you work from are the **code** and the **report ID**:

- The code says which part of TUR noticed what. Look it up in the code's
  registry: `TUR-DIAG-*` in `services/tur-diagnostics/src/error.rs`, a
  fault of the healing catalogue in
  `services/tur-diagnostics/heal/catalogue.json` (with its check, its
  remedies and its sources), others in their component's registry
  (`git grep -n CODE`). A report without a code is titled `[none]`.
- The report ID opens the full bundle, on the receiver, for 90 days after
  it arrived. Only someone who administers the receiver can do that.

## 1. Read the bundle (only if the summary is not enough)

On the receiver's server, as root:

```sh
podman exec tur-report-receiver tur-report-receiver decrypt \
  --config /etc/tur-report-receiver/config.json \
  --report R-0123456789abcdef --output /var/lib/tur-report-receiver/R-0123456789abcdef.tar.zst
```

Copy it to your computer (`scp`), delete it from the server, and read it
with `tur-diag verify-bundle FILE`, then `tur-diag report show FILE` (the
whole report as it was sent) and `tur-diag report show --file PATH FILE`.
Personal data was replaced on the person's computer, but treat the bundle as
personal anyway: keep it on your own computer, never in the repository, an
issue, a pull request, a log or a chat with a model provider, and delete it
when the fix is merged. Never attach it to a public issue.

## 2. Reproduce

- **A fault of the healing catalogue**: the `heal` workflow injects each
  fault into a booted TUR (`tests/boot/heal.py`,
  `tests/boot/guest/rootfs/usr/libexec/tur-boot-test/heal-agent`). Add the
  failing case there, or a unit test with the fake machine in
  `services/tur-diagnostics/src/heal/tests.rs`.
- **A failed update or boot** (kind `failed-update`, `boot-problem`, with
  boot health's notice): the `rollback` and `update` workflows.
- **Anything else**: the component's own tests, built in CI. Nothing is
  built on a development machine (AGENTS.md); push a branch and let CI run
  the test that fails.

Write the test first, so that the fix is proven by CI, not by reading.

## 3. Fix, and say what it fixes

In the same pull request as the fix:

1. Add the code to `image/rootfs/usr/share/tur/heal/fixes.json`:

   ```json
   { "code": "TUR-AUDIO-RESUME-001",
     "summary": "The sound comes back after the computer wakes up.",
     "change": "giorgiogiudice/TUR#200" }
   ```

   The summary is shown to people: one plain sentence about what they will
   notice, no internal names. `scripts/check-fixes.py` (in the `reports`
   workflow) checks the form and that the code is one TUR knows. A report
   without a code cannot be listed here; give the fault a code in its
   component's registry if it can happen again, and mark the issue fixed
   (step 5) so that the person still hears of it.
2. Say in the pull request which reports issue it closes (the private
   repository's issue number, never a report's contents).
3. Keep the entry in later releases until no supported version predates the
   fix: the list is per image, and an update that skips a version still
   carries it.

## 4. Release

The fix ships in a signed update like any other change
([publishing.md](../architecture/publishing.md), ADR-0023). Nothing about
reports changes the release process.

## 5. Tell the reporters

When the release is published, on the receiver:

```sh
podman exec tur-report-receiver tur-report-receiver mark-fixed \
  --config /etc/tur-report-receiver/config.json --issue 12 --version 0.2.1
```

Every report filed under issue 12 (the issue and its comments) now reads
`fixed in 0.2.1` for `tur-diag report status`. Close the issue.

## 6. What the person sees

After their computer updates to the release:

- for a fault of the healing catalogue, the healing service checks the
  fault again and tells them "The problem you reported is fixed", or that it
  happened again and they can send a new report;
- for any other code, `tur-diag report fixed` lists the report with "The
  problem you reported is fixed: … is addressed by TUR 0.2.1";
- for a report without a code, `tur-diag report status R-…` says "fixed in
  0.2.1".

If it happened again, a new report arrives with the new version in its
signature: a new issue, which links to nothing old. Look for the old one by
code.

## For agents

- You may read the summaries in the reports repository if you were given
  access, and you may write tests and fixes. You may not decrypt bundles,
  copy report text into a public place, a commit, a pull request or a
  model request beyond what the task needs, or contact a reporter.
- Work from the code, the component, the version and the redacted
  excerpts. If they are not enough, say so in your report and ask the
  owner; do not ask for the bundle to be pasted.
