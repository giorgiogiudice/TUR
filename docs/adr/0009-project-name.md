# ADR-0009: The project is TUR, The Unified Realm; the assistant is P@x

- **Status:** Accepted
- **Date:** 2026-11-12
- **Specification sections:** all (naming)

## Context

The working name "UOS" is also the name of UnionTech's commercial Linux
distribution. Sharing it invites confusion and a trademark dispute, and makes the
project hard to find.

The owner has chosen a new name and supplied a logo, and has named the AI
assistant.

## Decision

- The project is **TUR — The Unified Realm**.
- The AI assistant is **P@x**.
- The logo is `branding/tur-logo.png`.

## Consequences

One mechanical change renames everything, after the work in flight has merged, so
that no branch is renamed underneath its author:

| Was | Becomes |
|---|---|
| UOS, Universal Operating System | TUR, The Unified Realm |
| Repository `giorgiogiudice/UOS` | `giorgiogiudice/TUR` (GitHub redirects the old address) |
| Crates and programs `uos-*` (`uos-core`, `uos-diag`, `uos-runtime`, …) | `tur-*` |
| The assistant service `uos-agent` | `tur-pax` |
| Error codes `UOS-<FAMILY>-…` | `TUR-<FAMILY>-…` |
| D-Bus names `org.uos.*` | `org.tur.*` |
| Paths `/usr/lib/uos`, `/run/uos`, … | `/usr/lib/tur`, `/run/tur`, … |
| `IMAGE_ID=uos` in `os-release` | `IMAGE_ID=tur` |
| Journal fields `UOS_*` | `TUR_*` |

- In code, file names and identifiers the assistant is written `pax`, because `@`
  is not valid there. Its program is `tur-pax`, never `pax`: that name belongs to
  a standard archive utility.
- Earlier decision records keep their numbers and are renamed in place by the
  same mechanical change; their decisions are unchanged.
- The specification is renamed throughout but keeps its revision number; the
  rename is not a change of substance.

## Verification

- A trademark and name search for "TUR" and "The Unified Realm" before any public
  release. "TUR" is also the international three-letter code for Türkiye, which
  will affect how easily the project is found.
- `org.tur` is used as the D-Bus namespace provisionally. Convention is a reversed
  domain name the project controls; settle it when the project has a domain.
- After the rename, a check in CI that the old name no longer appears outside
  this record and the project history.
