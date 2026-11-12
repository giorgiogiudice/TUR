// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#include "address.h"

#include <QLatin1String>
#include <QRegularExpression>

namespace Settings {

namespace {

const QRegularExpression &pagePattern()
{
    static const QRegularExpression pattern(QStringLiteral("\\A(?:tur|kde)(?:\\.[a-z0-9_-]+)+\\z"));
    return pattern;
}

const QRegularExpression &rowPattern()
{
    static const QRegularExpression pattern(QStringLiteral("\\A[a-z0-9-]+\\z"));
    return pattern;
}

} // namespace

QString Address::nameSpace() const
{
    return page.section(QLatin1Char('.'), 0, 0);
}

QString Address::toString() const
{
    return hasRow() ? page + QLatin1Char('#') + row : page;
}

bool Address::isPage(const QString &text)
{
    return text.size() <= MaximumPageLength && pagePattern().match(text).hasMatch();
}

bool Address::isRowId(const QString &text)
{
    return text.size() <= MaximumRowLength && rowPattern().match(text).hasMatch();
}

Address Address::parse(const QString &text)
{
    QString rest = text;
    const QLatin1String scheme("tur-settings:");
    if (rest.startsWith(scheme)) {
        rest.remove(0, scheme.size());
        // A link may arrive with its fragment sign encoded.
        const qsizetype encoded = rest.indexOf(QLatin1String("%23"));
        if (encoded >= 0 && !rest.contains(QLatin1Char('#')))
            rest.replace(encoded, 3, QLatin1Char('#'));
    }

    Address out;
    const qsizetype hash = rest.indexOf(QLatin1Char('#'));
    const QString page = hash < 0 ? rest : rest.left(hash);
    if (!isPage(page))
        return out;
    if (hash >= 0) {
        const QString row = rest.mid(hash + 1);
        if (!isRowId(row))
            return out;
        out.row = row;
    }
    out.page = page;
    return out;
}

} // namespace Settings
