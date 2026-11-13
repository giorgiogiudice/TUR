// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#include "appsprocess.h"

#include "processsource.h"

#include <QMetaObject>
#include <QTimer>

#include <algorithm>
#include <utility>

struct AppsProcess::Job
{
    quint64 id = 0;
    AppsRequest request;
    QStringList arguments;
    QProcess *process = nullptr;
    QTimer *timer = nullptr;
    QByteArray output;
    QByteArray pending; // a line not complete yet
    QByteArray errorOutput;
    bool cancelSent = false;
};

AppsProcess::AppsProcess(const Settings &settings, QObject *parent)
    : AppsSource(parent)
    , m_settings(settings)
{
    m_settings.timeoutMs = std::max(1, m_settings.timeoutMs);
    m_settings.maximumOutputBytes = std::max<qint64>(1, m_settings.maximumOutputBytes);
}

AppsProcess::~AppsProcess()
{
    const QList<Job *> running = m_running.values();
    m_running.clear();
    for (Job *job : running) {
        QProcess *process = job->process;
        if (process) {
            process->disconnect(this);
            if (process->state() != QProcess::NotRunning) {
                // An installation is asked to take back what it did; a
                // change that cannot be cancelled is let finish. Only what
                // does not end in time is stopped.
                if (job->request.command == AppsRequest::Install && !job->cancelSent) {
                    process->write("cancel\n");
                    process->waitForBytesWritten(1000);
                }
                const int wait = job->request.streams() ? m_settings.shutdownWaitMs : 0;
                if (!process->waitForFinished(wait)) {
                    process->kill();
                    process->waitForFinished(2000);
                }
            }
        }
        delete job;
    }
}

QStringList AppsProcess::arguments(const AppsRequest &request, const Settings &settings)
{
    QStringList out;
    if (!settings.catalogueDirectory.isEmpty())
        out << QStringLiteral("--catalogue") << settings.catalogueDirectory;
    if (!settings.hostFacts.isEmpty())
        out << QStringLiteral("--host") << settings.hostFacts;
    if (!settings.preferences.isEmpty())
        out << QStringLiteral("--prefs") << settings.preferences;
    out += request.arguments();
    return out;
}

quint64 AppsProcess::submit(const AppsRequest &request)
{
    Job *job = new Job;
    job->id = nextId();
    job->request = request;
    job->arguments = arguments(request, m_settings);
    m_running.insert(job->id, job);
    // Never from inside submit(): the caller has the number first.
    QMetaObject::invokeMethod(
        this,
        [this, id = job->id] {
            Job *job = m_running.value(id, nullptr);
            if (!job)
                return;
            if (!isUsableRequest(job->request)) {
                AppsReply reply;
                reply.failure = AppsReply::Refused;
                answer(job, reply);
                return;
            }
            start(job);
        },
        Qt::QueuedConnection);
    return job->id;
}

void AppsProcess::cancel(quint64 id)
{
    Job *job = m_running.value(id, nullptr);
    if (!job || job->request.command != AppsRequest::Install || job->cancelSent || !job->process)
        return;
    job->cancelSent = true;
    job->process->write("cancel\n");
}

bool AppsProcess::launch(const QString &applicationId, QString *problem)
{
    if (!isUsableSubject(applicationId)) {
        if (problem)
            *problem = QStringLiteral("The identifier cannot be passed on.");
        return false;
    }
    QStringList args;
    if (!m_settings.catalogueDirectory.isEmpty())
        args << QStringLiteral("--catalogue") << m_settings.catalogueDirectory;
    args << QStringLiteral("launch") << QStringLiteral("--") << applicationId;
    QProcess process;
    process.setProgram(m_settings.program);
    process.setArguments(args);
    process.setStandardInputFile(QProcess::nullDevice());
    process.setStandardOutputFile(QProcess::nullDevice());
    process.setStandardErrorFile(QProcess::nullDevice());
    qint64 pid = 0;
    if (!process.startDetached(&pid)) {
        if (problem)
            *problem = ProcessSource::commandLine(m_settings.program, args) + QStringLiteral(": ")
                       + process.errorString();
        return false;
    }
    return true;
}

int AppsProcess::running() const
{
    return int(m_running.size());
}

