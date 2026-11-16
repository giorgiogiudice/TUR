#!/usr/bin/env bash
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# Installs, or updates, TUR's problem report receiver on a small server of
# its own (ADR-0024; docs/guides/deploy-report-receiver.md). One command, as
# root, on Debian 13 or Ubuntu 24.04 LTS:
#
#   sudo ./install.sh --domain reports.example.org \
#                     --repository OWNER/tur-problem-reports \
#                     --image tur-report-receiver-image.tar.gz
#
# It asks for the GitHub token on the terminal (nothing is echoed; it never
# appears in a command line, the environment, a log or the shell history),
# or reads it from --token-file. Run again with a new --image to update; the
# encryption key, the token and the stored reports are kept.
#
# What it does, in order:
#   1. packages: podman, caddy, nftables, unattended-upgrades (from the
#      distribution; automatic security updates switched on);
#   2. SSH: keys only (only if a key is installed, so that nobody is locked
#      out); firewall: SSH, HTTP and HTTPS in, nothing else;
#   3. the image, checked against its .sha256 when that file lies beside it,
#      tagged localhost/tur-report-receiver:current;
#   4. /etc/tur-report-receiver: config.json, keys/<id>.key (made once),
#      github-token (0400); /var/lib/tur-report-receiver (0700); all owned by
#      uid 10001, the container's user;
#   5. the systemd unit (Podman, loopback only) and Caddy in front, with a
#      certificate from Let's Encrypt, or the server's own with --tls-cert
#      and --tls-key;
#   6. checks: the configuration, the service on 127.0.0.1:8470, then HTTPS
#      on the domain.
#
# Exit status: 0 done and reachable over HTTPS; 1 refused or failed;
# 3 installed and running locally, HTTPS not reachable yet (DNS, ports).

set -euo pipefail
umask 022

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
UID_RECEIVER=10001
ETC=/etc/tur-report-receiver
DATA=/var/lib/tur-report-receiver
IMAGE_TAG=localhost/tur-report-receiver:current
LOCAL=http://127.0.0.1:8470
NO_TOKEN=0000000000000000000000000000000000000000000000000000000000000000

domain=""
repository=""
image=""
token_file=""
tls_cert=""
tls_key=""
key_id="$(date -u +%Y-%m)"
ssh_from=""
firewall=1
ssh_hardening=1
auto_updates=1
api_url="https://api.github.com"

usage() {
    sed -n '3,34p' "$0" | sed 's/^# \{0,1\}//'
    cat <<'EOF'

Options:
  --domain NAME          the receiver's name, with an A (and AAAA) record to this server
  --repository O/R       the private GitHub repository reports are filed in
  --image FILE           the image from the reports workflow (tur-report-receiver-image.tar.gz)
  --token-file FILE      read the GitHub token from FILE instead of asking
  --tls-cert FILE        the server's own certificate (full chain, PEM) instead of Let's Encrypt
  --tls-key FILE         its private key (PEM)
  --key-id ID            name of a new encryption key (default: this month, YYYY-MM)
  --ssh-from ADDRESS     accept SSH only from this address or network
  --no-firewall          leave the firewall alone
  --no-ssh-hardening     leave SSH alone
  --no-auto-updates      leave automatic updates alone
EOF
}

say() { printf '==> %s\n' "$*"; }
fail() { printf 'install.sh: %s\n' "$*" >&2; exit 1; }

while [ $# -gt 0 ]; do
    case "$1" in
        --domain) domain="${2:-}"; shift 2 ;;
        --repository) repository="${2:-}"; shift 2 ;;
        --image) image="${2:-}"; shift 2 ;;
        --token-file) token_file="${2:-}"; shift 2 ;;
        --tls-cert) tls_cert="${2:-}"; shift 2 ;;
        --tls-key) tls_key="${2:-}"; shift 2 ;;
        --key-id) key_id="${2:-}"; shift 2 ;;
        --ssh-from) ssh_from="${2:-}"; shift 2 ;;
        --no-firewall) firewall=0; shift ;;
        --no-ssh-hardening) ssh_hardening=0; shift ;;
        --no-auto-updates) auto_updates=0; shift ;;
        # For the end-to-end test only: a stand-in for GitHub's API.
        --github-api-url) api_url="${2:-}"; shift 2 ;;
        -h|--help) usage; exit 0 ;;
        *) usage >&2; fail "unknown option $1" ;;
    esac
