<!-- SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0 -->
# Deploying the problem report receiver

For the owner. This sets up TUR's problem report receiver
([problem-reports.md](../architecture/problem-reports.md), ADR-0024) on a
small server of its own: one job only, HTTPS only. Nothing here has been run
yet; it is written against the files in `server/report-receiver/deploy/` and
the image the `reports` workflow tests.

Do this only with the go-ahead of ADR-0024 item 4 (receiver and client green
in CI), and only after the privacy statement (problem-reports.md, section 9)
has been reviewed.

## What you need

| What | Notes |
|---|---|
| A server | A Linode "Nanode" (1 GB RAM, 1 CPU) is enough. Debian 13 or Ubuntu 24.04 LTS. Nothing else runs on it. |
| SSH | Your public key, to log in as root (or a sudo user) the first time. |
| A domain name | One name for the receiver, for example `reports.example.org`, with an `A` record (and `AAAA`, if the server has IPv6) pointing at the server. If the domain is behind Cloudflare, make this record **DNS only** (grey cloud), so that Caddy can obtain its certificate and sees the real client addresses. |
| A private GitHub repository | For example `tur-problem-reports`, **private**, issues enabled. |
| A GitHub token | Fine-grained, limited to that repository, permission **Issues: read and write** only (step 2). |

Decide once, before you start: the domain name, and the key name (any 1–32
letters, digits, `_` or `-`, for example `2026-11`).

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

Copy the token once; you paste it in step 5. It can open and comment on
issues in that one repository and do nothing else. Do not store it anywhere
else.

## 3. The server

Log in with SSH. As root:

```sh
# Updates, and automatic security updates from now on.
apt update && apt full-upgrade -y
apt install -y unattended-upgrades podman curl debian-keyring debian-archive-keyring apt-transport-https
dpkg-reconfigure -plow unattended-upgrades

# SSH: keys only.
sed -i 's/^#\?PasswordAuthentication.*/PasswordAuthentication no/' /etc/ssh/sshd_config
sed -i 's/^#\?PermitRootLogin.*/PermitRootLogin prohibit-password/' /etc/ssh/sshd_config
systemctl reload ssh

# Firewall: SSH, and HTTP/HTTPS for Caddy. Port 80 only redirects to HTTPS
# and answers the certificate authority's checks.
apt install -y nftables
cat > /etc/nftables.conf <<'EOF'
#!/usr/sbin/nft -f
flush ruleset
table inet filter {
  chain input {
    type filter hook input priority 0; policy drop;
    ct state established,related accept
    iif lo accept
    meta l4proto { icmp, ipv6-icmp } accept
    tcp dport 22 accept
    tcp dport { 80, 443 } accept
  }
}
EOF
systemctl enable --now nftables
```

To restrict SSH to your own address, replace `tcp dport 22 accept` with
`ip saddr YOUR.IP.ADDRESS tcp dport 22 accept`. Linode's Cloud Firewall can do
the same in front of the server.

