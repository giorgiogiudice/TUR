// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#pragma once

#include <QByteArray>
#include <QJsonObject>
#include <QObject>
#include <QPointer>
#include <QProcess>
#include <QString>
#include <QStringList>
#include <QTimer>
#include <QVariantList>
#include <QVariantMap>
#include <QtQml/qqmlregistration.h>

// One step of the installation engine: a run of
// `pkexec /usr/libexec/tur-installer/tur-install-engine STEP ...`, or, in
// tests and pictures, the replay of a file of its recorded answers. Either
// way it delivers one JSON object per line.
class EngineRun : public QObject
{
    Q_OBJECT

public:
    explicit EngineRun(QObject *parent = nullptr);
    ~EngineRun() override;

    void startProcess(const QString &program, const QStringList &arguments);
    void startFixture(const QStringList &lines, int intervalMs);

    // Sends one line to the program's standard input. Nothing in a replay.
    void send(const QByteArray &line);
    // Stops a replay where it is. A program is not stopped.
    void stopReplay();

    bool isFixture() const { return m_fixture; }
    bool running() const;
    QString commandLine() const { return m_commandLine; }
    QString errorOutput() const { return QString::fromUtf8(m_errorOutput); }

    // Lets a program that is still running carry on after this object and
    // the window are gone: an installation must not be cut off half way.
    void release();

Q_SIGNALS:
    void event(const QJsonObject &event);
    void ended(int exitCode, const QString &problem);

private:
    void readOutput();
    void deliver(const QByteArray &line);
    void replayNext();

    QPointer<QProcess> m_process;
    QByteArray m_buffer;
    QByteArray m_errorOutput;
    QStringList m_lines;
    int m_next = 0;
    int m_interval = 0;
    bool m_fixture = false;
    bool m_ended = false;
    QTimer m_timer;
    QString m_commandLine;
};

// The installer's view of the installation engine
// (installer/iso/rootfs/usr/libexec/tur-installer/tur-install-engine): the
// checks of the computer, the disks, the installation and its progress, and
// saving the log to a USB stick. The graphical installer runs as the live
// session's user; every step runs the engine through pkexec, as root. The
// engine trusts nothing it is told: it looks at the disk again and derives
// the confirmation phrase itself.
//
// With `fixture` set to a directory, nothing is run: each step replays
// <fixture>/<step>.jsonl, the engine's recorded answers.
class Engine : public QObject
{
    Q_OBJECT
    QML_ELEMENT

    Q_PROPERTY(QString fixture READ fixture WRITE setFixture NOTIFY configurationChanged FINAL)
    // The file of the install step in a fixture: "install" by default.
    Q_PROPERTY(QString installFixture READ installFixture WRITE setInstallFixture NOTIFY configurationChanged FINAL)
    // Milliseconds between two replayed lines.
    Q_PROPERTY(int fixtureInterval READ fixtureInterval WRITE setFixtureInterval NOTIFY configurationChanged FINAL)
    Q_PROPERTY(QString program READ program WRITE setProgram NOTIFY configurationChanged FINAL)
    // What starts the program as root: pkexec. Empty runs it directly.
    Q_PROPERTY(QString launcher READ launcher WRITE setLauncher NOTIFY configurationChanged FINAL)
    // Where the engine keeps its log on the medium.
    Q_PROPERTY(QString sessionDirectory READ sessionDirectory WRITE setSessionDirectory NOTIFY configurationChanged FINAL)

    // "idle", "running", "done" or "failed", for each step.
    Q_PROPERTY(QString checkState READ checkState NOTIFY checkChanged FINAL)
    Q_PROPERTY(QVariantList checks READ checks NOTIFY checkChanged FINAL)
    Q_PROPERTY(QVariantList blocking READ blocking NOTIFY checkChanged FINAL)
    Q_PROPERTY(QVariantMap facts READ facts NOTIFY checkChanged FINAL)
    Q_PROPERTY(int payloadDone READ payloadDone NOTIFY checkChanged FINAL)
    Q_PROPERTY(int payloadTotal READ payloadTotal NOTIFY checkChanged FINAL)

