# Architecture Decision Records

An ADR records one decision: the context, the options considered, what was decided
and what follows from it. ADRs are required for any change that affects an
interface, boot, storage, security or update behaviour, and for any deviation from
the [specification](../architecture/specification.md).

## Process

1. Copy [`0000-template.md`](0000-template.md) to the next free number.
2. Open it as a pull request with status **Proposed**.
3. On merge the status becomes **Accepted**.
4. An ADR is never rewritten after acceptance. To change a decision, write a new
   ADR and mark the old one **Superseded by ADR-NNNN**.

## Index

| ADR | Title | Status |
|---|---|---|
| [0001](0001-base-distribution.md) | Base distribution: Fedora Kinoite on bootc | Accepted |
| [0002](0002-ci-and-build-infrastructure.md) | CI and build infrastructure: GitHub Actions first | Accepted |
| [0003](0003-licence.md) | Licence: PolyForm Noncommercial 1.0.0 | Accepted |
| [0004](0004-ai-provider-model.md) | AI layer: optional, provider-agnostic, user-supplied key | Accepted |
| [0005](0005-languages-and-workspace.md) | Implementation languages and workspace layout | Accepted |
| [0006](0006-android-runtime.md) | Android runtime: Waydroid as an opt-in OS container | Proposed |

## Open questions from the specification

[Section 23](../architecture/specification.md#23-open-questions-to-resolve-through-adrs)
lists the questions still to be settled. ADR-0001 answers questions 2, 3 and 4.
Question 1 (bootloader) is to be answered with evidence from the first image build.
