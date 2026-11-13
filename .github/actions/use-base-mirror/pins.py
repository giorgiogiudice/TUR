#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""Every container image TUR pins by digest, and TUR's mirror of each.

    pins.py list [--json]       every pin: where it is written, what it names, its mirror
    pins.py check               fail when a pin has no mirror or a digest is not understood
    pins.py drop-in             the containers-registries.conf(5) drop-in for every mirror
    pins.py mirror IMAGE        the mirror repository of IMAGE (exit 1: none)
    pins.py resolve REF         the reference docker should pull REF by (see below)
    pins.py export ITEM...      for the action: resolve pins and write them to $GITHUB_ENV
    pins.py copy                copy every pinned digest to its mirror and check the copies
    pins.py age [--write]       compare every pin with its tag upstream; the weekly issue
    pins.py self-test

Part of .github/actions/use-base-mirror. Fedora deletes a digest from quay.io
about a day after it moves a tag, so every digest CI pulls is copied, byte for
byte, to a package in GitHub's registry (mirrors.tsv), and pulled from there.

A pin is found, not listed: every tracked file under .github/, tests/ and
scripts/, every *.lock file but Cargo.lock and every Containerfile is read,
and each `sha256:` digest in them must be one of

  - a reference written whole: REGISTRY/REPOSITORY[:TAG]@sha256:HEX, or
  - a group of variables in one file: P_IMAGE=REGISTRY/REPOSITORY and
    P_DIGEST=sha256:HEX, with P_TAG, P_VERSION and P_VERIFIED when they are
    there (image/base.lock, desktop/ci/builder.lock, ARG lines).

Anything else is reported by `check`, so that no new pin escapes the mirror.
A line that holds a digest on purpose without pulling it ends with the marker
"pins.py: ignore". Test fixtures (any path with /fixtures/) are not read.

