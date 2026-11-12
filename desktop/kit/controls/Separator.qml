// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick

// A hairline between things. Decoration only: nothing may be understood
// through it, which is why its colour is exempt from the contrast rule.
Rectangle {
    implicitWidth: Theme.spaceGroup
    implicitHeight: Theme.lineWidth
    color: Theme.lineSubtle

    Accessible.ignored: true
}
