// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#include "systemappearance.h"

#include <QGuiApplication>
#include <QSettings>
#include <QStandardPaths>
#include <QStringList>
#include <QStyleHints>
#include <QVariant>

namespace {
// Anything slower than this is a mistake in a configuration file, not a wish.
constexpr double MaximumFactor = 10.0;

bool standaloneLook = false;
} // namespace

void SystemAppearance::setStandalone(bool standalone)
{
    standaloneLook = standalone;
}

bool SystemAppearance::isStandalone()
{
    return standaloneLook;
}

bool SystemAppearance::lightFor(Qt::ColorScheme scheme, bool standalone)
{
    return !standalone && scheme == Qt::ColorScheme::Light;
}

SystemAppearance::SystemAppearance(QObject *parent)
    : QObject(parent)
{
    reload();
    connect(QGuiApplication::styleHints(), &QStyleHints::colorSchemeChanged, this,
            [this](Qt::ColorScheme) { readColorScheme(); });
}

bool SystemAppearance::prefersLight() const
{
    return m_prefersLight;
}

double SystemAppearance::animationFactor() const
{
    return m_animationFactor;
}

void SystemAppearance::reload()
{
    readColorScheme();

    const double factor = readAnimationFactor();
    if (factor != m_animationFactor) {
        m_animationFactor = factor;
        Q_EMIT animationFactorChanged();
    }
}

void SystemAppearance::readColorScheme()
{
    const bool light = lightFor(QGuiApplication::styleHints()->colorScheme(), standaloneLook);
    if (light != m_prefersLight) {
        m_prefersLight = light;
        Q_EMIT prefersLightChanged();
    }
}

double SystemAppearance::readAnimationFactor()
{
    bool ok = false;
    const double fromEnvironment = qEnvironmentVariable("TUR_ANIMATION_FACTOR").toDouble(&ok);
    if (ok && fromEnvironment >= 0.0)
        return qMin(fromEnvironment, MaximumFactor);

    // locateAll lists the user's file first, then the system's.
    const QStringList files =
        QStandardPaths::locateAll(QStandardPaths::GenericConfigLocation, QStringLiteral("kdeglobals"));
    for (const QString &file : files) {
        const QSettings settings(file, QSettings::IniFormat);
        const QVariant value = settings.value(QStringLiteral("KDE/AnimationDurationFactor"));
        if (!value.isValid())
            continue;
        const double factor = value.toString().toDouble(&ok);
        if (ok && factor >= 0.0)
            return qMin(factor, MaximumFactor);
    }
    return 1.0;
}
