// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#include "processsource.h"

#include <QMetaObject>
#include <QRegularExpression>
#include <QTimer>

#include <algorithm>
#include <utility>

struct ProcessSource::Job
{
    quint64 id = 0;
    CatalogueRequest request;
    QStringList arguments;
    QProcess *process = nullptr;
    QTimer *timer = nullptr;
    QByteArray output;
    QByteArray errorOutput;
};

ProcessSource::ProcessSource(const Settings &settings, QObject *parent)
    : CatalogueSource(parent)
    , m_settings(settings)
{
    m_settings.concurrency = std::max(1, m_settings.concurrency);
    m_settings.timeoutMs = std::max(1, m_settings.timeoutMs);
    m_settings.maximumOutputBytes = std::max<qint64>(1, m_settings.maximumOutputBytes);
}

ProcessSource::~ProcessSource()
{
    qDeleteAll(m_queue);
    m_queue.clear();
    const QList<Job *> running = m_running.values();
    m_running.clear();
    for (Job *job : running) {
        // The process is a child of this object and is destroyed with it;
        // its destructor stops a program that is still running.
        if (job->process) {
            job->process->disconnect(this);
            job->process->kill();
            // Reaped here, so that nothing of it is left behind. A program
            // that has been killed ends at once.
            job->process->waitForFinished(2000);
        }
        delete job;
    }
}

QStringList ProcessSource::arguments(const CatalogueRequest &request, const Settings &settings)
{
    QStringList out;
    if (!settings.catalogueDirectory.isEmpty())
        out << QStringLiteral("--catalogue") << settings.catalogueDirectory;
    out << request.commandName() << QStringLiteral("--json");
    if (request.command == CatalogueRequest::Route) {
        if (!settings.hostFacts.isEmpty())
            out << QStringLiteral("--host") << settings.hostFacts;
        if (!settings.preferences.isEmpty())
            out << QStringLiteral("--prefs") << settings.preferences;
    }
    // Everything after "--" is a value, whatever it begins with.
    out << QStringLiteral("--") << request.subject;
    return out;
}

QString ProcessSource::commandLine(const QString &program, const QStringList &arguments)
{
    static const QRegularExpression plain(QStringLiteral("^[A-Za-z0-9_./:=@%+,-]+$"));
    QStringList words { program };
    words += arguments;
    for (QString &word : words) {
        if (!plain.match(word).hasMatch()) {
            word.replace(QLatin1Char('\''), QStringLiteral("'\\''"));
            word = QLatin1Char('\'') + word + QLatin1Char('\'');
        }
    }
    return words.join(QLatin1Char(' '));
}

quint64 ProcessSource::submit(const CatalogueRequest &request)
{
    Job *job = new Job;
    job->id = nextId();
    job->request = request;
    job->arguments = arguments(request, m_settings);
    m_queue.append(job);
    // Never from inside submit(): the caller has the number first.
    QMetaObject::invokeMethod(this, &ProcessSource::pump, Qt::QueuedConnection);
    return job->id;
}

void ProcessSource::cancel(quint64 id)
{
    for (qsizetype i = 0; i < m_queue.size(); ++i) {
        if (m_queue.at(i)->id == id) {
            delete m_queue.takeAt(i);
            return;
        }
    }
    if (Job *job = m_running.value(id, nullptr)) {
        retire(job);
        QMetaObject::invokeMethod(this, &ProcessSource::pump, Qt::QueuedConnection);
    }
}

int ProcessSource::running() const
{
    return int(m_running.size());
}

int ProcessSource::queued() const
{
    return int(m_queue.size());
}

void ProcessSource::pump()
{
    while (!m_queue.isEmpty() && m_running.size() < m_settings.concurrency) {
        Job *job = m_queue.takeFirst();
        m_running.insert(job->id, job);
        if (!isUsableSubject(job->request.subject)) {
            CatalogueReply reply;
            reply.failure = CatalogueReply::Refused;
            answer(job, reply);
            continue;
        }
        start(job);
    }
}

