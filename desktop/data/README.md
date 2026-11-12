<!-- SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0 -->
# desktop/data/

Side B of the licence boundary: files installed as they are, with no code in
them ([desktop-experience.md](../../docs/architecture/desktop-experience.md),
sections 4 and 14.2). Empty until task D-04.

```text
theme/                 look-and-feel, colour schemes, icons, wallpaper
defaults/              xdg/, skel/, kwinrulesrc, plasmalogin, the System Settings
                       entry, the service menu, the search announcement
plymouth/              the boot splash
kde-settings-map.json  the mapping of KDE's settings (D-20)
```

The colour schemes are to be generated from
`desktop/kit/tokens/tokens.json`, the same table TUR's programs use, and
written from the format's key names: no file here starts as a copy of one of
KDE's.