done

# ---------------------------------------------------------------- checks

[ "$(id -u)" -eq 0 ] || fail "run as root (sudo $0 ...)"
[ -r /etc/os-release ] || fail "no /etc/os-release"
# shellcheck disable=SC1091
. /etc/os-release
case "${ID:-}" in
    debian|ubuntu) ;;
    *) fail "written for Debian 13 or Ubuntu 24.04; this is ${ID:-unknown}" ;;
esac

# Every value that goes into a file is checked against a strict form first,
# so that none can change the meaning of the configuration.
[[ "$domain" =~ ^[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?(\.[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?)+$ ]] \
    || fail "--domain must be a host name in lower case, such as reports.example.org"
[[ "$repository" =~ ^[A-Za-z0-9_.-]{1,100}/[A-Za-z0-9_.-]{1,100}$ ]] \
    || fail "--repository must be OWNER/NAME"
[[ "$key_id" =~ ^[A-Za-z0-9_-]{1,32}$ ]] || fail "--key-id: 1 to 32 of A-Z a-z 0-9 _ -"
[[ "$api_url" =~ ^https?://[A-Za-z0-9.:-]+(/[A-Za-z0-9._/-]*)?$ ]] || fail "--github-api-url"
[ -z "$ssh_from" ] || [[ "$ssh_from" =~ ^[0-9A-Fa-f.:/]+$ ]] || fail "--ssh-from: an address or network"
if [ -n "$tls_cert" ] || [ -n "$tls_key" ]; then
    if [ ! -r "$tls_cert" ] || [ ! -r "$tls_key" ]; then
        fail "--tls-cert and --tls-key go together, both readable"
    fi
fi
if ! podman image exists "$IMAGE_TAG" 2>/dev/null; then
    [ -n "$image" ] || fail "--image is needed the first time"
fi
[ -z "$image" ] || [ -r "$image" ] || fail "cannot read $image"
[ -z "$token_file" ] || [ -r "$token_file" ] || fail "cannot read $token_file"
for file in config.example.json tur-report-receiver.service Caddyfile; do
    [ -r "$HERE/$file" ] || fail "$HERE/$file is missing: run this script from server/report-receiver/deploy"
done

# ------------------------------------------------------------- packages

say "Packages"
export DEBIAN_FRONTEND=noninteractive
apt-get update -q
apt-get install -y -q podman caddy nftables curl ca-certificates unattended-upgrades
if [ "$auto_updates" -eq 1 ]; then
    cat > /etc/apt/apt.conf.d/20auto-upgrades <<'EOF'
APT::Periodic::Update-Package-Lists "1";
APT::Periodic::Unattended-Upgrade "1";
EOF
    systemctl enable --now unattended-upgrades.service >/dev/null 2>&1 || true
fi

# ------------------------------------------------------- SSH, firewall

if [ "$ssh_hardening" -eq 1 ]; then
    keys=0
    for file in /root/.ssh/authorized_keys /home/*/.ssh/authorized_keys; do
        if [ -s "$file" ]; then keys=1; fi
    done
    drop_in=/etc/ssh/sshd_config.d/10-tur-report-receiver.conf
    if [ "$keys" -eq 1 ] && [ -d /etc/ssh/sshd_config.d ]; then
        say "SSH: keys only"
        cat > "$drop_in" <<'EOF'
# Written by TUR's report receiver install.sh: keys only.
PasswordAuthentication no
KbdInteractiveAuthentication no
PermitRootLogin prohibit-password
EOF
        if sshd -t; then
            systemctl reload ssh 2>/dev/null || systemctl reload sshd 2>/dev/null || true
        else
            rm -f "$drop_in"
            say "SSH left as it is: its configuration did not accept the change"
        fi
    else
        say "SSH left as it is: no authorized key found, so passwords stay on (add a key, run again)"
    fi
fi

if [ "$firewall" -eq 1 ]; then
    say "Firewall: SSH, HTTP and HTTPS in, nothing else"
    ssh_rule="tcp dport 22 accept"
    if [ -n "$ssh_from" ]; then
        if [[ "$ssh_from" == *:* ]]; then
            ssh_rule="ip6 saddr $ssh_from tcp dport 22 accept"
        else
            ssh_rule="ip saddr $ssh_from tcp dport 22 accept"
        fi
    fi
    cat > /etc/nftables.conf <<EOF
#!/usr/sbin/nft -f
# Written by TUR's report receiver install.sh. This server runs the receiver only.
flush ruleset
table inet filter {
  chain input {
    type filter hook input priority 0; policy drop;
    ct state established,related accept
    ct state invalid drop
    iif lo accept
    meta l4proto { icmp, ipv6-icmp } accept
    $ssh_rule
    tcp dport { 80, 443 } accept
  }
  chain forward {
    type filter hook forward priority 0; policy drop;
  }
}
EOF
    nft -c -f /etc/nftables.conf
    systemctl enable nftables.service >/dev/null
    systemctl restart nftables.service
fi

# ---------------------------------------------------------------- image

if [ -n "$image" ]; then
    say "Image"
    if [ -r "$image.sha256" ]; then
        expected="$(cut -d' ' -f1 < "$image.sha256")"
        actual="$(sha256sum "$image" | cut -d' ' -f1)"
        [ "$expected" = "$actual" ] || fail "$image does not match $image.sha256"
        say "  SHA-256 matches $image.sha256"
    fi
    loaded="$(podman load -q -i "$image" | sed -n 's/^Loaded image[^:]*: *//p' | tail -n 1)"
    [ -n "$loaded" ] || fail "podman load named no image"
    podman tag "$loaded" "$IMAGE_TAG"
    say "  $loaded is now $IMAGE_TAG"
fi

# ------------------------------------------------ configuration, key, token

say "Configuration in $ETC"
install -d -m 0750 -o "$UID_RECEIVER" -g "$UID_RECEIVER" "$ETC"
install -d -m 0700 -o "$UID_RECEIVER" -g "$UID_RECEIVER" "$ETC/keys"
install -d -m 0700 -o "$UID_RECEIVER" -g "$UID_RECEIVER" "$DATA"

current_key="$key_id"
if [ -f "$ETC/config.json" ]; then
    # Keep the key the receiver uses; a new one is made only on purpose.
    kept="$(sed -n 's/^ *"current_key": *"\([A-Za-z0-9_-]*\)".*/\1/p' "$ETC/config.json")"
    if [ -n "$kept" ] && [ -f "$ETC/keys/$kept.key" ]; then
        current_key="$kept"
    fi
fi
if [ ! -f "$ETC/keys/$current_key.key" ]; then
    say "  a new encryption key: keys/$current_key.key"
    podman run --rm --network none --user "$UID_RECEIVER:$UID_RECEIVER" \
        --volume "$ETC/keys:/keys" "$IMAGE_TAG" keygen --output "/keys/$current_key.key" >/dev/null
fi

token_path="$ETC/github-token"
write_token() {
    # $1: the token. printf is a shell builtin: the token is in no command line.
    local token="$1"
    token="${token//[$'\r\n\t ']/}"
    [ -n "$token" ] || fail "the token is empty"
    if [ "$api_url" = "https://api.github.com" ] && [[ "$token" != github_pat_* ]]; then
        fail "use a fine-grained token (it starts with github_pat_), limited to $repository, Issues: read and write"
    fi
    ( umask 077 && printf '%s\n' "$token" > "$token_path.new" )
    chown "$UID_RECEIVER:$UID_RECEIVER" "$token_path.new"
    chmod 0400 "$token_path.new"
    mv -f "$token_path.new" "$token_path"
}
if [ -n "$token_file" ]; then
    say "  the GitHub token, from $token_file"
    write_token "$(cat "$token_file")"
elif [ ! -s "$token_path" ]; then
    if ! (exec < /dev/tty) 2>/dev/null; then
        fail "no terminal to ask for the token on: give --token-file"
    fi
    printf 'Paste the fine-grained GitHub token for %s (not shown), then press Enter: ' "$repository" > /dev/tty
    IFS= read -r -s token < /dev/tty
    printf '\n' > /dev/tty
    write_token "$token"
    unset token
else
    say "  the GitHub token already installed is kept"
fi

sed -e "s|https://reports.example.org|https://$domain|" \
    -e "s|OWNER/tur-problem-reports|$repository|" \
    -e "s|\"current_key\": \"[^\"]*\"|\"current_key\": \"$current_key\"|" \
    -e "s|https://api.github.com|$api_url|" \
    "$HERE/config.example.json" > "$ETC/config.json.new"
chown "$UID_RECEIVER:$UID_RECEIVER" "$ETC/config.json.new"
chmod 0640 "$ETC/config.json.new"
mv -f "$ETC/config.json.new" "$ETC/config.json"

podman run --rm --network none --user "$UID_RECEIVER:$UID_RECEIVER" \
    --volume "$ETC/config.json:$ETC/config.json:ro" \
    --volume "$ETC/keys:$ETC/keys:ro" \
    --volume "$ETC/github-token:$ETC/github-token:ro" \
    --volume "$DATA:$DATA" \
    "$IMAGE_TAG" check-config --config "$ETC/config.json"

# --------------------------------------------------------- service, Caddy

say "Service"
install -m 0644 "$HERE/tur-report-receiver.service" /etc/systemd/system/tur-report-receiver.service
systemctl daemon-reload
systemctl enable tur-report-receiver.service >/dev/null
systemctl restart tur-report-receiver.service

answer=""
for _ in $(seq 1 60); do
    answer="$(curl -s --max-time 2 "$LOCAL/v1/reports/$NO_TOKEN" || true)"
    case "$answer" in *TUR-DIAG-RECEIVER-011*) break ;; esac
    sleep 1
done
case "$answer" in
    *TUR-DIAG-RECEIVER-011*) say "  listening on 127.0.0.1:8470" ;;
    *) journalctl -u tur-report-receiver.service -n 30 --no-pager >&2 || true
       fail "the receiver does not answer on 127.0.0.1:8470" ;;
esac

say "Caddy for https://$domain"
if [ -n "$tls_cert" ]; then
    install -d -m 0750 -o root -g caddy /etc/caddy/tls
    install -m 0644 -o root -g caddy "$tls_cert" "/etc/caddy/tls/$domain.crt"
    install -m 0640 -o root -g caddy "$tls_key" "/etc/caddy/tls/$domain.key"
    tls_line="tls /etc/caddy/tls/$domain.crt /etc/caddy/tls/$domain.key"
else
    tls_line=""
fi
sed -e "s|^reports.example.org {|$domain {|" \
    -e "s|^\t# tls: written by install.sh.*|\t$tls_line|" \
    "$HERE/Caddyfile" > /etc/caddy/Caddyfile.new
caddy validate --adapter caddyfile --config /etc/caddy/Caddyfile.new >/dev/null
mv -f /etc/caddy/Caddyfile.new /etc/caddy/Caddyfile
systemctl enable caddy.service >/dev/null
systemctl restart caddy.service

say "Checking https://$domain"
reached=0
for _ in $(seq 1 90); do
    answer="$(curl -s --max-time 5 "https://$domain/v1/reports/$NO_TOKEN" || true)"
    case "$answer" in *TUR-DIAG-RECEIVER-011*) reached=1; break ;; esac
    sleep 2
done

cat <<EOF

TUR's problem report receiver is installed.
  Service:   systemctl status tur-report-receiver   (log: journalctl -u tur-report-receiver)
  Address:   https://$domain
  Files:     $ETC (configuration, key $current_key, token), $DATA (reports, encrypted)
  Filing to: $repository
On a TUR computer, /etc/tur/problem-reports.conf (or the image's
/usr/lib/tur/problem-reports.conf) names it:
  RECEIVER_URL=https://$domain
To update: run this command again with the new --image.
EOF

if [ "$reached" -eq 0 ]; then
    printf '\ninstall.sh: the receiver runs, but https://%s did not answer yet.\n' "$domain" >&2
    printf 'Check that the name points at this server and that ports 80 and 443 are open,\n' >&2
    printf 'then: journalctl -u caddy -n 50. Running this command again is safe.\n' >&2
    exit 3
fi
say "https://$domain answers"