void ProcessSource::start(Job *job)
{
    auto *process = new QProcess(this);
    job->process = process;
    process->setProgram(m_settings.program);
    process->setArguments(job->arguments);
    process->setProcessChannelMode(QProcess::SeparateChannels);
    process->setStandardInputFile(QProcess::nullDevice());

    connect(process, &QProcess::readyReadStandardOutput, this, [this, job] { readOutput(job); });
    connect(process, &QProcess::readyReadStandardError, this, [this, job] { readError(job); });
    connect(process, &QProcess::errorOccurred, this, [this, job](QProcess::ProcessError error) {
        // A program that crashes also ends: ended() reports that.
        if (error == QProcess::FailedToStart)
            failedToStart(job);
    });
    connect(process, &QProcess::finished, this,
            [this, job](int exitCode, QProcess::ExitStatus status) { ended(job, exitCode, status); });

    job->timer = new QTimer(this);
    job->timer->setSingleShot(true);
    connect(job->timer, &QTimer::timeout, this, [this, job] { timedOut(job); });
    job->timer->start(m_settings.timeoutMs);

    process->start(QIODevice::ReadOnly);
}

void ProcessSource::readOutput(Job *job)
{
    job->output += job->process->readAllStandardOutput();
    if (job->output.size() > m_settings.maximumOutputBytes) {
        CatalogueReply reply;
        reply.failure = CatalogueReply::TooLarge;
        reply.detail = QStringLiteral("More than %1 bytes of output; the program was stopped.")
                           .arg(m_settings.maximumOutputBytes);
        reply.errorOutput = job->errorOutput;
        answer(job, reply);
    }
}

void ProcessSource::readError(Job *job)
{
    job->errorOutput += job->process->readAllStandardError();
    if (job->errorOutput.size() > MaximumErrorBytes)
        job->errorOutput.truncate(MaximumErrorBytes);
}

void ProcessSource::failedToStart(Job *job)
{
    CatalogueReply reply;
    reply.failure = CatalogueReply::NotStarted;
    reply.detail = job->process->errorString();
    answer(job, reply);
}

void ProcessSource::ended(Job *job, int exitCode, QProcess::ExitStatus status)
{
    readError(job);
    job->output += job->process->readAllStandardOutput();

    CatalogueReply reply;
    reply.errorOutput = job->errorOutput;
    if (job->output.size() > m_settings.maximumOutputBytes) {
        reply.failure = CatalogueReply::TooLarge;
        reply.detail = QStringLiteral("More than %1 bytes of output.").arg(m_settings.maximumOutputBytes);
    } else if (status == QProcess::CrashExit) {
        reply.failure = CatalogueReply::Crashed;
        reply.detail = job->process->errorString();
    } else {
        reply.exitStatus = exitCode;
        reply.output = job->output;
    }
    answer(job, reply);
}

void ProcessSource::timedOut(Job *job)
{
    CatalogueReply reply;
    reply.failure = CatalogueReply::TimedOut;
    reply.detail = QStringLiteral("No answer after %1 ms; the program was stopped.").arg(m_settings.timeoutMs);
    reply.errorOutput = job->errorOutput;
    answer(job, reply);
}

void ProcessSource::answer(Job *job, CatalogueReply reply)
{
    const quint64 id = job->id;
    reply.commandLine = commandLine(m_settings.program, job->arguments);
    retire(job);
    QMetaObject::invokeMethod(this, &ProcessSource::pump, Qt::QueuedConnection);
    Q_EMIT finished(id, reply);
}

// Lets go of a job: nothing more is heard from its program, which is stopped
// if it still runs.
void ProcessSource::retire(Job *job)
{
    m_running.remove(job->id);
    if (job->timer) {
        job->timer->stop();
        job->timer->deleteLater();
    }
    if (QProcess *process = job->process) {
        process->disconnect(this);
        if (process->state() != QProcess::NotRunning) {
            // Destroyed once it has really ended, so that Qt does not have
            // to wait for it.
            connect(process, &QProcess::finished, process, &QObject::deleteLater);
            process->kill();
        } else {
            process->deleteLater();
        }
    }
    delete job;
}
