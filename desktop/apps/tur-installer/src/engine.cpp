// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#include "engine.h"

#include "transcript.h"

#include <QDir>
#include <QFile>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonParseError>
#include <QProcess>
#include <QRegularExpression>
#include <algorithm>
#include <utility>

namespace {

// Lines of output kept for the screen and the drawer; the whole of it is in
// the engine's log on the medium.
constexpr int keptLines = 400;

QStringList readLines(const QString &path)
{
    QFile file(path);
    if (!file.open(QIODevice::ReadOnly | QIODevice::Text))
        return {};
    QStringList lines;
    while (!file.atEnd()) {
        const QString line = QString::fromUtf8(file.readLine()).trimmed();
        if (!line.isEmpty())
            lines << line;
    }
    return lines;
}

QString quoted(const QString &word)
{
    static const QRegularExpression plain(QStringLiteral("^[A-Za-z0-9_./:=@%+,-]+$"));
    if (plain.match(word).hasMatch())
        return word;
    QString escaped = word;
    escaped.replace(QLatin1Char('\''), QStringLiteral("'\\''"));
    return QLatin1Char('\'') + escaped + QLatin1Char('\'');
}

} // namespace

// ---- EngineRun -------------------------------------------------------------

EngineRun::EngineRun(QObject *parent)
    : QObject(parent)
{
    m_timer.setSingleShot(true);
    connect(&m_timer, &QTimer::timeout, this, &EngineRun::replayNext);
}

EngineRun::~EngineRun()
{
    if (m_process && m_process->state() != QProcess::NotRunning) {
        // A step that only looks (check, disks, sticks) is stopped; an
        // installation was released before this, and is not ours to stop.
        m_process->disconnect(this);
        m_process->kill();
        m_process->waitForFinished(2000);
    }
}

void EngineRun::startProcess(const QString &program, const QStringList &arguments)
{
    m_fixture = false;
    QStringList words { program };
    words += arguments;
    for (QString &word : words)
        word = quoted(word);
    m_commandLine = words.join(QLatin1Char(' '));
    m_process = new QProcess(this);
    m_process->setProcessChannelMode(QProcess::SeparateChannels);
    connect(m_process, &QProcess::readyReadStandardOutput, this, &EngineRun::readOutput);
    connect(m_process, &QProcess::readyReadStandardError, this, [this] {
        m_errorOutput += m_process->readAllStandardError();
        if (m_errorOutput.size() > 65536)
            m_errorOutput = m_errorOutput.right(65536);
    });
    connect(m_process, &QProcess::finished, this, [this](int exitCode, QProcess::ExitStatus status) {
        readOutput();
        if (!m_buffer.isEmpty()) {
            deliver(m_buffer);
            m_buffer.clear();
        }
        if (m_ended)
            return;
        m_ended = true;
        Q_EMIT ended(status == QProcess::NormalExit ? exitCode : -1,
                     status == QProcess::NormalExit ? QString() : QStringLiteral("the engine stopped unexpectedly"));
    });
    connect(m_process, &QProcess::errorOccurred, this, [this](QProcess::ProcessError error) {
        if (error != QProcess::FailedToStart || m_ended)
            return;
        m_ended = true;
        Q_EMIT ended(-1, m_process->errorString());
    });
    m_process->start(program, arguments);
}

void EngineRun::startFixture(const QStringList &lines, int intervalMs)
{
    m_fixture = true;
    m_lines = lines;
    m_next = 0;
    m_interval = std::max(0, intervalMs);
    m_commandLine = QStringLiteral("(recorded answers)");
    m_timer.start(0);
}

void EngineRun::replayNext()
{
    if (m_ended)
        return;
    if (m_next >= m_lines.size()) {
        m_ended = true;
        Q_EMIT ended(0, QString());
        return;
    }
    const QByteArray line = m_lines.at(m_next++).toUtf8();
    // {"event":"pause"} holds a replay where it is, with no end: the states
    // in the middle of an installation, for tests and pictures.
    if (line.contains("\"event\":\"pause\""))
        return;
    deliver(line);
    if (!m_ended)
        m_timer.start(m_interval);
}

void EngineRun::stopReplay()
{
    if (!m_fixture)
        return;
    m_timer.stop();
    m_ended = true;
}

