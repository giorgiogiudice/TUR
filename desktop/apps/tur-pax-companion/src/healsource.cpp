// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#include "healsource.h"

#include <QFileInfo>
#include <QJsonArray>
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

QString HealSource::defaultStateFile()
{
    return QStringLiteral("/var/log/tur/heal/state.json");
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
    , m_stateFile(defaultStateFile())
{
    // The healing service writes its state in more than one step.
    m_settle.setSingleShot(true);
    m_settle.setInterval(700);
    connect(&m_settle, &QTimer::timeout, this, &HealSource::refresh);
    connect(&m_watcher, &QFileSystemWatcher::fileChanged, this, [this] {
        watch();
        m_settle.start();
    });
    connect(&m_watcher, &QFileSystemWatcher::directoryChanged, this, [this] {
        watch();
        m_settle.start();
    });
    connect(&m_process, &QProcess::finished, this, &HealSource::finished);
    connect(&m_process, &QProcess::errorOccurred, this, [this](QProcess::ProcessError error) {
        if (error == QProcess::FailedToStart)
            setProblem(tr("The healing service's program could not be started."));
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

void HealSource::setStateFile(const QString &file)
{
    if (file == m_stateFile)
        return;
    m_stateFile = file;
    Q_EMIT stateFileChanged();
    if (m_started)
        watch();
}

void HealSource::setProblem(const QString &problem)
{
    if (problem == m_problem)
        return;
    m_problem = problem;
    Q_EMIT problemChanged();
}

void HealSource::watch()
{
    if (m_stateFile.isEmpty())
        return;
    // The file is replaced, not rewritten, so its directory is watched too;
    // a watch on a file that was replaced has to be set again.
    const QFileInfo info(m_stateFile);
    if (info.exists() && !m_watcher.files().contains(m_stateFile))
        m_watcher.addPath(m_stateFile);
    const QString directory = info.absolutePath();
    if (QFileInfo::exists(directory) && !m_watcher.directories().contains(directory))
        m_watcher.addPath(directory);
}

void HealSource::start()
{
    m_started = true;
    watch();
    refresh();
}

void HealSource::refresh()
{
    if (m_process.state() != QProcess::NotRunning) {
        m_again = true;
        return;
    }
    const QStringList command = m_command.isEmpty() ? defaultCommand() : m_command;
    m_process.start(command.constFirst(), command.mid(1), QIODevice::ReadOnly);
}

void HealSource::finished(int exitCode, QProcess::ExitStatus status)
{
    const QByteArray printed = m_process.readAllStandardOutput();
    if (status != QProcess::NormalExit || exitCode != 0) {
        setProblem(tr("The healing service's notices could not be read (exit status %1).").arg(exitCode));
    } else if (ingest(QString::fromUtf8(printed)) < 0) {
        setProblem(tr("The healing service's notices were not in the form expected."));
    } else {
        setProblem(QString());
    }
    if (m_again) {
        m_again = false;
        m_settle.start();
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
