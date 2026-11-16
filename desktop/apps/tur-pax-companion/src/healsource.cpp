// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#include "healsource.h"

#include <QJsonDocument>
#include <QJsonValue>

namespace {

// A body longer than this does not fit one bubble; the title is said
// instead, and the body is what "Show me" opens.
constexpr int bodyLimit = 180;

} // namespace

QStringList HealSource::defaultCommand()
{
    return { QStringLiteral("tur-diag"), QStringLiteral("heal"), QStringLiteral("notices"), QStringLiteral("--json") };
}

QString HealSource::kindOf(const QString &event)
{
    if (event == QLatin1String("repaired") || event == QLatin1String("fixed-by-update")
        || event == QLatin1String("undone") || event == QLatin1String("held"))
        return QStringLiteral("repair");
    if (event == QLatin1String("detected") || event == QLatin1String("not-repaired")
        || event == QLatin1String("report-offered") || event == QLatin1String("recurred-after-update"))
        return QStringLiteral("fault");
    return QString();
}

QVariantMap HealSource::noticeFrom(const QJsonObject &heal)
{
    const QString kind = kindOf(heal.value(QLatin1String("kind")).toString());
    const QString title = heal.value(QLatin1String("title")).toString().trimmed();
    const QString body = heal.value(QLatin1String("body")).toString().trimmed();
    const QJsonValue id = heal.value(QLatin1String("id"));
    if (kind.isEmpty() || !id.isDouble() || (title.isEmpty() && body.isEmpty()))
        return {};
    QVariantMap notice;
    notice.insert(QStringLiteral("id"), QStringLiteral("heal:%1").arg(qint64(id.toDouble())));
    notice.insert(QStringLiteral("kind"), kind);
    // The healing service is TUR's, not P@x's.
    notice.insert(QStringLiteral("sender"), QStringLiteral("tur"));
    const bool brief = !body.isEmpty() && body.size() <= bodyLimit;
    notice.insert(QStringLiteral("sentence"), brief ? body : (title.isEmpty() ? body.left(bodyLimit) : title));
    notice.insert(QStringLiteral("title"), title);
    notice.insert(QStringLiteral("detail"), body);
    notice.insert(QStringLiteral("code"), heal.value(QLatin1String("code")).toString());
    notice.insert(QStringLiteral("event"), heal.value(QLatin1String("kind")).toString());
    notice.insert(QStringLiteral("at"), heal.value(QLatin1String("at_utc")).toString());
    QStringList commands;
    const QJsonArray actions = heal.value(QLatin1String("actions")).toArray();
    for (const QJsonValue &action : actions) {
        if (action.isString())
            commands.append(action.toString());
    }
    notice.insert(QStringLiteral("commands"), commands);
    notice.insert(QStringLiteral("actions"), QStringList { QStringLiteral("show"), QStringLiteral("dismiss") });
    notice.insert(QStringLiteral("opens"), QStringLiteral("notice"));
    return notice;
}

HealSource::HealSource(QObject *parent)
    : QObject(parent)
{
    connect(&m_process, &QProcess::finished, this, &HealSource::finished);
    connect(&m_process, &QProcess::errorOccurred, this, [this](QProcess::ProcessError error) {
        if (error != QProcess::FailedToStart)
            return;
        setProblem(tr("The healing service's program could not be started."));
        // `finished` does not follow a program that never started.
        const QList<qint64> ids = m_reading;
        m_reading.clear();
        answer(ids, nullptr);
        if (!m_asked.isEmpty())
            run();
    });
}

void HealSource::setCompanion(Companion *companion)
{
    if (companion == m_companion)
        return;
    m_companion = companion;
    Q_EMIT companionChanged();
}

void HealSource::setCommand(const QStringList &command)
{
    if (command == m_command)
        return;
    m_command = command;
    Q_EMIT commandChanged();
}

void HealSource::setProblem(const QString &problem)
{
    if (problem == m_problem)
        return;
    m_problem = problem;
    Q_EMIT problemChanged();
}

void HealSource::announce(qint64 id)
{
    m_asked.append(id);
    if (m_process.state() == QProcess::NotRunning && m_reading.isEmpty())
        run();
}

void HealSource::run()
{
    m_reading = m_asked;
    m_asked.clear();
    const QStringList command = m_command.isEmpty() ? defaultCommand() : m_command;
    m_process.start(command.constFirst(), command.mid(1), QIODevice::ReadOnly);
}

void HealSource::finished(int exitCode, QProcess::ExitStatus status)
{
    const QByteArray printed = m_process.readAllStandardOutput();
    const QList<qint64> ids = m_reading;
    m_reading.clear();
    const QJsonDocument document = QJsonDocument::fromJson(printed);
    if (status != QProcess::NormalExit || exitCode != 0) {
        setProblem(tr("The healing service's notices could not be read (exit status %1).").arg(exitCode));
        answer(ids, nullptr);
    } else if (!document.isArray()) {
        setProblem(tr("The healing service's notices were not in the form expected."));
        answer(ids, nullptr);
    } else {
        setProblem(QString());
        const QJsonArray notices = document.array();
        answer(ids, &notices);
    }
    if (!m_asked.isEmpty())
        run();
}

void HealSource::answer(const QList<qint64> &ids, const QJsonArray *notices)
{
    if (notices) {
        int count = 0;
        for (const QJsonValue &entry : *notices) {
            if (!noticeFrom(entry.toObject()).isEmpty())
                ++count;
        }
        if (count != m_count) {
            m_count = count;
            Q_EMIT countChanged();
        }
    }
    for (const qint64 id : ids) {
        QString result = QStringLiteral("unread");
        if (notices) {
            result = QStringLiteral("unknown");
            for (const QJsonValue &entry : *notices) {
                const QJsonObject heal = entry.toObject();
                if (!heal.value(QLatin1String("id")).isDouble() || qint64(heal.value(QLatin1String("id")).toDouble()) != id)
                    continue;
                const QVariantMap notice = noticeFrom(heal);
                if (notice.isEmpty())
                    break;
                result = m_companion ? m_companion->announce(notice) : QStringLiteral("hidden");
                break;
            }
        }
        Q_EMIT announced(id, result);
    }
}

int HealSource::ingest(const QString &json)
{
    const QJsonDocument document = QJsonDocument::fromJson(json.toUtf8());
    if (!document.isArray())
        return -1;
    const QJsonArray notices = document.array();
    int count = 0;
    for (const QJsonValue &entry : notices) {
        const QVariantMap notice = noticeFrom(entry.toObject());
        if (notice.isEmpty())
            continue;
        ++count;
        if (m_companion)
            m_companion->offer(notice);
    }
    if (count != m_count) {
        m_count = count;
        Q_EMIT countChanged();
    }
    return count;
}
