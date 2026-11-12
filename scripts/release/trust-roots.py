#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""Write Sigstore's public-good trust roots as the files the TUR image ships.

    scripts/release/trust-roots.py TRUSTED_ROOT_JSON OUT_DIR
    scripts/release/trust-roots.py TRUSTED_ROOT_JSON --check DIR

TRUSTED_ROOT_JSON is Sigstore's `trusted_root.json` as its TUF repository
distributes it (cosign keeps the verified copy under ~/.sigstore/root after
`cosign initialize`). Two files are written:

- `rekor.pub`: the public key of the Rekor transparency log at
  https://rekor.sigstore.dev, the log whose signed entry timestamps
  containers/image can check offline (`rekorPublicKeyPath` in
  containers-policy.json(5)). Rekor v2 logs (`log20xx-N.rekor.sigstore.dev`)
  give no signed entry timestamp and are left out.
- `fulcio.pem`: the certificate chain of the Fulcio instance at
  https://fulcio.sigstore.dev that is valid now (no end date), intermediate
  first, then root (`caPath` of a `fulcio` requirement).

With --check, nothing is written: the files in DIR must be byte-for-byte what
would be written, or the exit status is 1. The release workflow runs this
against cosign's TUF-verified copy, so the files committed in the image are
the ones Sigstore's signed root names.
"""

from __future__ import annotations

import base64
import json
import pathlib
import sys

REKOR = "https://rekor.sigstore.dev"
FULCIO = "https://fulcio.sigstore.dev"


def pem(kind: str, der_b64: str) -> str:
    body = base64.b64encode(base64.b64decode(der_b64)).decode()
    lines = [body[i:i + 64] for i in range(0, len(body), 64)]
    return f"-----BEGIN {kind}-----\n" + "\n".join(lines) + f"\n-----END {kind}-----\n"


def render(root: dict) -> dict[str, str]:
    logs = [t for t in root.get("tlogs", []) if t.get("baseUrl") == REKOR]
    if len(logs) != 1:
        sys.exit(f"expected one transparency log at {REKOR}, found {len(logs)}")
    key = logs[0]["publicKey"]
    if key.get("keyDetails") != "PKIX_ECDSA_P256_SHA_256" or "end" in key.get("validFor", {}):
        sys.exit(f"the key of {REKOR} is not a current ECDSA P-256 key: {key.get('keyDetails')} {key.get('validFor')}")
    cas = [c for c in root.get("certificateAuthorities", [])
           if c.get("uri") == FULCIO and "end" not in c.get("validFor", {})]
    if len(cas) != 1:
        sys.exit(f"expected one current certificate authority at {FULCIO}, found {len(cas)}")
    chain = cas[0]["certChain"]["certificates"]
    return {
        "rekor.pub": pem("PUBLIC KEY", key["rawBytes"]),
        "fulcio.pem": "".join(pem("CERTIFICATE", c["rawBytes"]) for c in chain),
    }


def main() -> int:
    if len(sys.argv) == 4 and sys.argv[2] == "--check":
        files, check = render(json.loads(pathlib.Path(sys.argv[1]).read_text())), pathlib.Path(sys.argv[3])
        wrong = [name for name, text in files.items()
                 if not (check / name).exists() or (check / name).read_text() != text]
        for name in sorted(files):
            print(f"{name}: {'DIFFERS from' if name in wrong else 'matches'} Sigstore's trusted root")
        return 1 if wrong else 0
    if len(sys.argv) != 3:
        sys.exit(__doc__)
    out = pathlib.Path(sys.argv[2])
    out.mkdir(parents=True, exist_ok=True)
    for name, text in render(json.loads(pathlib.Path(sys.argv[1]).read_text())).items():
        (out / name).write_text(text)
        print(f"wrote {out / name}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
