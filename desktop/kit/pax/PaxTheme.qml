// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
//
// GENERATED from tokens/tokens.json by tokens/gen_tokens.py. Do not edit.
// Change tokens.json, then run: python3 desktop/kit/tokens/gen_tokens.py --write
pragma Singleton

import QtQuick
import Tur.Kit as Kit

// Gold is P@x and nothing else (desktop-experience.md, section 5.1).
// Only P@x's surfaces import Tur.Kit.Pax; the boundary check enforces it.
QtObject {
    // pax: P@x's name, icons and lines on a surface
    readonly property color pax:      Kit.Theme.light ? "#7A5200" : "#E8B44C"
    // fill: Fill of P@x's pointer, pill and tray glyph
    readonly property color fill:     Kit.Theme.light ? "#E8B44C" : "#E8B44C"
    // fillText: Text and outline on pax.fill
    readonly property color fillText: Kit.Theme.light ? "#1A1204" : "#1A1204"

    // The overlay's palette is fixed: it sits over content TUR does not control.
    readonly property color overlayFill: "#E8B44C"
    readonly property color overlayOutline: "#0B1220"
    readonly property color overlayHairline: "#FFFFFF"
    readonly property color overlayText: "#1A1204"
    readonly property color overlayQuietText: "#EEF1F6"
    readonly property color overlayPaused: "#A9B4C6"
}
