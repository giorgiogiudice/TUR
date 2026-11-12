// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#pragma once

#include <QDBusArgument>
#include <QList>
#include <QMetaType>
#include <QString>

namespace Settings {

// The names and paths of the two interfaces (desktop/apps/tur-settings/dbus).
namespace Bus {
inline QString settingsService() { return QStringLiteral("org.tur.Settings1"); }
inline QString settingsPath() { return QStringLiteral("/org/tur/Settings1"); }
inline QString settingsInterface() { return QStringLiteral("org.tur.Settings1"); }
inline QString addressablePath() { return QStringLiteral("/org/tur/Addressable1"); }
inline QString addressableInterface() { return QStringLiteral("org.tur.Addressable1"); }
} // namespace Bus

// One match of org.tur.Settings1.Search: (ssd).
struct SearchHit
{
    QString address;
    QString title;
    double score = 0;
};

// A rectangle relative to the program's window, in the window's own
// (logical) pixels: (dddd), as org.tur.PaxOverlay1.HighlightInWindow takes it.
struct RectD
{
    double x = 0;
    double y = 0;
    double width = 0;
    double height = 0;
};

QDBusArgument &operator<<(QDBusArgument &argument, const SearchHit &hit);
const QDBusArgument &operator>>(const QDBusArgument &argument, SearchHit &hit);
QDBusArgument &operator<<(QDBusArgument &argument, const RectD &rect);
const QDBusArgument &operator>>(const QDBusArgument &argument, RectD &rect);

// Tells Qt D-Bus about the two types. Call it once before using either.
void registerBusTypes();

} // namespace Settings

Q_DECLARE_METATYPE(Settings::SearchHit)
Q_DECLARE_METATYPE(Settings::RectD)
