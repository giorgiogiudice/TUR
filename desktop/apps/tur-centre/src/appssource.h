// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#pragma once

#include <QByteArray>
#include <QMetaType>
#include <QObject>
#include <QString>
#include <QStringList>

// Where installing, updating, removing and listing applications is done.
//
// tur-centre installs nothing itself. It asks `tur-apps`, the install
// backend (docs/architecture/app-install-backend.md), which installs per
// user, by the route the router chose, and reports what it does as JSON.
// tur-apps is run as a child process of the person's own session, as
// `tur-catalogue` is (docs/architecture/tur-centre.md, section 8, says why
// this and not the proposed org.tur.Apps1). In tests and the screenshot job
// the same questions are answered from recorded runs of tur-apps
// (AppsFixture). Everything above this interface is the same for both.

// One thing to do or ask.
struct AppsRequest
{
    enum Command {
        Plan,            // subject: an application; installerFile optional
        Install,         // subject: an application; installerFile optional; streams events
        Update,          // subject: an application; streams events
        Remove,          // subject: an application; deleteData; streams events
        List,            // no subject
        ListWithUpdates, // no subject; asks the sources for updates
        Sources,         // no subject
        EnableSource,    // subject: a source (flathub, fedora)
        DisableSource,   // subject: a source
    };

    Command command = List;
    QString subject;
    // A file the person chose: the installer of a Windows application.
    QString installerFile;
    // Remove: delete the application's settings and documents as well.
    // The default keeps them (desktop-experience.md, 8.5).
    bool deleteData = false;

    // Whether tur-apps reports progress as JSON lines (`--events`) instead
    // of printing one document at the end.
    bool streams() const;
    // Whether the request names an application or a source.
    bool hasSubject() const;
    // The subcommand and its arguments, without the global options. The
    // subject comes after "--", so that it is a value whatever it begins
    // with. The recorded fixtures write a chosen file as "<installer>".
    QStringList arguments() const;
    // "plan", "install", ...: for messages.
    QString commandName() const;
};

// How a run ended. Nothing is interpreted here; apps.cpp does that.
struct AppsReply
{
    enum Failure {
        None,        // the program ran and ended; see exitStatus and output
        NotStarted,  // it could not be started
        TimedOut,    // it did not end within the time limit and was stopped
        TooLarge,    // it printed more than the limit and was stopped
        Crashed,     // it ended abnormally
        NotRecorded, // a fixture directory has no answer for the request
        Refused,     // the request was not made: its subject is not usable
    };

    Failure failure = None;
    int exitStatus = -1;
    QByteArray output;      // standard output, complete
    QByteArray errorOutput; // standard error; may be cut short
    QString detail;         // for a failure: what the system said
    QString commandLine;    // the command as it was run, for the technical layer
};

Q_DECLARE_METATYPE(AppsReply)

class AppsSource : public QObject
{
    Q_OBJECT

public:
    explicit AppsSource(QObject *parent = nullptr);
    ~AppsSource() override;

    // Starts a request. Returns at once with a number; line() and
    // finished() carry it later, never before this returns.
    virtual quint64 submit(const AppsRequest &request) = 0;

    // Asks a running installation to cancel: tur-apps is sent the line
    // "cancel" and decides whether stopping is still safe. Nothing happens
    // for any other request.
    virtual void cancel(quint64 id) = 0;

    // Starts an installed application, as `tur-apps launch`, detached from
    // the application centre: it keeps running when the window is closed.
    // False, with the reason, when it could not be started at all.
    virtual bool launch(const QString &applicationId, QString *problem) = 0;

    // The longest subject a request may have, in characters.
    static constexpr int MaximumSubjectLength = 255;

    // Whether a subject can be passed on: not empty, not too long, no
    // control characters.
    static bool isUsableSubject(const QString &subject);

    // Whether a request can be passed on at all.
    static bool isUsableRequest(const AppsRequest &request);

Q_SIGNALS:
    // One line of a request that streams events, as it arrives.
    void line(quint64 id, const QByteArray &line);
    void finished(quint64 id, const AppsReply &reply);

protected:
    quint64 nextId();

private:
    quint64 m_lastId = 0;
};