void EngineRun::send(const QByteArray &line)
{
    if (m_process && m_process->state() == QProcess::Running)
        m_process->write(line + '\n');
}

bool EngineRun::running() const
{
    if (m_fixture)
        return !m_ended;
    return m_process && m_process->state() != QProcess::NotRunning;
}

void EngineRun::release()
{
    if (!m_process || m_process->state() == QProcess::NotRunning)
        return;
    // A QProcess stops its program when it is destroyed. This one is not
    // destroyed: the program carries on, and when this process ends its
    // pipes close, which the engine does not take as a request to stop.
    m_process->disconnect();
    m_process->setParent(nullptr);
    m_process = nullptr;
}

void EngineRun::readOutput()
{
    if (!m_process)
        return;
    m_buffer += m_process->readAllStandardOutput();
    qsizetype newline;
    while ((newline = m_buffer.indexOf('\n')) >= 0) {
        const QByteArray line = m_buffer.left(newline);
        m_buffer.remove(0, newline + 1);
        deliver(line);
    }
}

void EngineRun::deliver(const QByteArray &line)
{
    const QByteArray trimmed = line.trimmed();
    if (trimmed.isEmpty())
        return;
    QJsonParseError error {};
    const QJsonDocument document = QJsonDocument::fromJson(trimmed, &error);
    if (error.error != QJsonParseError::NoError || !document.isObject())
        return;
    const QJsonObject object = document.object();
    // A recorded answer may begin with a line that only carries its licence.
    if (!object.contains(QStringLiteral("event")))
        return;
    Q_EMIT event(object);
}

// ---- Engine ----------------------------------------------------------------

Engine::Engine(QObject *parent)
    : QObject(parent)
{
}

Engine::~Engine()
{
    if (m_installRun && m_installRun->running() && !m_installRun->isFixture())
        m_installRun->release();
}

void Engine::setFixture(const QString &fixture)
{
    if (fixture == m_fixture)
        return;
    m_fixture = fixture;
    Q_EMIT configurationChanged();
}

void Engine::setInstallFixture(const QString &name)
{
    if (name == m_installFixture)
        return;
    m_installFixture = name;
    Q_EMIT configurationChanged();
}

void Engine::setFixtureInterval(int interval)
{
    if (interval == m_fixtureInterval)
        return;
    m_fixtureInterval = interval;
    Q_EMIT configurationChanged();
}

void Engine::setProgram(const QString &program)
{
    if (program == m_program)
        return;
    m_program = program;
    Q_EMIT configurationChanged();
}

void Engine::setLauncher(const QString &launcher)
{
    if (launcher == m_launcher)
        return;
    m_launcher = launcher;
    Q_EMIT configurationChanged();
}

void Engine::setSessionDirectory(const QString &directory)
{
    if (directory == m_sessionDirectory)
        return;
    m_sessionDirectory = directory;
    Q_EMIT configurationChanged();
}

EngineRun *Engine::start(const QString &step, const QStringList &arguments, const QString &fixtureFile)
{
    auto *run = new EngineRun(this);
    QStringList words { step };
    words += arguments;
    if (!m_fixture.isEmpty()) {
        const QString path = QDir(m_fixture).filePath(fixtureFile + QStringLiteral(".jsonl"));
        QStringList lines = readLines(path);
        m_received << QStringLiteral("$ tur-install-engine %1   (recorded answers: %2)").arg(step, path);
        run->startFixture(lines, m_fixtureInterval);
    } else if (m_launcher.isEmpty()) {
        run->startProcess(m_program, words);
    } else {
        run->startProcess(m_launcher, QStringList { m_program } + words);
    }
    m_lastAction = QStringLiteral("tur-install-engine ") + step;
    Q_EMIT outputChanged();
    TranscriptFile::write(QStringLiteral("engine step=%1 fixture=%2").arg(step, m_fixture.isEmpty() ? QStringLiteral("no")
                                                                                                    : QStringLiteral("yes")));
    connect(run, &EngineRun::event, this, [this](const QJsonObject &event) {
        m_received << QString::fromUtf8(QJsonDocument(event).toJson(QJsonDocument::Compact));
        if (m_received.size() > 4 * keptLines)
            m_received = m_received.mid(m_received.size() - 2 * keptLines);
    });
    return run;
}

