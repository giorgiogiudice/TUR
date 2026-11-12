<!-- SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0 -->
# TUR's artwork

| File | What |
|---|---|
| `tur-logo.png` | The full logo, for the README and "About" |
| `tur-mark.svg` | **First draft for the owner:** the flat mark, the lotus and flame redrawn in few colours for small sizes and the boot screen (desktop-experience.md, 17.1, answer 5). For dark grounds |
| `tur-icon.svg`, `tur-icon-small.svg` | The mark on a round dark tile, readable on light and dark panels; below 32 px without the infinity sign |
| `tur-mark-symbolic.svg` | One colour, following the text colour, for symbolic icons |
| `tur-wordmark.svg` | "TUR" in silver, after the logo's lettering |
| `png/`, `plymouth/`, `wallpaper/` | Exports at the sizes the image installs; the boot splash's mark and spinner; the wallpaper |
| `render.py` | Draws all of the above from one definition of the geometry |

Colours follow desktop-experience.md, 5.1 and 5.2: the lotus in the system
blues, the flame in silver. Gold is P@x's and is not used here.

To change the mark, edit `render.py` and run `python3 branding/render.py`
(Python's standard library only; about three minutes, most of it the
wallpaper), then look at every picture before committing.
`python3 branding/render.py --check` says which files would change. The image
build installs the files through `image/branding/branding.sh`; it does not run
`render.py`.
