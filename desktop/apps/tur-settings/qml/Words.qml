// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma Singleton

import QtQuick

// The sentences more than one page uses, and the ones whose exact wording is
// somebody's decision. The titles of pages, sections and rows are not here:
// they are in the registry (registry/org.tur.Settings.json).
QtObject {
    id: words

    // A value the system's own files do not hold. Nothing is put in its
    // place.
    readonly property string notRecorded: qsTr("Not recorded")

    // A row whose source is not part of the system.
    readonly property string notAvailable: qsTr("Not available yet")

    // The statement of origin (desktop-experience.md, 12.2): the second of
    // the two forms the design gives, the one that uses no Fedora mark as
    // branding. Which form TUR uses is the owner's and the lawyer's decision
    // (the design's open question 3); this is the text until they decide.
    readonly property string origin: qsTr("TUR is built from software packaged by the Fedora Project and many other projects. TUR is not provided, endorsed or supported by the Fedora Project or by Red Hat. Official Fedora software is available at https://fedoraproject.org/.")

    // What an address separator looks like in a path: Appearance › Light or dark.
    readonly property string pathSeparator: " › "

    // The page's path and the row's section, as one line.
    function where(path: var, section: string): string {
        const steps = []
        for (const step of path)
            steps.push(qsTranslate("SettingsRegistry", step))
        if (section.length > 0)
            steps.push(qsTranslate("SettingsRegistry", section))
        return steps.join(words.pathSeparator)
    }
}
