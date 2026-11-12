// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
//
// TUR's defaults for Firefox (desktop-experience.md, 12.4, B6). Firefox reads
// the files in this directory in name order, so these come after the base's
// firefox-redhat-default-prefs.js and replace its start page and pinned tab.
// They are defaults: a person's own choice in Firefox still wins.
pref("browser.startup.homepage", "about:home");
pref("browser.newtabpage.pinned", "[]");
pref("startup.homepage_override_url", "");
