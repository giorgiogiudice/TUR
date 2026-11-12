<!-- SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0 -->
# desktop/data/

Side B of the licence boundary: files installed as they are, with no code in
them ([desktop-experience.md](../../docs/architecture/desktop-experience.md),
sections 4 and 14.2). Task D-04 put TUR's theme and the defaults of a new
account here; the design and its decisions are in
[tur-theme.md](../../docs/architecture/tur-theme.md).

```text
rootfs/            what the image installs, laid out as on the installed system;
                   image/Containerfile copies it with one COPY, as it is
  usr/share/color-schemes/TurDark.colors, TurLight.colors     GENERATED
  usr/share/plasma/look-and-feel/org.tur.dark/, org.tur.light/
                   the two global themes: metadata.json and contents/defaults
  usr/share/tur/xdg/          TUR's system defaults for KDE (kdeglobals,
                              ksplashrc, plasma-welcomerc, kicker-extra-favoritesrc)
  etc/xdg/                    a link to each of those: the one place all three
                              kinds of session read
  etc/skel/.config/           the panel of a new account
gen_theme.py       generates the colour schemes from desktop/kit/tokens/tokens.json,
                   and checks everything in rootfs/ (see its --help)
schemes.json       GENERATED: the schemes as data, for the picture below
MANIFEST.tsv       GENERATED: every installed file with its SHA-256, or a link
                   with its target; the boot test compares the image with it
tests/render_schemes.qml   the reference picture of both schemes, drawn by
                   the screenshots job (desktop/tests/screenshots/colour-scheme-*)
```

No file here starts as a copy of one of KDE's: the colour schemes are written
from the format's key names with TUR's values, and the rest by hand from the
keys Plasma 6.8 reads, which `gen_theme.py` lists with the source file each
was read from.

## Changing something

```sh
# a colour: change desktop/kit/tokens/tokens.json (and the design first), then
python3 desktop/data/gen_theme.py --write   # schemes, schemes.json, MANIFEST.tsv
python3 desktop/data/gen_theme.py --check   # what CI runs; --table prints the contrast
```

A hand-written file under `rootfs/` needs `--write` too, for the manifest.
Then push: the `desktop` workflow checks the tree and redraws the picture of
the schemes (accept it as in desktop/README.md, "Reference images"); the
`boot` workflow builds the image and signs a new account in to see the
result (tests/boot/splash.py, tests/boot/desktop_checks.py).

**Gold is not used here.** It is P@x's colour and nothing else's (design,
5.1); `gen_theme.py` fails on any of P@x's colours or any colour of the gold
family anywhere in `rootfs/`.

Still to come here: `plymouth/` stays in `branding/` and `image/` for now,
`kde-settings-map.json` is task D-20.
