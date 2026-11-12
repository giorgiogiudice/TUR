// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Shapes

// P@x's flame as a flat mark: the flame at the heart of the lotus in
// branding/tur-logo.png, redrawn in few colours (the owner's answer of 12
// November 2026, desktop-experience.md 17.1, answer 5). A FIRST DRAFT, for
// the owner to look at; PaxMark is kept as it was until the owner has.
//
// An upright flame on a 32 by 32 grid: the tip at the top centre, a round
// body below, an inner flame and a core, as in the logo. With `inner`
// transparent it is a silhouette in `tint`. An outline and a hairline round
// it, for the pointer, are optional.
Item {
    id: flame

    property real size: 32
    property color tint: PaxTheme.overlayFill
    // The inner flame, and the core inside it, as in the logo.
    property color inner: "transparent"
    property color core: flame.tint
    property color outline: "transparent"
    property real outlineWidth: 0
    property color hairline: "transparent"
    property real hairlineWidth: 0

    // The outer flame, the inner flame and the core, on the 32 grid.
    readonly property string outerPath: "M 16 0 C 19 7 27 13 27 21 A 11 11 0 0 1 5 21 C 5 13 13 7 16 0 Z"
    readonly property string innerPath: "M 16 14 C 17.5 16.5 20 19 20 22.5 A 4 4 0 0 1 12 22.5 C 12 19 14.5 16.5 16 14 Z"
    readonly property string corePath: "M 16 20.5 A 2.5 2.5 0 1 1 15.99 20.5 Z"

    implicitWidth: flame.size
    implicitHeight: flame.size

    Accessible.ignored: true

    Shape {
        anchors.fill: parent
        antialiasing: true

        ShapePath {
            strokeColor: flame.hairline
            strokeWidth: flame.hairlineWidth
            fillColor: "transparent"
            joinStyle: ShapePath.RoundJoin
            scale: Qt.size(flame.size / 32, flame.size / 32)

            PathSvg {
                path: flame.outerPath
            }
        }

        ShapePath {
            strokeColor: flame.outline
            strokeWidth: flame.outlineWidth
            fillColor: flame.tint
            joinStyle: ShapePath.RoundJoin
            scale: Qt.size(flame.size / 32, flame.size / 32)

            PathSvg {
                path: flame.outerPath
            }
        }

        ShapePath {
            strokeColor: "transparent"
            strokeWidth: 0
            fillColor: flame.inner
            scale: Qt.size(flame.size / 32, flame.size / 32)

            PathSvg {
                path: flame.innerPath
            }
        }

        ShapePath {
            strokeColor: "transparent"
            strokeWidth: 0
            fillColor: flame.inner.a > 0 ? flame.core : "transparent"
            scale: Qt.size(flame.size / 32, flame.size / 32)

            PathSvg {
                path: flame.corePath
            }
        }
    }
}
