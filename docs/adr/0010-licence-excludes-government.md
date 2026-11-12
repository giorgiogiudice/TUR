# ADR-0010: Licence — noncommercial, and not for government use

- **Status:** Accepted
- **Date:** 2026-11-12
- **Supersedes:** the choice of licence text in [ADR-0003](0003-licence.md). The
  licence boundary defined there still applies unchanged.

## Context

ADR-0003 chose the PolyForm Noncommercial License 1.0.0 and noted that it permits
use by government institutions as well as by charities and schools. The owner has
decided that government use is not to be permitted; charities and schools are.

No standard, widely recognised licence expresses "noncommercial, and not
government". The PolyForm licences may be adapted, on condition that an adapted
text removes every mention of PolyForm.

## Options considered

1. **Keep the standard licence** and accept government use. Rejected by the owner.
2. **Adapt the standard text** with the smallest change that expresses the
   decision. Keeps professionally drafted wording for everything else.
3. **Commission a new licence** from a lawyer. The right end state, but it blocks
   work now.

## Decision

Option 2 now, reviewed under option 3 before any public release.

The project's licence is the **TUR Noncommercial License 1.0.0**: the standard
noncommercial text with these changes only:

- Use by a charitable organisation or an educational institution remains
  permitted, whatever its funding.
- Use by or on behalf of a government institution is not a permitted purpose. An
  educational institution is not treated as a government institution for this
  purpose, even where it is publicly run.
- All mention of the source licence's name and website is removed, as its terms
  require.

Its identifier in source files is `LicenseRef-TUR-Noncommercial-1.0.0`.

## Consequences

- The licence is now a custom one. It has no SPDX list entry, licence scanners
  will not recognise it, and anyone evaluating the project must read it. This
  narrows who will use or contribute to the project.
- "Government institution" is not sharply defined, and bodies such as public
  hospitals, public research institutes and state broadcasters fall on the line.
  The licence text takes the simple position above; a lawyer should decide
  whether it holds in the jurisdictions that matter.
- A restriction on a class of user is unusual and may be unenforceable or
  unlawful in some jurisdictions. That is a question for legal review.
- Everything in ADR-0003 about the boundary still holds. The third-party
  components shipped in the image keep their own licences and remain usable by
  anyone, including governments and businesses, under those licences. Patches to
  an upstream project and code linking into a GPL work carry the upstream
  licence, not this one.
- The licence change is applied together with the rename
  ([ADR-0009](0009-project-name.md)), in one change to the licence file and to
  the identifier at the top of every source file.

## Verification

- Legal review of the adapted text before the first public release, covering the
  definition of a government institution, enforceability of the exclusion, and
  the boundary with copyleft components.
- A check in CI that every source file carries the new identifier.
