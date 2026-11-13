<!-- SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0 -->
# desktop-7 — The application centre installs, updates and removes (D-11)

DRAFT: the results below are filled in from the final CI run.

## The Chrome decision

The owner asked for Chrome for people coming from Windows
(desktop-experience.md, 17.1, answer 4): offered, not preinstalled,
"installed from Google's own source and updated by it", and the welcome
saying plainly that it comes from Google.

| Way | What it is | Verdict |
|---|---|---|
| Flathub's `com.google.Chrome` | A Flatpak maintained by Flathub's volunteers, **not verified** by Flathub as coming from Google (`/api/v2/verification/com.google.Chrome/status`: `"verified": false`), licence `LicenseRef-proprietary`. It contains no copy of Chrome: its metadata declares *extra data*, Google's Debian package at `https://dl.google.com/linux/chrome/deb/pool/main/g/google-chrome-stable/google-chrome-stable_157.0.8103.78-1_amd64.deb` (147 497 676 bytes) with its SHA-256, which Flatpak downloads from Google and checks at installation, and on every update | **Chosen.** Per user, no administrator, no restart; `tur-apps` already installs it; the browser itself comes from Google's server; TUR bundles nothing |
| Google's RPM repository (`dl.google.com/linux/chrome/rpm/stable/x86_64`, `google-chrome-stable`) | Google's own Linux package and updates | Not chosen: on Fedora Kinoite it means `rpm-ostree install`, a layered system package for every user, administrator rights and a restart (or a live apply), outside `tur-apps`; reports of signature trouble with layering Google's repository |
| An official Google Flatpak | Does not exist. Google merged experimental Flatpak packaging into Chromium's source in August 2026, behind a build flag that is off; Chromium's developers say it does not mean an official Flatpak is coming | When it exists and Flathub verifies it, the catalogue entry changes its `flatpak_id` and `packaging = "publisher"` |

Sources: `https://flathub.org/apps/com.google.Chrome` and Flathub's API
(`/api/v2/appstream/com.google.Chrome`, `/api/v2/summary/com.google.Chrome`,
`/api/v2/verification/com.google.Chrome/status`), read 13 November 2026;
`https://github.com/flathub/com.google.Chrome`; Fedora Kinoite's "Getting
Started" (package layering); reports of Chromium's experimental Flatpak
packaging (It's FOSS, OMG! Ubuntu, August 2026).

What the person reads, on Home, on Chrome's page and in its plan: "Chrome
is made by Google, and Google's terms apply to it. TUR does not include it.
Flatpak downloads it from Google's own server while it installs; the
package around it is kept by Flathub's volunteers, not by Google. It is
updated the same way."
