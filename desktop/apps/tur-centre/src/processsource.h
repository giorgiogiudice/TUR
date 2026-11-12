// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#pragma once

#include "cataloguesource.h"

#include <QHash>
#include <QList>
#include <QProcess>
#include <QString>
#include <QStringList>

class QTimer;

// Answers by running `tur-catalogue <command> --json -- <subject>` as a
// separate program, as tur-diagnostics runs tur-diag (desktop-experience.md,
// section 8.10).
//
//  - Never through a shell: the program and its arguments are a vector, and
//    the subject comes after "--", so that nothing a person types, and
//    nothing in an address handed to tur-centre, is read as an option.
//  - Never blocking: every run is asynchronous, and at most `concurrency`
//    run at once.
//  - Bounded: a run that prints more than `maximumOutputBytes` or lasts
//    longer than `timeoutMs` is stopped and reported as a failure.
//  - Nothing is written to the program's input.
class ProcessSource : public CatalogueSource
{
    Q_OBJECT

public:
    struct Settings
    {
        // Found on the PATH unless it is a path.
        QString program = QStringLiteral("tur-catalogue");
        // --catalogue DIR. Empty: the program's own default.
        QString catalogueDirectory;
        // --host FILE, for `route`. Empty: the router assumes a machine, and
        // says so in its answer.
        QString hostFacts;
        // --prefs FILE, for `route`.
        QString preferences;
        int timeoutMs = 10000;
        qint64 maximumOutputBytes = 4 * 1024 * 1024;
        int concurrency = 4;
    };

    // What is kept of the program's standard error; the rest is read and
    // dropped.
    static constexpr qint64 MaximumErrorBytes = 64 * 1024;

    explicit ProcessSource(const Settings &settings, QObject *parent = nullptr);
    ~ProcessSource() override;

    // The argument vector for a question.
    static QStringList arguments(const CatalogueRequest &request, const Settings &settings);

    // A program and its arguments on one line, for the technical layer of a
    // message. For reading, not for running.
    static QString commandLine(const QString &program, const QStringList &arguments);

    quint64 submit(const CatalogueRequest &request) override;
    void cancel(quint64 id) override;

    int running() const;
    int queued() const;

private:
    struct Job;

    void pump();
    void start(Job *job);
    void readOutput(Job *job);
    void readError(Job *job);
    void failedToStart(Job *job);
    void ended(Job *job, int exitCode, QProcess::ExitStatus status);
    void timedOut(Job *job);
    void answer(Job *job, CatalogueReply reply);
    void retire(Job *job);

    Settings m_settings;
    QList<Job *> m_queue;
    QHash<quint64, Job *> m_running;
};