void AppsProcess::start(Job *job)
{
    auto *process = new QProcess(this);
    job->process = process;
    process->setProgram(m_settings.program);
    process->setArguments(job->arguments);
    process->setProcessChannelMode(QProcess::SeparateChannels);
    const bool install = job->request.command == AppsRequest::Install;
    if (!install)
        process->setStandardInputFile(QProcess::nullDevice());

    connect(process, &QProcess::readyReadStandardOutput, this, [this, job] { readOutput(job); });
    connect(process, &QProcess::readyReadStandardError, this, [this, job] { readError(job); });
    connect(process, &QProcess::errorOccurred, this, [this, job](QProcess::ProcessError error) {
        if (error == QProcess::FailedToStart)
            failedToStart(job);
    });
    connect(process, &QProcess::finished, this,
            [this, job](int exitCode, QProcess::ExitStatus status) { ended(job, exitCode, status); });

    if (!job->request.streams()) {
        job->timer = new QTimer(this);
        job->timer->setSingleShot(true);
        connect(job->timer, &QTimer::timeout, this, [this, job] { timedOut(job); });
        job->timer->start(m_settings.timeoutMs);
    }

    process->start(install ? QIODevice::ReadWrite : QIODevice::ReadOnly);
}

void AppsProcess::readOutput(Job *job)
{
    const QByteArray chunk = job->process->readAllStandardOutput();
    job->output += chunk;
    if (job->output.size() > m_settings.maximumOutputBytes) {
        AppsReply reply;
        reply.failure = AppsReply::TooLarge;
        reply.detail = QStringLiteral("More than %1 bytes of output; the program was stopped.")
                           .arg(m_settings.maximumOutputBytes);
        reply.errorOutput = job->errorOutput;
        answer(job, reply);
        return;
    }
    if (!job->request.streams())
        return;
    job->pending += chunk;
    qsizetype end;
    while ((end = job->pending.indexOf('\n')) >= 0) {
        const QByteArray line = job->pending.left(end).trimmed();
        job->pending.remove(0, end + 1);
        if (!line.isEmpty())
            Q_EMIT this->line(job->id, line);
        // The receiver may have let go of the job.
        if (!m_running.contains(job->id))
            return;
    }
}

void AppsProcess::readError(Job *job)
{
    job->errorOutput += job->process->readAllStandardError();
    if (job->errorOutput.size() > MaximumErrorBytes)
        job->errorOutput.truncate(MaximumErrorBytes);
}

void AppsProcess::failedToStart(Job *job)
{
    AppsReply reply;
    reply.failure = AppsReply::NotStarted;
    reply.detail = job->process->errorString();
    answer(job, reply);
}

void AppsProcess::ended(Job *job, int exitCode, QProcess::ExitStatus status)
{
    readError(job);
    if (job->process->bytesAvailable() > 0 || !job->pending.isEmpty()) {
        readOutput(job);
        if (!m_running.contains(job->id))
            return;
        // A last line without its end.
        const QByteArray rest = job->pending.trimmed();
        job->pending.clear();
        if (job->request.streams() && !rest.isEmpty()) {
            Q_EMIT line(job->id, rest);
            if (!m_running.contains(job->id))
                return;
        }
    }

    AppsReply reply;
    reply.errorOutput = job->errorOutput;
    if (status == QProcess::CrashExit) {
        reply.failure = AppsReply::Crashed;
        reply.detail = job->process->errorString();
    } else {
        reply.exitStatus = exitCode;
        reply.output = job->output;
    }
    answer(job, reply);
}

void AppsProcess::timedOut(Job *job)
{
    AppsReply reply;
    reply.failure = AppsReply::TimedOut;
    reply.detail = QStringLiteral("No answer after %1 ms; the program was stopped.").arg(m_settings.timeoutMs);
    reply.errorOutput = job->errorOutput;
    answer(job, reply);
}

void AppsProcess::answer(Job *job, AppsReply reply)
{
    const quint64 id = job->id;
    reply.commandLine = ProcessSource::commandLine(m_settings.program, job->arguments);
    retire(job);
    Q_EMIT finished(id, reply);
}

void AppsProcess::retire(Job *job)
{
    m_running.remove(job->id);
    if (job->timer) {
        job->timer->stop();
        job->timer->deleteLater();
    }
    if (QProcess *process = job->process) {
        process->disconnect(this);
        if (process->state() != QProcess::NotRunning) {
            connect(process, &QProcess::finished, process, &QObject::deleteLater);
            process->kill();
        } else {
            process->deleteLater();
        }
    }
    delete job;
}
