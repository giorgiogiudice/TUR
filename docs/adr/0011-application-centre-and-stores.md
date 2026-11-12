# ADR-0011: One application centre, integrating every store

- **Status:** Accepted
- **Date:** 2026-11-12
- **Specification sections:** §9.1; [ADR-0007](0007-app-centric-routing.md)

## Context

ADR-0007 makes the application, not the executable, the unit the user deals with.
The owner has added that the stores people already use should be integrated,
Google Play among them, "as well as any other".

A store is where most people find software. If each ecosystem keeps its own store
window with its own account, search and update mechanism, TUR is four systems
side by side, not one.

## Decision

TUR has **one application centre**. It searches every integrated source at once,
shows which route an application will run through and how well it is known to
work, installs it, and keeps it updated.

| Ecosystem | Sources to integrate |
|---|---|
| Linux | Flathub and other Flatpak remotes; the base system's packages; containers of other distributions for their packages |
| Windows games | Steam; Epic, GOG and Amazon through an open launcher; direct installers |
| Windows applications | Direct installers; a package manager inside the Windows guest where the guest route is used |
| Android | An open-source store by default; **Google Play as an opt-in**; direct APK installation |
| Web | Installable web applications, for services that have no better edition |

Rules:

1. **TUR integrates stores; it does not redistribute them.** A proprietary store
   client or service is never part of the image. Where a store is proprietary,
   the application centre offers it as a clearly labelled opt-in: the user
   chooses it, accepts that provider's terms, signs in with their own account,
   and the component is fetched from its publisher at that moment.
2. **Google Play is offered this way**, as a guided, one-step opt-in in the
   application centre and through P@x. It needs the Android image that includes
   Google's services, and registration of the device with Google by the user.
   Its limits are stated at the point of opting in: applications that require
   Google's certified-device check will still refuse to run.
3. **One identity for an application.** If an application is available from
   several sources, the application centre shows one entry and recommends a
   source by the routing ladder of ADR-0007. The user can pick another.
4. **Accounts and purchases stay with their store.** TUR does not proxy payments
   or hold store credentials outside the system keyring.
5. **A source that cannot be integrated lawfully is not integrated.** The
   Microsoft Store and Apple's App Store are in this category.

## Consequences

- The application centre is a main component of the desktop, alongside the
  catalogue and router of ADR-0007. It replaces the specification's plan to adapt
  an existing software centre.
- Each integrated store is an adapter with its own maintenance cost and its own
  terms of use to respect. They are added one at a time, each with a stated
  licence and terms review.
- P@x can install an application from any integrated source on request.
  Installing is a class 2 action; anything involving payment always is.
- The Android design (addendum B) gains Google Play as a supported opt-in, not
  only a possibility the user must arrange alone.

## Verification

- For each store, before its adapter is written: the terms under which a
  third-party client may search, install and update; whether an official
  interface exists; what the user must agree to.
- For Google Play specifically: the current terms for using Google's services on
  an uncertified device, and the current registration procedure.
