#!/usr/bin/env bash
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# Download Microsoft's evaluation medium from Microsoft's own server onto the
# runner's temporary disk, and record its name, size and SHA-256.
#
#   fetch-windows.sh URL DEST.iso MEDIA.json
#
# The medium is never committed, cached or uploaded: it stays in DEST and
# dies with the runner. Only MEDIA.json (facts about it) is kept.
set -euo pipefail
url="$1"; dest="$2"; facts="$3"

final=$(curl -sIL -o /dev/null -w '%{url_effective}' --max-time 60 "$url")
host=$(printf '%s' "$final" | sed -E 's#^https?://([^/]+)/.*#\1#')
case "$host" in
  *.microsoft.com) ;;
  *) echo "refusing: the link leads to $host, not to a microsoft.com server" >&2; exit 2 ;;
esac
echo "medium: $final"
mkdir -p "$(dirname "$dest")"
start=$(date +%s)
curl -L --fail --retry 5 --retry-delay 10 -C - -o "$dest" "$final"
secs=$(( $(date +%s) - start ))
bytes=$(stat -c %s "$dest")
sha=$(sha256sum "$dest" | cut -d' ' -f1)
python3 - "$url" "$final" "$bytes" "$sha" "$secs" > "$facts" <<'PY'
import json, sys, time
url, final, size, sha, secs = sys.argv[1:6]
print(json.dumps({
    "link": url, "resolved": final, "name": final.rsplit("/", 1)[-1],
    "bytes": int(size), "sha256": sha, "download_seconds": int(secs),
    "fetched_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
}, indent=2))
PY
cat "$facts"