Caddy, from its own repository (the distributions' packages are older):

```sh
curl -1sLf 'https://dl.cloudsmith.io/public/caddy/stable/gpg.key' | gpg --dearmor -o /usr/share/keyrings/caddy-stable-archive-keyring.gpg
curl -1sLf 'https://dl.cloudsmith.io/public/caddy/stable/debian.deb.txt' > /etc/apt/sources.list.d/caddy-stable.list
apt update && apt install -y caddy
```

## 4. The image

The `reports` workflow builds the image, tests it end to end, and keeps it
for 14 days as the artifact `tur-report-receiver-image` of the run. Take it
from a run on `main`:

1. GitHub → the TUR repository → **Actions → reports** → the latest green run
   on `main` → **Artifacts → tur-report-receiver-image** → download, unzip on
   your computer. It holds `tur-report-receiver-image.tar.gz` and its
   `.sha256`.
2. Check and copy it:

   ```sh
   sha256sum -c tur-report-receiver-image.tar.gz.sha256
   scp tur-report-receiver-image.tar.gz root@reports.example.org:/root/
   ```

3. On the server:

   ```sh
   podman load -i /root/tur-report-receiver-image.tar.gz
   podman tag localhost/tur-report-receiver:ci localhost/tur-report-receiver:current
   rm /root/tur-report-receiver-image.tar.gz
   ```

(Alternatively, build it on the server from a checkout with
`podman build -f server/report-receiver/Containerfile -t localhost/tur-report-receiver:current .`;
on a 1 GB server this needs a 2 GB swap file and takes a long while. The
artifact is the image CI tested.)

## 5. Configuration, key and token

The container runs as user 10001. Everything it reads or writes belongs to
that user.

```sh
install -d -m 0750 -o 10001 -g 10001 /etc/tur-report-receiver
install -d -m 0700 -o 10001 -g 10001 /etc/tur-report-receiver/keys
install -d -m 0700 -o 10001 -g 10001 /var/lib/tur-report-receiver

# The encryption key: 32 random bytes, readable only by the receiver.
podman run --rm --user 10001:10001 \
  --volume /etc/tur-report-receiver/keys:/keys \
  localhost/tur-report-receiver:current keygen --output /keys/2026-11.key

# The GitHub token: run this, paste the token, press Enter, then Ctrl-D.
# It does not go into your shell history.
( umask 077 && cat > /etc/tur-report-receiver/github-token )
chown 10001:10001 /etc/tur-report-receiver/github-token
chmod 0400 /etc/tur-report-receiver/github-token
```

Copy `server/report-receiver/deploy/config.example.json` to
`/etc/tur-report-receiver/config.json` and change three values:

- `public_url`: `https://reports.example.org` (your name);
- `github.repository`: `OWNER/tur-problem-reports`;
- `current_key`: the key name from above (`2026-11`).

Keep `listen` on `127.0.0.1:8470`. Then:

```sh
chown 10001:10001 /etc/tur-report-receiver/config.json
chmod 0640 /etc/tur-report-receiver/config.json

podman run --rm --network host --user 10001:10001 \
  --volume /etc/tur-report-receiver/config.json:/etc/tur-report-receiver/config.json:ro \
  --volume /etc/tur-report-receiver/keys:/etc/tur-report-receiver/keys:ro \
  --volume /etc/tur-report-receiver/github-token:/etc/tur-report-receiver/github-token:ro \
  --volume /var/lib/tur-report-receiver:/var/lib/tur-report-receiver \
  localhost/tur-report-receiver:current check-config --config /etc/tur-report-receiver/config.json
```

It prints `configuration, keys and token file are usable`, or what is wrong.
(It cannot tell whether GitHub accepts the token; the first filing does.)

## 6. Start it

```sh
cp tur-report-receiver.service /etc/systemd/system/     # from server/report-receiver/deploy/
systemctl daemon-reload
systemctl enable --now tur-report-receiver
journalctl -u tur-report-receiver -n 20                 # expect: level=info event=listening address=127.0.0.1:8470
```

Then Caddy: copy `server/report-receiver/deploy/Caddyfile` to
`/etc/caddy/Caddyfile`, replace `reports.example.org` with your name, and:

```sh
systemctl reload caddy
journalctl -u caddy -n 20                               # expect: certificate obtained successfully
```

## 7. Check it from outside

From your own computer:

```sh
curl -s https://reports.example.org/v1/reports/0000000000000000000000000000000000000000000000000000000000000000
# {"schema_version":1,"code":"TUR-DIAG-RECEIVER-011","message":"No report has this status token.","detail_code":null}
curl -sI http://reports.example.org/v1/reports | head -1    # a redirect to https
```

Then one real report, from a TUR computer (or any machine with `tur-diag`):

```sh
echo 'RECEIVER_URL=https://reports.example.org' | sudo tee /etc/tur/problem-reports.conf
tur-diag report prepare --title "Receiver test" --component other --description "A test report."
tur-diag report send --to https://reports.example.org ~/.local/state/tur/reports/prepared/report-*.tar.zst
tur-diag report status R-...                               # filed, within a minute
```

The issue appears in `tur-problem-reports` with the label `problem-report`.
Close it.

## 8. Running it

| Task | Command (on the server, as root) |
|---|---|
| See what it does | `journalctl -u tur-report-receiver -f` |
| Mark an issue fixed (people then see "fixed in 0.2.1") | `podman exec tur-report-receiver tur-report-receiver mark-fixed --config /etc/tur-report-receiver/config.json --issue 12 --version 0.2.1` |
| Read a report's bundle (within 90 days) | `podman exec tur-report-receiver tur-report-receiver decrypt --config /etc/tur-report-receiver/config.json --report R-… --output /var/lib/tur-report-receiver/R-….tar.zst`, copy it off with `scp`, then delete it from the server |
| Delete expired data now | `podman exec tur-report-receiver tur-report-receiver sweep --config /etc/tur-report-receiver/config.json` (it also runs every hour) |
| Replace the token | Write the new one as in step 5; it is read at every filing, no restart needed |
| Rotate the key | `keygen` a new key as in step 5, set `current_key` to it, `systemctl restart tur-report-receiver`; delete the old `.key` file 90 days later |
| Update | Load the new image as in step 4, tag it `current`, `systemctl restart tur-report-receiver` |

Reports that arrive while GitHub is down wait in the receiver and are filed
when it is back; nothing to do. If filing keeps failing, the log says why
(`event=filing-failed reason=http-401` means the token expired or was
revoked).

**Backups.** Do not back up `/var/lib/tur-report-receiver`: bundles must be
gone after 90 days, and a backup would keep them. If you back up the
configuration, leave out the key and the token.

**SELinux.** On a host with SELinux enforcing (Fedora, RHEL), add `,Z` to
each `--volume` option in the unit and the commands above.

## 9. Letting TUR use it

TUR computers send nowhere until their configuration names the receiver. Once
the receiver works, a change to the image (not part of this work) installs
`/usr/lib/tur/problem-reports.conf` with

```text
RECEIVER_URL=https://reports.example.org
```

Until then, a person can set it in `/etc/tur/problem-reports.conf` as in
step 7.