    Q_PROPERTY(QString disksState READ disksState NOTIFY disksChanged FINAL)
    Q_PROPERTY(QVariantList disks READ disks NOTIFY disksChanged FINAL)
    Q_PROPERTY(int selectable READ selectable NOTIFY disksChanged FINAL)

    // "idle", "running", "success", "failed", "refused" or "cancelled".
    Q_PROPERTY(QString installState READ installState NOTIFY installChanged FINAL)
    // The engine's current stage: revalidate, prepare, copy, bootloader,
    // finish, check, record.
    Q_PROPERTY(QString stage READ stage NOTIFY installChanged FINAL)
    Q_PROPERTY(QStringList stagesSeen READ stagesSeen NOTIFY installChanged FINAL)
    Q_PROPERTY(double bytesWritten READ bytesWritten NOTIFY installChanged FINAL)
    // What bootc said it would copy, as it said it: "70 (3.2 GB)".
    Q_PROPERTY(QString copyTotal READ copyTotal NOTIFY installChanged FINAL)
    // True while cancelling changes nothing on the disk.
    Q_PROPERTY(bool cancellable READ cancellable NOTIFY installChanged FINAL)
    Q_PROPERTY(bool cancelRefused READ cancelRefused NOTIFY installChanged FINAL)
    Q_PROPERTY(QVariantMap result READ result NOTIFY installChanged FINAL)

    Q_PROPERTY(QString sticksState READ sticksState NOTIFY sticksChanged FINAL)
    Q_PROPERTY(QVariantList sticks READ sticks NOTIFY sticksChanged FINAL)
    Q_PROPERTY(QString saveState READ saveState NOTIFY sticksChanged FINAL)
    Q_PROPERTY(QVariantMap saveResult READ saveResult NOTIFY sticksChanged FINAL)

    // Why the last check or disk step failed, when it did: code, message, detail.
    Q_PROPERTY(QVariantMap stepFailure READ stepFailure NOTIFY stepFailureChanged FINAL)
    // The installation's own output, as it arrives.
    Q_PROPERTY(QStringList outputLines READ outputLines NOTIFY outputChanged FINAL)
    // The last command or component, for the diagnostics drawer.
    Q_PROPERTY(QString lastAction READ lastAction NOTIFY outputChanged FINAL)
    // "reboot" or "poweroff" once asked; empty before.
    Q_PROPERTY(QString powerRequest READ powerRequest NOTIFY powerRequestChanged FINAL)

public:
    explicit Engine(QObject *parent = nullptr);
    ~Engine() override;

    QString fixture() const { return m_fixture; }
    void setFixture(const QString &fixture);
    QString installFixture() const { return m_installFixture; }
    void setInstallFixture(const QString &name);
    int fixtureInterval() const { return m_fixtureInterval; }
    void setFixtureInterval(int interval);
    QString program() const { return m_program; }
    void setProgram(const QString &program);
    QString launcher() const { return m_launcher; }
    void setLauncher(const QString &launcher);
    QString sessionDirectory() const { return m_sessionDirectory; }
    void setSessionDirectory(const QString &directory);

    QString checkState() const { return m_checkState; }
    QVariantList checks() const { return m_checks; }
    QVariantList blocking() const { return m_blocking; }
    QVariantMap facts() const { return m_facts; }
    int payloadDone() const { return m_payloadDone; }
    int payloadTotal() const { return m_payloadTotal; }

    QString disksState() const { return m_disksState; }
    QVariantList disks() const { return m_disks; }
    int selectable() const { return m_selectable; }

    QString installState() const { return m_installState; }
    QString stage() const { return m_stage; }
    QStringList stagesSeen() const { return m_stagesSeen; }
    double bytesWritten() const { return m_bytesWritten; }
    QString copyTotal() const { return m_copyTotal; }
    bool cancellable() const;
    bool cancelRefused() const { return m_cancelRefused; }
    QVariantMap result() const { return m_result; }

