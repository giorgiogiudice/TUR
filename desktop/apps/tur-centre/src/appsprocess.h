// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#pragma once

#include "appssource.h"

#include <QHash>
#include <QList>
#include <QPointer>
#include <QProcess>
#include <QString>
#include <QStringList>

class QTimer;

// Runs `tur-apps` as a separate program of the person's session, as
// tur-centre runs tur-catalogue (ProcessSource).
//
//  - Never through a shell; the subject comes after "--".
//  - Never blocking: every run is asynchronous.
//  - Bounded: a run that prints more than `maximumOutputBytes` is stopped.
//    A run that asks or lists is stopped after `timeoutMs` (it may ask a
//    remote: Flathub for sizes, or for updates); an installation, update or
//    removal is not timed out, because a large download may take an hour,
//    and it reports progress all along.
//  - An installation keeps its standard input open, so that "cancel" can be
//    written to it (tur-apps src/cancel.rs). Every other run gets none.
//  - When the application centre is closed during an installation, the
//    installation is asked to cancel and is given time to take back what it
//    had done; an update or a removal is given time to finish.
class AppsProcess : public AppsSource
{
    Q_OBJECT

public:
    struct Settings
    {
        // Found on the PATH unless it is a path.
        QString program = QStringLiteral("tur-apps");
        // --catalogue, --host, --prefs: each may be empty.
        QString catalogueDirectory;
        QString hostFacts;
        QString preferences;
        int timeoutMs = 120000;
        qint64 maximumOutputBytes = 16 * 1024 * 1024;
        // How long the destructor waits for a running change to end.
        int shutdownWaitMs = 30000;
    };

    static constexpr qint64 MaximumErrorBytes = 64 * 1024;

    explicit AppsProcess(const Settings &settings, QObject *parent = nullptr);
    ~AppsProcess() override;

    // The whole argument vector of a request: the global options, then the
    // request's own.
    static QStringList arguments(const AppsRequest &request, const Settings &settings);

    quint64 submit(const AppsRequest &request) override;
    void cancel(quint64 id) override;
    bool launch(const QString &applicationId, QString *problem) override;

    int running() const;

private:
    struct Job;

    void start(Job *job);
    void readOutput(Job *job);
    void readError(Job *job);
    void failedToStart(Job *job);
    void ended(Job *job, int exitCode, QProcess::ExitStatus status);
    void timedOut(Job *job);
    void answer(Job *job, AppsReply reply);
    void retire(Job *job);

    Settings m_settings;
    QHash<quint64, Job *> m_running;
};
