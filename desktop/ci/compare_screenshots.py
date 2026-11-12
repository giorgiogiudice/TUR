#!/usr/bin/env python3
# SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
"""Compare rendered pictures with their reference images.

    compare_screenshots.py --actual DIR --repeat DIR --reference DIR --out DIR

Runs in the builder image (it needs Pillow), from desktop/ci/screenshots.sh.

Each picture in --actual is judged three ways:

  repeatable   it must be pixel for pixel the picture of the same name in
               --repeat, which the same program rendered a second time. A
               picture that changes from one run to the next cannot be tested.
  reference    it must have a reference of the same name in --reference, of
               the same size, and
  tolerance    no pixel may differ from the reference by more than
               CHANNEL_TOLERANCE in any channel, with at most ALLOWED_PIXELS
               pixels beyond that.

A reference with no picture is stale and also fails.

The tolerance. Qt Quick's software renderer is deterministic, so with the
same builder image the pictures match exactly. CHANNEL_TOLERANCE absorbs
only what cannot be seen and cannot be controlled: a difference of a few
levels at an anti-aliased edge between processors. It is not there to let a
change through; ALLOWED_PIXELS is 0. A real change (a moved control, a wrong
colour, a missing glyph) differs by far more than this in many pixels. If a
font or Qt update shifts the text, the comparison fails and the references
are regenerated and looked at; the tolerance is not loosened.

Written to --out: reference/ (copies), diff/ (reference in grey, differing
pixels in magenta), proposed/ (every picture that is new or different, under
the name it would have as a reference), contact-*.png (contact sheets),
report.json and report.md.
"""

from __future__ import annotations

import argparse
import json
import os
import pathlib
import shutil
import sys

from PIL import Image, ImageChops, ImageDraw

CHANNEL_TOLERANCE = 8  # of 255, per channel
ALLOWED_PIXELS = 0     # pixels that may exceed it

SHEET_WIDTH = 400      # width of one picture on a contact sheet
SHEET_GAP = 16


def load(path: pathlib.Path) -> Image.Image:
    with Image.open(path) as image:
        return image.convert("RGB")


def differing(a: Image.Image, b: Image.Image, tolerance: int) -> tuple[int, Image.Image]:
    """How many pixels differ by more than `tolerance` in a channel, and where."""
    difference = ImageChops.difference(a, b)
    red, green, blue = difference.split()
    worst = ImageChops.lighter(ImageChops.lighter(red, green), blue)
    mask = worst.point(lambda value: 255 if value > tolerance else 0)
    return mask.histogram()[255], mask


