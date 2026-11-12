// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#include "bustypes.h"

#include <QDBusMetaType>

namespace PaxOverlay {

QDBusArgument &operator<<(QDBusArgument &argument, const RectD &rect)
{
    argument.beginStructure();
    argument << rect.x << rect.y << rect.width << rect.height;
    argument.endStructure();
    return argument;
}

const QDBusArgument &operator>>(const QDBusArgument &argument, RectD &rect)
{
    argument.beginStructure();
    argument >> rect.x >> rect.y >> rect.width >> rect.height;
    argument.endStructure();
    return argument;
}

void registerBusTypes()
{
    qDBusRegisterMetaType<RectD>();
}

} // namespace PaxOverlay
