// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#include "fixturesource.h"

#include "processsource.h"

#include <QDir>
#include <QFile>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QJsonParseError>
#include <QJsonValue>
#include <QMetaObject>

FixtureSource::FixtureSource(const QString &directory, QObject *parent)
    : CatalogueSource(parent)
    , m_directory(directory)
{
    load();
}

QString FixtureSource::key(const QString &command, const QString &subject)
{
    return command + QLatin1Char('\n') + subject;
}

void FixtureSource::load()
{
    const QDir directory(m_directory);
    if (m_directory.isEmpty() || !directory.exists()) {
        m_problem = QStringLiteral("The fixture directory %1 does not exist.").arg(m_directory);
        return;
    }
    const QStringList names = directory.entryList({ QStringLiteral("*.json") }, QDir::Files, QDir::Name);
    if (names.isEmpty()) {
        m_problem = QStringLiteral("The fixture directory %1 holds no .json file.").arg(m_directory);
        return;
    }

    for (const QString &name : names) {
        const auto refuse = [this, &name](const QString &why) {
            m_problem = QStringLiteral("The fixture file %1 cannot be used: %2").arg(name, why);
            m_invocations.clear();
            m_index.clear();
        };

        QFile file(directory.filePath(name));
        if (!file.open(QIODevice::ReadOnly))
            return refuse(file.errorString());
        if (file.size() > MaximumFileBytes)
            return refuse(QStringLiteral("it is larger than %1 bytes.").arg(MaximumFileBytes));
        QJsonParseError error;
        const QJsonDocument document = QJsonDocument::fromJson(file.readAll(), &error);
        if (error.error != QJsonParseError::NoError)
            return refuse(error.errorString());
        const QJsonValue list = document.object().value(QLatin1String("invocations"));
        if (!list.isArray())
            return refuse(QStringLiteral("it has no list \"invocations\"."));

        const QJsonArray invocations = list.toArray();
        for (const QJsonValue &value : invocations) {
            const QJsonObject object = value.toObject();
            const QString command = object.value(QLatin1String("command")).toString();
            const QJsonValue subject = object.value(QLatin1String("subject"));
            const QJsonValue status = object.value(QLatin1String("exit_status"));
            const QJsonValue output = object.value(QLatin1String("stdout"));

            Recorded recorded;
            if (command == QLatin1String("search"))
                recorded.request.command = CatalogueRequest::Search;
            else if (command == QLatin1String("show"))
                recorded.request.command = CatalogueRequest::Show;
            else if (command == QLatin1String("route"))
                recorded.request.command = CatalogueRequest::Route;
            else if (command == QLatin1String("validate"))
                recorded.request.command = CatalogueRequest::Validate;
            else
                return refuse(QStringLiteral("an invocation has the command \"%1\".").arg(command));
            if (!subject.isString() || !status.isDouble() || !(output.isObject() || output.isNull()))
                return refuse(QStringLiteral("an invocation of \"%1\" lacks its subject, exit status or output.")
                                  .arg(command));

            recorded.request.subject = subject.toString();
            recorded.host = object.value(QLatin1String("host")).toString();
            const QJsonArray arguments = object.value(QLatin1String("arguments")).toArray();
            for (const QJsonValue &argument : arguments)
                recorded.arguments.append(argument.toString());
            recorded.exitStatus = status.toInt();
            if (output.isObject())
                recorded.output = QJsonDocument(output.toObject()).toJson(QJsonDocument::Compact);
            recorded.errorOutput = object.value(QLatin1String("stderr")).toString().toUtf8();
            recorded.file = name;

            const QString entry = key(command, recorded.request.subject);
            if (m_index.contains(entry))
                return refuse(QStringLiteral("\"%1 %2\" is recorded twice.").arg(command, recorded.request.subject));
            m_index.insert(entry, m_invocations.size());
            m_invocations.append(recorded);
        }
    }
}

QString FixtureSource::problem() const
{
    return m_problem;
}

QList<FixtureSource::Recorded> FixtureSource::invocations() const
{
    return m_invocations;
}

quint64 FixtureSource::submit(const CatalogueRequest &request)
{
    const quint64 id = nextId();
    m_pending.insert(id);
    // Later, through the event loop: the caller has the number first, and
    // takes the same path as with a real program.
    QMetaObject::invokeMethod(this, [this, id, request] { deliver(id, request); }, Qt::QueuedConnection);
    return id;
}

void FixtureSource::cancel(quint64 id)
{
    m_pending.remove(id);
}

void FixtureSource::deliver(quint64 id, const CatalogueRequest &request)
{
    if (!m_pending.remove(id))
        return;

    CatalogueReply reply;
    QStringList asked { request.commandName(), QStringLiteral("--json") };
    if (request.hasSubject())
        asked << QStringLiteral("--") << request.subject;
    reply.commandLine = ProcessSource::commandLine(QStringLiteral("tur-catalogue"), asked);

    if (request.hasSubject() && !isUsableSubject(request.subject)) {
        reply.failure = CatalogueReply::Refused;
    } else if (!m_problem.isEmpty()) {
        reply.failure = CatalogueReply::NotRecorded;
        reply.detail = m_problem;
    } else if (const auto found = m_index.constFind(key(request.commandName(), request.subject));
               found != m_index.constEnd()) {
        const Recorded &recorded = m_invocations.at(found.value());
        reply.exitStatus = recorded.exitStatus;
        reply.output = recorded.output;
        reply.errorOutput = recorded.errorOutput;
        reply.commandLine = ProcessSource::commandLine(QStringLiteral("tur-catalogue"), recorded.arguments);
        reply.detail = QStringLiteral("Recorded answer, from the fixture file %1").arg(recorded.file);
    } else {
        reply.failure = CatalogueReply::NotRecorded;
        reply.detail = QStringLiteral("The fixture directory %1 holds no answer for this question.").arg(m_directory);
    }
    Q_EMIT finished(id, reply);
}