def diff_picture(reference: Image.Image, mask: Image.Image) -> Image.Image:
    """The reference, faded to grey, with every differing pixel in magenta."""
    grey = reference.convert("L").point(lambda value: 128 + value // 3).convert("RGB")
    magenta = Image.new("RGB", reference.size, (255, 0, 255))
    return Image.composite(magenta, grey, mask)


def combination(name: str) -> str:
    """buttons-dark-rtl-long-1x -> dark-rtl-long-1x; probe-1x -> probe."""
    stem = name.removesuffix(".png")
    for marker in ("-dark-", "-light-"):
        if marker in stem:
            return stem[stem.index(marker) + 1:]
    return "probe"


def contact_sheets(directory: pathlib.Path, names: list[str], out: pathlib.Path) -> list[str]:
    """One sheet per combination of variant, direction and scale: its pages side by side."""
    groups: dict[str, list[str]] = {}
    for name in names:
        groups.setdefault(combination(name), []).append(name)
    written = []
    for group, members in sorted(groups.items()):
        tiles = []
        for name in members:
            image = load(directory / name)
            height = max(1, round(image.height * SHEET_WIDTH / image.width))
            tiles.append((name, image.resize((SHEET_WIDTH, height), Image.Resampling.LANCZOS)))
        width = SHEET_GAP + len(tiles) * (SHEET_WIDTH + SHEET_GAP)
        height = max(tile.height for _, tile in tiles) + 2 * SHEET_GAP + 14
        sheet = Image.new("RGB", (width, height), (96, 96, 96))
        draw = ImageDraw.Draw(sheet)
        x = SHEET_GAP
        for name, tile in tiles:
            draw.text((x, 4), name, fill=(255, 255, 255))
            sheet.paste(tile, (x, SHEET_GAP + 6))
            x += SHEET_WIDTH + SHEET_GAP
        target = out / f"contact-{group}.png"
        sheet.save(target, optimize=True)
        written.append(target.name)
    return written


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--actual", required=True, type=pathlib.Path)
    parser.add_argument("--repeat", required=True, type=pathlib.Path)
    parser.add_argument("--reference", required=True, type=pathlib.Path)
    parser.add_argument("--out", required=True, type=pathlib.Path)
    args = parser.parse_args()

    for name in ("reference", "diff", "proposed"):
        (args.out / name).mkdir(parents=True, exist_ok=True)

    actual = sorted(path.name for path in args.actual.glob("*.png"))
    references = sorted(path.name for path in args.reference.glob("*.png"))
    for name in references:
        shutil.copy2(args.reference / name, args.out / "reference" / name)

    rows = []
    for name in actual:
        picture = load(args.actual / name)
        row = {"name": name, "size": f"{picture.width}x{picture.height}", "status": "match", "detail": ""}

        again = args.repeat / name
        if not again.exists():
            row.update(status="not repeatable", detail="the second run did not produce this picture")
        else:
            second = load(again)
            if second.size != picture.size:
                row.update(status="not repeatable", detail=f"the second run was {second.width}x{second.height}")
            else:
                count, _ = differing(picture, second, 0)
                if count:
                    row.update(status="not repeatable", detail=f"{count} pixels differ between two runs")

        if row["status"] == "match":
            if name not in references:
                row.update(status="no reference", detail="new picture; look at it, then commit it as the reference")
            else:
                reference = load(args.reference / name)
                if reference.size != picture.size:
                    row.update(status="different",
                               detail=f"size {picture.width}x{picture.height}, reference {reference.width}x{reference.height}")
                else:
                    count, mask = differing(picture, reference, CHANNEL_TOLERANCE)
                    exact, _ = differing(picture, reference, 0)
                    if count > ALLOWED_PIXELS:
                        share = 100.0 * count / (picture.width * picture.height)
                        row.update(status="different", detail=f"{count} pixels ({share:.3f} %) beyond the tolerance")
                        diff_picture(reference, mask).save(args.out / "diff" / name, optimize=True)
                    elif exact:
                        row["detail"] = f"{exact} pixels differ within the tolerance"

        if row["status"] != "match":
            # The same pixels, packed as tightly as PNG allows: a reference is
            # kept in the repository for good.
            picture.save(args.out / "proposed" / name, optimize=True)
        rows.append(row)

    stale = [name for name in references if name not in actual]
    for name in stale:
        rows.append({"name": name, "size": "", "status": "stale reference",
                     "detail": "no picture of this name was rendered; remove the reference"})

    sheets = contact_sheets(args.actual, actual, args.out) if actual else []

    failed = [row for row in rows if row["status"] != "match"]
    report = {
        "channel_tolerance": CHANNEL_TOLERANCE,
        "allowed_pixels": ALLOWED_PIXELS,
        "pictures": len(actual),
        "references": len(references),
        "matched": len(rows) - len(failed),
        "failed": len(failed),
        "stale": stale,
        "sheets": sheets,
        "rows": rows,
    }
    (args.out / "report.json").write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")

    lines = [
        "### Screenshots",
        "",
        f"{len(actual)} pictures rendered, {len(references)} references, "
        f"**{report['matched']} match, {len(failed)} do not**. "
        f"Tolerance: {CHANNEL_TOLERANCE}/255 per channel, {ALLOWED_PIXELS} pixels beyond it; "
        "every picture is rendered twice and the two must be identical.",
        "",
    ]
    if not actual:
        lines += ["**No picture was rendered.**", ""]
    if failed:
        lines += ["| Picture | Size | Result | Detail |", "|---|---|---|---|"]
        lines += [f"| `{row['name']}` | {row['size']} | **{row['status']}** | {row['detail']} |" for row in failed]
        lines += ["",
                  "The artifact `desktop-screenshots` has the pictures (`actual/`), the references, the "
                  "differences (`diff/`) and the proposed new references (`proposed/`). To accept them after "
                  "looking: `desktop/ci/update-references.py RUN_ID` (desktop/README.md, \"Reference images\").",
                  ""]
    within = [row for row in rows if row["status"] == "match" and row["detail"]]
    if within:
        lines += ["Matches that are not exact:", ""]
        lines += [f"- `{row['name']}`: {row['detail']}" for row in within]
        lines.append("")
    text = "\n".join(lines)
    (args.out / "report.md").write_text(text, encoding="utf-8")
    print(text)
    summary = os.environ.get("GITHUB_STEP_SUMMARY")
    if summary:
        with open(summary, "a", encoding="utf-8") as handle:
            handle.write(text + "\n")

    for row in failed:
        print(f"::error::screenshot {row['name']}: {row['status']}: {row['detail']}")
    if not actual:
        print("::error::no picture was rendered")
        return 1
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
