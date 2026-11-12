# ADR-0003: Licence — PolyForm Noncommercial 1.0.0

- **Status:** Accepted
- **Date:** 2026-11-12
- **Specification sections:** §17.1, §17.2

## Context

The project owner requires that TUR be available to private individuals and not
usable by businesses. The repository is public, so the source is visible to
everyone; the licence decides what they may do with it.

TUR is mostly an integration of third-party projects, many under copyleft licences
(GPL and LGPL). Those licences cannot be restricted by TUR, and code that forms a
single work with GPL code cannot carry a noncommercial restriction.

## Options considered

1. **PolyForm Noncommercial 1.0.0.** A standard, professionally drafted
   source-available licence with an SPDX identifier
   (`PolyForm-Noncommercial-1.0.0`). Permits personal and other noncommercial use,
   modification and redistribution; prohibits commercial use.
2. **A custom "personal use only" licence.** Could match the intent exactly, but
   untested wording is a legal risk and deters contributors.
3. **Creative Commons BY-NC-SA.** Well known, but Creative Commons advises against
   using its licences for software.
4. **An open-source licence (Apache-2.0, GPL-3.0).** Cannot prohibit business use.

## Decision

TUR's own code and documentation are licensed under the PolyForm Noncommercial
License 1.0.0.

## Consequences

- TUR is **source-available**, not open source in the Open Source Initiative's
  sense. It must be described that way.
- Business use is prohibited. Note one difference from "private users only": the
  licence also permits use by charities, educational institutions, public research
  bodies and government institutions. Narrowing that would require a custom
  licence (option 2).
- **Licence boundary.** The noncommercial terms apply only to code that is TUR's
  own separate work: standalone services, tools, image definitions and
  documentation that talk to other components over D-Bus, sockets or the command
  line. They do **not** and cannot apply to:
  - the third-party components shipped in the image, which keep their licences and
    remain usable commercially under them;
  - patches to an upstream project, which carry that project's licence;
  - code that links into a GPL work, such as a KWin or Plasma plugin or a kernel
    module, which must be licensed compatibly with it.

  Such code lives in clearly marked directories with its own licence file.
- A new TUR component should be designed as a separate process with a defined
  interface, not as a plugin to a copyleft project, unless there is a reason
  recorded in an ADR.
- Some distributions and package repositories do not accept noncommercial software.
- If commercial licences are ever to be sold separately, the project needs the
  right to relicense contributions. A contributor agreement would be required
  before accepting outside contributions on that basis. Not decided here.

## Verification

- Legal review before the first public release, as the specification requires
  (§17.2), covering the licence boundary above.
- An SPDX identifier on every source file, and a check in CI that code in an
  upstream-licensed directory is not marked noncommercial, and the reverse.