void Engine::failStep(const QString &step, int exitCode, const QString &problem, EngineRun *run)
{
    QVariantMap failure;
    // pkexec: 126 when the person or polkit said no, 127 when it could not
    // ask (polkit says so on standard error).
    const bool refused = exitCode == 126 || exitCode == 127;
    failure.insert(QStringLiteral("step"), step);
    failure.insert(QStringLiteral("code"), refused ? QStringLiteral("TUR-INSTALL-MEDIUM-004")
                                                   : QStringLiteral("TUR-INSTALL-MEDIUM-005"));
    failure.insert(QStringLiteral("refused"), refused);
    failure.insert(QStringLiteral("exitCode"), exitCode);
    QStringList detail;
    if (run)
        detail << QStringLiteral("$ ") + run->commandLine();
    detail << QStringLiteral("exit status %1").arg(exitCode);
    if (!problem.isEmpty())
        detail << problem;
    if (run && !run->errorOutput().trimmed().isEmpty())
        detail << run->errorOutput().trimmed();
    failure.insert(QStringLiteral("detail"), detail.join(QLatin1Char('\n')));
    m_stepFailure = failure;
    Q_EMIT stepFailureChanged();
    TranscriptFile::write(QStringLiteral("engine-failed step=%1 status=%2 code=%3")
                              .arg(step)
                              .arg(exitCode)
                              .arg(failure.value(QStringLiteral("code")).toString()));
}

void Engine::addOutput(const QString &line)
{
    m_outputLines << line;
    if (m_outputLines.size() > keptLines)
        m_outputLines.removeFirst();
    m_lastAction = line;
    Q_EMIT outputChanged();
}

// --- check ------------------------------------------------------------------

void Engine::runCheck()
{
    if (m_checkRun && m_checkRun->running())
        return;
    m_checkState = QStringLiteral("running");
    m_checks.clear();
    m_blocking.clear();
    m_facts.clear();
    m_payloadDone = 0;
    m_payloadTotal = 0;
    m_stepFailure.clear();
    Q_EMIT stepFailureChanged();
    Q_EMIT checkChanged();
    m_checkRun = start(QStringLiteral("check"), {}, QStringLiteral("check"));
    connect(m_checkRun, &EngineRun::event, this, &Engine::onCheckEvent);
    connect(m_checkRun, &EngineRun::ended, this, [this](int exitCode, const QString &problem) {
        EngineRun *run = m_checkRun;
        if (m_checkState == QStringLiteral("running")) {
            m_checkState = QStringLiteral("failed");
            failStep(QStringLiteral("check"), exitCode, problem, run);
            Q_EMIT checkChanged();
        }
        if (run)
            run->deleteLater();
    });
}

void Engine::onCheckEvent(const QJsonObject &event)
{
    const QString kind = event.value(QStringLiteral("event")).toString();
    if (kind == QStringLiteral("progress")) {
        m_payloadDone = event.value(QStringLiteral("done")).toInt();
        m_payloadTotal = event.value(QStringLiteral("total")).toInt();
        Q_EMIT checkChanged();
    } else if (kind == QStringLiteral("check")) {
        m_checks << event.toVariantMap();
        Q_EMIT checkChanged();
    } else if (kind == QStringLiteral("checks")) {
        m_facts = event.toVariantMap();
        m_blocking = event.value(QStringLiteral("blocking")).toArray().toVariantList();
        m_checkState = QStringLiteral("done");
        TranscriptFile::write(QStringLiteral("checks blocking=%1 payload=%2 secure_boot=%3")
                                  .arg(m_blocking.size())
                                  .arg(m_facts.value(QStringLiteral("payload")).toString(),
                                       m_facts.value(QStringLiteral("secure_boot")).toString()));
        Q_EMIT checkChanged();
    }
}

// --- disks ------------------------------------------------------------------

