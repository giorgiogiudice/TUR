// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#pragma once

#include "appssource.h"

#include <QHash>
#include <QJsonObject>
#include <QObject>
#include <QPointer>
#include <QString>
#include <QStringList>
#include <QVariantList>
#include <QVariantMap>
#include <QtQml/qqmlregistration.h>

#include <functional>

class Apps;

// One installation, update or removal of one application, from its plan to
// its end (desktop-experience.md, 8.5). What tur-apps reports is kept as it
// arrives: the plan, the step under way, the last activity, progress with a
// total only when tur-apps knows one, and at the end the result or the
// failure. Nothing is decided here about how the application runs.
class AppsOperation : public QObject
{
    Q_OBJECT
    QML_ELEMENT
    QML_UNCREATABLE("An operation belongs to Apps.")

    Q_PROPERTY(QString applicationId READ applicationId CONSTANT FINAL)
    Q_PROPERTY(Kind kind READ kind CONSTANT FINAL)
    Q_PROPERTY(Phase phase READ phase NOTIFY changed FINAL)
    // tur-apps' plan: `plan --json`, then the plan event of the installation.
    Q_PROPERTY(QVariantMap plan READ plan NOTIFY changed FINAL)
    // The installer file the person chose, if any.
    Q_PROPERTY(QString installerFile READ installerFile CONSTANT FINAL)
    // Remove: whether the person chose to delete the data too.
    Q_PROPERTY(bool deleteData READ deleteData CONSTANT FINAL)
    // The step under way: one of the plan's steps (download, verify,
    // install, record), or update, uninstall, keep, unused; empty before
    // the first.
    Q_PROPERTY(QString step READ step NOTIFY changed FINAL)
    // The last thing tur-apps said is happening, as it said it.
    Q_PROPERTY(QString activity READ activity NOTIFY changed FINAL)
    // Bytes done in the step, and in all; total is -1 when not known, and a
    // bar with numbers is then not shown (8.5).
    Q_PROPERTY(double done READ done NOTIFY changed FINAL)
    Q_PROPERTY(double total READ total NOTIFY changed FINAL)
    // Whether cancelling is safe now (tur-apps src/cancel.rs): installing,
    // before tur-runtime starts and before the installation is recorded.
    Q_PROPERTY(bool cancellable READ cancellable NOTIFY changed FINAL)
    Q_PROPERTY(bool cancelRequested READ cancelRequested NOTIFY changed FINAL)
    // The content of the two-layer message when phase is Failed.
    Q_PROPERTY(QVariantMap failure READ failure NOTIFY changed FINAL)
    // tur-apps' result document when phase is Succeeded.
    Q_PROPERTY(QVariantMap result READ result NOTIFY changed FINAL)

public:
    enum Kind {
        Install,
        Update,
        Remove,
    };
    Q_ENUM(Kind)

    enum Phase {
        Planning,  // the plan is being made
        Planned,   // the plan is shown; nothing is done until confirm()
        Running,   // tur-apps is carrying it out
        Succeeded,
        Failed,    // the page returns to its state before the attempt
        Cancelled, // "Cancelled. Nothing was installed."
    };
    Q_ENUM(Phase)

    AppsOperation(Apps *apps, const QString &applicationId, Kind kind, const QString &installerFile, bool deleteData);

    QString applicationId() const;
    Kind kind() const;
    Phase phase() const;
    QVariantMap plan() const;
    QString installerFile() const;
    bool deleteData() const;
    QString step() const;
    QString activity() const;
    double done() const;
    double total() const;
    bool cancellable() const;
    bool cancelRequested() const;
    QVariantMap failure() const;
    QVariantMap result() const;

    // Carries the plan out, after the person said yes in its sheet.
    Q_INVOKABLE void confirm();
    // Asks to cancel, when cancellable.
    Q_INVOKABLE void cancel();
    // Forgets the operation: the page shows the application as it is.
    Q_INVOKABLE void dismiss();
    // Starts the same operation again, after a failure.
    Q_INVOKABLE void retry();

Q_SIGNALS:
    void changed();

private:
    friend class Apps;

    void start();
    void receive(const QByteArray &line);
    void end(const AppsReply &reply);
    void fail(const QVariantMap &failure);
    void setPhase(Phase phase);

    Apps *m_apps;
    QString m_id;
    Kind m_kind;
    QString m_installerFile;
    bool m_deleteData;
    Phase m_phase = Planning;
    QVariantMap m_plan;
    QString m_step;
    QString m_activity;
    double m_done = 0;
    double m_total = -1;
    bool m_cancelRequested = false;
    QVariantMap m_failure;
    QVariantMap m_result;
    QVariantMap m_eventError;
    bool m_sawResult = false;
    quint64 m_request = 0;
};

