// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#pragma once

#include <QAbstractListModel>
#include <QList>
#include <QPointer>
#include <QString>
#include <QVariant>
#include <QVariantMap>
#include <QtQml/qqmlregistration.h>

#include "agentclient.h"

// What the conversation panel shows (desktop-experience.md, 9.2), built
// from what org.tur.Agent1 signals, in the order it signals it:
//
//   you      the person's words
//   pax      P@x's words: `state` is "interim" (between steps) or "answer"
//   step     one action of P@x's, updated in place from proposed to ended:
//            `step` is the service's step document (pax-service.md, 2.4)
//   system   what the system says about P@x, never P@x's words: a refusal
//            before a task started, a notice (a limit reached), a task that
//            was stopped or failed, a task P@x started on a schedule.
//            `state` says which; `code` is the TUR-AI-* code if there is one
//   summary  the end of a task that completed: `detail` has the counts
//
// The words of P@x and of the system are kept apart, as the design asks,
// because the service already keeps them apart (TextAdded's kind).
class Conversation : public QAbstractListModel
{
    Q_OBJECT
    QML_ELEMENT

    // The service. The conversation listens to its signals and starts and
    // stops tasks through it.
    Q_PROPERTY(AgentClient *client READ client WRITE setClient NOTIFY clientChanged FINAL)
    Q_PROPERTY(int count READ count NOTIFY countChanged FINAL)
    // The task that runs, or empty.
    Q_PROPERTY(QString task READ task NOTIFY taskChanged FINAL)
    Q_PROPERTY(bool running READ running NOTIFY taskChanged FINAL)
    // A request was sent and the service has not said yet that it started.
    Q_PROPERTY(bool starting READ starting NOTIFY taskChanged FINAL)
    // The service says the running task is paused.
    Q_PROPERTY(bool paused READ paused NOTIFY taskChanged FINAL)
    // The last task that ended here: its identifier and outcome.
    Q_PROPERTY(QString lastTask READ lastTask NOTIFY taskChanged FINAL)
    Q_PROPERTY(QString lastOutcome READ lastOutcome NOTIFY taskChanged FINAL)
    // What the task costs and what the day has cost (pax-service.md,
    // section 9; design, 9.11):
    //   priceKnown   TUR knows the model's price, so costs are amounts
    //   currency
    //   taskCost     the task so far (the day's change since it started,
    //                because the service does not stream a running task's
    //                cost), or what the ended task cost; null if unknown
    //   taskTokens   the same, in tokens
    //   todayCost, todayTokens
    //   dayLimitCost, dayLimitTokens, taskLimitCost, taskLimitTokens
    //   dayLimitReached
    Q_PROPERTY(QVariantMap spend READ spend NOTIFY spendChanged FINAL)

public:
    enum Role {
        KindRole = Qt::UserRole + 1,
        TextRole,
        TaskRole,
        StepRole,
        StateRole,
        CodeRole,
        TimeRole,
        DetailRole,
    };
    Q_ENUM(Role)

    explicit Conversation(QObject *parent = nullptr);
    ~Conversation() override;

    int rowCount(const QModelIndex &parent = QModelIndex()) const override;
    QVariant data(const QModelIndex &index, int role = Qt::DisplayRole) const override;
    QHash<int, QByteArray> roleNames() const override;

    AgentClient *client() const { return m_client; }
    void setClient(AgentClient *client);
    int count() const { return int(m_entries.size()); }
    QString task() const { return m_task; }
    bool running() const { return !m_task.isEmpty(); }
    bool starting() const { return m_starting; }
    bool paused() const;
    QString lastTask() const { return m_lastTask; }
    QString lastOutcome() const { return m_lastOutcome; }
    QVariantMap spend() const;

    // Sends a request to P@x: shows the person's words at once and asks the
    // service to start the task. A refusal becomes a system card.
    Q_INVOKABLE void ask(const QString &request);
    // Stops the running task now (CancelTask).
    Q_INVOKABLE void stop();
    Q_INVOKABLE void pause();
    Q_INVOKABLE void resume();
    Q_INVOKABLE void clear();
    // One entry as an object with the role names as keys; for tests.
    Q_INVOKABLE QVariantMap entry(int row) const;
    // The rows of a kind, in order; for tests.
    Q_INVOKABLE QVariantList entriesOf(const QString &kind) const;

public Q_SLOTS:
    // What the service says, fed in by the client, or by a test directly.
    void requested(const QString &request);
    void startRefused(const QString &code, const QString &message);
    void taskStarted(const QString &task, const QVariant &details);
    void textAdded(const QString &task, const QString &kind, const QString &text);
    void actionProposed(const QString &task, const QVariant &step);
    void actionCompleted(const QString &task, const QVariant &step);
    void taskFinished(const QString &task, const QVariant &result);

Q_SIGNALS:
    void clientChanged();
    void countChanged();
    void taskChanged();
    void spendChanged();
    // A task started, or something happened that the person should see:
    // the window opens its panel.
    void attentionWanted();

private:
    struct Entry
    {
        QString kind;
        QString text;
        QString task;
        QVariantMap step;
        QString state;
        QString code;
        qint64 time = 0;
        QVariantMap detail;
    };

    void append(const Entry &entry);
    void changed(int row);
    int findStep(const QString &task, int index) const;
    int findPending(const QString &request) const;
    void updateStep(const QString &task, const QVariantMap &step);
    void setTask(const QString &task);
    QVariantMap today() const;

    QPointer<AgentClient> m_client;
    QList<Entry> m_entries;
    QString m_task;
    bool m_starting = false;
    QString m_lastTask;
    QString m_lastOutcome;
    QStringList m_answered;
    // The day's use when the running task started, and what the last task
    // cost when it ended.
    QVariantMap m_todayAtStart;
    QVariant m_lastCost;
    QString m_lastCurrency;
    qint64 m_lastTokens = -1;
};
