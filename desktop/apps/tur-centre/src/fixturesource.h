// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#pragma once

#include "cataloguesource.h"

#include <QByteArray>
#include <QHash>
#include <QList>
#include <QSet>
#include <QString>
#include <QStringList>

// Answers from a directory of recorded answers: what `tur-catalogue`
// printed, and how it ended, for a fixed list of questions
// (desktop-experience.md, sections 8.10 and 15.2). The tests, the screenshot
// job and `tur-centre --fixture DIR` use it.
//
// The files are written by services/tur-catalogue/tests/desktop_fixtures.rs
// and checked by it in the Rust workflow, so what this source answers is
// what the router answers. It makes nothing up: a question that was not
// recorded gets the failure NotRecorded, never a guess.
//
// A file is a JSON object with a list "invocations"; each has "command",
// "subject", "exit_status", "stdout" (the program's JSON document, or null)
// and "stderr". The directory is read once, when the source is made: a few
// small files, and only where a fixture is asked for. Answers are still
// delivered later, through the event loop, as a real program's are.
class FixtureSource : public CatalogueSource
{
    Q_OBJECT

public:
    struct Recorded
    {
        CatalogueRequest request;
        QString host; // "stated", "assumed" or "not used"
        QStringList arguments;
        int exitStatus = 0;
        QByteArray output;
        QByteArray errorOutput;
        QString file;
    };

    // The largest fixture file that is read.
    static constexpr qint64 MaximumFileBytes = 8 * 1024 * 1024;

    explicit FixtureSource(const QString &directory, QObject *parent = nullptr);

    quint64 submit(const CatalogueRequest &request) override;
    void cancel(quint64 id) override;

    // Empty if the directory was read; otherwise what is wrong with it.
    QString problem() const;

    // Everything the directory holds, in the order of its files.
    QList<Recorded> invocations() const;

private:
    static QString key(const QString &command, const QString &subject);
    void load();
    void deliver(quint64 id, const CatalogueRequest &request);

    QString m_directory;
    QString m_problem;
    QList<Recorded> m_invocations;
    QHash<QString, qsizetype> m_index;
    QSet<quint64> m_pending;
};
