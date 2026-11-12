// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#include "documents.h"

#include <QFile>

Documents::Documents(QObject *parent)
    : QObject(parent)
{
}

QString Documents::text(const QString &name) const
{
    QString path;
    if (name == QLatin1String("licence"))
        path = QStringLiteral(":/tur/settings/texts/LICENSE.md");
    else if (name == QLatin1String("notice"))
        path = QStringLiteral(":/tur/settings/texts/NOTICE");
    else if (name == QLatin1String("third-party"))
        path = QStringLiteral(":/tur/settings/texts/THIRD_PARTY_NOTICES.md");
    else
        return QString();

    QFile file(path);
    if (!file.open(QIODevice::ReadOnly))
        return QString();
    return QString::fromUtf8(file.readAll()).trimmed();
}
