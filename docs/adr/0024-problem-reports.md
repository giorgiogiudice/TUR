# ADR-0024: Problem reports — prepared by P@x, sent only with consent, to a private receiver

- **Status:** Accepted
- **Date:** 2026-11-12
- **Related:** specification §29 (diagnostics), `AGENTS.md` ("Never collect or
  upload diagnostics silently"), [ADR-0023](0023-publishing-signed-updates.md)

## Context

The owner wants P@x to report problems so that updates can fix them at the
operating-system level. TUR can already collect a diagnostic bundle locally
(`tur-diag`) and boot health records failed updates, but nothing sends a
report and nothing receives one. Bundles can contain personal details, and the
repository's issues are public.

## Decision

1. **P@x prepares reports**: it notices a problem (a failed update, a crash, a
   repair it could not finish, a person's request), collects the relevant
   evidence with `tur-diag`, removes personal data, and writes a clear summary.
2. **Sending needs consent.** By default P@x asks "Send this report?" and shows
   exactly what is in it. The person may switch on "send problem reports
   automatically" in Settings: automatic, never silent; each sent report is
   listed in P@x's activity and in Settings, and the switch is off by default.
   Sending is an action of class 2 when P@x proposes it.
3. **Reports go to TUR's own receiver**, a small service whose code lives in
   this repository and is tested in CI. It accepts only well-formed TUR
   bundles within a size limit, rate-limits by address, removes personal data
   again, stores bundles encrypted for a fixed period (90 days) and then
   deletes them, and files a summary — never the raw bundle — as an issue in a
   **private** GitHub repository, with a token that can write only there.
4. **It runs on a server of its own**, nothing else on it (the owner proposed
   a small Linode "Nanode"); HTTPS only, SSH restricted, automatic security
   updates. It is deployed only after the receiver and the client side are
   green in CI, and only with the owner's go-ahead.
5. **Triage**: duplicates are grouped; a fix is an ordinary pull request; a
   public issue without personal data is opened where useful; the person who
   reported can see the state of their report.
6. **A failed update reports itself the same way**: boot health's record of a
   restored version becomes a report proposal on the next start.

## Consequences

- Reports never reach the public issue tracker with personal data.
- Without consent nothing leaves the computer; the receiver being down loses
  nothing (reports wait and are retried).
- Running a public receiver means handling abuse and personal data
  responsibly; the retention, the stripping and the access rules are part of
  its tests, and its privacy statement is reviewed with the lawyer.
