// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick

// Text in one of the kit's type styles (desktop-experience.md, 5.3) and in a
// colour chosen by role. There is no way to ask for gold here: gold is P@x's
// and lives in Tur.Kit.Pax.
Text {
    id: label

    enum Kind {
        Body,
        Strong,
        Small,
        Caps,
        Heading,
        Title,
        Display,
        Mono
    }

    enum Tone {
        Primary,
        Secondary,
        Accent,
        Positive,
        Warning,
        Negative,
        OnAccent,
        OnSelection
    }

    property int kind: Label.Body
    property int tone: Label.Primary

    // For figures that change (progress, time left, cost): every digit has
    // the same width, so the text does not shift.
    property bool tabular: false

    // Running text: a sentence or more that may wrap. Its lines are 1.4 times
    // the font's height apart (desktop-experience.md, 5.3), with the extra
    // space shared above and below each line, so that one line of it still
    // sits level with what is beside it.
    property bool paragraph: false

    readonly property real halfLeading: label.paragraph
                                        ? Math.round(metrics.height * (Theme.lineHeight - 1) / 2) : 0

    readonly property real typeSize: {
        switch (label.kind) {
        case Label.Small:
            return Theme.typeSmallSize
        case Label.Caps:
            return Theme.typeCapsSize
        case Label.Heading:
            return Theme.typeHeadingSize
        case Label.Title:
            return Theme.typeTitleSize
        case Label.Display:
            return Theme.typeDisplaySize
        case Label.Mono:
            return Theme.typeMonoSize
        default:
            return Theme.typeBodySize
        }
    }

    readonly property int typeWeight: {
        switch (label.kind) {
        case Label.Strong:
            return Theme.typeStrongWeight
        case Label.Caps:
            return Theme.typeCapsWeight
        case Label.Heading:
            return Theme.typeHeadingWeight
        case Label.Title:
            return Theme.typeTitleWeight
        case Label.Display:
            return Theme.typeDisplayWeight
        default:
            return Theme.typeBodyWeight
        }
    }

    readonly property color toneColor: {
        switch (label.tone) {
        case Label.Secondary:
            return Theme.textSecondary
        case Label.Accent:
            return Theme.accentText
        case Label.Positive:
            return Theme.positive
        case Label.Warning:
            return Theme.warning
        case Label.Negative:
            return Theme.negative
        case Label.OnAccent:
            return Theme.accentOn
        case Label.OnSelection:
            return Theme.selectionOn
        default:
            return Theme.textPrimary
        }
    }

    color: label.enabled ? label.toneColor : Theme.textDisabled

    font.family: label.kind === Label.Mono ? Theme.monoFamily : Application.font.family
    font.pointSize: label.typeSize * Theme.fontScale
    font.weight: label.typeWeight
    font.capitalization: label.kind === Label.Caps ? Font.AllUppercase : Font.MixedCase
    // 8 % of the size, in pixels at 96 dpi (4/3 px to the point).
    font.letterSpacing: label.kind === Label.Caps ? label.typeSize * Theme.fontScale * 4 / 3 * Theme.capsSpacing : 0
    font.features: label.tabular ? ({ "tnum": 1 }) : ({})

    // Registry text and file names are shown as they are, never as markup.
    textFormat: Text.PlainText
    wrapMode: Text.Wrap
    // "Left" is the start of the line: it becomes the right in a
    // right-to-left layout, whatever script the text is in.
    horizontalAlignment: Text.AlignLeft
    lineHeight: label.paragraph ? Theme.lineHeight : 1.0
    topPadding: label.halfLeading
    bottomPadding: -label.halfLeading

    Accessible.role: label.kind === Label.Heading || label.kind === Label.Title || label.kind === Label.Display
                     ? Accessible.Heading : Accessible.StaticText
    Accessible.name: label.text

    FontMetrics {
        id: metrics

        font: label.font
    }
}
