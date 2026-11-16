<!-- SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0 -->
# fixtures/

What P@x's character is tested and photographed against, where it is not
the recorded answers of P@x's service (those are P@x's window's:
`desktop/apps/tur-pax-ui/fixtures/agent1`, recorded from the real service in
CI, and used here as they are).

| File | What it is | Where it comes from |
|---|---|---|
| `heal/notices.json` | Under `printed`: what `tur-diag heal notices --json` prints, an array of the notices a person has not seen (`id`, `at_utc`, `code`, `kind`, `title`, `body`, `actions`) | **Written by hand** from `services/tur-diagnostics/src/heal/engine.rs` (the struct `Notice`) and the texts of `services/tur-diagnostics/heal/catalogue.json`. Not recorded from a run |

`desktop/README.md` says that nobody writes a fixture by hand. This one is,
and says so in itself: the healing service runs as root in a booted guest
(the `heal` workflow), and recording its output from there is a follow-up
(docs/reports/desktop-8.md). Until then a change to the healing service's
texts or fields will not fail a test here; the fields are declared stable by
that component (fields may be added, never renamed).