void Engine::runDisks()
{
    if (m_disksRun && m_disksRun->running())
        return;
    m_disksState = QStringLiteral("running");
    m_pendingDisks.clear();
    m_stepFailure.clear();
    Q_EMIT stepFailureChanged();
    Q_EMIT disksChanged();
    m_disksRun = start(QStringLiteral("disks"), {}, QStringLiteral("disks"));
    connect(m_disksRun, &EngineRun::event, this, &Engine::onDisksEvent);
    connect(m_disksRun, &EngineRun::ended, this, [this](int exitCode, const QString &problem) {
        EngineRun *run = m_disksRun;
        if (m_disksState == QStringLiteral("running")) {
            m_disksState = QStringLiteral("failed");
            failStep(QStringLiteral("disks"), exitCode, problem, run);
            Q_EMIT disksChanged();
        }
        if (run)
            run->deleteLater();
    });
}

void Engine::onDisksEvent(const QJsonObject &event)
{
    const QString kind = event.value(QStringLiteral("event")).toString();
    if (kind == QStringLiteral("disk")) {
        m_pendingDisks << event.toVariantMap();
    } else if (kind == QStringLiteral("disks")) {
        m_disks = m_pendingDisks;
        m_pendingDisks.clear();
        m_selectable = event.value(QStringLiteral("selectable")).toInt();
        m_disksState = QStringLiteral("done");
        for (const QVariant &item : std::as_const(m_disks)) {
            const QVariantMap disk = item.toMap();
            TranscriptFile::write(
                QStringLiteral("disk-listed name=%1 serial=\"%2\" status=%3 windows=%4 bitlocker=%5 partitions=%6 beside=%7")
                    .arg(disk.value(QStringLiteral("name")).toString(), disk.value(QStringLiteral("serial")).toString(),
                         disk.value(QStringLiteral("status")).toString(),
                         disk.value(QStringLiteral("windows")).toBool() ? QStringLiteral("yes") : QStringLiteral("no"),
                         disk.value(QStringLiteral("bitlocker")).toBool() ? QStringLiteral("yes") : QStringLiteral("no"))
                    .arg(disk.value(QStringLiteral("partition_count")).toInt())
                    .arg(!disk.value(QStringLiteral("beside")).isValid() || disk.value(QStringLiteral("beside")).isNull()
                             ? QStringLiteral("none")
                             : disk.value(QStringLiteral("beside")).toMap().value(QStringLiteral("possible")).toBool()
                                 ? QStringLiteral("possible")
                                 : disk.value(QStringLiteral("beside")).toMap().value(QStringLiteral("code")).toString()));
        }
        TranscriptFile::write(QStringLiteral("disks total=%1 selectable=%2").arg(m_disks.size()).arg(m_selectable));
        Q_EMIT disksChanged();
    }
}

// --- install ----------------------------------------------------------------

bool Engine::cancellable() const
{
    return m_installState == QStringLiteral("running") && !m_cancelRequested
           && (m_stage.isEmpty() || m_stage == QStringLiteral("revalidate"));
}

