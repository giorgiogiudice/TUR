#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""What F-Droid applications are made of, without downloading them.

An APK is a ZIP archive, and a ZIP archive lists its files in a central
directory at its end. Two HTTP range requests fetch that directory, a few
tens of kilobytes, and the file names say which framework the application
is built with: Flutter, React Native, Unity, Godot, a web view around
bundled web assets (Cordova, Capacitor, or a plain index.html), or the
Android toolkit itself.

    payload.py classify INDEX N SEED OUT_DIR   classify N applications drawn at random
    payload.py rehost OUT_DIR WORK_DIR [MAX]   download up to MAX web-bundle applications found by
                                               classify, unpack their web assets, and serve each on
                                               127.0.0.1 for a browser to screenshot (the workflow
                                               runs the browser)

Runs in CI only (AGENTS.md). Standard library only.
"""

from __future__ import annotations

import collections
import hashlib
import http.server
import json
import math
import pathlib
import random
import re
import struct
import sys
import threading
import time
import urllib.error
import urllib.request
import zipfile

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
import fdroid_index  # noqa: E402

UA = fdroid_index.UA
REPO = fdroid_index.REPO


def ranged(url: str, spec: str) -> tuple[bytes, int]:
    """Bytes of a range request and the total size of the file."""
    last = None
    for attempt in range(4):
        try:
            req = urllib.request.Request(url, headers={"User-Agent": UA, "Range": f"bytes={spec}"})
            with urllib.request.urlopen(req, timeout=60) as r:
                data = r.read()
                total = int(r.headers.get("Content-Range", "bytes 0-0/0").rsplit("/", 1)[1] or 0)
                if r.status != 206:
                    raise RuntimeError(f"status {r.status}: the server ignored the range")
                return data, total
        except urllib.error.HTTPError as e:
            if e.code in (404, 416):
                raise
            last = e
        except Exception as e:  # noqa: BLE001
            last = e
        time.sleep(2 * (attempt + 1))
    raise RuntimeError(f"range {spec} of {url}: {last}")


def central_directory(url: str) -> tuple[list[tuple[str, int, int]], int]:
    """(name, compressed size, uncompressed size) of every file in a remote ZIP, and bytes fetched."""
    tail, total = ranged(url, "-65536")
    fetched = len(tail)
    pos = tail.rfind(b"PK\x05\x06")
    if pos < 0:
        raise RuntimeError("no end-of-central-directory record in the last 64 KiB")
    _sig, _d, _cd, _n_here, n, cd_size, cd_offset, _clen = struct.unpack_from("<IHHHHIIH", tail, pos)
    if cd_offset == 0xFFFFFFFF or n == 0xFFFF:
        raise RuntimeError("ZIP64 archive")
    tail_start = total - len(tail)
    if cd_offset >= tail_start:
        cd = tail[cd_offset - tail_start: cd_offset - tail_start + cd_size]
    else:
        cd, _ = ranged(url, f"{cd_offset}-{cd_offset + cd_size - 1}")
        fetched += len(cd)
    out = []
    p = 0
    while p + 46 <= len(cd) and cd[p:p + 4] == b"PK\x01\x02":
        comp, uncomp, nlen, elen, clen = struct.unpack_from("<IIHHH", cd, p + 20)
        name = cd[p + 46:p + 46 + nlen].decode("utf-8", "replace")
        out.append((name, comp, uncomp))
        p += 46 + nlen + elen + clen
    if len(out) != n:
        raise RuntimeError(f"central directory: parsed {len(out)} of {n} entries")
    return out, fetched


WEB_EXT = (".html", ".htm", ".js", ".mjs", ".css", ".wasm")
MARKERS = [
    # (label, test over the set of names and the list of base names)
    ("flutter", lambda names, base: "libflutter.so" in base or any(n.startswith("assets/flutter_assets/") for n in names)),
    ("react-native", lambda names, base: bool(base & {"libreactnativejni.so", "libreactnative.so", "libhermes.so", "libhermesvm.so", "libjsc.so", "libjscexecutor.so"}) or "assets/index.android.bundle" in names),
    ("unity", lambda names, base: "libunity.so" in base),
    ("godot", lambda names, base: "libgodot_android.so" in base),
    ("cordova", lambda names, base: "assets/www/cordova.js" in names),
    ("capacitor", lambda names, base: bool({"assets/capacitor.config.json", "assets/native-bridge.js", "assets/public/cordova.js"} & names)),
    ("xamarin-maui", lambda names, base: bool(base & {"libmonodroid.so", "libmonosgen-2.0.so", "libxamarin-app.so"})),
    ("qt", lambda names, base: any(b.startswith(("libQt5Core", "libQt6Core")) for b in base)),
    ("python", lambda names, base: any(b.startswith("libpython3") for b in base)),
    ("libgdx", lambda names, base: "libgdx.so" in base),
    ("sdl", lambda names, base: bool(base & {"libSDL2.so", "libSDL3.so"})),
    ("love2d", lambda names, base: "liblove.so" in base),
    ("gecko", lambda names, base: "libxul.so" in base),
    ("compose", lambda names, base: any(n.startswith("META-INF/androidx.compose.ui_ui") for n in names)),
    ("kotlin", lambda names, base: any(n.startswith("kotlin/") or n.endswith(".kotlin_module") for n in names)),
]
PRIMARY_ORDER = ["flutter", "react-native", "unity", "godot", "cordova", "capacitor", "web-bundle",
                 "xamarin-maui", "qt", "python", "libgdx", "sdl", "love2d", "gecko"]
PORTABLE = {"flutter", "react-native", "unity", "godot", "cordova", "capacitor", "web-bundle",
            "xamarin-maui", "qt", "python", "libgdx", "sdl", "love2d"}


def classify_entries(entries: list[tuple[str, int, int]]) -> dict:
    names = {e[0] for e in entries}
    base = {n.rsplit("/", 1)[-1] for n in names if n.startswith("lib/")}
    marks = [label for label, test in MARKERS if test(names, base)]
    web = [(n, u) for n, _c, u in entries
           if n.startswith("assets/") and not n.startswith("assets/flutter_assets/") and n.lower().endswith(WEB_EXT)]
    index_pages = sorted(n for n, _u in web if n.lower().endswith("/index.html"))
    web_bytes = sum(u for _n, u in web)
    js_bytes = sum(u for n, u in web if n.lower().endswith((".js", ".mjs", ".wasm")))
    dex_bytes = sum(u for n, _c, u in entries if re.fullmatch(r"classes\d*\.dex", n))
    native_bytes = sum(u for n, _c, u in entries if n.startswith("lib/") and n.endswith(".so"))
    # A web bundle: an index.html in the assets together with script of some
    # size. A help page or a licence page is HTML without much script.
    if index_pages and js_bytes >= 20_000 and "cordova" not in marks and "capacitor" not in marks:
        marks.append("web-bundle")
    primary = next((m for m in PRIMARY_ORDER if m in marks), None)
    if primary is None:
        primary = "android-compose" if "compose" in marks else "android-views"
    return {"marks": marks, "primary": primary, "index_pages": index_pages[:5], "web_bytes": web_bytes,
            "js_bytes": js_bytes, "dex_bytes": dex_bytes, "native_bytes": native_bytes,
            "files": len(entries), "abis": sorted({n.split("/")[1] for n in names if n.startswith("lib/") and n.count("/") >= 2})}


def wilson(k: int, n: int) -> tuple[float, float]:
    if n == 0:
        return 0.0, 0.0
    z = 1.96
    p = k / n
    d = 1 + z * z / n
    c = p + z * z / (2 * n)
    w = z * math.sqrt(p * (1 - p) / n + z * z / (4 * n * n))
    return max(0.0, (c - w) / d), min(1.0, (c + w) / d)


def cmd_classify(index_path: str, n: str, seed: str, out_dir: str) -> int:
    out = pathlib.Path(out_dir)
    out.mkdir(parents=True, exist_ok=True)
    idx = fdroid_index.load(index_path)
    names = sorted(idx["packages"])
    random.Random(seed).shuffle(names)
    results, failed, fetched_total = [], [], 0
    for name in names:
        if len(results) >= int(n):
            break
        row = fdroid_index.lock_row(name, "payload", idx["packages"][name], "x86_64")
        if row is None:
            continue
        try:
            entries, fetched = central_directory(f"{REPO}/{row['file']}")
        except Exception as e:  # noqa: BLE001 - one unreadable APK must not stop the survey
            failed.append({"package": name, "error": str(e)[:200]})
            continue
        fetched_total += fetched
        info = classify_entries(entries)
        info.update({"package": name, "version_name": row["version_name"], "version_code": row["version_code"],
                     "sha256": row["sha256"], "size": int(row["size"]), "file": row["file"],
                     "categories": idx["packages"][name].get("metadata", {}).get("categories") or []})
        results.append(info)
        time.sleep(0.05)
    total = len(results)
    (out / "payload-apps.json").write_text(json.dumps(results, indent=1) + "\n")

    def share(k: int) -> str:
        lo, hi = wilson(k, total)
        return f"{100 * k / total:.1f}% ({100 * lo:.1f}–{100 * hi:.1f})" if total else "–"

    primary = collections.Counter(r["primary"] for r in results)
    marks = collections.Counter(m for r in results for m in r["marks"])
    portable = sum(1 for r in results if r["primary"] in PORTABLE)
    web = [r for r in results if r["primary"] in ("cordova", "capacitor", "web-bundle")]
    md = ["## What F-Droid applications are made of\n",
          f"{total} applications drawn with `random.Random({seed!r}).shuffle` over the sorted package names "
          f"of the index; the newest release's APK of each (x86-64 where there is one per ABI). Read from the "
          f"ZIP central directory by HTTP range requests: {fetched_total // 1024} KiB fetched in all, "
          f"{len(failed)} APKs could not be read. Shares are followed by their 95% Wilson interval.\n",
          fdroid_index.table(["Built with (first match)", "Apps", "Share (95% interval)"],
                             [[k, v, share(v)] for k, v in primary.most_common()]),
          f"Portable payloads (everything except the Android toolkit itself and Gecko): {portable} of {total}, {share(portable)}.\n",
          f"A web view around bundled web assets (Cordova, Capacitor, or an `index.html` with at least 20 kB of "
          f"script in the assets): {len(web)} of {total}, {share(len(web))}.\n",
          "Markers, not exclusive:\n",
          fdroid_index.table(["Marker", "Apps", "Share (95% interval)"], [[k, v, share(v)] for k, v in marks.most_common()]),
          "### The web-bundle applications found\n",
          fdroid_index.table(["Package", "Version", "Kind", "APK", "Web assets", "Dex", "Native", "Entry page"],
                             [[r["package"], r["version_name"], r["primary"], f"{r['size'] // 1024} KiB",
                               f"{r['web_bytes'] // 1024} KiB", f"{r['dex_bytes'] // 1024} KiB",
                               f"{r['native_bytes'] // 1024} KiB", (r["index_pages"] or ["–"])[0]] for r in web])]
    (out / "payload.md").write_text("\n".join(md))
    (out / "payload.json").write_text(json.dumps({"apps": total, "seed": seed, "primary": dict(primary), "marks": dict(marks),
                                                  "portable": portable, "web": len(web), "failed": failed,
                                                  "bytes_fetched": fetched_total}, indent=2) + "\n")
    print("\n".join(md))
    return 0


# ---------------------------------------------------------------- re-hosting

SHIM = b"""// TUR experiment: a stand-in for cordova.js. It gives the page the objects
// it looks for and fires deviceready; plugin calls fail through their error
// callbacks.
(function () {
  var c = { platformId: 'browser', version: '0', plugins: {}, define: function () {}, require: function () { return {}; },
            exec: function (ok, fail) { if (typeof fail === 'function') { setTimeout(function () { fail('TUR: no native bridge'); }, 0); } },
            fireDocumentEvent: function (n) { document.dispatchEvent(new Event(n)); } };
  window.cordova = window.cordova || c;
  window.device = window.device || { platform: 'browser', model: 'TUR', uuid: 'tur', version: '0', cordova: '0' };
  function ready() { setTimeout(function () { document.dispatchEvent(new Event('deviceready')); }, 50); }
  if (document.readyState === 'loading') { document.addEventListener('DOMContentLoaded', ready); } else { ready(); }
})();
"""


def make_handler(root: str, shim: bool):
    class Handler(http.server.SimpleHTTPRequestHandler):
        def __init__(self, *a, **kw):
            super().__init__(*a, directory=root, **kw)

        def do_GET(self):  # noqa: N802 - the base class's name
            path = self.path.split("?", 1)[0]
            if shim and path.rsplit("/", 1)[-1] in ("cordova.js", "cordova_plugins.js", "capacitor.js"):
                body = SHIM if not path.endswith("cordova_plugins.js") else b""
                self.send_response(200)
                self.send_header("Content-Type", "application/javascript")
                self.send_header("Content-Length", str(len(body)))
                self.end_headers()
                self.wfile.write(body)
                return
            super().do_GET()

        def log_message(self, fmt, *args):
            with open(pathlib.Path(root).parent / ("requests-shim.log" if shim else "requests.log"), "a") as f:
                f.write(fmt % args + "\n")
    return Handler


def cmd_rehost(out_dir: str, work_dir: str, max_apps: str = "3") -> int:
    """Unpack the candidates and serve them until a file named `stop` appears in WORK_DIR."""
    out, work = pathlib.Path(out_dir), pathlib.Path(work_dir)
    work.mkdir(parents=True, exist_ok=True)
    apps = json.loads((out / "payload-apps.json").read_text())
    cands = [a for a in apps if a["primary"] in ("cordova", "capacitor", "web-bundle") and a["size"] <= 60 * 1024 * 1024]
    # Hybrid-framework applications first, then the largest share of web assets.
    cands.sort(key=lambda a: (a["primary"] == "web-bundle", -a["web_bytes"] / max(1, a["dex_bytes"])))
    served = []
    port = 8600
    for a in cands[: int(max_apps)]:
        d = work / a["package"]
        d.mkdir(parents=True, exist_ok=True)
        apk = d / "app.apk"
        try:
            fdroid_index.http_get(f"{REPO}/{a['file']}", apk)
        except Exception as e:  # noqa: BLE001
            print(f"{a['package']}: download failed: {e}")
            continue
        digest = hashlib.sha256(apk.read_bytes()).hexdigest()
        if digest != a["sha256"]:
            print(f"{a['package']}: sha256 mismatch")
            continue
        entry = a["index_pages"][0] if a["index_pages"] else "assets/www/index.html"
        site_root = entry.rsplit("/", 1)[0]
        site = d / "site"
        with zipfile.ZipFile(apk) as z:
            for info in z.infolist():
                if info.filename.startswith(site_root + "/") and not info.is_dir() and ".." not in info.filename:
                    target = site / info.filename[len(site_root) + 1:]
                    target.parent.mkdir(parents=True, exist_ok=True)
                    target.write_bytes(z.read(info))
        row = {"package": a["package"], "version_name": a["version_name"], "sha256": a["sha256"], "kind": a["primary"],
               "entry": entry, "files": sum(1 for _ in site.rglob("*") if _.is_file()), "ports": {}}
        for shim in (False, True):
            srv = http.server.ThreadingHTTPServer(("127.0.0.1", port), make_handler(str(site), shim))
            threading.Thread(target=srv.serve_forever, daemon=True).start()
            row["ports"]["shim" if shim else "plain"] = port
            port += 1
        served.append(row)
    (work / "served.json").write_text(json.dumps(served, indent=2) + "\n")
    print(json.dumps(served, indent=2))
    deadline = time.time() + 900
    while time.time() < deadline and not (work / "stop").exists():
        time.sleep(1)
    return 0


def main(argv: list[str]) -> int:
    if len(argv) >= 6 and argv[1] == "classify":
        return cmd_classify(*argv[2:6])
    if len(argv) >= 4 and argv[1] == "rehost":
        return cmd_rehost(*argv[2:5])
    print(__doc__)
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv))
