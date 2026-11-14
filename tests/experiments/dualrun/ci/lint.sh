#!/usr/bin/env bash
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#
# The quick job: syntax, SPDX lines, then the synthetic tests (volumes made
# with mkfs.ntfs and mkfs.vfat, device-mapper views on loop devices). No KVM
# and no Microsoft software.
#
#   lint.sh OUT_DIR
set -euo pipefail
here="$(cd "$(dirname "$0")/.." && pwd)"
out="${1:?out directory}"
mkdir -p "$out"

echo "== syntax"
python3 -m compileall -q "$here"
if [ -f "$here/tur-dualrun" ]; then python3 -m py_compile "$here/tur-dualrun"; fi
find "$here" -name '__pycache__' -prune -exec rm -rf {} +
for f in "$here"/ci/*.sh; do bash -n "$f"; done

echo "== SPDX lines"
missing=0
while IFS= read -r f; do
  if ! head -n 3 "$f" | grep -q 'SPDX-License-Identifier:'; then
    echo "no SPDX line: $f"; missing=1
  fi
done < <(find "$here" -type f ! -name '*.pyc' ! -path '*/__pycache__/*')
[ "$missing" = 0 ]

echo "== synthetic tests"
if [ -f "$here/ci/synthetic.py" ]; then
  sudo -E python3 "$here/ci/synthetic.py" "$out"
  sudo chown -R "$(id -u):$(id -g)" "$out"
else
  echo "not present yet"
fi