void Engine::install(const QVariantMap &disk, const QString &phrase, const QString &mode, const QString &defaultSystem,
                     double turBytes)
{
    if (m_installRun && m_installRun->running())
        return;
    m_installState = QStringLiteral("running");
    m_stage.clear();
    m_stagesSeen.clear();
    m_bytesWritten = 0;
    m_copyTotal.clear();
    m_cancelRequested = false;
    m_cancelRefused = false;
    m_result.clear();
    m_outputLines.clear();
    Q_EMIT outputChanged();
    Q_EMIT installChanged();
    const bool beside = mode == QStringLiteral("beside");
    const bool shrink = mode == QStringLiteral("shrink");
    QStringList arguments;
    if (shrink) {
        arguments << QStringLiteral("--mode") << QStringLiteral("shrink")
                  << QStringLiteral("--tur-size") << QString::number(static_cast<qint64>(turBytes))
                  << QStringLiteral("--default")
                  << (defaultSystem == QStringLiteral("windows") ? QStringLiteral("windows") : QStringLiteral("tur"));
    }
    if (beside) {
        const QVariantMap room = disk.value(QStringLiteral("beside")).toMap();
        arguments << QStringLiteral("--mode") << QStringLiteral("beside")
                  << QStringLiteral("--region-start") << QString::number(room.value(QStringLiteral("free_start")).toLongLong())
                  << QStringLiteral("--region-size") << QString::number(room.value(QStringLiteral("free_size")).toLongLong())
                  << QStringLiteral("--default")
                  << (defaultSystem == QStringLiteral("windows") ? QStringLiteral("windows") : QStringLiteral("tur"));
    }
    arguments << QStringList {
        QStringLiteral("--by-id"), disk.value(QStringLiteral("by_id")).toString(),
        QStringLiteral("--serial"), disk.value(QStringLiteral("serial")).toString(),
        QStringLiteral("--wwn"), disk.value(QStringLiteral("wwn")).toString(),
        QStringLiteral("--size"), QString::number(disk.value(QStringLiteral("size")).toLongLong()),
        QStringLiteral("--fingerprint"), disk.value(QStringLiteral("fingerprint")).toString(),
        QStringLiteral("--phrase"), phrase,
    };
    TranscriptFile::write(QStringLiteral("install-requested by_id=%1 mode=%2")
                              .arg(disk.value(QStringLiteral("by_id")).toString(),
                                   shrink ? QStringLiteral("shrink") : beside ? QStringLiteral("beside") : QStringLiteral("erase")));
    QString fixtureFile = m_installFixture;
    if (m_installFixture == QStringLiteral("install") && (beside || shrink))
        fixtureFile = shrink ? QStringLiteral("install-shrink") : QStringLiteral("install-beside");
    m_installRun = start(QStringLiteral("install"), arguments, fixtureFile);
    connect(m_installRun, &EngineRun::event, this, &Engine::onInstallEvent);
    connect(m_installRun, &EngineRun::ended, this, [this](int exitCode, const QString &problem) {
        EngineRun *run = m_installRun;
        if (m_installState == QStringLiteral("running")) {
            // The engine ended without saying how. If it was refused before
            // it could start, nothing was written; otherwise the disk may
            // have been.
            const bool beforeWriting = m_stage.isEmpty() || m_stage == QStringLiteral("revalidate");
            QVariantMap outcome;
            outcome.insert(QStringLiteral("outcome"), QStringLiteral("failed"));
            outcome.insert(QStringLiteral("code"), (exitCode == 126 || exitCode == 127)
                                                       ? QStringLiteral("TUR-INSTALL-MEDIUM-004")
                                                       : QStringLiteral("TUR-INSTALL-MEDIUM-005"));
            outcome.insert(QStringLiteral("message"),
                           QStringLiteral("The installation engine stopped without a result (exit status %1). %2")
                               .arg(exitCode)
                               .arg(problem));
            outcome.insert(QStringLiteral("disk_state"), beforeWriting ? QStringLiteral("untouched")
                                                                       : QStringLiteral("unknown"));
            if (run)
                outcome.insert(QStringLiteral("detail"), run->errorOutput().trimmed());
            m_result = outcome;
            m_installState = beforeWriting ? QStringLiteral("refused") : QStringLiteral("failed");
            TranscriptFile::write(QStringLiteral("result outcome=%1 code=%2")
                                      .arg(m_installState, outcome.value(QStringLiteral("code")).toString()));
            Q_EMIT installChanged();
        }
        if (run)
            run->deleteLater();
    });
}

void Engine::onInstallEvent(const QJsonObject &event)
{
    const QString kind = event.value(QStringLiteral("event")).toString();
    if (kind == QStringLiteral("stage")) {
        m_stage = event.value(QStringLiteral("stage")).toString();
        if (!m_stagesSeen.contains(m_stage))
            m_stagesSeen << m_stage;
        TranscriptFile::write(QStringLiteral("stage name=%1 cancellable=%2")
                                  .arg(m_stage, cancellable() ? QStringLiteral("yes") : QStringLiteral("no")));
        Q_EMIT installChanged();
    } else if (kind == QStringLiteral("line")) {
        addOutput(event.value(QStringLiteral("text")).toString());
    } else if (kind == QStringLiteral("written")) {
        m_bytesWritten = event.value(QStringLiteral("bytes")).toDouble();
        Q_EMIT installChanged();
    } else if (kind == QStringLiteral("copy-total")) {
        m_copyTotal = event.value(QStringLiteral("text")).toString();
        Q_EMIT installChanged();
    } else if (kind == QStringLiteral("cancel")) {
        if (!event.value(QStringLiteral("accepted")).toBool()) {
            m_cancelRefused = true;
            Q_EMIT installChanged();
        }
    } else if (kind == QStringLiteral("result")) {
        m_result = event.toVariantMap();
        const QString outcome = m_result.value(QStringLiteral("outcome")).toString();
        if (outcome == QStringLiteral("success") || outcome == QStringLiteral("failed")
            || outcome == QStringLiteral("refused") || outcome == QStringLiteral("cancelled"))
            m_installState = outcome;
        else
            m_installState = QStringLiteral("failed");
        TranscriptFile::write(QStringLiteral("result outcome=%1 code=%2 disk_state=%3")
                                  .arg(m_installState, m_result.value(QStringLiteral("code")).toString(),
                                       m_result.value(QStringLiteral("disk_state")).toString()));
        Q_EMIT installChanged();
    }
}