    QString sticksState() const { return m_sticksState; }
    QVariantList sticks() const { return m_sticks; }
    QString saveState() const { return m_saveState; }
    QVariantMap saveResult() const { return m_saveResult; }

    QVariantMap stepFailure() const { return m_stepFailure; }
    QStringList outputLines() const { return m_outputLines; }
    QString lastAction() const { return m_lastAction; }
    QString powerRequest() const { return m_powerRequest; }

    // Checks the image on the medium and the computer.
    Q_INVOKABLE void runCheck();
    // Lists the disks.
    Q_INVOKABLE void runDisks();
    // Erases `disk` (one of `disks`) and installs TUR on it. `phrase` is what
    // the person typed; the engine compares it with the phrase it derives.
    Q_INVOKABLE void install(const QVariantMap &disk, const QString &phrase);
    // Asks the engine to stop, which it does only before it writes.
    Q_INVOKABLE void cancel();
    // Forgets the last installation's outcome, to choose a disk again.
    Q_INVOKABLE void resetInstall();
    // Lists USB sticks the log can be saved to.
    Q_INVOKABLE void findSticks();
    // Saves the installer's log to a folder on the stick `device` names.
    Q_INVOKABLE void saveLog(const QString &device);

    // The installer's log as it is now: the engine's log on the medium, or
    // in a replay what was received.
    Q_INVOKABLE QString logText() const;

    Q_INVOKABLE void restart();
    Q_INVOKABLE void powerOff();

    // The engine's sentence for a stage, for the person.
    Q_INVOKABLE QString stageTitle(const QString &stage) const;

Q_SIGNALS:
    void configurationChanged();
    void checkChanged();
    void disksChanged();
    void installChanged();
    void sticksChanged();
    void stepFailureChanged();
    void outputChanged();
    void powerRequestChanged();

private:
    EngineRun *start(const QString &step, const QStringList &arguments, const QString &fixtureFile);
    void failStep(const QString &step, int exitCode, const QString &problem, EngineRun *run);
    void addOutput(const QString &line);
    void onCheckEvent(const QJsonObject &event);
    void onDisksEvent(const QJsonObject &event);
    void onInstallEvent(const QJsonObject &event);
    void onSticksEvent(const QJsonObject &event);
    void onSaveEvent(const QJsonObject &event);

    QString m_fixture;
    QString m_installFixture = QStringLiteral("install");
    int m_fixtureInterval = 0;
    QString m_program = QStringLiteral("/usr/libexec/tur-installer/tur-install-engine");
    QString m_launcher = QStringLiteral("pkexec");
    QString m_sessionDirectory = QStringLiteral("/run/tur-installer");

    QPointer<EngineRun> m_checkRun;
    QPointer<EngineRun> m_disksRun;
    QPointer<EngineRun> m_installRun;
    QPointer<EngineRun> m_sticksRun;
    QPointer<EngineRun> m_saveRun;

    QString m_checkState = QStringLiteral("idle");
    QVariantList m_checks;
    QVariantList m_blocking;
    QVariantMap m_facts;
    int m_payloadDone = 0;
    int m_payloadTotal = 0;

    QString m_disksState = QStringLiteral("idle");
    QVariantList m_disks;
    QVariantList m_pendingDisks;
    int m_selectable = 0;

    QString m_installState = QStringLiteral("idle");
    QString m_stage;
    QStringList m_stagesSeen;
    double m_bytesWritten = 0;
    QString m_copyTotal;
    bool m_cancelRequested = false;
    bool m_cancelRefused = false;
    QVariantMap m_result;

    QString m_sticksState = QStringLiteral("idle");
    QVariantList m_sticks;
    QVariantList m_pendingSticks;
    QString m_saveState = QStringLiteral("idle");
    QVariantMap m_saveResult;

    QVariantMap m_stepFailure;
    QStringList m_outputLines;
    QStringList m_received;
    QString m_lastAction;
    QString m_powerRequest;
};
