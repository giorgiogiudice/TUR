// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#pragma once

#include <QObject>
#include <QtQml/qqmlregistration.h>

// What the desktop says about appearance, for the tokens to follow: whether it
// prefers a light scheme, and how fast animations should run.
//
// This reads; it decides nothing. Theme and Motion are bound to it and a
// program may override either.
class SystemAppearance : public QObject
{
    Q_OBJECT
    QML_ELEMENT
    QML_SINGLETON

    // True only when the desktop states a preference for a light scheme. No
    // stated preference means dark: TUR is dark by default.
    Q_PROPERTY(bool prefersLight READ prefersLight NOTIFY prefersLightChanged FINAL)

    // The desktop's animation speed: 1 is normal, 0 is "no animations"
    // (reduced motion). Qt 6.11 has no reduced-motion hint, so this is KDE's
    // AnimationDurationFactor, read from kdeglobals.
    Q_PROPERTY(double animationFactor READ animationFactor NOTIFY animationFactorChanged FINAL)

public:
    explicit SystemAppearance(QObject *parent = nullptr);

    bool prefersLight() const;
    double animationFactor() const;

    // Reads the settings again. The animation factor is not watched for
    // changes yet; a program that needs to follow it calls this.
    Q_INVOKABLE void reload();

    // TUR_ANIMATION_FACTOR if set, else [KDE] AnimationDurationFactor from the
    // most specific kdeglobals that has it, else 1.
    static double readAnimationFactor();

    // For programs outside a user's session: do not ask the desktop whether
    // it prefers light. There is no desktop to ask, and Qt would guess from
    // its own default palette. Such a program is dark unless it sets
    // Theme.light itself. Set through TurKit::useStandaloneLook().
    static void setStandalone(bool standalone);
    static bool isStandalone();

    // The rule itself: light only if the desktop says light and is asked.
    static bool lightFor(Qt::ColorScheme scheme, bool standalone);

Q_SIGNALS:
    void prefersLightChanged();
    void animationFactorChanged();

private:
    void readColorScheme();

    bool m_prefersLight = false;
    double m_animationFactor = 1.0;
};
