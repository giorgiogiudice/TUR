<!-- SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0 -->
# Deploying the problem report receiver

For the owner. This puts TUR's problem report receiver
([problem-reports.md](../architecture/problem-reports.md), ADR-0024) on a
small server of its own, with one command:
`server/report-receiver/deploy/install.sh`. The command and the image it
installs are tested together by the `reports-deployed` workflow (started by
hand; see report [reports-e2e-1](../reports/reports-e2e-1.md) for what has
and has not run).

Do this only with the go-ahead of ADR-0024 item 4 (receiver and client green
in CI), and after the privacy statement (problem-reports.md, section 9) has
been reviewed.

## What you need, and what it costs

| # | What | Cost | Notes |
|---|---|---|---|
| 1 | **A server**: a Linode Nanode (1 GB memory, 1 shared CPU, 25 GB disk), Debian 13 or Ubuntu 24.04 LTS | about US$5 a month (Linode's list price; check when ordering) | Enough; see "Is a Nanode enough?" below. Nothing else runs on it. Do **not** turn on Linode Backups: reports must be gone after 90 days, and a backup would keep them. |
| 2 | **A name** for it: a subdomain of a domain you have, for example `reports.<your-domain>` | nothing, if you have a domain; a new domain is about US$10–15 a year | An `A` record (and `AAAA`, if the server has IPv6) pointing at the server. Behind Cloudflare: **DNS only** (grey cloud), so that the server gets its own certificate and sees the real client addresses for its rate limits. |
| 3 | **A private GitHub repository** for the reports, for example `tur-problem-reports`, with the label `problem-report` | nothing (GitHub Free) | Private: summaries are redacted twice, but they are about people's computers. |
| 4 | **A fine-grained token** limited to that repository, permission **Issues: Read and write** only | nothing | Pasted once, into the install command; it lives only on the server, in `/etc/tur-report-receiver/github-token` (mode 0400). Renew it before it expires (366 days at most). |
| 5 | Your **SSH key** on the server | nothing | The install command turns password logins off only if a key is installed, so you are never locked out. |

Total: about US$5 a month, plus a domain if you do not have one.

**Is a Nanode enough?** From the receiver's design (measured numbers come
from the `reports-deployed` workflow when it runs, report reports-e2e-1):

- Idle, the receiver is one small Rust program on a single-threaded runtime:
  a few MiB to a few tens of MiB. Caddy uses a few tens of MiB. Debian
  itself about 150 MiB.
- An upload is held in memory while it is checked: the bundle (at most
  8 MiB), its unpacked files (at most 64 MiB), what the second redaction
  pass reads and writes over them, and the sealed copy. Worst case about
  150–200 MiB per upload, and at most two are checked at once
  (`max_concurrent_uploads`); a third waits or is told to come back
  (`TUR-DIAG-RECEIVER-010`). The container is capped at 768 MiB.
- CPU: unpacking 64 MiB and scanning it with the redaction rules is seconds
  of one CPU for the largest reports; typical reports (a few hundred KiB)
  take a fraction of a second. At most 120 uploads an hour in all, 6 an
  hour from one address.
- Disk: bundles are at most 8 MiB, kept 90 days. 1 000 large reports would
  be 8 GB; the 25 GB disk is ample.

So a Nanode is enough for a project of TUR's size; if reports ever arrive
faster than 120 an hour, the limits refuse them before the server strains.

## 1. The private repository

1. On GitHub: **New repository** → name `tur-problem-reports` → **Private** →
   create.
2. **Issues → Labels → New label**: `problem-report`. (The receiver puts this
   label on every issue it opens; it must exist.)

## 2. The token

GitHub → **Settings → Developer settings → Personal access tokens →
Fine-grained tokens → Generate new token**:

- Token name: `tur-report-receiver`
- Expiration: 366 days (put a reminder in your calendar a week before)
- Resource owner: the account or organisation that owns the repository
- Repository access: **Only select repositories** → `tur-problem-reports`
- Permissions → Repository permissions → **Issues: Read and write**.
  Leave everything else at "No access" (Metadata: read-only is added by
  GitHub automatically).

Copy the token; you paste it once, in step 5. It can open and comment on
issues in that one repository and do nothing else. Do not store it anywhere
else, and never in the TUR repository.

## 3. The server and its name

1. Linode → **Create Linode** → Debian 13 (or Ubuntu 24.04 LTS), the
   region nearest to you, **Nanode 1 GB**, your SSH key, a strong root
   password (you will not need it after the key works). No Backups.
2. In your DNS: `reports.<your-domain>` → `A` → the server's IPv4 address
   (and `AAAA` → its IPv6 address). Wait until `dig +short
   reports.<your-domain>` on your computer shows the address.

## 4. The image

The `reports` workflow builds the image, tests it end to end, and keeps it
for 14 days as the artifact `tur-report-receiver-image` of the run. Take it
from a green run on `main`:

1. GitHub → the TUR repository → **Actions → reports** → the latest green
   run on `main` → **Artifacts → tur-report-receiver-image** → download and
   unzip. It holds `tur-report-receiver-image.tar.gz` and its `.sha256`.
