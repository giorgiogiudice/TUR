#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""Fail if Fedora's marks are in a TUR image (desktop-experience.md, 12.5).

    tests/image/marks.py IMAGE OUT_DIR [--base BASE_REF] [--layers OCI_LAYOUT[:REF]]

Looks at the built image from four sides and fails if any finds something
the allow-list (tests/image/marks-allowlist.tsv) does not account for:

- text: files (outside documentation, licence texts and translations) that
  contain the word "Fedora", in the image and inside its initramfs;
- names: files named after Fedora in the places artwork lives (icons, pixmaps,
  the boot splash, wallpapers, Plasma's themes, the installer's art);
- artwork: any file in those places, in /etc or in the initramfs whose
  SHA-256 is that of a file of Fedora's logo package `fedora-logos`, taken
  from the base image. This one has no allow-list;
- packages: Fedora's logo and release packages must not be installed.

Those four look at the image as a container sees it: the layers merged. A
file that a later layer deletes or replaces is still in the earlier layer,
and whoever pulls the image downloads it (desktop-experience.md, 12.1, item
3). With --layers, every layer of the same image, read from an OCI layout
(the published image, scripts/release/), is also checked, as the tar stream
it is:

- whiteouts: no layer may delete anything a layer below it holds;
- shadowed files: no file or link may be in more than one layer;
- artwork and names: the two checks above, on the files of each layer.