podman and skopeo pull through the mirror by themselves (the drop-in). docker
reads no registries.conf: `resolve` gives it the mirror's reference when the
mirror has the digest (the same bytes: a digest names exactly one manifest),
and the upstream reference otherwise.
"""

from __future__ import annotations

import dataclasses
import datetime
import hashlib
import json
import os
import pathlib
import re
import subprocess
import sys
import tempfile

HERE = pathlib.Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import upstream  # noqa: E402  (same directory)

TABLE = HERE / "mirrors.tsv"
IGNORE_MARK = "pins.py: ignore"
# Images TUR builds and keeps in its own namespace need no mirror.
OWN = "ghcr.io/giorgiogiudice/"

DIGEST = re.compile(r"sha256:[0-9a-f]{64}")
REF = re.compile(
    r"(?P<repo>(?:[a-z0-9-]+\.)+[a-z0-9-]+(?::[0-9]+)?(?:/[a-z0-9._-]+)+)"
    r"(?::(?P<tag>[A-Za-z0-9_][A-Za-z0-9_.-]{0,127}))?"
    r"@(?P<digest>sha256:[0-9a-f]{64})"
)
KV = re.compile(
    r"^\s*(?:ARG\s+|export\s+)?(?P<prefix>[A-Z][A-Z0-9_]*?)_(?P<field>IMAGE|TAG|DIGEST|VERSION|VERIFIED)"
    r"\s*[=:]\s*[\"']?(?P<value>[^\"'\s#]*)"
)


@dataclasses.dataclass
class Pin:
    repo: str                 # fully qualified: docker.io/library/rust
    digest: str
    tag: str = ""
    version: str = ""
    verified: str = ""
    file: str = ""
    line: int = 0
    name: str = ""            # the variable, when the pin is one (BASE, FEDORA_IMAGE)
    written: str = ""         # the reference as a FROM line or a command names it

    @property
    def ref(self) -> str:
        return f"{self.repo}@{self.digest}"

    @property
    def where(self) -> str:
        return f"{self.file}:{self.line}"


def normalize(repo: str) -> str:
    first, _, rest = repo.partition("/")
    if not rest or ("." not in first and ":" not in first and first != "localhost"):
        repo = "docker.io/" + repo
    if repo.startswith("docker.io/") and repo.count("/") == 1:
        repo = "docker.io/library/" + repo[len("docker.io/"):]
    return repo


def table(path: pathlib.Path = TABLE) -> dict[str, str]:
    out = {}
    for raw in path.read_text().splitlines():
        line = raw.split("#", 1)[0].strip()
        if line:
            up, mirror = line.split()
            out[normalize(up)] = mirror
    return out


def mirror_of(repo: str, mirrors: dict[str, str] | None = None) -> str | None:
    return (table() if mirrors is None else mirrors).get(normalize(repo))


def scanned(path: str) -> bool:
    if "/fixtures/" in f"/{path}" or path.startswith("docs/"):
        return False
    name = path.rsplit("/", 1)[-1]
    if name.startswith("Containerfile") and not name.endswith("ignore"):
        return True
    if name.endswith(".lock") and name != "Cargo.lock":
        return True
    return path.startswith((".github/", "tests/", "scripts/"))


def tracked(root: pathlib.Path) -> list[str]:
    out = subprocess.run(["git", "-C", str(root), "ls-files", "-z"], check=True, capture_output=True).stdout
    return sorted(p for p in out.decode().split("\0") if p)


def scan_text(path: str, text: str) -> tuple[list[Pin], list[str]]:
    """The pins in one file, and the digests in it that are not understood."""
    pins: list[Pin] = []
    groups: dict[str, dict[str, tuple[str, int]]] = {}
    loose: list[tuple[int, int, str]] = []   # (line, column, digest) not part of a whole reference
    for n, line in enumerate(text.splitlines(), 1):
        if IGNORE_MARK in line:
            continue
        spans = []
        for m in REF.finditer(line):
            spans.append(m.span())
            kv = KV.match(line)
            pins.append(Pin(repo=normalize(m["repo"]), digest=m["digest"], tag=m["tag"] or "",
                            file=path, line=n, name=kv["prefix"] + "_" + kv["field"] if kv else "",
                            written=m.group()))
        kv = KV.match(line)
        if kv and "@" not in kv["value"]:
            groups.setdefault(kv["prefix"], {})[kv["field"]] = (kv["value"], n)
        for m in DIGEST.finditer(line):
            if not any(a <= m.start() < b for a, b in spans):
                loose.append((n, m.start(), m.group()))
    used = set()
    for prefix, g in groups.items():
        if "DIGEST" not in g or not g["DIGEST"][0]:
            continue
        digest, n = g["DIGEST"]
        if not DIGEST.fullmatch(digest) or "IMAGE" not in g:
            continue
        pins.append(Pin(repo=normalize(g["IMAGE"][0]), digest=digest, tag=g.get("TAG", ("", 0))[0],
                        version=g.get("VERSION", ("", 0))[0], verified=g.get("VERIFIED", ("", 0))[0],
                        file=path, line=n, name=prefix,
                        # used as ${P_IMAGE}@${P_DIGEST}: without the tag
                        written=f"{g['IMAGE'][0]}@{digest}"))
        used.add((n, digest))
    problems = [f"{path}:{n}: {d[:19]}… is not part of a pin pins.py understands"
                for n, _, d in loose if (n, d) not in used]
    return pins, problems


def scan(root: pathlib.Path) -> tuple[list[Pin], list[str]]:
    pins, problems = [], []
    for path in tracked(root):
        if not scanned(path):
            continue
        try:
            text = (root / path).read_text()
        except (UnicodeDecodeError, IsADirectoryError, FileNotFoundError):
            continue
        p, q = scan_text(path, text)
        pins += p
        problems += q
    return pins, problems


def root() -> pathlib.Path:
    return pathlib.Path(subprocess.run(["git", "-C", str(HERE), "rev-parse", "--show-toplevel"],
                                       check=True, capture_output=True, text=True).stdout.strip())


def distinct(pins: list[Pin]) -> list[Pin]:
    """One pin per (repository, digest); the first one found names it."""
    seen, out = set(), []
    for p in pins:
        if (p.repo, p.digest) not in seen:
            seen.add((p.repo, p.digest))
            out.append(p)
    return out


# ---- registry access ------------------------------------------------------------

def skopeo(*args: str, check: bool = True) -> subprocess.CompletedProcess:
    return subprocess.run(["skopeo", *args], check=check, capture_output=True)


def has(ref: str) -> bool:
    """Whether a registry gives REF's manifest, with this user's credentials."""
    return skopeo("inspect", "--raw", "--retry-times", "2", f"docker://{ref}", check=False).returncode == 0


def manifest_digest(ref: str) -> str:
    return "sha256:" + hashlib.sha256(skopeo("inspect", "--raw", "--retry-times", "3", f"docker://{ref}").stdout).hexdigest()


def notice(kind: str, title: str, text: str) -> None:
    print(f"::{kind} title={title}::{text}", flush=True)


def resolve(ref: str, mirrors: dict[str, str] | None = None) -> str:
    """The reference to pull REF by: the mirror's when it has the digest,
    REF itself when only upstream has it. Exits when neither has it."""
    m = REF.fullmatch(ref)
    if not m:
        sys.exit(f"not a reference by digest: {ref}")
    repo, digest = normalize(m["repo"]), m["digest"]
    mirror = mirror_of(repo, mirrors)
    if mirror is None:
        print(f"{ref}: no mirror (TUR's own image or not in mirrors.tsv)", file=sys.stderr)
        return ref
    if has(f"{mirror}@{digest}"):
        print(f"{ref}: from the mirror, {mirror}@{digest}", file=sys.stderr)
        return f"{mirror}@{digest}"
    code, text = upstream.probe(f"{repo}@{digest}")
    if code == 0:
        notice("notice", "Image not mirrored yet",
               f"{ref} comes from {repo}; base-mirror.yml copies it to {mirror} when the pin changes, and daily.")
        return ref
    notice("error", "Pinned image unavailable",
           f"{ref} is in neither {mirror} nor {repo} ({text}). {repo} has dropped the digest and it was "
           f"never mirrored: pin the current one (image/README.md, \"The base image and its mirror\").")
    sys.exit(1)


# ---- commands ---------------------------------------------------------------------

def cmd_list(args: list[str]) -> int:
    pins, _ = scan(root())
    mirrors = table()
    if "--json" in args:
        print(json.dumps([dataclasses.asdict(p) | {"mirror": mirror_of(p.repo, mirrors)} for p in pins], indent=1))
        return 0
    for p in pins:
        print(f"{p.where:48} {p.repo}{':' + p.tag if p.tag else ''}@{p.digest[:19]}…  -> "
              f"{mirror_of(p.repo, mirrors) or ('own image' if p.repo.startswith(OWN) else 'NO MIRROR')}")
    return 0


def cmd_check(_: list[str]) -> int:
    pins, problems = scan(root())
    mirrors = table()
    for p in pins:
        if not p.repo.startswith(OWN) and mirror_of(p.repo, mirrors) is None:
            problems.append(f"{p.where}: {p.repo} has no mirror: add a line to {TABLE.name}")
    used = {p.repo for p in pins}
    for up in mirrors:
        if up not in used:
            print(f"note: {up} in {TABLE.name} is pinned nowhere")
    print(f"{len(pins)} pins, {len(distinct(pins))} distinct digests, {len(set(mirrors.values()))} mirrors")
    for q in problems:
        print(f"::error::{q}")
    return 1 if problems else 0


def cmd_drop_in(_: list[str]) -> int:
    print("# Written by .github/actions/use-base-mirror (TUR CI) from mirrors.tsv. See")
    print("# image/README.md, \"The base image and its mirror\". A pull by digest of one")
    print("# of these repositories tries TUR's mirror first, then the repository itself.")
    print("# Pulls by tag do not use it.")
    for up, mirror in table().items():
        print(f'\n[[registry]]\nprefix = "{up}"\nlocation = "{up}"\nmirror-by-digest-only = true\n'
              f'\n[[registry.mirror]]\nlocation = "{mirror}"')
    return 0


def cmd_mirror(args: list[str]) -> int:
    m = mirror_of(args[0].split("@", 1)[0])
    if m:
        print(m)
    return 0 if m else 1


def cmd_resolve(args: list[str]) -> int:
    print(resolve(args[0]))
    return 0


def cmd_export(args: list[str]) -> int:
    """ITEM is the name of an environment variable that holds a reference by
    digest, or a file whose pins are all wanted. Writes NAME_PULL for each
    variable, and TUR_BUILD_CONTEXTS (docker build --build-context flags that
    take every resolved pin from where it was resolved) for all of them."""
    env_file = os.environ.get("GITHUB_ENV")
    lines, contexts = [], []
    top = root()
    for item in args:
        if re.fullmatch(r"[A-Z_][A-Z0-9_]*", item):
            ref = os.environ.get(item, "")
            if not ref:
                sys.exit(f"{item} is not set")
            pull = resolve(ref)
            lines.append(f"{item}_PULL={pull}")
            refs = [(ref, pull)]
        else:
            pins, _ = scan_text(item, (top / item).read_text())
            refs = [(p.written, resolve(p.ref)) for p in pins if not p.repo.startswith(OWN)]
            if not refs:
                sys.exit(f"{item}: no pin found")
        for ref, pull in refs:
            print(f"{item}: {ref}\n    pulled as {pull}")
            if pull != ref:
                # docker build (BuildKit) takes an image a FROM line names
                # from a named context of the same name instead.
                contexts.append(f"--build-context={ref}=docker-image://{pull}")
    lines.append("TUR_BUILD_CONTEXTS=" + " ".join(dict.fromkeys(contexts)))
    for line in lines:
        print(line)
    if env_file:
        with open(env_file, "a") as f:
            f.write("\n".join(lines) + "\n")
    return 0


def cmd_copy(_: list[str]) -> int:
    """Copy every pinned digest to its mirror, tagged sha256-HEX (and with
    the pin's version, when it has one), and check the copy byte for byte."""
    pins, problems = scan(root())
    if problems:
        for q in problems:
            print(f"::error::{q}")
        return 1
    mirrors = table()
    failed, rows = [], []
    for p in distinct(pins):
        mirror = mirror_of(p.repo, mirrors)
        if mirror is None:
            continue
        hexd = p.digest.split(":", 1)[1]
        tags = [f"sha256-{hexd}"] + ([p.version] if p.version else [])
        src_up, src_mirror = f"docker://{p.ref}", f"docker://{mirror}@{p.digest}"
        print(f"--- {p.ref} ({p.where}) -> {mirror}", flush=True)
        if all(has(f"{mirror}:{t}") and manifest_digest(f"{mirror}:{t}") == p.digest for t in tags):
            print("    already mirrored, with its tags")
            state = "already mirrored"
        else:
            if upstream.probe(p.ref)[0] == 0:
                src = src_up
            elif has(f"{mirror}@{p.digest}"):
                src = src_mirror
                print(f"    {p.repo} no longer has it; tagging the mirror's copy")
            else:
                notice("error", "Pinned image unavailable",
                       f"{p.ref} ({p.where}) is in neither {p.repo} nor {mirror}: it was removed upstream before it "
                       f"was mirrored. Pin the current digest of {p.repo}:{p.tag or '<tag>'}.")
                failed.append(p.ref)
                rows.append((p, mirror, "UNAVAILABLE"))
                continue
            for t in tags:
                start = datetime.datetime.now()
                r = subprocess.run(["skopeo", "copy", "--all", "--preserve-digests", "--retry-times", "3",
                                    src, f"docker://{mirror}:{t}"], capture_output=True, text=True)
                if r.returncode != 0:
                    print(r.stdout[-2000:], r.stderr[-2000:])
                    failed.append(p.ref)
                    break
                print(f"    copied from {src} to {mirror}:{t} in {(datetime.datetime.now() - start).seconds} s")
            state = "copied"
        # The same bytes, by digest, by every tag and for every platform.
        ok = True
        checks = [(f"{mirror}@{p.digest}", p.digest)] + [(f"{mirror}:{t}", p.digest) for t in tags]
        try:
            index = json.loads(skopeo("inspect", "--raw", f"docker://{mirror}@{p.digest}").stdout)
            checks += [(f"{mirror}@{c['digest']}", c["digest"]) for c in index.get("manifests", [])]
            for ref, want in checks:
                got = manifest_digest(ref)
                print(f"    {'ok  ' if got == want else 'FAIL'}  {ref} -> {got}")
                ok &= got == want
        except subprocess.CalledProcessError as e:
            print(f"    FAIL  {e.stderr.decode(errors='replace')[-500:]}")
            ok = False
        if not ok and p.ref not in failed:
            failed.append(p.ref)
        rows.append((p, mirror, state if ok else "CHECK FAILED"))
    # Who can read each mirror. Visibility is the owner's setting; this only says what it is.
    public = {}
    for mirror in sorted({m for _, m, _ in rows}):
        any_digest = next(p.digest for p, m, _ in rows if m == mirror)
        public[mirror] = skopeo("inspect", "--raw", "--no-creds", f"docker://{mirror}@{any_digest}",
                                check=False).returncode == 0
        if public[mirror]:
            notice("warning", "Mirror is public", f"{mirror} can be read without credentials.")
    summary = ["### Image mirrors", "", "| pin | where | mirror | result | readable by anyone |", "|---|---|---|---|---|"]
    for p, mirror, state in rows:
        summary.append(f"| `{p.repo}{':' + p.tag if p.tag else ''}@{p.digest[:19]}…` | `{p.where}` | `{mirror}` "
                       f"| {state} | {'yes' if public.get(mirror) else 'no'} |")
    text = "\n".join(summary) + "\n"
    print(text)
    if os.environ.get("GITHUB_STEP_SUMMARY"):
        with open(os.environ["GITHUB_STEP_SUMMARY"], "a") as f:
            f.write(text)
    return 1 if failed else 0


def created(ref: str) -> datetime.date | None:
    r = skopeo("--override-os", "linux", "--override-arch", "amd64", "inspect", "--retry-times", "3",
               f"docker://{ref}", check=False)
    if r.returncode != 0:
        return None
    stamp = json.loads(r.stdout).get("Created") or ""
    try:
        return datetime.date.fromisoformat(stamp[:10])
    except ValueError:
        return None


def pin_age(p: Pin, mirror: str | None, today: datetime.date) -> tuple[int | None, str]:
    m = re.match(r"^\d+\.(\d{8})\.", p.version)
    if m:
        return (today - datetime.datetime.strptime(m.group(1), "%Y%m%d").date()).days, f"version {p.version}"
    for ref in ([f"{mirror}@{p.digest}"] if mirror else []) + [p.ref]:
        born = created(ref)
        if born:
            return (today - born).days, f"built {born.isoformat()}"
    return None, "build date unknown"


def cmd_age(args: list[str]) -> int:
    """For every pin with a tag: is the tag still at the pinned digest, and
    how old is the pin? A pin is stale when the tag has moved on and the
    pinned image is more than MAX_AGE_DAYS (7) days old. With --write (the
    weekly run, or by hand), one issue lists the stale pins; it is closed
    when there are none. Without, it prints what it would write."""
    write = "--write" in args
    limit = int(os.environ.get("MAX_AGE_DAYS", "7"))
    today = datetime.datetime.now(datetime.timezone.utc).date()
    pins, _ = scan(root())
    mirrors = table()
    seen, rows = set(), []
    for p in pins:
        if not p.tag or p.repo.startswith(OWN) or (p.repo, p.tag, p.digest) in seen:
            continue
        seen.add((p.repo, p.tag, p.digest))
        where = [q.where for q in pins if (q.repo, q.tag, q.digest) == (p.repo, p.tag, p.digest)]
        try:
            current = manifest_digest(f"{p.repo}:{p.tag}")
        except subprocess.CalledProcessError as e:
            current = "unknown: " + e.stderr.decode(errors="replace").strip().splitlines()[-1][:120]
        age, basis = pin_age(p, mirror_of(p.repo, mirrors), today)
        stale = current.startswith("sha256:") and current != p.digest and age is not None and age > limit
        verdict = "re-pin" if stale else ("up to date" if current == p.digest else
                                         ("tag unreadable" if not current.startswith("sha256:") else
                                          f"tag moved; within {limit} days"))
        rows.append(dict(pin=p, where=where, current=current, age=age, basis=basis, stale=stale, verdict=verdict))
        print(f"{p.repo}:{p.tag}\n    pinned  {p.digest} ({basis}, {age if age is not None else '?'} days)"
              f"\n    now     {current}\n    in      {', '.join(where)}\n    verdict {verdict}")

    stale = [r for r in rows if r["stale"]]
    oldest = max((r["age"] for r in stale), default=0)
    title = f"Pinned images: {len(stale)} to re-pin, the oldest {oldest} days old"
    table_md = ["| image | pinned | the tag now | age | where | verdict |", "|---|---|---|---|---|---|"]
    for r in rows:
        p = r["pin"]
        table_md.append(f"| `{p.repo}:{p.tag}` | `{p.digest[:19]}…` | `{r['current'][:19]}…` | "
                        f"{r['age'] if r['age'] is not None else '?'} days ({r['basis']}) | "
                        f"{'<br>'.join('`' + w + '`' for w in r['where'])} | {r['verdict']} |")
    run_url = os.environ.get("RUN_URL", "")
    body = f"""Images CI pulls by digest whose tag has moved on more than {limit} days ago. Fedora moves its tags daily, mostly with security updates; quay.io removes a digest about a day after its tag moves, and only TUR's mirror keeps it after that (image/README.md, "The base image and its mirror").

{chr(10).join(table_md)}

To re-pin, put the tag's current digest where the table says, in a pull request of its own (for `image/base.lock`, also `BASE_VERSION` and `BASE_VERIFIED`; for `desktop/ci/builder.lock`, see desktop/README.md, "The builder image"). The pull request runs `base-mirror`, which copies the new digest to the mirror. Merge it within a day or so: only a mirrored digest outlives quay.io's clean-up.

Checked by {run_url}. The weekly check updates this issue and closes it when no pin is stale.
"""
    print(f"\n{title}\n")
    if not write:
        print("dry run: the issue tracker is not changed. The issue would read:\n---")
        print(body + "---")
        return 0

    repo = os.environ["REPO"]

    def api(method: str, path: str, body: dict | None = None):
        import urllib.request
        req = urllib.request.Request(
            f"https://api.github.com/repos/{repo}{path}", method=method,
            data=json.dumps(body).encode() if body is not None else None,
            headers={"Authorization": f"Bearer {os.environ['GH_TOKEN']}", "Accept": "application/vnd.github+json"})
        with urllib.request.urlopen(req, timeout=30) as r:
            return json.load(r)

    # "Base image pin is ..." is the title the check used while it covered only image/base.lock.
    prefixes = ("Pinned images: ", "Base image pin is ")
    open_issues = [i for i in api("GET", "/issues?state=open&creator=github-actions%5Bbot%5D&per_page=100")
                   if "pull_request" not in i and i["title"].startswith(prefixes)]
    if not stale:
        for i in open_issues:
            api("POST", f"/issues/{i['number']}/comments", {"body": f"No pin is stale now. Closing. ({run_url})"})
            api("PATCH", f"/issues/{i['number']}", {"state": "closed", "state_reason": "completed"})
            print(f"closed #{i['number']}")
    elif open_issues:
        api("PATCH", f"/issues/{open_issues[0]['number']}", {"title": title, "body": body})
        print(f"updated #{open_issues[0]['number']}: {title}")
    else:
        print(f"opened #{api('POST', '/issues', {'title': title, 'body': body})['number']}: {title}")
    return 0


def cmd_self_test(_: list[str]) -> int:
    d = "sha256:" + "ab" * 32
    e = "sha256:" + "cd" * 32
    lock = f"BASE_IMAGE=quay.io/fedora/fedora\nBASE_TAG=45\nBASE_DIGEST={d}\nBASE_VERSION=45.20261113.0\n"
    pins, problems = scan_text("x/builder.lock", lock)
    assert not problems and len(pins) == 1, (pins, problems)
    assert (pins[0].repo, pins[0].tag, pins[0].digest, pins[0].version, pins[0].line, pins[0].written) == \
        ("quay.io/fedora/fedora", "45", d, "45.20261113.0", 3, f"quay.io/fedora/fedora@{d}"), pins
    arg = f"ARG RUST_IMAGE=docker.io/library/rust\nARG RUST_TAG=1.99.1\nARG RUST_DIGEST={d}\nFROM ${{RUST_IMAGE}}@${{RUST_DIGEST}}\n"
    pins, problems = scan_text("Containerfile", arg)
    assert not problems and [p.repo for p in pins] == ["docker.io/library/rust"], (pins, problems)
    yml = f"env:\n  MOTO_IMAGE: docker.io/motoserver/moto:5.2.5@{d}\n  X: registry.fedoraproject.org/fedora@{e}\n"
    pins, problems = scan_text("w.yml", yml)
    assert not problems and [(p.repo, p.tag, p.name) for p in pins] == [
        ("docker.io/motoserver/moto", "5.2.5", "MOTO_IMAGE"), ("registry.fedoraproject.org/fedora", "", "")], pins
    pins, problems = scan_text("f", f"FROM docker.io/library/debian:trixie-slim@{d}\nFROM rust@{e}\n")
    assert len(pins) == 1 and len(problems) == 1 and "f:2" in problems[0], (pins, problems)
    pins, problems = scan_text("f", f"NEVER: quay.io/a/b@{d} # {IGNORE_MARK}\nDIGEST_ONLY={e}\n")
    assert not pins and len(problems) == 1 and "f:2" in problems[0], (pins, problems)
    assert normalize("debian") == "docker.io/library/debian" and normalize("docker.io/debian") == "docker.io/library/debian"
    assert scanned(".github/workflows/a.yml") and scanned("image/base.lock") and scanned("server/x/Containerfile")
    assert not scanned("services/x/Cargo.lock") and not scanned("tests/x/fixtures/y.json") and not scanned("docs/r.md")
    with tempfile.TemporaryDirectory() as t:
        tsv = pathlib.Path(t) / "m.tsv"
        tsv.write_text("# c\nquay.io/fedora/fedora  ghcr.io/o/tur-mirror-fedora\ndebian ghcr.io/o/d\n")
        m = table(tsv)
        assert m == {"quay.io/fedora/fedora": "ghcr.io/o/tur-mirror-fedora", "docker.io/library/debian": "ghcr.io/o/d"}, m
    print("self-test passed")
    return 0


COMMANDS = {"list": cmd_list, "check": cmd_check, "drop-in": cmd_drop_in, "mirror": cmd_mirror,
            "resolve": cmd_resolve, "export": cmd_export, "copy": cmd_copy, "age": cmd_age,
            "self-test": cmd_self_test}

if __name__ == "__main__":
    if len(sys.argv) < 2 or sys.argv[1] not in COMMANDS:
        sys.exit(__doc__)
    sys.exit(COMMANDS[sys.argv[1]](sys.argv[2:]))