// What the application centre's Install, Installed, Updates and Sources
// read and do: tur-apps, through whichever AppsSource the configuration
// selects (AppsProcess, or AppsFixture with `fixture`).
//
//     Apps { fixture: "/path/to/recorded/runs" }          tests, pictures
//     Apps { program: "tur-apps"; catalogueDirectory: ... }  the real thing
class Apps : public QObject
{
    Q_OBJECT
    QML_ELEMENT

    Q_PROPERTY(QString fixture READ fixture WRITE setFixture NOTIFY configurationChanged FINAL)
    Q_PROPERTY(QString program READ program WRITE setProgram NOTIFY configurationChanged FINAL)
    Q_PROPERTY(QString catalogueDirectory READ catalogueDirectory WRITE setCatalogueDirectory NOTIFY
                   configurationChanged FINAL)
    Q_PROPERTY(QString hostFacts READ hostFacts WRITE setHostFacts NOTIFY configurationChanged FINAL)
    Q_PROPERTY(QString preferences READ preferences WRITE setPreferences NOTIFY configurationChanged FINAL)

    // Everything installed for this person, whatever it runs as: tur-apps'
    // list, each application as tur-apps writes it.
    Q_PROPERTY(QVariantList installed READ installed NOTIFY installedChanged FINAL)
    Q_PROPERTY(Status installedStatus READ installedStatus NOTIFY installedChanged FINAL)
    Q_PROPERTY(QVariantMap installedFailure READ installedFailure NOTIFY installedChanged FINAL)
    // Whether the sources were asked for updates when the list was made.
    Q_PROPERTY(bool updatesChecked READ updatesChecked NOTIFY installedChanged FINAL)
    // What tur-apps could not find out while making the list, in its words.
    Q_PROPERTY(QStringList installedProblems READ installedProblems NOTIFY installedChanged FINAL)

    // The Flatpak sources, for this person: tur-apps' list.
    Q_PROPERTY(QVariantList sources READ sources NOTIFY sourcesChanged FINAL)
    Q_PROPERTY(Status sourcesStatus READ sourcesStatus NOTIFY sourcesChanged FINAL)
    Q_PROPERTY(QVariantMap sourcesFailure READ sourcesFailure NOTIFY sourcesChanged FINAL)
    // The source being switched on or off, while it is.
    Q_PROPERTY(QString sourceChanging READ sourceChanging NOTIFY sourcesChanged FINAL)

    // The operation of each application that has one, by identifier.
    Q_PROPERTY(QVariantMap operations READ operations NOTIFY operationsChanged FINAL)
    // Whether any installation, update or removal is under way.
    Q_PROPERTY(bool busy READ busy NOTIFY operationsChanged FINAL)

public:
    enum Status {
        Idle,
        Loading,
        Ready,
        Failed,
    };
    Q_ENUM(Status)

    // This program's own codes for what goes wrong in running tur-apps; the
    // catalogue's equivalents are in answers.h. Recorded in the same family.
    static inline const QString NotStarted = QStringLiteral("TUR-DESKTOP-CENTRE-001");
    static inline const QString TimedOut = QStringLiteral("TUR-DESKTOP-CENTRE-002");
    static inline const QString TooLarge = QStringLiteral("TUR-DESKTOP-CENTRE-003");
    static inline const QString Unreadable = QStringLiteral("TUR-DESKTOP-CENTRE-004");
    static inline const QString Crashed = QStringLiteral("TUR-DESKTOP-CENTRE-005");
    static inline const QString NotRecorded = QStringLiteral("TUR-DESKTOP-CENTRE-006");
    static inline const QString Refused = QStringLiteral("TUR-DESKTOP-CENTRE-007");
    // An installed application could not be started.
    static inline const QString NotLaunched = QStringLiteral("TUR-DESKTOP-CENTRE-008");

    // tur-apps' code for a cancelled installation (and for "nothing was
    // done without confirmation").
    static inline const QString NothingDone = QStringLiteral("TUR-DESKTOP-APPS-013");
    // tur-apps' code for a source that is switched off.
    static inline const QString SourceOff = QStringLiteral("TUR-DESKTOP-APPS-008");

    explicit Apps(QObject *parent = nullptr);
    ~Apps() override;

