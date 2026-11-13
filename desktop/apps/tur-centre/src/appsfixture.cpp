// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#include "appsfixture.h"

#include "processsource.h"

#include <QDir>
#include <QFile>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QJsonParseError>
#include <QMetaObject>
#include <QTimer>

#include <algorithm>

namespace {

const QString InstallerPlaceholder = QStringLiteral("<installer>");

} // namespace

struct AppsFixture::Playing
{
    AppsRequest request;
    qsizetype index = -1; // into m_invocations; -1: not recorded
    int next = 0;         // the next line to play
    bool cancelled = false;
    bool waiting = false; // stopped at a hold or a cancel point
    QTimer *timer = nullptr;
};

AppsFixture::AppsFixture(const QString &directory, QObject *parent)
    : AppsSource(parent)
    , m_directory(directory)
{
    load();
}

AppsFixture::~AppsFixture()
{
    qDeleteAll(m_playing);
}

void AppsFixture::load()
{
    const QDir directory(m_directory);
    if (m_directory.isEmpty() || !directory.exists()) {
        m_problem = QStringLiteral("The fixture directory %1 does not exist.").arg(m_directory);
        return;
    }
    const QStringList names = directory.entryList({ QStringLiteral("*.json") }, QDir::Files, QDir::Name);
    for (const QString &name : names) {
        const auto refuse = [this, &name](const QString &why) {
            m_problem = QStringLiteral("The fixture file %1 cannot be used: %2").arg(name, why);
            m_invocations.clear();
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
        for (const QJsonValue &value : list.toArray()) {
            const QJsonObject object = value.toObject();
            const QJsonValue arguments = object.value(QLatin1String("arguments"));
            const QJsonValue status = object.value(QLatin1String("exit_status"));
            const QJsonValue output = object.value(QLatin1String("stdout"));
            if (!arguments.isArray() || !status.isDouble() || !output.isArray())
                return refuse(QStringLiteral("an invocation lacks its arguments, exit status or output."));
            Recorded recorded;
            for (const QJsonValue &argument : arguments.toArray())
                recorded.arguments.append(argument.toString());
            recorded.exitStatus = status.toInt();
            for (const QJsonValue &line : output.toArray())
                recorded.lines.append(line.toString().toUtf8());
            recorded.errorOutput = object.value(QLatin1String("stderr")).toString().toUtf8();
            recorded.cancelAfter = object.value(QLatin1String("cancel_after")).toInt(-1);
            recorded.file = name;
            m_invocations.append(recorded);
        }
    }
    if (m_invocations.isEmpty() && m_problem.isEmpty())
        m_problem = QStringLiteral("The fixture directory %1 holds no recorded run.").arg(m_directory);
}

QString AppsFixture::problem() const
{
    return m_problem;
}

QList<AppsFixture::Recorded> AppsFixture::invocations() const
{
    return m_invocations;
}

bool AppsFixture::matches(const QStringList &recorded, const QStringList &asked)
{
    if (recorded.size() != asked.size())
        return false;
    for (qsizetype i = 0; i < recorded.size(); ++i) {
        if (recorded.at(i) == InstallerPlaceholder && i > 0 && recorded.at(i - 1) == QLatin1String("--installer"))
            continue;
        if (recorded.at(i) != asked.at(i))
            return false;
    }
    return true;
}

quint64 AppsFixture::submit(const AppsRequest &request)
{
    const quint64 id = nextId();
    auto *playing = new Playing;
    playing->request = request;
    const QStringList asked = request.arguments();
    qsizetype last = -1;
    for (qsizetype i = 0; i < m_invocations.size(); ++i) {
        if (!matches(m_invocations.at(i).arguments, asked))
            continue;
        last = i;
        if (!m_used.contains(i)) {
            playing->index = i;
            break;
        }
    }
    if (playing->index < 0)
        playing->index = last;
    if (playing->index >= 0)
        m_used.insert(playing->index);
    m_playing.insert(id, playing);

    playing->timer = new QTimer(this);
    playing->timer->setSingleShot(true);
    connect(playing->timer, &QTimer::timeout, this, [this, id] { step(id); });
    playing->timer->start(0);
    return id;
}

void AppsFixture::step(quint64 id)
{
    Playing *playing = m_playing.value(id, nullptr);
    if (!playing)
        return;
    playing->waiting = false;
    if (playing->index < 0 || !isUsableRequest(playing->request) || !m_problem.isEmpty()
        || !playing->request.streams()) {
        finish(id);
        return;
    }
    const Recorded &recorded = m_invocations.at(playing->index);
    if (playing->next >= recorded.lines.size()) {
        finish(id);
        return;
    }
    // Where the recorder sent "cancel": the run goes on only once the
    // application centre has asked to cancel too.
    if (recorded.cancelAfter >= 0 && playing->next == recorded.cancelAfter && !playing->cancelled) {
        playing->waiting = true;
        return;
    }
    if (m_hold >= 0 && playing->next >= m_hold) {
        playing->waiting = true;
        return;
    }
    const QByteArray line = recorded.lines.at(playing->next++);
    Q_EMIT this->line(id, line);
    if (m_playing.contains(id))
        playing->timer->start(m_interval);
}

void AppsFixture::finish(quint64 id)
{
    Playing *playing = m_playing.take(id);
    if (!playing)
        return;
    AppsReply reply;
    const QStringList asked = playing->request.arguments();
    reply.commandLine = ProcessSource::commandLine(QStringLiteral("tur-apps"), asked);
    if (!isUsableRequest(playing->request)) {
        reply.failure = AppsReply::Refused;
    } else if (!m_problem.isEmpty()) {
        reply.failure = AppsReply::NotRecorded;
        reply.detail = m_problem;
    } else if (playing->index < 0) {
        reply.failure = AppsReply::NotRecorded;
        reply.detail = QStringLiteral("The fixture directory %1 holds no recorded run of this request.").arg(m_directory);
    } else {
        const Recorded &recorded = m_invocations.at(playing->index);
        reply.exitStatus = recorded.exitStatus;
        for (const QByteArray &line : recorded.lines)
            reply.output += line + '\n';
        reply.errorOutput = recorded.errorOutput;
        reply.commandLine = ProcessSource::commandLine(QStringLiteral("tur-apps"), recorded.arguments);
        reply.detail = QStringLiteral("Recorded run, from the fixture file %1").arg(recorded.file);
    }
    playing->timer->deleteLater();
    delete playing;
    Q_EMIT finished(id, reply);
}

void AppsFixture::cancel(quint64 id)
{
    Playing *playing = m_playing.value(id, nullptr);
    if (!playing || playing->request.command != AppsRequest::Install || playing->cancelled)
        return;
    playing->cancelled = true;
    if (playing->waiting)
        playing->timer->start(m_interval);
}

bool AppsFixture::launch(const QString &applicationId, QString *problem)
{
    if (!isUsableSubject(applicationId)) {
        if (problem)
            *problem = QStringLiteral("The identifier cannot be passed on.");
        return false;
    }
    m_launched.append(applicationId);
    return true;
}

void AppsFixture::setHold(int lines)
{
    m_hold = lines;
}

void AppsFixture::release()
{
    m_hold = -1;
    for (auto it = m_playing.cbegin(); it != m_playing.cend(); ++it) {
        if (it.value()->waiting)
            it.value()->timer->start(m_interval);
    }
}

QStringList AppsFixture::launched() const
{
    return m_launched;
}

void AppsFixture::setInterval(int milliseconds)
{
    m_interval = std::max(0, milliseconds);
}
