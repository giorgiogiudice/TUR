// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#pragma once

#include "appssource.h"

#include <QByteArray>
#include <QHash>
#include <QList>
#include <QSet>
#include <QString>
#include <QStringList>

class QTimer;

// Answers from recorded runs of the real `tur-apps`: what it printed, line
// by line, and how it ended (docs/architecture/tur-centre.md, section 4).
// The tests, the screenshot job and `tur-centre --apps-fixture DIR` use it.
//
// The files are written by desktop/ci/centre-e2e/record.py, which runs
// tur-apps on a CI runner with the argument vectors the application centre
// uses, against Flathub and the real flatpak (and, for the one failure that
// cannot be had on demand, against tur-apps' own stand-in for flatpak); and
// by fixtures/apps/update.py, which copies them out of that job's artifact.
// Nobody writes them by hand.
//
// A file holds a list "invocations"; each has "arguments" (after the global
// options; a chosen file is written "<installer>"), "exit_status", "stderr",
// "stdout" (a list of lines) and, for a run that was cancelled,
// "cancel_after": how many lines had been read when "cancel" was sent.
//
// A request is answered by the first recorded run with its arguments that
// has not answered yet, and once all have, by the last of them: so a
// directory can hold the list before and after an installation. A streaming
// run is played back line by line through the event loop. A run that was
// cancelled stops where the recorder cancelled it, until cancel() is
// called. A request that was not recorded is NotRecorded, never a guess.
class AppsFixture : public AppsSource
{
    Q_OBJECT

public:
    struct Recorded
    {
        QStringList arguments;
        int exitStatus = 0;
        QList<QByteArray> lines;
        QByteArray errorOutput;
        int cancelAfter = -1;
        QString file;
    };

    static constexpr qint64 MaximumFileBytes = 8 * 1024 * 1024;

    explicit AppsFixture(const QString &directory, QObject *parent = nullptr);
    ~AppsFixture() override;

    quint64 submit(const AppsRequest &request) override;
    void cancel(quint64 id) override;
    bool launch(const QString &applicationId, QString *problem) override;

    QString problem() const;
    QList<Recorded> invocations() const;

    // Tests: a streaming run stops after this many lines until release();
    // -1 plays every run to its end.
    void setHold(int lines);
    void release();
    // Tests: the applications launch() was asked to start.
    QStringList launched() const;
    // The milliseconds between two lines played back.
    void setInterval(int milliseconds);

    static bool matches(const QStringList &recorded, const QStringList &asked);

private:
    struct Playing;

    void load();
    void step(quint64 id);
    void finish(quint64 id);

    QString m_directory;
    QString m_problem;
    QList<Recorded> m_invocations;
    QSet<qsizetype> m_used;
    QHash<quint64, Playing *> m_playing;
    int m_hold = -1;
    int m_interval = 5;
    QStringList m_launched;
};
