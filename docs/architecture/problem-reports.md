<!-- SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0 -->
# Problem reports

As built for [ADR-0024](../adr/0024-problem-reports.md). Specification §6.5
(upload is a separate explicit action), §29 (bundles and codes);
[diagnostics.md](diagnostics.md) (the bundle and the client's redaction);
[ai-control-layer.md](ai-control-layer.md) §5, §22, §23 (classes).

- Client: `services/tur-diagnostics/` — `tur-diag report prepare|send|list|status`,
  module `tur_diag::report`.
- Receiver: `server/report-receiver/` — program `tur-report-receiver`.
- Schemas: `schemas/report-request.schema.json` (report.json),
  `report-preview.schema.json`, `report-record.schema.json`,
  `report-receiver.schema.json`.
- Proof: `.github/workflows/reports.yml`, report [reports-1](../reports/reports-1.md).
- Deployment: [deploy-report-receiver.md](../guides/deploy-report-receiver.md).

## 1. The flow

```text
 TUR computer                                      receiver (own server)                private GitHub repo
 ------------                                      ---------------------                -------------------
 tur-diag report prepare   -> prepared bundle
   (person, or P@x preview)    shown in full
 tur-diag report send --to URL --yes  --HTTPS-->  POST /v1/reports
   (person only; foreground)                        verify (tur-diag's checks)
                                                    second redaction -> summary
                                                    seal bundle + summary (90 days)
                         <-- receipt: id, token --  202
                                                    filer thread -------------------->  issue (first of a signature)
                                                                                         comment (later ones)
 tur-diag report status ID  ---------------------> GET /v1/reports/<token>
                         <-- received|filed|fixed
```

Nothing is sent in the background, nothing is retried by the client on its
own, and nothing can be sent while no receiver is configured.

## 2. Client: `tur-diag report`

```sh
tur-diag report prepare (--title T --component C | --from-boot-health) [--description D | --description-file F]
                        [--error-code CODE] [--kind K] [--since-boot] [--prepared-by person|pax]
                        [--preview | --output FILE] [--json] [--state-dir DIR] [--config FILE]
tur-diag report send --to URL [--yes] [--json] FILE [--state-dir DIR] [--config FILE]
tur-diag report list [--json] [--state-dir DIR]
tur-diag report status REPORT_ID [--json] [--state-dir DIR] [--config FILE]
```

**prepare** collects a bundle with the collectors of `tur-diag collect`,
except that each journal is limited to its newest 5 000 entries and each
collector to 8 MiB, always with the `default` redaction profile, and adds
`report.json`. The report's own text is redacted together with the collected
files, with the same pseudonyms. It then checks the bundle exactly as a
receiver will (`inspect_archive`: at most 8 MiB sent, 64 MiB unpacked, verifies,
`default` profile, valid `report.json`) and prints what would be sent: the
whole `report.json`, every file with its size, what could not be collected,
how many identifiers were replaced per category, the receiver (or that none
is configured), and where the bundle was saved
(`$XDG_STATE_HOME/tur/reports/prepared/report-<time>-<sha>.tar.zst`, mode
0600, directories 0700). `--preview` saves nothing. `--json` prints
`report-preview.schema.json`.

**send** refuses, before anything leaves the computer, unless all of these
hold:

1. a receiver is configured (`TUR-DIAG-REPORT-002` otherwise);
2. `--to` names exactly that receiver (`TUR-DIAG-REPORT-003`), so that a
   suggested or mistyped address never receives a report;
3. the file is a prepared report a receiver accepts (`-005`, `-012`);
4. the reports directory can be written, so that a sent report is never
   missing from the list (`-010`);
5. the person confirmed: `--yes`, or `yes` typed at the question on a
   terminal (`-004`). Without a terminal, only `--yes` confirms.

It then shows a short description of what is sent and to whom, runs one
`curl` (the system's, so Fedora's trust store and crypto policy apply; only
`https`, or `http` to this computer for tests; no redirects; 300 s), checks
that the receipt's status address is on the configured receiver, writes
`sent/<report_id>.json` (`report-record.schema.json`, mode 0600) and removes
the prepared file if it lies in the reports directory. A failure leaves the
prepared file in place, to be sent again later by the person.

**list** shows sent reports (newest first) and prepared ones. It never shows
the status address, which holds the token, nor the directory, which names the
account. **status** asks the receiver for one report's state and updates the
record.

**Configuration.** `RECEIVER_URL=` in the first of
`/etc/tur/problem-reports.conf` and `/usr/lib/tur/problem-reports.conf` that
exists; an empty value, or no file, means no receiver. Nothing ships either
file yet (the image is out of this change's scope), so on TUR today nothing
can be sent.

**From boot health.** `--from-boot-health` runs `tur-boot-health status
--json` (or reads `--boot-health-status FILE`) and turns its pending notice
into a proposal: `fallback`, `not-applied` and `refused` become a
`failed-update` report on component `update`, with the notice's error code
and, where `rejected` lists the event, the version that failed as
`affected_version`; `unhealthy` becomes `boot-problem` on `boot`. Flags given
alongside override the proposal's title, description, code, component or
kind. ADR-0024 item 6 asks for the proposal "on the next start": the command
exists; showing it at the next start belongs to the desktop (Settings or
P@x's notice), which is not built yet.

### 2.1 report.json

One line of JSON, redacted like a journal line. Fields: `schema_version` (1),
`kind` (`failed-update`, `boot-problem`, `crash`, `repair-failed`,
`request`), `title` (1 to 120 characters, one line), `description` (up to
4 000), `error_code` (a TUR code or null), `component` (fixed list:
android, apps, audio, boot, catalogue, desktop, device, diag, display,
install, network, other, pax, runtime, settings, storage, update, windows),
`affected_version` (null or a version), `prepared_by` (`person` or `pax`),
`boot_health` (null, or `notice_kind`, `error_code`, `text`, `failed_check`,
`rejected_version`). The receiver accepts each text up to twice its limit,
because a pseudonym can be longer than what it replaced. Error messages
about a field name the field, never its value.

### 2.2 P@x

Two read-only tools are written, both class 0: `diag_report_prepare` runs
`tur-diag report prepare --preview --json --prepared-by=pax ...` (nothing is
saved, nothing is sent) and `diag_report_list` runs `tur-diag report list
--json`. **There is no tool that sends**; a test fails if an adapter names
`send`, `--yes` or `--output`. Sending a report P@x proposes is class 2, and
P@x performs nothing above class 1 until the approval sheet exists (§23,
item 1); the person sends with the command line, later in Settings.

They are in the shipped `services/tur-diagnostics/agent-tools.json` and
`agent-tools.adapter.json` (they were first in `agent-tools.next*.json`,
promoted in pax-5 when P@x's tests stopped listing every tool by name), and
are installed in the image with the rest of tur-diagnostics' tools. Inside P@x's read-only sandbox the home
directory is hidden, so `diag_report_list` there reports
`directory_found: false` until a D-Bus service (`org.tur.Diagnostics1`)
performs it.

## 3. Receiver API

All answers are `application/json`, `Cache-Control: no-store`
(`report-receiver.schema.json`).

| Request | Answer |
|---|---|
| `POST /v1/reports`, body = bundle, `Content-Type: application/vnd.tur.diag-bundle+zstd` | `202 {schema_version, report_id, status_token, status_url, state: "received", retention_days}` |
| `GET /v1/reports/<status_token>` | `200 {schema_version, state, fixed_in}`; state is `received`, `filed` or `fixed` |
| anything refused | `{schema_version, code, message, detail_code}` with the status below |

Order of checks for an upload: rate limits (per address, then global) before
the body is read; media type; declared length; at most
`max_concurrent_uploads` at once; body read with size and time limits;
`tur_diag::verify_members` with the unpacked limit (tur-diag's own
verification, linked as a library, so the two cannot disagree); profile
`default`; `report.json` read and validated by `tur_diag::report::read_report`.

| Code | HTTP | Meaning |
|---|---|---|
| `TUR-DIAG-RECEIVER-001` | 404 | No such path. |
| `TUR-DIAG-RECEIVER-002` | 405 | Method not accepted here. |
| `TUR-DIAG-RECEIVER-003` | 413 | Larger than accepted, sent (8 MiB) or unpacked (64 MiB; `detail_code` `TUR-DIAG-BUNDLE-009`). |
| `TUR-DIAG-RECEIVER-004` | 415 | Not `application/vnd.tur.diag-bundle+zstd`. |
| `TUR-DIAG-RECEIVER-005` | 422 | Not a bundle that verifies; `detail_code` is the `TUR-DIAG-BUNDLE-*` code. |
| `TUR-DIAG-RECEIVER-006` | 422 | No valid `report.json`. |
| `TUR-DIAG-RECEIVER-007` | 422 | Made with the technical profile. |
| `TUR-DIAG-RECEIVER-008` | 429 | Too many uploads from this address (default 6 an hour); `Retry-After`. |
| `TUR-DIAG-RECEIVER-009` | 429 | Too many uploads in all (default 120 an hour); `Retry-After`. |
| `TUR-DIAG-RECEIVER-010` | 503 | Too many uploads being verified at once. |
| `TUR-DIAG-RECEIVER-011` | 404 | No report has this status token (also for a malformed token). |
| `TUR-DIAG-RECEIVER-012` | 500 | The report could not be stored; nothing was kept. |
| `TUR-DIAG-RECEIVER-013` | 400 | Upload cut short or slower than 120 s. |
| `TUR-DIAG-RECEIVER-014` | 429 | Too many status requests from this address (default 120 an hour). |

The client address used for rate limits is the peer's, or, when the peer is
a trusted proxy (default `127.0.0.1`, `::1`), the last `X-Forwarded-For`
entry. IPv6 clients are counted per /64. Addresses are kept in memory only,
for at most an hour, and are never logged or stored.

## 4. Duplicates and filing

The signature is the first 16 hex digits of SHA-256 over
`tur-report-signature-1`, the error code (or `none`), the component and the
version (`affected_version`, else the bundle's TUR version, else `unknown`),
each on its own line (`tur_diag::report::signature`). The first report of a
signature opens an issue (title `[CODE] component version: title`, label
`problem-report`); later ones add a comment to it. A filer thread files due
reports oldest first every 30 s; a failure (network, any non-201 answer, a
token file that cannot be used) reschedules the report with a back-off
doubling from 60 s to 1 h and ends the round, and nothing is ever dropped. A
comment on an issue that was deleted (404/410) forgets the signature and
opens a new issue. Known limit: if GitHub creates an issue but the answer is
lost, the retry opens a second issue for that signature.

A maintainer marks a fix with `tur-report-receiver mark-fixed --issue N
--version V`; every report filed under that issue then reads `fixed` with
`fixed_in: V`.

## 5. The second redaction pass

`server/report-receiver/src/scrub.rs`, written independently of
`tur_diag::redact`. It first learns, from the whole bundle and the report's
texts, values recognisable only by their label, then scrubs each text that
goes into a summary with these rules, in this order, and finally replaces
every learnt value wherever it stands as a whole word, ignoring case:

| Rule | Replaced by |
|---|---|
| PEM private key blocks (also when cut short) | `<private-key>` |
| SSH public keys with their comment | `<ssh-key>` |
| Credentials in a URL or share (`scheme://user:pass@`, `//user:pass@`) | `<credentials>` |
| Known token formats: GitHub (`ghp_` …, `github_pat_`), Anthropic (`sk-ant-`), OpenAI (`sk-`), Google (`AIza`), AWS (`AKIA`/`ASIA`), Slack (`xox?-`), GitLab (`glpat-`), npm (`npm_`); JSON web tokens | `<token>` |
| `Authorization:` and `Proxy-Authorization:` to the end of the line; `Bearer`/`Basic` values | `<secret>` |
| The value after a key containing password, passwd, passphrase, psk, secret, token, api key, access key, private key, credential(s), session id or cookie (not systemd unit names) | `<secret>` |
| E-mail addresses (not systemd units such as `user@1000.service`) | `<email>` |
| `user@host` without a domain (not template units such as `getty@tty1`) | `<user>@<host>` |
| Home paths `/home/`, `/var/home/`, `/Users/`, `/run/media/`, `/media/` | the name: `<user>` |
| Windows profiles `C:\Users\`, `drive_c/users/` (Wine prefixes), except Public and Default | the name: `<user>` |
| User names after `USER=`, `LOGNAME=`, `user:`, `login=`, `"USER":"…"`, `uid=N(…)`, `for user`, `of user`, `Accepted/Failed … for [invalid user]` | `<user>` |
| Host names after `hostname`, `_HOSTNAME`, `Static/Transient hostname`, `nodename`, in `uname` output; names ending `.local`, `.lan`, `.home`, `.internal`, `.localdomain`, `.home.arpa`, `.fritz.box` | `<host>` |
| Wi-Fi names after `SSID=`/`ESSID:`, `SSID '…'`, NetworkManager's `access point '…'`, `connection '…'`, `network '…'` | `<wifi>` |
| Serial numbers after a serial label (`Serial Number:`, `ID_SERIAL(_SHORT)=`, `iSerial`, `product/board/chassis_serial`, `S/N`, `SN`) or `wwn`, if they contain a digit | `<serial>` |
| MAC addresses: 6 to 8 hex pairs joined by `:` or `-` (not all-zero or broadcast), `xxxx.xxxx.xxxx`, 12 hex digits after a MAC label | `<mac>` |
| IPv6 addresses, with zone, validated by parsing (not `::`, `::1`) | `<ipv6>` |
| IPv4 addresses, private ones included (not `127/8`, `0.0.0.0`, `255.255.255.255`, nor four numbers inside a longer dotted version) | `<ipv4>` |
| UUIDs, also systemd's `\x2d` spelling | `<uuid>` |
| 32 hex digits standing alone (machine ID, boot ID) | `<id>` |
| 13 to 19 digits passing the Luhn check | `<card>` |
| Runs of 32 or more of `A-Z a-z 0-9 + / _ = -` mixing upper case, lower case and digits (not paths) | `<token>` |
| Every learnt user, host, Wi-Fi name (4+ characters) and serial (6+, with a digit) | its tag |

Generic names (root, nobody, localhost, fedora, the tags themselves, …) are
never learnt. Client pseudonyms (`<user-1>`) are left alone. When a rule has
to guess, it removes. It is stricter than the client in places on purpose:
private IPv4 addresses, e-mail addresses, Windows profile names, unlabelled
tokens and card numbers.

**What goes into a summary** (`summary.rs`): checked fields (signature, error
code, component, version, kind, prepared by, architecture, build ID; anything
of the wrong form becomes `unknown`); scrubbed texts (title, description,
boot health notice, kernel release, up to 20 lines of failed units, the newest
15 journal messages of priority 3 or more urgent from each boot), each list
scrubbed as one text so that multi-line secrets are caught, then cut to 300
characters per line; counts. Every text from the report sits in a fenced code
block of four backticks (runs of three backticks inside are broken up), so
nothing is rendered: no mention notifies anyone, no image or link is loaded.
The title loses `@` and backticks. The raw bundle is never filed.

**Tests.** `src/scrub/tests.rs` has one test per rule, with what must stay
readable (unit names, versions, times, digests, D-Bus paths, kernel log
lines). The planted corpus (`src/testing.rs`, 47 values in 18 categories:
e-mail, home path, Windows profile, user name, host name, IPv4, IPv6, MAC,
serial, Wi-Fi, UUID, machine ID, token, secret, private key, SSH key, card,
random string) is put, unredacted, into `report.json`, both journals and the
failed units of a real bundle; `tests/pipeline.rs` proves that none of the
values appears in what is filed, in any answer, in the log or in plain text in
the data directory, and that useful lines survive. The `reports` workflow
repeats this over HTTP with the container, adds personal data the client
cannot recognise to a report made by `tur-diag` on the runner, and checks the
runner's host name never reaches the stand-in.

Known limits: personal names in prose, street addresses, phone numbers and
free-form identifiers without a label are not recognised; a learnt value is
replaced only as a whole word.

## 6. Storage, encryption and retention

```text
/var/lib/tur-report-receiver/      0700, owned by the service user
  lock                             every read or change holds it (flock)
  reports/<id>.json                record: id, SHA-256 of the token, times, key id,
                                   signature, code, component, version, state, filing
  tokens/<sha256(token)>           -> report id; the token itself is never stored
  bundles/<id>.bin                 the raw bundle, sealed
  summaries/<id>.bin               the summary until it is filed, sealed
  signatures.json                  signature -> issue number
  tmp/                             writes in progress (synced, then renamed)
```

Sealing is XChaCha20-Poly1305 (`chacha20poly1305` 0.10) with a 256-bit key
and a random 192-bit nonce per blob; the associated data names the purpose
and the report, so blobs cannot be swapped. Blob: `TURR1`, key id, nonce,
ciphertext. Keys are files `<keys_dir>/<id>.key` holding 64 hex digits,
outside the data directory (the configuration refuses otherwise), mode 0400
or 0600 (the program refuses others), made with `tur-report-receiver keygen
--output FILE` (32 bytes from `getrandom`). Rotation: make a new key, set
`current_key` to it and restart; new data is sealed with it, every key in the
directory still opens what it sealed; remove the old key file after the
retention period (90 days), when nothing sealed with it remains. Nothing in
the data directory is readable without the key, and nothing derived from a
report is kept in plain text.

The sweep runs at start and every hour (and as `tur-report-receiver sweep`):
a bundle is deleted 90 days after receipt (`retention_days`); its record,
which holds nothing personal, 365 days after (`status_retention_days`), after
which the status token no longer answers. Files left by an interrupted write
are removed. `sweep --now EPOCH` sweeps as of another time; the tests use it
and an injected clock. A maintainer reads a bundle within the period with
`tur-report-receiver decrypt --report ID --output FILE` (mode 0600).

## 7. Logs

One line per event on standard error (journald or the container log):
`level=info event=received report=R-… signature=… bytes=… scrubbed=…`, and
`refused code=…`, `filed … issue=… as=issue|comment`, `filing-failed
reason=http-503 attempts=… retry_in=…`, `swept`, `marked-fixed`,
`decrypted`, `listening`, `stopping`. Values are restricted by construction
to `A-Z a-z 0-9 . _ : / + -` and 64 characters; call sites pass only
identifiers, codes and counts. No client address, token, file name or text
from a report is ever logged; the tests scan the logs for all of them.

## 8. Choices

- **HTTP server: hyper 1 (HTTP/1 only) on a single-threaded tokio runtime.**
  It is the HTTP implementation the Rust ecosystem's servers are built on,
  actively maintained (1.12, October 2026), with header timeouts and strict
  parsing; no framework on top (two routes do not need one). `tiny_http` was
  considered and rejected: its last release is from 2022. Verification and
  disk work run on tokio's blocking threads.
- **TLS: Caddy in front**, which obtains certificates, redirects HTTP to
  HTTPS and writes no access log unless told to. The receiver listens on the
  loopback interface only.
- **GitHub client: ureq 3** (blocking, rustls with ring and the Mozilla
  roots compiled in, no other features), on the filer thread.
- **Client transport: the system's curl**, not a TLS stack in tur-diag: it
  honours Fedora's trust store and crypto policies, and adds no crate to the
  image.
- **Encryption: chacha20poly1305 0.10**, the line whose interface has been
  stable for years; 0.11 (August 2026) moved to new array types.
- **Storage: files and a lock**, not a database: a few reports a day, and
  every write is atomic.
- Every third-party crate compiled into the receiver is listed with its
  licence in [receiver-crates.csv](../dependencies/receiver-crates.csv); CI
  fails if one is not permissive or the list is stale. None of them is in the
  TUR image.

## 9. Privacy statement (draft for review by the lawyer)

> **Problem reports from TUR**
>
> TUR can send a problem report to its developers so that a later update can
> fix the problem. A report is sent only when you choose to send it, after
> you have seen what it contains. TUR never sends reports in the background,
> and your assistant P@x cannot send one for you.
>
> **What a report contains.** A short description of the problem (written by
> you, or proposed by P@x for you to read), an error code where one is known,
> the part of TUR concerned, the TUR version, and a diagnostic bundle: system
> logs of the current and, where relevant, the previous start, the list of
> failed services, and technical facts about the hardware and the system's
> configuration. It does not contain your documents, browser data,
> passwords, keys, or anything else from your home folder.
>
> **What is removed before it leaves your computer.** User names, the
> computer's name, serial numbers, network hardware addresses, public IP
> addresses, disk and other unique identifiers, Wi-Fi network names, the
> machine ID and labelled secrets are replaced by placeholders on your
> computer. The rules cannot recognise every personal detail; you can read
> every file before you send.
>
> **Where it goes.** To TUR's own report server, over an encrypted
> connection. The server removes personal data a second time, with rules of
> its own (including e-mail addresses, all IP addresses, user names in file
> paths, tokens and keys), and files a short summary for TUR's developers in
> a private issue tracker hosted by GitHub, Inc. The full diagnostic bundle
> is never placed in the tracker.
>
> **How long it is kept.** The full bundle is stored encrypted on the report
> server for 90 days, so that a developer can examine it if the summary is
> not enough, and is then deleted automatically. The server keeps a record
> without personal data (an identifier, the error code, the part of TUR, the
> version and the state) for one year, so that you can see what became of
> your report. Summaries in the private tracker are kept while the problem is
> worked on.
>
> **Your address.** The server uses the IP address your report comes from
> only to limit how many reports can be sent from one address in an hour. It
> is held in memory for at most an hour and is never written to disk or to
> a log.
>
> **Seeing the state of your report.** When a report is accepted, your
> computer receives a report identifier and a private status link. TUR shows
> your sent reports in Settings (`tur-diag report list`), and whether each was
> received, filed, or fixed in a given version.
>
> **Who can see it.** Only TUR's developers with access to the private
> tracker see summaries; only the server's administrators can decrypt a
> bundle, within the 90 days.
>
> **Questions and deletion.** *[Contact address and the controller's
> identity to be added. Legal basis (consent), the right to withdraw consent
> and to ask for deletion of a report by its identifier, and the transfer to
> GitHub, Inc. (United States) to be reviewed.]*

## 10. Not done here

- The image does not install a `problem-reports.conf`; Settings has no page
  for reports yet; the "send problem reports automatically" switch (ADR-0024,
  item 2) does not exist — when it does, it must go through the same
  `send` path with the same checks.
- Opening a public issue without personal data (ADR-0024, item 5) is left
  to maintainers.
- The receiver has not been deployed; see the guide.
