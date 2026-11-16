#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""F-Droid's index as data for the Android experiment (docs/reports/exp-android-1.md).

    fdroid_index.py fetch DIR [REPO_URL]           download entry.json and index-v2.json, verify the hash
                                                   (REPO_URL: another repository in F-Droid's format)
    fdroid_index.py stats INDEX OUT_DIR            native-ABI shares by category and recency (hypothesis B)
    fdroid_index.py resolve INDEX SAMPLE OUT.tsv [--prefer-abi ABI]
                                                   pin a list of packages to version, file and SHA-256
    fdroid_index.py random INDEX N SEED OUT.tsv    pin N packages drawn at random with a fixed seed
    fdroid_index.py download LOCK.tsv DIR          download the pinned APKs and verify each SHA-256

Runs in CI only (AGENTS.md). Standard library only.
"""

from __future__ import annotations

import collections
import datetime
import hashlib
import json
import os
import pathlib
import random
import sys
import time
import urllib.error
import urllib.request

REPO = "https://f-droid.org/repo"
ARCHIVE = "https://f-droid.org/archive"
UA = "TUR-experiment-android/1 (CI; F-Droid index statistics)"
ARM = {"arm64-v8a", "armeabi-v7a", "armeabi"}
LOCK_COLUMNS = ["package", "group", "version_name", "version_code", "sha256", "size", "file",
                "nativecode", "abi_class", "min_sdk", "target_sdk", "name"]


def http_get(url: str, dest: pathlib.Path | None = None, tries: int = 4) -> bytes | None:
    last = None
    for attempt in range(tries):
        try:
            req = urllib.request.Request(url, headers={"User-Agent": UA})
            with urllib.request.urlopen(req, timeout=120) as r:
                if dest is None:
                    return r.read()
                tmp = dest.with_suffix(dest.suffix + ".part")
                with open(tmp, "wb") as f:
                    while True:
                        chunk = r.read(1 << 20)
                        if not chunk:
                            break
                        f.write(chunk)
                tmp.replace(dest)
                return None
        except urllib.error.HTTPError as e:
            if e.code == 404:
                raise
            last = e
        except Exception as e:  # noqa: BLE001 - network errors of every kind are retried
            last = e
        time.sleep(3 * (attempt + 1))
    raise RuntimeError(f"GET {url} failed: {last}")


def sha256_file(path: pathlib.Path) -> str:
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


# ---------------------------------------------------------------- fetch

def cmd_fetch(dest_dir: str, repo: str = REPO) -> int:
    d = pathlib.Path(dest_dir)
    d.mkdir(parents=True, exist_ok=True)
    repo = repo.rstrip("/")
    entry = json.loads(http_get(f"{repo}/entry.json"))
    want = entry["index"]["sha256"]
    index = d / "index-v2.json"
    if not (index.exists() and sha256_file(index) == want):
        http_get(f"{repo}{entry['index']['name']}", index)
    got = sha256_file(index)
    if got != want:
        # The index is replaced a few times a week; a download that straddles
        # a replacement is retried once.
        entry = json.loads(http_get(f"{repo}/entry.json"))
        want = entry["index"]["sha256"]
        http_get(f"{repo}{entry['index']['name']}", index)
        got = sha256_file(index)
    if got != want:
        print(f"index-v2.json: sha256 {got}, entry.json says {want}", file=sys.stderr)
        return 1
    meta = {"index_sha256": got, "index_size": index.stat().st_size,
            "index_timestamp_ms": entry["timestamp"],
            "index_date": datetime.datetime.fromtimestamp(entry["timestamp"] / 1000, datetime.timezone.utc).isoformat(),
            "packages_in_entry": entry["index"].get("numPackages"), "source": f"{repo}/index-v2.json"}
    (d / "index-meta.json").write_text(json.dumps(meta, indent=2) + "\n")
    print(json.dumps(meta))
    return 0


# ---------------------------------------------------------------- model

def load(index_path: str) -> dict:
    with open(index_path, encoding="utf-8") as f:
        return json.load(f)


def text(value, default="") -> str:
    """A localised string of the index ({"en-US": "..."}) as plain text."""
    if isinstance(value, dict):
        return value.get("en-US") or next(iter(value.values()), default)
    return value or default


def versions_of(pkg: dict) -> list[dict]:
    out = []
    for v in pkg.get("versions", {}).values():
        man = v.get("manifest", {})
        out.append({
            "code": man.get("versionCode", 0), "name": man.get("versionName", ""),
            "nativecode": sorted(man.get("nativecode", []) or []),
            "file": v.get("file", {}).get("name", ""), "sha256": v.get("file", {}).get("sha256", ""),
            "size": v.get("file", {}).get("size", 0), "added": v.get("added", 0),
            "beta": "Beta" in (v.get("releaseChannels") or []),
            "min_sdk": (man.get("usesSdk") or {}).get("minSdkVersion"),
            "target_sdk": (man.get("usesSdk") or {}).get("targetSdkVersion"),
            "anti": sorted((v.get("antiFeatures") or {}).keys()),
        })
    out.sort(key=lambda x: x["code"], reverse=True)
    return out


def latest_group(pkg: dict) -> list[dict]:
    """The newest release: F-Droid publishes one APK per ABI for many
    applications, as separate version codes with one version name. The group
    is every APK that shares the newest non-beta version name."""
    vs = versions_of(pkg)
    if not vs:
        return []
    stable = [v for v in vs if not v["beta"]] or vs
    head = stable[0]
    return [v for v in stable if v["name"] == head["name"]]


def abi_class(abis: set[str]) -> str:
    if not abis:
        return "no native code"
    if "x86_64" in abis:
        return "x86-64 native code"
    if "x86" in abis:
        return "x86 32-bit only"
    if abis <= ARM:
        return "ARM only"
    return "other"


CLASSES = ["no native code", "x86-64 native code", "x86 32-bit only", "ARM only", "other"]


def pick_apk(group: list[dict], prefer: str = "x86_64") -> dict:
    for want in (prefer, None, "arm64-v8a"):
        for v in group:
            if want is None and not v["nativecode"]:
                return v
            if want is not None and want in v["nativecode"]:
                return v
    return group[0]


# ---------------------------------------------------------------- stats

def pct(n: int, d: int) -> str:
    return "–" if d == 0 else f"{100.0 * n / d:.1f}%"


def table(header: list[str], rows: list[list]) -> str:
    out = ["| " + " | ".join(header) + " |", "|" + "|".join("---" for _ in header) + "|"]
    out += ["| " + " | ".join(str(c) for c in r) + " |" for r in rows]
    return "\n".join(out) + "\n"


def cmd_stats(index_path: str, out_dir: str) -> int:
    out = pathlib.Path(out_dir)
    out.mkdir(parents=True, exist_ok=True)
    idx = load(index_path)
    apps = []
    for name, pkg in idx["packages"].items():
        group = latest_group(pkg)
        if not group:
            continue
        union = set()
        for v in group:
            union |= set(v["nativecode"])
        every = set()
        for v in versions_of(pkg):
            every |= set(v["nativecode"])
        md = pkg.get("metadata", {})
        head = group[0]
        apps.append({
            "package": name, "categories": md.get("categories") or ["(none)"],
            "last_updated": md.get("lastUpdated", 0), "added": md.get("added", 0),
            "abis": sorted(union), "class": abi_class(union), "class_all_versions": abi_class(every),
            "apks_in_group": len(group), "min_sdk": head["min_sdk"], "target_sdk": head["target_sdk"],
            "size": pick_apk(group)["size"], "anti": head["anti"],
        })
    total = len(apps)
    (out / "abi-apps.json").write_text(json.dumps(apps, separators=(",", ":")) + "\n")

    def counts(sel):
        c = collections.Counter(a["class"] for a in sel)
        return [c.get(k, 0) for k in CLASSES]

    def row(label, sel):
        c = counts(sel)
        n = len(sel)
        runnable = c[0] + c[1]
        native = n - c[0]
        return [label, n, pct(c[0], n), pct(c[1], n), pct(runnable, n), pct(c[2], n), pct(c[3], n),
                pct(c[1], native) if native else "–"]

    header = ["", "Apps", "No native code", "x86-64 native code", "**Runs without translation**",
              "x86 32-bit only", "ARM only", "Of apps with native code: has x86-64"]
    meta_path = pathlib.Path(index_path).with_name("index-meta.json")
    meta = json.loads(meta_path.read_text()) if meta_path.exists() else {}
    md = [f"## Native ABIs of the applications in {meta.get('source', index_path).split('/')[2] if '//' in meta.get('source', '') else 'the index'}\n"]
    md.append(f"Index: `{meta.get('source', index_path)}`, sha256 `{meta.get('index_sha256', '?')}`, "
              f"dated {meta.get('index_date', '?')}; {total} applications with at least one APK. "
              "The newest non-beta release of each application is counted, taking the union of the "
              "ABIs of every APK of that release (F-Droid publishes one APK per ABI for many "
              "applications).\n")
    md.append(table(header, [row("**All**", apps)]))
    recent = [a for a in apps if a["last_updated"] >= (meta.get("index_timestamp_ms") or 0) - 730 * 86400 * 1000]
    md.append("Applications updated in the two years before the index's date:\n")
    md.append(table(header, [row("Updated in the last two years", recent)]))

    c_all = counts(apps)
    arm = [a for a in apps if a["class"] == "ARM only"]
    arm64 = sum(1 for a in arm if "arm64-v8a" in a["abis"])
    md.append(f"ARM-only applications: {len(arm)}; {arm64} of them have 64-bit ARM code, "
              f"{len(arm) - arm64} have 32-bit ARM code only.\n")
    alt = collections.Counter(a["class_all_versions"] for a in apps)
    md.append("Sensitivity (the union over every version still in the index, not only the newest "
              "release): " + ", ".join(f"{k} {alt.get(k, 0)} ({pct(alt.get(k, 0), total)})" for k in CLASSES) + ".\n")

    md.append("### By category\n")
    md.append("An application is counted in each of its categories.\n")
    by_cat = collections.defaultdict(list)
    for a in apps:
        for c in a["categories"]:
            by_cat[c].append(a)
    md.append(table(header, [row(c, by_cat[c]) for c in sorted(by_cat, key=lambda k: -len(by_cat[k]))]))

    md.append("### By year of last update\n")
    by_year = collections.defaultdict(list)
    for a in apps:
        y = datetime.datetime.fromtimestamp(a["last_updated"] / 1000, datetime.timezone.utc).year if a["last_updated"] else 0
        by_year[y].append(a)
    md.append(table(header, [row(str(y), by_year[y]) for y in sorted(by_year, reverse=True)]))

    md.append("### By year added to F-Droid\n")
    by_added = collections.defaultdict(list)
    for a in apps:
        y = datetime.datetime.fromtimestamp(a["added"] / 1000, datetime.timezone.utc).year if a["added"] else 0
        by_added[y].append(a)
    md.append(table(header, [row(str(y), by_added[y]) for y in sorted(by_added, reverse=True)]))

    md.append("### Minimum Android version\n")
    md.append("Waydroid's official image is Android 13 (API level 33). An application whose minimum "
              "API level is higher cannot be installed in it.\n")
    buckets = collections.Counter()
    for a in apps:
        m = a["min_sdk"] or 1
        buckets["over 33" if m > 33 else "30 to 33" if m >= 30 else "26 to 29" if m >= 26 else "21 to 25" if m >= 21 else "under 21"] += 1
    md.append(table(["Minimum API level", "Apps", "Share"],
                    [[k, buckets.get(k, 0), pct(buckets.get(k, 0), total)]
                     for k in ["under 21", "21 to 25", "26 to 29", "30 to 33", "over 33"]]))
    tgt = collections.Counter()
    for a in apps:
        t = a["target_sdk"] or 0
        tgt["35 or newer" if t >= 35 else "34" if t == 34 else "33" if t == 33 else "30 to 32" if t >= 30 else "under 30"] += 1
    md.append(table(["Target API level", "Apps", "Share"],
                    [[k, tgt.get(k, 0), pct(tgt.get(k, 0), total)]
                     for k in ["under 30", "30 to 32", "33", "34", "35 or newer"]]))

    md.append("### Anti-features recorded by F-Droid (newest release)\n")
    anti = collections.Counter()
    for a in apps:
        for k in a["anti"]:
            anti[k] += 1
    md.append(table(["Anti-feature", "Apps", "Share"], [[k, n, pct(n, total)] for k, n in anti.most_common()]))

    summary = {"index": meta, "apps": total, "classes": dict(zip(CLASSES, c_all)),
               "arm_only_with_arm64": arm64, "arm_only_32bit_only": len(arm) - arm64,
               "runs_without_translation": c_all[0] + c_all[1],
               "by_category": {c: dict(zip(CLASSES, counts(v))) for c, v in by_cat.items()},
               "by_year_updated": {str(y): dict(zip(CLASSES, counts(v))) for y, v in by_year.items()},
               "min_sdk": dict(buckets), "target_sdk": dict(tgt), "anti_features": dict(anti)}
    (out / "abi.json").write_text(json.dumps(summary, indent=2) + "\n")
    (out / "abi.md").write_text("\n".join(md))
    print("\n".join(md))
    return 0


# ---------------------------------------------------------------- locks

def lock_row(name: str, group_label: str, pkg: dict, prefer: str) -> dict | None:
    group = latest_group(pkg)
    if not group:
        return None
    v = pick_apk(group, prefer)
    return {"package": name, "group": group_label, "version_name": v["name"], "version_code": v["code"],
            "sha256": v["sha256"], "size": v["size"], "file": v["file"].lstrip("/"),
            "nativecode": ",".join(v["nativecode"]) or "-", "abi_class": abi_class(set(v["nativecode"])),
            "min_sdk": v["min_sdk"] or "", "target_sdk": v["target_sdk"] or "",
            "name": text(pkg.get("metadata", {}).get("name"), name).replace("\t", " ")}


def write_lock(path: str, rows: list[dict], note: str) -> None:
    with open(path, "w", encoding="utf-8") as f:
        f.write("# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0\n")
        f.write(f"# {note}\n")
        f.write("\t".join(LOCK_COLUMNS) + "\n")
        for r in rows:
            f.write("\t".join(str(r[c]) for c in LOCK_COLUMNS) + "\n")


def read_lock(path: str) -> list[dict]:
    rows = []
    header = None
    with open(path, encoding="utf-8") as f:
        for line in f:
            line = line.rstrip("\n")
            if not line or line.startswith("#"):
                continue
            cells = line.split("\t")
            if header is None:
                header = cells
                continue
            rows.append(dict(zip(header, cells)))
    return rows


def index_note(index_path: str) -> str:
    meta_path = pathlib.Path(index_path).with_name("index-meta.json")
    if meta_path.exists():
        m = json.loads(meta_path.read_text())
        return f"F-Droid index sha256 {m['index_sha256']} dated {m['index_date']}"
    return "F-Droid index"


def cmd_resolve(index_path: str, sample: str, out_path: str, prefer: str) -> int:
    idx = load(index_path)
    rows, missing = [], []
    with open(sample, encoding="utf-8") as f:
        for line in f:
            line = line.split("#", 1)[0].strip()
            if not line:
                continue
            name, _, label = line.partition(" ")
            pkg = idx["packages"].get(name)
            row = lock_row(name, label.strip() or "-", pkg, prefer) if pkg else None
            if row is None:
                missing.append(name)
            else:
                rows.append(row)
    write_lock(out_path, rows, f"Sample pinned from {index_note(index_path)}; preferred ABI {prefer}.")
    print(f"resolved {len(rows)} packages; missing: {missing or 'none'}")
    return 0


def cmd_random(index_path: str, n: str, seed: str, out_path: str) -> int:
    idx = load(index_path)
    names = sorted(idx["packages"])
    rng = random.Random(seed)
    rng.shuffle(names)
    rows = []
    for name in names:
        row = lock_row(name, "random", idx["packages"][name], "x86_64")
        # An APK above 120 MB is skipped to keep the job light; the number
        # skipped is printed.
        if row is None or int(row["size"]) > 120 * 1024 * 1024:
            print(f"skipped {name}: {'no APK' if row is None else 'APK over 120 MB'}")
            continue
        rows.append(row)
        if len(rows) == int(n):
            break
    write_lock(out_path, rows, f"{n} packages drawn with random.Random({seed!r}).shuffle over the sorted "
               f"package names of {index_note(index_path)}.")
    print(f"drew {len(rows)} packages")
    return 0


def cmd_download(lock: str, dest_dir: str) -> int:
    d = pathlib.Path(dest_dir)
    d.mkdir(parents=True, exist_ok=True)
    bad = 0
    for r in read_lock(lock):
        dest = d / f"{r['sha256']}.apk"
        if dest.exists() and sha256_file(dest) == r["sha256"]:
            continue
        for base in (REPO, ARCHIVE):
            try:
                http_get(f"{base}/{r['file']}", dest)
                break
            except urllib.error.HTTPError:
                continue
            except RuntimeError as e:
                print(f"{r['package']}: {e}", file=sys.stderr)
        if not dest.exists():
            print(f"{r['package']}: {r['file']} not found in the repository or the archive", file=sys.stderr)
            bad += 1
            continue
        got = sha256_file(dest)
        if got != r["sha256"]:
            print(f"{r['package']}: sha256 {got}, lock says {r['sha256']}", file=sys.stderr)
            dest.unlink()
            bad += 1
        else:
            print(f"{r['package']} {r['version_name']} ({int(r['size']) // 1024} KiB) ok")
    print(f"download finished; {bad} failed")
    return 0


def main(argv: list[str]) -> int:
    if len(argv) < 2:
        print(__doc__)
        return 2
    cmd, args = argv[1], argv[2:]
    if cmd == "fetch":
        return cmd_fetch(*args)
    if cmd == "stats":
        return cmd_stats(*args)
    if cmd == "resolve":
        prefer = "x86_64"
        if "--prefer-abi" in args:
            i = args.index("--prefer-abi")
            prefer = args[i + 1]
            args = args[:i] + args[i + 2:]
        return cmd_resolve(*args, prefer)
    if cmd == "random":
        return cmd_random(*args)
    if cmd == "download":
        return cmd_download(*args)
    print(__doc__)
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv))