void Engine::cancel()
{
    if (!m_installRun || !m_installRun->running())
        return;
    if (!cancellable()) {
        m_cancelRefused = true;
        TranscriptFile::write(QStringLiteral("cancel-asked accepted=no stage=%1").arg(m_stage));
        Q_EMIT installChanged();
        return;
    }
    m_cancelRequested = true;
    TranscriptFile::write(QStringLiteral("cancel-asked accepted=pending stage=%1").arg(m_stage));
    if (m_installRun->isFixture()) {
        // A replay has no engine to ask: it answers as the engine would
        // before writing.
        m_installRun->stopReplay();
        QVariantMap outcome;
        outcome.insert(QStringLiteral("outcome"), QStringLiteral("cancelled"));
        outcome.insert(QStringLiteral("code"), QString());
        outcome.insert(QStringLiteral("message"),
                       QStringLiteral("The installation was cancelled before anything was written. Nothing was changed."));
        outcome.insert(QStringLiteral("disk_state"), QStringLiteral("untouched"));
        m_result = outcome;
        m_installState = QStringLiteral("cancelled");
        TranscriptFile::write(QStringLiteral("result outcome=cancelled code= disk_state=untouched"));
    } else {
        m_installRun->send("cancel");
    }
    Q_EMIT installChanged();
}

void Engine::resetInstall()
{
    if (m_installRun && m_installRun->running())
        return;
    m_installState = QStringLiteral("idle");
    m_stage.clear();
    m_stagesSeen.clear();
    m_result.clear();
    m_cancelRequested = false;
    m_cancelRefused = false;
    Q_EMIT installChanged();
}

QString Engine::stageTitle(const QString &stage) const
{
    if (stage == QStringLiteral("revalidate"))
        return tr("Checking the disk again");
    if (stage == QStringLiteral("prepare"))
        return tr("Preparing the disk");
    if (stage == QStringLiteral("copy"))
        return tr("Copying TUR to the disk");
    if (stage == QStringLiteral("bootloader"))
        return tr("Making the computer start TUR");
    if (stage == QStringLiteral("finish"))
        return tr("Finishing the disk");
    if (stage == QStringLiteral("check"))
        return tr("Checking the installation");
    if (stage == QStringLiteral("record"))
        return tr("Keeping the installation record");
    if (stage == QStringLiteral("shrink"))
        return tr("Making room: shrinking Windows");
    if (stage == QStringLiteral("boot-menu"))
        return tr("Adding Windows to TUR's start menu");
    return stage;
}

// --- the log, to a USB stick ------------------------------------------------

void Engine::findSticks()
{
    if (m_sticksRun && m_sticksRun->running())
        return;
    m_sticksState = QStringLiteral("running");
    m_pendingSticks.clear();
    Q_EMIT sticksChanged();
    m_sticksRun = start(QStringLiteral("sticks"), {}, QStringLiteral("sticks"));
    connect(m_sticksRun, &EngineRun::event, this, &Engine::onSticksEvent);
    connect(m_sticksRun, &EngineRun::ended, this, [this](int exitCode, const QString &problem) {
        EngineRun *run = m_sticksRun;
        if (m_sticksState == QStringLiteral("running")) {
            m_sticksState = QStringLiteral("failed");
            failStep(QStringLiteral("sticks"), exitCode, problem, run);
            Q_EMIT sticksChanged();
        }
        if (run)
            run->deleteLater();
    });
}

