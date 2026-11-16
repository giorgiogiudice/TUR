// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#pragma once

#include "agentclient.h"
#include "companion.h"

#include <QHash>
#include <QObject>
#include <QPointer>
#include <QString>
#include <QStringList>
#include <QVariant>
#include <QVariantMap>
#include <QtQml/qqmlregistration.h>

// What the character learns from P@x's session service, org.tur.Agent1, as
// it exists (docs/architecture/pax-service.md, section 2), and the three
// things it does there when the person presses something.
//
// It listens and never starts the service: a character on the bar is not a
// reason for P@x to run (the client's autoStart is false, so P@x counts as
// on only while its service is running and says it is switched on).
//
//  - GetStatus and StateChanged: whether P@x is on, what it is doing, why it
//    cannot take a task, how many proposals wait for the person, and how
//    much of the day's limit is used.
//  - ActionProposed and ActionCompleted: the step P@x is on, for "What are
//    you watching?".
//  - TaskFinished: a notice of kind "task" when the task ended in a way the
//    person should hear of: refused steps, a limit, a failure, or a task
//    that ran long enough, or with nobody there, that its end may have been
//    missed. A task the person stopped is not announced.
//  - ScheduleProposed: a notice of kind "approval".
//  - The status again: one suggestion, when most of the day's limit is used
//    (limitSuggestion), which the rules drop unless suggestions are on.
//
// The words of a refusal, a limit or a failure are the service's own.
class AgentSource : public QObject
{
    Q_OBJECT
    QML_ELEMENT

    // The bus P@x's service is on; empty for the session bus.
    Q_PROPERTY(QString busAddress READ busAddress WRITE setBusAddress NOTIFY busAddressChanged FINAL)
    Q_PROPERTY(Companion *companion READ companion WRITE setCompanion NOTIFY companionChanged FINAL)
    // A task that ran at least this long is announced when it ends well.
    Q_PROPERTY(int longTaskSeconds READ longTaskSeconds WRITE setLongTaskSeconds NOTIFY longTaskSecondsChanged FINAL)
    // The last thing that went wrong when the character called another
    // program; empty when nothing did.
    Q_PROPERTY(QString problem READ problem NOTIFY problemChanged FINAL)

public:
    explicit AgentSource(QObject *parent = nullptr);

    QString busAddress() const { return m_client.busAddress(); }
    void setBusAddress(const QString &address);
    Companion *companion() const { return m_companion; }
    void setCompanion(Companion *companion);
    int longTaskSeconds() const { return m_longTaskSeconds; }
    void setLongTaskSeconds(int seconds);
    QString problem() const { return m_problem; }

    AgentClient *client() { return &m_client; }

    // Connects and starts listening.
    Q_INVOKABLE void start();
    // Stops P@x's running task (CancelTask). Never starts the service.
    Q_INVOKABLE void stop();
    // Asks P@x's window to show "ask", "panel", "activity" or "setup"
    // (org.tur.PaxUi1.Show). The person pressed something, so the window
    // may be started for it.
    Q_INVOKABLE void showWindow(const QString &mode);
    // Starts a task with the person's words, then shows the panel. Only for
    // a button the person pressed.
    Q_INVOKABLE void ask(const QString &request);

    // The share of the day's limit that a status says is used; negative
    // when the status does not say.
    static double spentOf(const QVariantMap &status);
    // The one suggestion this version makes, by a plain rule over the
    // status the character already reads: nine tenths of the day's limit
    // are used and the limit is not yet reached. Empty otherwise. It is
    // made once a day, and only if the person switched suggestions on.
    static QVariantMap limitSuggestion(const QVariantMap &status);

Q_SIGNALS:
    void busAddressChanged();
    void companionChanged();
    void longTaskSecondsChanged();
    void problemChanged();
    // The window was asked for (for the tests).
    void windowRequested(const QString &mode);

private:
    void onStatus();
    void onTaskStarted(const QString &task, const QVariant &details);
    void onStep(const QString &task, const QVariant &step);
    void onTaskFinished(const QString &task, const QVariant &result);
    void onScheduleProposed(const QString &proposal, const QVariant &details);
    void setProblem(const QString &problem);

    struct Running {
        double startedAt = 0;
        bool attended = true;
        // The steps refused, by their audit identifier: the service reports
        // a refused step twice, when it is proposed and when it is
        // completed (fixtures/agent1/tasks/refused.json).
        QStringList refused;
        QString lastRefusal;
    };

    AgentClient m_client;
    QPointer<Companion> m_companion;
    int m_longTaskSeconds = 20;
    QString m_problem;
    QHash<QString, Running> m_tasks;
};