2. Copy both, and the `deploy` directory of the same commit, to the server:

   ```sh
   scp tur-report-receiver-image.tar.gz tur-report-receiver-image.tar.gz.sha256 root@reports.<your-domain>:/root/
   scp -r server/report-receiver/deploy root@reports.<your-domain>:/root/deploy
   ```

## 5. The one command

On the server, as root:

```sh
/root/deploy/install.sh --domain reports.<your-domain> \
                        --repository <you>/tur-problem-reports \
                        --image /root/tur-report-receiver-image.tar.gz
```

It asks for the token (paste it, press Enter; nothing is shown), then:
installs Podman, Caddy, nftables and automatic security updates from the
distribution; turns SSH password logins off (only if your key is
installed); opens only SSH, HTTP and HTTPS in the firewall (add
`--ssh-from <your-address>` to accept SSH from your address only); checks
the image against its `.sha256` and loads it; makes the encryption key;
writes the configuration; starts the service on `127.0.0.1:8470`; puts
Caddy in front, which gets the server's certificate from Let's Encrypt;
and checks the receiver over HTTPS. It ends with "https://reports.… answers"
(exit status 0). Exit status 3 means everything is installed and running
but HTTPS did not answer yet: usually DNS that has not reached the server,
or ports 80 and 443 closed in Linode's Cloud Firewall. Fix that and run the
same command again; it is safe to repeat.

To use a certificate of your own instead of Let's Encrypt, add
`--tls-cert fullchain.pem --tls-key key.pem`.

The token never appears in a command line, the environment, a log or the
shell history. Then delete the image from `/root`:

```sh
rm /root/tur-report-receiver-image.tar.gz*
```

## 6. Check it from outside

From your own computer:

```sh
curl -s https://reports.<your-domain>/v1/reports/0000000000000000000000000000000000000000000000000000000000000000
# {"schema_version":1,"code":"TUR-DIAG-RECEIVER-011","message":"No report has this status token.","detail_code":null}
curl -sI http://reports.<your-domain>/v1/reports | head -1    # a redirect to https
```

Then one real report, from a TUR computer:

```sh
echo 'RECEIVER_URL=https://reports.<your-domain>' | sudo tee /etc/tur/problem-reports.conf
tur-diag report submit --title "Receiver test" --component other --description "A test report."
#   ... read what it shows, type yes ...
tur-diag report status R-...                               # filed, within a minute
```

The issue appears in `tur-problem-reports` with the label `problem-report`.
Close it.

## 7. Running it

| Task | Command (on the server, as root) |
|---|---|
| See what it does | `journalctl -u tur-report-receiver -f` |
| Update | Copy the new image (step 4), then run the command of step 5 again with the new `--image`. Key, token and reports are kept. |
| Mark an issue fixed (people then see "fixed in 0.2.1") | `podman exec tur-report-receiver tur-report-receiver mark-fixed --config /etc/tur-report-receiver/config.json --issue 12 --version 0.2.1` |
| Read a report's bundle (within 90 days) | `podman exec tur-report-receiver tur-report-receiver decrypt --config /etc/tur-report-receiver/config.json --report R-… --output /var/lib/tur-report-receiver/R-….tar.zst`, copy it off with `scp`, then delete it from the server |
| Delete expired data now | `podman exec tur-report-receiver tur-report-receiver sweep --config /etc/tur-report-receiver/config.json` (it also runs every hour) |
| Replace the token | `rm /etc/tur-report-receiver/github-token`, then run step 5's command (without `--image`): it asks for the new token. The receiver reads the file at every filing; no restart is needed |
| Rotate the key | `podman run --rm --network none --user 10001:10001 -v /etc/tur-report-receiver/keys:/keys localhost/tur-report-receiver:current keygen --output /keys/<new-id>.key`, set `current_key` in `/etc/tur-report-receiver/config.json`, `systemctl restart tur-report-receiver`; delete the old `.key` file 90 days later |

The procedure from a filed report to the update that fixes it is in
[fix-a-reported-problem.md](fix-a-reported-problem.md).

Reports that arrive while GitHub is down wait in the receiver and are filed
when it is back; nothing to do. If filing keeps failing, the log says why
(`event=filing-failed reason=http-401` means the token expired or was
revoked).

**Backups.** Do not back up `/var/lib/tur-report-receiver`: bundles must be
gone after 90 days, and a backup would keep them. If you back up the
configuration, leave out the key and the token.

**SELinux.** The command is written for Debian and Ubuntu. On a host with
SELinux enforcing (Fedora, RHEL), add `,Z` to each `--volume` option in the
unit.

## 8. Letting TUR use it

TUR computers send nowhere until their configuration names the receiver.
Once the receiver works, a change to the image (not part of this work)
installs `/usr/lib/tur/problem-reports.conf` with

```text
RECEIVER_URL=https://reports.<your-domain>
```

Until then, a person can set it in `/etc/tur/problem-reports.conf` as in
step 6.