void Engine::onSticksEvent(const QJsonObject &event)
{
    const QString kind = event.value(QStringLiteral("event")).toString();
    if (kind == QStringLiteral("stick")) {
        m_pendingSticks << event.toVariantMap();
    } else if (kind == QStringLiteral("sticks")) {
        m_sticks = m_pendingSticks;
        m_pendingSticks.clear();
        m_sticksState = QStringLiteral("done");
        TranscriptFile::write(QStringLiteral("sticks found=%1").arg(m_sticks.size()));
        Q_EMIT sticksChanged();
    }
}

void Engine::saveLog(const QString &device)
{
    if (m_saveRun && m_saveRun->running())
        return;
    m_saveState = QStringLiteral("running");
    m_saveResult.clear();
    Q_EMIT sticksChanged();
    m_saveRun = start(QStringLiteral("save-log"), { QStringLiteral("--to"), device }, QStringLiteral("save-log"));
    connect(m_saveRun, &EngineRun::event, this, &Engine::onSaveEvent);
    connect(m_saveRun, &EngineRun::ended, this, [this](int exitCode, const QString &problem) {
        EngineRun *run = m_saveRun;
        if (m_saveState == QStringLiteral("running")) {
            m_saveState = QStringLiteral("failed");
            QVariantMap outcome;
            outcome.insert(QStringLiteral("outcome"), QStringLiteral("failed"));
            outcome.insert(QStringLiteral("message"),
                           QStringLiteral("The log could not be saved (exit status %1). %2").arg(exitCode).arg(problem));
            m_saveResult = outcome;
            Q_EMIT sticksChanged();
        }
        if (run)
            run->deleteLater();
    });
}

void Engine::onSaveEvent(const QJsonObject &event)
{
    if (event.value(QStringLiteral("event")).toString() != QStringLiteral("result"))
        return;
    m_saveResult = event.toVariantMap();
    m_saveState = m_saveResult.value(QStringLiteral("outcome")).toString() == QStringLiteral("success")
                      ? QStringLiteral("done")
                      : QStringLiteral("failed");
    TranscriptFile::write(QStringLiteral("log-saved outcome=%1").arg(m_saveState));
    Q_EMIT sticksChanged();
}

// --- the rest ---------------------------------------------------------------

QString Engine::logText() const
{
    QStringList parts;
    if (m_fixture.isEmpty()) {
        for (const QString &name : { QStringLiteral("installer.log"), QStringLiteral("install.log") }) {
            QFile file(QDir(m_sessionDirectory).filePath(name));
            if (!file.open(QIODevice::ReadOnly | QIODevice::Text))
                continue;
            QByteArray data = file.readAll();
            // The end of a long log is what matters on a screen.
            if (data.size() > 262144)
                data = data.right(262144);
            parts << QStringLiteral("==== %1").arg(file.fileName()) << QString::fromUtf8(data);
        }
    }
    parts << QStringLiteral("==== what the installer received from the engine") << m_received.join(QLatin1Char('\n'));
    return parts.join(QLatin1Char('\n'));
}

void Engine::restart()
{
    m_powerRequest = QStringLiteral("reboot");
    TranscriptFile::write(QStringLiteral("power request=reboot"));
    Q_EMIT powerRequestChanged();
    if (m_fixture.isEmpty()) {
        const bool started = QProcess::startDetached(QStringLiteral("systemctl"), { QStringLiteral("reboot") });
        TranscriptFile::write(QStringLiteral("power systemctl=reboot started=%1").arg(started ? QStringLiteral("yes") : QStringLiteral("no")));
    }
}

void Engine::powerOff()
{
    m_powerRequest = QStringLiteral("poweroff");
    TranscriptFile::write(QStringLiteral("power request=poweroff"));
    Q_EMIT powerRequestChanged();
    if (m_fixture.isEmpty()) {
        const bool started = QProcess::startDetached(QStringLiteral("systemctl"), { QStringLiteral("poweroff") });
        TranscriptFile::write(QStringLiteral("power systemctl=poweroff started=%1").arg(started ? QStringLiteral("yes") : QStringLiteral("no")));
    }
}
