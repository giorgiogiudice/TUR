# ADR-0019: Installing applications: one backend, two routes, pinned downloads through curl

- **Status:** Accepted (12 November 2026)
- **Date:** 2026-11-12
- **Specification sections:** §7.2, §7.4, §15.2, §29.3; [ADR-0007](0007-app-centric-routing.md), [ADR-0011](0011-application-centre-and-stores.md)

## Context

The catalogue and router (ADR-0007) say how an application would run; the
runtime manager installs a Windows application from a file it is given; the
application centre's Install button is disabled. Nothing connected the
router's answer to an installation, nothing installed a Linux application,
and nothing downloaded anything. Three decisions of weight are needed to
change that: which component installs, what TUR may download and how it
checks it, and how a download reaches the network.

## Decision

1. **One program installs: `tur-apps`**, a standalone service-to-be with a
   command line now and a proposed session-bus interface `org.tur.Apps1`. It
   links the router as a library and starts from its answer; it installs by
   two routes only, **Flatpak per user** through the `flatpak` command line,
   and **Windows through `tur-runtime app install`**, used as it is. Every
   other route is refused with a stable code. Installing is kept apart from
   the read-only catalogue service the desktop design names
   (`org.tur.Catalogue1`) because it is a different authority: it changes
   the computer (class 2 for the assistant).

2. **TUR downloads an installer only when the catalogue pins it, and only for
   free software.** A Windows edition may carry `[editions.installer]`: the
   publisher's own URL, size, SHA-256, version, licence and where each was
   checked. The validator accepts it only on an edition obtained from the
   publisher's page and marked `open-source`. The file is fetched when the
   user asks for the application, written beside its name, checked for size
   and digest, and only then used; a file that does not match is deleted and
   nothing in it runs. Proprietary or paid applications only ever take the
   user's own file, and a user's file for an application with a pin must be
   the pinned file. This amends the catalogue's earlier rule that an entry
   never names a file to download: `download_page` still never does; the
   pin is the single, checked exception.

3. **The network is reached through the system's `curl`, from one module.**
   Not an HTTP client linked into the program. `curl` is in the Fedora 45
   Kinoite base (8.21.0, with the full libcurl), so it costs nothing in the
   image; its TLS is the system's OpenSSL with the system's certificate
   store and crypto policy, which the rest of the system uses and Fedora
   updates; and the program carries no TLS code of its own to audit or keep
   current. The cost is a process per download and parsing its output; for
   a download measured in megabytes that is nothing. Every run is `-q`
   first (no configuration file), `https` only on every request and
   redirect, at most ten redirects, `--fail`, `--max-filesize` at the pinned
   size, time limits, and the URL after `--`. The alternative weighed was
   `ureq` with `rustls`, as `tur-pax-providers` uses for streaming model
   APIs (MIT OR Apache-2.0, pure Rust, its own web PKI roots): reasonable,
   but it would bring a second TLS stack and certificate store into a
   component that only fetches a few files, and its roots would follow the
   crate's releases rather than the system's.

4. **Removal keeps the user's data unless told otherwise.** For a Windows
   application the private home and the Windows profiles are copied aside
   before `tur-runtime` deletes them; if the copy fails nothing is removed.
   Over D-Bus there is no default: the caller must pass the user's answer.

5. **Flathub should be on for every user by default**, as the design's
   Sources view shows, added per user because a system remote does not
   serve per-user installations. Fedora 45 Kinoite adds only Fedora's own
   registry, system-wide, and Flathub only after the user opts into
   third-party repositories. Turning Flathub on at first login belongs to
   the image or the first-run setup and is not done by this change.

## Consequences

- The application centre can install (task D-11) by running `tur-apps` with
  `--json` and `--events`, before the D-Bus service exists.
- Every pinned installer is a fact to maintain: a new release means a new
  pin, checked by a person against the publisher's digest. An outdated pin
  still installs the version it names; the update check reports the new pin
  as an update.
- The plan, the events and the record are interfaces (three new schemas);
  they are proposed, not frozen, until the service is built.
- Flatpak's command line gives no progress in numbers; the centre shows
  activity for a Flatpak and a bar only for a pinned download.
- `tur-runtime` keeps its own removal; `tur-apps` adds the step before it.
  Calling `tur-runtime app uninstall` directly still deletes documents
  without asking (issue #32).

## Verification

- The apps workflow installs a real Flatpak from Flathub per user and a real
  Windows application from its pinned download, and removes both, checking
  that nothing is left: [apps-1](../reports/apps-1.md).
- To verify before Flathub is turned on by default: Flathub's terms of use
  for a client that installs from it on the user's request.
