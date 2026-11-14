#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""Read the licence terms inside Microsoft's evaluation medium, before use.

    terms.py WINDOWS.iso OUT_DIR

The evaluation's licence terms are not published on Microsoft's download
page; they are inside the medium. This mounts the medium read-only on the
runner, lists the images in its install archive, takes the licence text of
the evaluation edition out of it into a temporary directory, and writes
`terms-excerpt.md`: where the text was found, its SHA-256 and length, its
opening paragraphs and every paragraph about evaluation, testing, virtual
machines, time limits and remote use. The licence file itself is deleted
again and is never an artifact; the excerpt is a quotation for review.

Nothing is installed or started.
"""

from __future__ import annotations

import hashlib
import json
import pathlib
import re
import shutil
import subprocess
import sys
import tempfile

KEY = re.compile(
    r"evaluat|demonstrat|live operating|\btest|virtual|time-sensitive|expire|benchmark|automat|"
    r"remote|install and use|not for resale|activation|number of|internal",
    re.I,
)
DESTINATIONS = frozenset("""
aftncn aftnsep aftnsepc annotation atnauthor atndate atnicn atnid atnparent atnref atntime atrfend
atrfstart author background bkmkend bkmkstart blipuid buptim category colorschememapping colortbl
comment company creatim datafield datastore defchp defpap do doccomm docvar dptxbxtext ebcend
ebcstart factoidname falt fchars ffdeftext ffentrymcr ffexitmcr ffformat ffhelptext ffl ffname
ffstattext field file filetbl fldinst fldrslt fldtype fname fontemb fontfile fonttbl footer footerf
footerl footerr footnote formfield ftncn ftnsep ftnsepc g generator gridtbl header headerf headerl
headerr hl hlfr hlinkbase hlloc hlsrc hsv htmltag info keycode keywords latentstyles lchars
levelnumbers leveltext lfolevel linkval list listlevel listname listoverride listoverridetable
listpicture liststylename listtable lsdlockedexcept macc maccPr mailmerge manager maln malnScr
margPr mbar mbarPr mbaseJc mbegChr mborderBox mborderBoxPr mbox mboxPr mchr mcount mctrlPr md mdeg
mdegHide mden mdiff mdPr me mendChr meqArr meqArrPr mf mfName mfPr mfunc mfuncPr mgroupChr
mgroupChrPr mgrow mhideBot mhideLeft mhideRight mhideTop mhtmltag mlim mlimloc mlimlow mlimlowPr
mlimupp mlimuppPr mm mmaddfieldname mmath mmathPict mmathPr mmaxdist mmc mmcJc mmconnectstr
mmconnectstrdata mmcPr mmcs mmdatasource mmheadersource mmmailsubject mmodso mmodsofilter
mmodsofldmpdata mmodsomappedname mmodsoname mmodsorecipdata mmodsosort mmodsosrc mmodsotable
mmodsoudl mmodsoudldata mmodsouniquetag mmPr mmquery mmr mnary mnaryPr mnoBreak mnum mobjDist
moMath moMathPara moMathParaPr mopEmu mphant mphantPr mplcHide mpos mr mrad mradPr mrPr msepChr
mshow mshp msPre msPrePr msSub msSubPr msSubSup msSubSupPr msSup msSupPr mstrikeBLTR mstrikeH
mstrikeTLBR mstrikeV msub msubHide msup msupHide mtransp mtype mvertJc mvfmf mvfml mvtof mvtol
mzeroAsc mzeroDesc mzeroWid nesttableprops nextfile nonesttables objalias objclass objdata object
objname objsect objtime oldcprops oldpprops oldsprops oldtprops oleclsid operator panose password
passwordhash pgp pgptbl picprop pict pn pnseclvl pntext pntxta pntxtb printim private propname
protend protstart protusertbl pxe result revtbl revtim rsidtbl rxe shp shpgrp shpinst shppict
shprslt shptxt sn sp staticval stylesheet subject sv svb tc template themedata title txe ud upr
userprops wgrffmtfilter windowcaption writereservation writereservhash xe xform xmlattrname
xmlattrvalue xmlclose xmlname xmlnstbl xmlopen
""".split())
SPECIAL = {
    "par": "\n", "sect": "\n\n", "page": "\n\n", "line": "\n", "tab": "\t",
    "emdash": "\u2014", "endash": "\u2013", "emspace": " ", "enspace": " ", "qmspace": " ",
    "bullet": "\u2022", "lquote": "\u2018", "rquote": "\u2019",
    "ldblquote": "\u201c", "rdblquote": "\u201d", "row": "\n", "cell": "\t",
}
TOKEN = re.compile(r"\\([a-zA-Z]{1,32})(-?\d{1,10})?[ ]?|\\'([0-9a-fA-F]{2})|\\([^a-zA-Z])|([{}])|[\r\n]+|(.)")


def rtf_to_text(data: bytes) -> str:
    """Plain text of an RTF document (the usual token walk; no dependency)."""
    text = data.decode("latin-1")
    stack: list[tuple[int, bool]] = []
    ignorable = False
    ucskip, curskip = 1, 0
    out: list[str] = []
    for match in TOKEN.finditer(text):
        word, arg, hexa, char, brace, plain = match.groups()
        if brace:
            curskip = 0
            if brace == "{":
                stack.append((ucskip, ignorable))
            elif stack:
                ucskip, ignorable = stack.pop()
        elif char:
            curskip = 0
            if char == "~":
                if not ignorable:
                    out.append(" ")
            elif char in "{}\\":
                if not ignorable:
                    out.append(char)
            elif char == "*":
                ignorable = True
        elif word:
            curskip = 0
            if word in DESTINATIONS:
                ignorable = True
            elif ignorable:
                pass
            elif word in SPECIAL:
                out.append(SPECIAL[word])
            elif word == "uc":
                ucskip = int(arg or 1)
            elif word == "u" and arg:
                code = int(arg)
                if code < 0:
                    code += 0x10000
                out.append(chr(code))
                curskip = ucskip
        elif hexa:
            if curskip > 0:
                curskip -= 1
            elif not ignorable:
                out.append(bytes([int(hexa, 16)]).decode("cp1252", "replace"))
        elif plain:
            if curskip > 0:
                curskip -= 1
            elif not ignorable:
                out.append(plain)
    return "".join(out)


def run(*argv: str, check: bool = True) -> str:
    done = subprocess.run(argv, capture_output=True, text=True, errors="replace")
    if check and done.returncode != 0:
        raise RuntimeError(f"{' '.join(argv)}: exit {done.returncode}: {done.stderr.strip()[:400]}")
    return done.stdout


def excerpt(text: str, limit: int = 14000) -> list[str]:
    paragraphs = [re.sub(r"[ \t]+", " ", p).strip() for p in text.split("\n")]
    paragraphs = [p for p in paragraphs if p]
    chosen: list[str] = []
    used = 0
    for index, paragraph in enumerate(paragraphs):
        if index < 12 or KEY.search(paragraph):
            piece = paragraph if len(paragraph) <= 1500 else paragraph[:1500] + " […]"
            if used + len(piece) > limit:
                chosen.append("[… the excerpt stops here: length limit …]")
                break
            chosen.append(piece)
            used += len(piece)
    return chosen


def main() -> int:
    iso, out = pathlib.Path(sys.argv[1]), pathlib.Path(sys.argv[2])
    out.mkdir(parents=True, exist_ok=True)
    work = pathlib.Path(tempfile.mkdtemp(prefix="dualrun-terms-"))
    mount = work / "mnt"
    mount.mkdir()
    facts: dict = {"medium": iso.name, "images": [], "licences": []}
    lines = ["# Licence terms inside the evaluation medium (excerpt)", ""]
    mounted = False
    try:
        try:
            run("sudo", "mount", "-o", "loop,ro", str(iso), str(mount))
            mounted = True
        except RuntimeError as error:
            # No UDF driver on this kernel: take the one archive out with 7z.
            facts["mount_error"] = str(error)
            run("7z", "x", "-y", f"-o{mount}", str(iso), "sources/install.wim", "sources/install.esd",
                check=False)
        archive = None
        for name in ("install.wim", "install.esd"):
            for candidate in (mount / "sources").glob("*"):
                if candidate.name.lower() == name:
                    archive = candidate
        if archive is None:
            raise RuntimeError("no sources/install.wim or install.esd on the medium")
        facts["archive"] = {"name": archive.name, "bytes": archive.stat().st_size}
        info = run("wimlib-imagex", "info", str(archive))
        image: dict = {}
        for line in info.splitlines():
            key, _, value = line.partition(":")
            key, value = key.strip(), value.strip()
            if key == "Index":
                image = {"index": int(value)}
                facts["images"].append(image)
            elif key in ("Name", "Description", "Edition ID", "Build", "Major Version", "Languages",
                         "Installation Type", "Product Name") and image:
                image[key.lower().replace(" ", "_")] = value
        listing = run("wimlib-imagex", "dir", str(archive), "1")
        licences = [p for p in listing.splitlines() if p.lower().endswith("license.rtf")]
        facts["licence_paths_total"] = len(licences)
        wanted = [p for p in licences if "eval" in p.lower()] or licences
        english = [p for p in wanted if "en-us" in p.lower()] or wanted
        seen: set[str] = set()
        for path in english[:6]:
            dest = work / "x"
            shutil.rmtree(dest, ignore_errors=True)
            dest.mkdir()
            run("wimlib-imagex", "extract", str(archive), "1", path, f"--dest-dir={dest}",
                "--no-acls", "--no-attributes")
            file = next(dest.rglob("*.rtf"), None) or next(dest.rglob("*.RTF"), None)
            if file is None:
                continue
            data = file.read_bytes()
            digest = hashlib.sha256(data).hexdigest()
            record = {"path_in_image": path, "sha256": digest, "bytes": len(data)}
            facts["licences"].append(record)
            if digest in seen:
                record["same_text_as_above"] = True
                continue
            seen.add(digest)
            text = rtf_to_text(data)
            record["characters"] = len(text)
            lines += [f"## `{path}`", "",
                      f"SHA-256 `{digest}`, {len(data)} bytes, {len(text)} characters of text. "
                      "Opening paragraphs, then every paragraph that mentions evaluation, testing, "
                      "virtual machines, time limits, remote use or activation.", ""]
            lines += [f"> {p}\n>" for p in excerpt(text)]
            lines.append("")
        if not facts["licences"]:
            lines.append("No licence text was found in the image. Paths ending in license.rtf: "
                         f"{len(licences)}.")
    except Exception as error:  # a finding, not a crash: the caller decides
        facts["error"] = str(error)
        lines.append(f"**Could not read the terms:** {error}")
    finally:
        if mounted:
            subprocess.run(["sudo", "umount", str(mount)], check=False)
        shutil.rmtree(work, ignore_errors=True)
    (out / "terms.json").write_text(json.dumps(facts, indent=2) + "\n")
    (out / "terms-excerpt.md").write_text("\n".join(lines) + "\n")
    print(json.dumps(facts, indent=2))
    return 0


if __name__ == "__main__":
    sys.exit(main())