    QString fixture() const;
    void setFixture(const QString &fixture);
    QString program() const;
    void setProgram(const QString &program);
    QString catalogueDirectory() const;
    void setCatalogueDirectory(const QString &directory);
    QString hostFacts() const;
    void setHostFacts(const QString &file);
    QString preferences() const;
    void setPreferences(const QString &file);

    QVariantList installed() const;
    Status installedStatus() const;
    QVariantMap installedFailure() const;
    bool updatesChecked() const;
    QStringList installedProblems() const;
    QVariantList sources() const;
    Status sourcesStatus() const;
    QVariantMap sourcesFailure() const;
    QString sourceChanging() const;
    QVariantMap operations() const;
    bool busy() const;

    // Asks for the list of what is installed; with `checkUpdates`, the
    // sources are asked for updates too.
    Q_INVOKABLE void refresh(bool checkUpdates);
    Q_INVOKABLE void refreshSources();
    // Switches a source on or off for this person, then lists them again.
    // With `thenPlan`, once the source is switched, the plan of installing
    // that application is made again ("Switch on Flathub and try again").
    Q_INVOKABLE void setSource(const QString &name, bool on, const QString &thenPlan = QString(),
                               const QString &installerFile = QString());

    // Makes the plan of installing an application, and shows it: the
    // operation's phase becomes Planned. Nothing is installed until
    // confirm(). Replaces an operation that has ended.
    Q_INVOKABLE AppsOperation *plan(const QString &applicationId, const QString &installerFile = QString());
    Q_INVOKABLE AppsOperation *update(const QString &applicationId);
    // Updates every application the list says has an update, one after
    // the other.
    Q_INVOKABLE void updateAll();
    // Removes an application. `deleteData` is the person's answer to the
    // one question; there is no default here.
    Q_INVOKABLE AppsOperation *remove(const QString &applicationId, bool deleteData);
    Q_INVOKABLE AppsOperation *operation(const QString &applicationId) const;

    // The application in the list, or an empty map.
    Q_INVOKABLE QVariantMap installedEntry(const QString &applicationId) const;

    // Starts an installed application. An empty map, or the failure.
    Q_INVOKABLE QVariantMap launch(const QString &applicationId);

    // Tests and pictures, with a fixture: hold a streaming run after this
    // many lines; let it go on; what launch() started.
    Q_INVOKABLE void holdAfter(int lines);
    Q_INVOKABLE void release();
    Q_INVOKABLE QStringList launched() const;

    // Uses this source instead of the configured one. For tests.
    void useSource(AppsSource *source);

    // The two-layer message for a run that could not be made, or whose
    // answer is a failure. Empty when the reply is a success.
    static QVariantMap failureOf(const AppsReply &reply, const QJsonObject &error = QJsonObject());
    // The words for one of tur-apps' codes.
    static QString titleOf(const QString &code, const QString &theirs);

Q_SIGNALS:
    void configurationChanged();
    void installedChanged();
    void sourcesChanged();
    void operationsChanged();

private:
    friend class AppsOperation;

    using LineHandler = std::function<void(const QByteArray &)>;
    using ReplyHandler = std::function<void(const AppsReply &)>;

    AppsSource *source();
    void dropSource();
    void listInstalled(bool checkUpdates, bool afresh);
    quint64 submit(const AppsRequest &request, LineHandler onLine, ReplyHandler onEnd);
    void forget(quint64 id);
    void lineArrived(quint64 id, const QByteArray &line);
    void answered(quint64 id, const AppsReply &reply);
    AppsOperation *begin(const QString &applicationId, AppsOperation::Kind kind, const QString &installerFile,
                         bool deleteData);
    void operationEnded(AppsOperation *operation);
    void nextUpdate();
    void cancelOperation(AppsOperation *operation);
    void dropOperation(AppsOperation *operation);

    QString m_fixture;
    QString m_program = QStringLiteral("tur-apps");
    QString m_catalogueDirectory;
    QString m_hostFacts;
    QString m_preferences;

    AppsSource *m_source = nullptr;
    QHash<quint64, LineHandler> m_lineHandlers;
    QHash<quint64, ReplyHandler> m_replyHandlers;

    QVariantList m_installed;
    Status m_installedStatus = Idle;
    QVariantMap m_installedFailure;
    bool m_updatesChecked = false;
    QStringList m_installedProblems;
    quint64 m_listRequest = 0;
    bool m_listChecksUpdates = false;

    QVariantList m_sources;
    Status m_sourcesStatus = Idle;
    QVariantMap m_sourcesFailure;
    QString m_sourceChanging;
    quint64 m_sourcesRequest = 0;

    QHash<QString, AppsOperation *> m_operations;
    QStringList m_updateQueue;
};
