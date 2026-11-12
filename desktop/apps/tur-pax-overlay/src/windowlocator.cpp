// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#include "windowlocator.h"

NoWindowLocator::NoWindowLocator(const QString &why, QObject *parent)
    : WindowLocator(parent)
    , m_why(why)
{
}
