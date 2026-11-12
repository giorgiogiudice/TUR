// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#pragma once

#include <QDBusArgument>
#include <QMetaType>
#include <QRectF>
#include <QString>

namespace PaxOverlay {

// The name and path of org.tur.PaxOverlay1 (dbus/org.tur.PaxOverlay1.xml).
namespace Bus {
inline QString service() { return QStringLiteral("org.tur.PaxOverlay1"); }
inline QString path() { return QStringLiteral("/org/tur/PaxOverlay1"); }
inline QString interface() { return QStringLiteral("org.tur.PaxOverlay1"); }
} // namespace Bus

// A rectangle on the bus: (dddd), x, y, width and height in logical pixels.
// The same shape as org.tur.Settings1.Reveal's `rect`.
struct RectD
{
    double x = 0;
    double y = 0;
    double width = 0;
    double height = 0;

    QRectF toRect() const { return QRectF(x, y, width, height); }
};

QDBusArgument &operator<<(QDBusArgument &argument, const RectD &rect);
const QDBusArgument &operator>>(const QDBusArgument &argument, RectD &rect);

// Tells Qt D-Bus about RectD. Call it once, before registering the adaptor.
void registerBusTypes();

} // namespace PaxOverlay

Q_DECLARE_METATYPE(PaxOverlay::RectD)
