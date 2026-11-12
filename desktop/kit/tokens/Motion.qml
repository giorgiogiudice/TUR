// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
pragma Singleton

import QtQuick

// Durations for animations (desktop-experience.md, 5.5). Three lengths, ease
// out, nothing bounces. Every animation in a TUR program takes its duration
// from here, so that reduced motion is honoured in one place.
QtObject {
    id: motion

    // The desktop's animation speed: 1 is normal, 0 is reduced motion. It
    // follows the desktop until a program assigns it.
    property real factor: SystemAppearance.animationFactor

    // Tests set this: nothing animates at all, so a picture is repeatable.
    property bool frozen: false

    readonly property bool reduced: motion.factor <= 0

    // For something that moves or changes size. Zero when motion is reduced:
    // nothing moves.
    readonly property int control: motion.frozen ? 0 : Math.round(Theme.motionControl * motion.factor)
    readonly property int appear: motion.frozen ? 0 : Math.round(Theme.motionAppear * motion.factor)
    readonly property int panel: motion.frozen ? 0 : Math.round(Theme.motionPanel * motion.factor)

    // For a cross-fade. When motion is reduced a fade is still allowed, but
    // never longer than 100 ms.
    readonly property int fadeControl: motion.fade(Theme.motionControl)
    readonly property int fadeAppear: motion.fade(Theme.motionAppear)
    readonly property int fadePanel: motion.fade(Theme.motionPanel)

    // Whether an indicator of real activity (a spinner, an indeterminate
    // progress bar) may loop. Nothing else loops, ever.
    readonly property bool loops: !motion.frozen && !motion.reduced

    readonly property int easing: Easing.OutCubic

    function fade(base: int): int {
        if (motion.frozen)
            return 0
        if (motion.reduced)
            return Math.min(base, Theme.motionReducedFade)
        return Math.round(base * motion.factor)
    }
}
