// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#include "cataloguesource.h"

#include <QChar>

QString CatalogueRequest::commandName() const
{
    switch (command) {
    case Show:
        return QStringLiteral("show");
    case Route:
        return QStringLiteral("route");
    case Validate:
        return QStringLiteral("validate");
    case Search:
        break;
    }
    return QStringLiteral("search");
}

CatalogueSource::CatalogueSource(QObject *parent)
    : QObject(parent)
{
    qRegisterMetaType<CatalogueReply>();
}

CatalogueSource::~CatalogueSource() = default;

bool CatalogueSource::isUsableSubject(const QString &subject)
{
    if (subject.isEmpty() || subject.size() > MaximumSubjectLength)
        return false;
    for (const QChar character : subject) {
        const QChar::Category category = character.category();
        if (category == QChar::Other_Control || category == QChar::Separator_Line
            || category == QChar::Separator_Paragraph)
            return false;
    }
    return true;
}

quint64 CatalogueSource::nextId()
{
    return ++m_lastId;
}
