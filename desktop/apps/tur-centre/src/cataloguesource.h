// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#pragma once

#include <QByteArray>
#include <QMetaType>
#include <QObject>
#include <QString>
#include <QStringList>

// Where the application centre's answers come from.
//
// tur-centre holds no routing logic and writes no verdict of its own
// (desktop-experience.md, section 8): it asks the catalogue and the router,
// and shows what they say. Until org.tur.Catalogue1 exists, asking means
// running `tur-catalogue ... --json` (ProcessSource). In tests, in the
// screenshot job and with --fixture, the same questions are answered from
// recorded output of that program (FixtureSource). Everything above this
// interface is the same in both cases.

// One question.
struct CatalogueRequest
{
    enum Command {
        Search, // subject: what the person typed
        Show,   // subject: an application's identifier
        Route,  // subject: an application's identifier
        // No subject: the catalogue's counts (`validate --json`), from which
        // Home learns whether TUR has rated any application itself.
        Validate,
    };

    Command command = Search;
    QString subject;

    // "search", "show", "route" or "validate": the subcommand of
    // tur-catalogue.
    QString commandName() const;

    // Whether the question has a subject. Only Validate has none.
    bool hasSubject() const { return command != Validate; }
};

// One answer: what the program printed and how it ended, or why there is no
// answer. Nothing is interpreted here; answers.h does that.
struct CatalogueReply
{
    enum Failure {
        None,        // the program ran and ended; see exitStatus and output
        NotStarted,  // the program could not be started
        TimedOut,    // it did not end within the time limit and was stopped
        TooLarge,    // it printed more than the limit and was stopped
        Crashed,     // it ended abnormally
        NotRecorded, // a fixture directory has no answer for the question
        Refused,     // the question was not asked: its subject is not usable
    };

    Failure failure = None;
    int exitStatus = -1;
    QByteArray output;      // standard output, complete
    QByteArray errorOutput; // standard error; may be cut short
    // For a failure: what the system said, for the technical layer.
    QString detail;
    // The command as it was run, or as it would have been: for the
    // technical layer. Never shown as a sentence.
    QString commandLine;
};

Q_DECLARE_METATYPE(CatalogueReply)

class CatalogueSource : public QObject
{
    Q_OBJECT

public:
    explicit CatalogueSource(QObject *parent = nullptr);
    ~CatalogueSource() override;

    // Asks. Returns at once with a number; finished() carries the same
    // number later, never before this returns. The caller's thread is not
    // blocked (specification 30.1).
    virtual quint64 submit(const CatalogueRequest &request) = 0;

    // Forgets a question that has not been answered yet. No finished() is
    // emitted for it.
    virtual void cancel(quint64 id) = 0;

    // The longest subject a question may have, in characters.
    static constexpr int MaximumSubjectLength = 200;

    // Whether a subject can be passed to the program at all: not empty, not
    // too long, and without control characters. Whether it names anything
    // is the catalogue's to say.
    static bool isUsableSubject(const QString &subject);

Q_SIGNALS:
    void finished(quint64 id, const CatalogueReply &reply);

protected:
    quint64 nextId();

private:
    quint64 m_lastId = 0;
};
