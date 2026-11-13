// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#include "appssource.h"

#include <QChar>

bool AppsRequest::streams() const
{
    return command == Install || command == Update || command == Remove;
}

bool AppsRequest::hasSubject() const
{
    return command != List && command != ListWithUpdates && command != Sources;
}

QString AppsRequest::commandName() const
{
    switch (command) {
    case Plan:
        return QStringLiteral("plan");
    case Install:
        return QStringLiteral("install");
    case Update:
        return QStringLiteral("update");
    case Remove:
        return QStringLiteral("remove");
    case List:
    case ListWithUpdates:
        return QStringLiteral("list");
    case Sources:
    case EnableSource:
    case DisableSource:
        break;
    }
    return QStringLiteral("sources");
}

QStringList AppsRequest::arguments() const
{
    QStringList out;
    switch (command) {
    case Plan:
        out << QStringLiteral("plan") << QStringLiteral("--json");
        break;
    case Install:
        // Confirmed in advance: the person confirmed in the plan's sheet.
        out << QStringLiteral("install") << QStringLiteral("--yes") << QStringLiteral("--events");
        break;
    case Update:
        out << QStringLiteral("update") << QStringLiteral("--events");
        break;
    case Remove:
        // Never without the person's answer: there is no default here, as
        // over the proposed D-Bus interface.
        out << QStringLiteral("remove")
            << (deleteData ? QStringLiteral("--delete-data") : QStringLiteral("--keep-data"))
            << QStringLiteral("--events");
        break;
    case List:
        out << QStringLiteral("list") << QStringLiteral("--json");
        break;
    case ListWithUpdates:
        out << QStringLiteral("list") << QStringLiteral("--check-updates") << QStringLiteral("--json");
        break;
    case Sources:
        out << QStringLiteral("sources") << QStringLiteral("--json");
        break;
    case EnableSource:
        out << QStringLiteral("sources") << QStringLiteral("enable") << QStringLiteral("--json");
        break;
    case DisableSource:
        out << QStringLiteral("sources") << QStringLiteral("disable") << QStringLiteral("--json");
        break;
    }
    if ((command == Plan || command == Install) && !installerFile.isEmpty())
        out << QStringLiteral("--installer") << installerFile;
    if (hasSubject())
        out << QStringLiteral("--") << subject;
    return out;
}

AppsSource::AppsSource(QObject *parent)
    : QObject(parent)
{
    qRegisterMetaType<AppsReply>();
}

AppsSource::~AppsSource() = default;

bool AppsSource::isUsableSubject(const QString &subject)
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

bool AppsSource::isUsableRequest(const AppsRequest &request)
{
    if (request.hasSubject() && !isUsableSubject(request.subject))
        return false;
    // A chosen file is a path, given whole; it may contain spaces.
    if (!request.installerFile.isEmpty()) {
        if (request.installerFile.size() > 4096)
            return false;
        for (const QChar character : request.installerFile) {
            if (character.category() == QChar::Other_Control)
                return false;
        }
    }
    return true;
}

quint64 AppsSource::nextId()
{
    return ++m_lastId;
}