Without whiteouts and shadowed files, the layers hold exactly the files of
the merged image, so the text, initramfs and package checks of the merged
image hold for every layer as well. The base's own layers fail the first two
(Fedora's logos are in them), which is why the published image is re-chunked.

The allow-list is table C of section 12.4 written down: each line is a kind
(text or name), a path pattern and the reason it may stay. A pattern that
matched nothing is reported, so that the list stays as short as the image
allows. Writes OUT_DIR/marks.json and OUT_DIR/marks.md. PODMAN names the
container tool. Run in CI (ADR-0002).
"""

from __future__ import annotations

import argparse
import fnmatch
import gzip
import hashlib
import json
import os
import pathlib
import subprocess
import sys
import tarfile

ROOT = pathlib.Path(__file__).resolve().parent.parent.parent
ALLOWLIST = pathlib.Path(__file__).resolve().parent / "marks-allowlist.tsv"
FORBIDDEN_PACKAGES = [
    "fedora-logos", "fedora-logos-httpd", "fedora-release", "fedora-release-common", "fedora-release-kinoite",
    "fedora-release-identity-kinoite", "fedora-release-ostree-desktop", "fedora-release-notes",
    "fedora-bookmarks", "plasma-welcome-fedora",
]
# Where artwork lives: file names are checked here.
ART_DIRS = ["/usr/share/icons", "/usr/share/pixmaps", "/usr/share/plymouth", "/usr/share/wallpapers",
            "/usr/share/backgrounds", "/usr/share/plasma", "/usr/share/anaconda", "/usr/share/firstboot",
            "/usr/share/sddm", "/usr/share/plasmalogin", "/usr/share/applications",
            "/usr/share/fedora-logos", "/usr/share/oxygen"]
# Checksums against fedora-logos are checked in a wider set.
DIGEST_DIRS = ART_DIRS + ["/usr/share/metainfo", "/usr/share/tur", "/etc"]

# Runs inside the image, with the image's own python3. Prints one JSON object.
SCANNER = r'''
import hashlib, json, os, subprocess, sys, tempfile
config = json.loads(sys.argv[1])
TEXT_ROOTS = ["/etc", "/usr/bin", "/usr/sbin", "/usr/lib", "/usr/lib64", "/usr/libexec", "/usr/share"]
SKIP_DIRS = ["/usr/share/doc", "/usr/share/licenses", "/usr/share/man", "/usr/share/info", "/usr/share/locale",
             "/usr/share/gtk-doc", "/usr/share/help", "/usr/lib/modules", "/usr/lib/firmware", "/usr/lib/sysimage",
             "/usr/lib/.build-id", "/usr/lib/debug"]
ART_DIRS = config["art_dirs"]
DIGEST_DIRS = config["digest_dirs"]
fedora_digests = set(config["fedora_logos_digests"])

def grep(roots, skip):
    """Text files under the roots that contain "Fedora"; the skipped directories are not read."""
    targets = []
    for root in roots:
        if not os.path.isdir(root) or os.path.islink(root):
            continue
        for name in sorted(os.listdir(root)):
            path = os.path.join(root, name)
            if path not in skip:
                targets.append(path)
    if not targets:
        return []
    found = subprocess.run(["grep", "-rIl", "--null", "-e", "Fedora"] + targets, capture_output=True).stdout
    out = []
    for f in found.split(b"\0"):
        f = f.decode(errors="replace")
        if f and not any(f == d or f.startswith(d + "/") for d in skip):
            out.append(f)
    return sorted(out)

def lines(path):
    shown = []
    try:
        with open(path, "rb") as fh:
            for n, line in enumerate(fh, 1):
                if b"Fedora" in line:
                    shown.append(f"{n}: " + line.decode(errors="replace").strip()[:160])
                    if len(shown) == 3:
                        break
    except OSError as error:
        shown.append(str(error))
    return shown

def owners(paths):
    result = {}
    for path in paths:
        out = subprocess.run(["rpm", "-qf", "--qf", "%{NAME} ", path], capture_output=True, text=True).stdout
        result[path] = "-" if "not owned" in out or not out.strip() else out.strip()
    return result

def sha256(path):
    h = hashlib.sha256()
    with open(path, "rb") as fh:
        for block in iter(lambda: fh.read(1 << 20), b""):
            h.update(block)
    return h.hexdigest()

def walk(roots):
    for root in roots:
        for top, dirs, files in os.walk(root):
            for name in files:
                yield os.path.join(top, name)

result = {"text": [], "names": [], "digests": [], "initramfs": {}}
texts = grep(TEXT_ROOTS, SKIP_DIRS)
own = owners(texts)
result["text"] = [{"path": p, "owner": own.get(p, "?"), "lines": lines(p)} for p in texts]

for path in walk([d for d in DIGEST_DIRS if os.path.isdir(d)]):
    if "fedora" in os.path.basename(path).lower() and any(path.startswith(d + "/") for d in ART_DIRS):
        result["names"].append({"path": path, "target": os.path.realpath(path)})
    if os.path.isfile(path) and not os.path.islink(path):
        try:
            if os.path.getsize(path) and sha256(path) in fedora_digests:
                result["digests"].append(path)
        except OSError:
            pass
for top, dirs, files in os.walk("/usr/share"):
    for name in dirs + files:
        path = os.path.join(top, name)
        if "fedora" in name.lower() and os.path.islink(path) and path not in [n["path"] for n in result["names"]]:
            if any(path.startswith(d + "/") for d in ART_DIRS):
                result["names"].append({"path": path, "target": os.path.realpath(path)})
result["names"].sort(key=lambda n: n["path"])

# The initramfs: unpacked, then the same text and artwork checks.
kver = sorted(os.listdir("/usr/lib/modules"))[0]
image = f"/usr/lib/modules/{kver}/initramfs.img"
tmp = tempfile.mkdtemp()
unpack = subprocess.run(["lsinitrd", "--unpack", image], cwd=tmp, capture_output=True, text=True)
initrd = {"image": image, "unpacked": unpack.returncode == 0, "text": [], "digests": [], "release": ""}
if unpack.returncode == 0:
    skip = [os.path.join(tmp, d) for d in ("usr/lib/modules", "usr/lib/firmware", "lib/modules", "lib/firmware")]
    for p in grep([tmp], skip):
        initrd["text"].append({"path": "initramfs:" + p[len(tmp):], "owner": "initramfs", "lines": lines(p)})
    for path in walk([tmp]):
        if os.path.isfile(path) and not os.path.islink(path) and not any(path.startswith(s + "/") for s in skip):
            try:
                if os.path.getsize(path) and sha256(path) in fedora_digests:
                    initrd["digests"].append("initramfs:" + path[len(tmp):])
            except OSError:
                pass
    rel = os.path.join(tmp, "usr/lib/initrd-release")
    if os.path.exists(rel):
        initrd["release"] = open(rel).read()
else:
    initrd["error"] = unpack.stderr[-2000:]
result["initramfs"] = initrd
installed = subprocess.run(["rpm", "-q"] + config["forbidden"], capture_output=True, text=True).stdout
result["forbidden_installed"] = [l for l in installed.splitlines() if "not installed" not in l]
print(json.dumps(result))
'''


def podman() -> list[str]:
    return os.environ.get("PODMAN", "podman").split()


def run(args: list[str], **kwargs) -> str:
    return subprocess.run(args, check=True, capture_output=True, text=True, **kwargs).stdout


def fedora_logos_digests(base: str) -> dict[str, str]:
    """SHA-256 of each file of fedora-logos, as the base image's package database records it."""
    out = run(podman() + ["run", "--rm", "--network", "none", base, "rpm", "-q", "--dump", "fedora-logos"])
    digests = {}
    for line in out.splitlines():
        fields = line.split(" ")
        # path size mtime digest mode owner group isconfig isdoc rdev symlink
        if len(fields) >= 4 and len(fields[3]) == 64 and fields[3] != "0" * 64:
            digests[fields[3]] = fields[0]
    if len(digests) < 50:
        sys.exit(f"only {len(digests)} file digests of fedora-logos found in the base: the check would see nothing")
    return digests


def open_blob(path: pathlib.Path):
    """A layer blob as an uncompressed stream, whatever its compression."""
    with open(path, "rb") as stream:
        magic = stream.read(4)
    if magic[:2] == b"\x1f\x8b":
        return gzip.open(path, "rb"), None
    if magic == b"\x28\xb5\x2f\xfd":
        process = subprocess.Popen(["zstd", "-dc", str(path)], stdout=subprocess.PIPE)
        return process.stdout, process
    return open(path, "rb"), None


def layer_name(member: tarfile.TarInfo) -> str:
    name = member.name
    while name.startswith("./"):
        name = name[2:]
    return "/" + name.strip("/")


def scan_layers(layout: str, digests: dict[str, str], entries: list[dict]) -> dict:
    """Every layer of the image in an OCI layout, file by file."""
    directory, _, ref = layout.partition(":")
    root = pathlib.Path(directory)
    blob = lambda digest: root / "blobs" / digest.split(":")[0] / digest.split(":")[1]  # noqa: E731
    manifests = json.loads((root / "index.json").read_text())["manifests"]
    if ref:
        manifests = [m for m in manifests if m.get("annotations", {}).get("org.opencontainers.image.ref.name") == ref]
    if len(manifests) != 1:
        sys.exit(f"{layout}: expected one image, found {len(manifests)}")
    manifest = json.loads(blob(manifests[0]["digest"]).read_text())
    if "layers" not in manifest:
        sys.exit(f"{layout}: not an image manifest (an index of several images?)")
    holder: dict[str, int] = {}
    layers, findings = [], []
    for number, layer in enumerate(manifest["layers"]):
        stream, process = open_blob(blob(layer["digest"]))
        files = links = dirs = size = 0
        with tarfile.open(fileobj=stream, mode="r|") as archive:
            for member in archive:
                path = layer_name(member)
                base = os.path.basename(path)
                if base.startswith(".wh."):
                    findings.append(f"layer {number}: deletes {os.path.dirname(path)}/{base[4:]} (a whiteout): "
                                    "a layer below holds what this image does not show")
                    continue
                if member.isdir():
                    dirs += 1
                    continue
                if path in holder:
                    findings.append(f"layer {number}: {path} is also in layer {holder[path]}: one hides the other")
                holder[path] = number
                if member.isfile():
                    files += 1
                    size += member.size
                else:
                    links += 1
                if "fedora" in base.lower() and any(path.startswith(d + "/") for d in ART_DIRS):
                    if not allowed(entries, "name", path):
                        findings.append(f"layer {number}: named after Fedora: {path}")
                if member.isfile() and member.size and any(path.startswith(d + "/") for d in DIGEST_DIRS):
                    h = hashlib.sha256()
                    data = archive.extractfile(member)
                    for chunk in iter(lambda: data.read(1 << 20), b""):
                        h.update(chunk)
                    if h.hexdigest() in digests:
                        findings.append(f"layer {number}: {path} is {digests[h.hexdigest()]} of fedora-logos")
        stream.close()
        if process is not None and process.wait() != 0:
            findings.append(f"layer {number}: {layer['digest']} could not be decompressed")
        layers.append({"layer": number, "digest": layer["digest"], "media_type": layer.get("mediaType"),
                       "compressed_bytes": layer.get("size"), "files": files, "links": links,
                       "directories": dirs, "file_bytes": size})
    return {"layout": layout, "manifest": manifests[0]["digest"], "layers": layers,
            "paths": len(holder), "findings": findings}


def load_allowlist() -> list[dict]:
    entries = []
    for number, line in enumerate(ALLOWLIST.read_text().splitlines(), 1):
        if not line.strip() or line.startswith("#"):
            continue
        parts = line.split("\t")
        if len(parts) != 3 or parts[0] not in ("text", "name") or not parts[2].strip():
            sys.exit(f"{ALLOWLIST.name}:{number}: expected 'kind<TAB>pattern<TAB>reason', kind text or name")
        entries.append({"kind": parts[0], "pattern": parts[1], "reason": parts[2], "line": number, "used": 0})
    return entries


def allowed(entries: list[dict], kind: str, path: str) -> dict | None:
    for entry in entries:
        if entry["kind"] == kind and fnmatch.fnmatchcase(path, entry["pattern"]):
            entry["used"] += 1
            return entry
    return None


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("image")
    ap.add_argument("out")
    ap.add_argument("--base", help="the base image (default: the pin in image/base.lock)")
    ap.add_argument("--layers", metavar="OCI_LAYOUT[:REF]",
                    help="also check every layer of the same image, read from this OCI layout")
    args = ap.parse_args()
    out = pathlib.Path(args.out)
    out.mkdir(parents=True, exist_ok=True)
    if args.base:
        base = args.base
    else:
        lock = dict(line.split("=", 1) for line in (ROOT / "image/base.lock").read_text().splitlines()
                    if "=" in line and not line.startswith("#"))
        base = f"{lock['BASE_IMAGE']}@{lock['BASE_DIGEST']}"

    digests = fedora_logos_digests(base)
    config = json.dumps({"fedora_logos_digests": sorted(digests), "forbidden": FORBIDDEN_PACKAGES,
                         "art_dirs": ART_DIRS, "digest_dirs": DIGEST_DIRS})
    scan = json.loads(run(podman() + ["run", "--rm", "-i", "--network", "none", args.image,
                                      "python3", "-", config], input=SCANNER))
    entries = load_allowlist()
    layers = scan_layers(args.layers, digests, load_allowlist()) if args.layers else None

    failures: list[str] = []
    rows: list[str] = []
    for hit in scan["text"] + scan["initramfs"].get("text", []):
        entry = allowed(entries, "text", hit["path"])
        verdict = f"allowed (line {entry['line']}: {entry['reason']})" if entry else "NOT ALLOWED"
        rows.append(f"text  {hit['path']}  [{hit['owner']}]  {verdict}")
        if not entry:
            failures.append(f"'Fedora' in {hit['path']} ({hit['owner']}): " + " | ".join(hit["lines"]))
    for hit in scan["names"]:
        entry = allowed(entries, "name", hit["path"])
        verdict = f"allowed (line {entry['line']}: {entry['reason']})" if entry else "NOT ALLOWED"
        rows.append(f"name  {hit['path']} -> {hit['target']}  {verdict}")
        if not entry:
            failures.append(f"named after Fedora: {hit['path']}")
    for path in scan["digests"] + scan["initramfs"].get("digests", []):
        failures.append(f"a file of fedora-logos: {path}")
        rows.append(f"art   {path}  NOT ALLOWED")
    for pkg in scan["forbidden_installed"]:
        failures.append(f"installed: {pkg}")
    initrd = scan["initramfs"]
    if not initrd.get("unpacked"):
        failures.append(f"the initramfs could not be unpacked: {initrd.get('error', '')[-400:]}")
    elif 'NAME="TUR"' not in initrd.get("release", ""):
        failures.append("the initramfs's os-release does not name TUR")
    if layers is not None:
        failures += layers["findings"]
        for layer in layers["layers"]:
            rows.append(f"layer {layer['layer']:3}  {layer['digest'][:19]}  {layer['files']} files, "
                        f"{layer['links']} links, {layer['file_bytes']} bytes")
    unused = [e for e in entries if not e["used"]]

    report = {
        "image": args.image,
        "base": base,
        "fedora_logos_files": len(digests),
        "text_hits": len(scan["text"]),
        "initramfs_text_hits": len(initrd.get("text", [])),
        "name_hits": len(scan["names"]),
        "artwork_hits": len(scan["digests"]) + len(initrd.get("digests", [])),
        "forbidden_installed": scan["forbidden_installed"],
        "failures": failures,
        "unused_allowlist_lines": [f"{e['line']}: {e['kind']} {e['pattern']}" for e in unused],
        "layers": layers,
        "scan": scan,
    }
    (out / "marks.json").write_text(json.dumps(report, indent=2) + "\n")
    md = [
        "## Fedora's marks in the image",
        "",
        f"{'**FAILED**' if failures else 'Passed'}: {len(failures)} finding(s) the allow-list does not account for.",
        "",
        f"- files of `fedora-logos` in the base: {len(digests)}; found again in the image by checksum: "
        f"{report['artwork_hits']}",
        f"- files containing \"Fedora\": {report['text_hits']} in the image, "
        f"{report['initramfs_text_hits']} in the initramfs; named after Fedora: {report['name_hits']}",
        f"- allow-list lines that matched nothing: {len(unused)}",
    ]
    if layers is not None:
        md.append(f"- every layer checked: {len(layers['layers'])} layers of {layers['manifest'][:19]}, "
                  f"{layers['paths']} files and links, {len(layers['findings'])} finding(s) "
                  "(whiteouts, shadowed files, artwork, names)")
    md.append("")
    md += [f"- {f}" for f in failures[:60]]
    (out / "marks.md").write_text("\n".join(md) + "\n")
    print("\n".join(rows))
    print()
    for e in unused:
        print(f"allow-list line {e['line']} matched nothing: {e['kind']} {e['pattern']}")
    print("\n".join(md))
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
