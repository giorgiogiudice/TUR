// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#pragma once

#include <QDBusConnection>
#include <QJSValue>
#include <QObject>
#include <QString>
#include <QStringList>
#include <QVariant>
#include <QVariantList>
#include <QVariantMap>
#include <QtQml/qqmlregistration.h>

#include <functional>
#include <optional>

class QDBusServiceWatcher;

// P@x's session service, org.tur.Agent1, as tur-pax-ui sees it
// (docs/architecture/pax-service.md, section 2; docs/architecture/
// tur-pax-ui.md, section 3).
//
// Every method of the interface is reached through call(), asynchronously;
// structured replies, which the service sends as JSON text, arrive parsed.
// The service's signals are passed on with their JSON parsed too. A refusal
// arrives as its TUR-AI-* code and the service's own sentence.
//
// The client keeps nothing that a person typed: a key given to SetKey goes
// into one D-Bus message and is not stored, logged or repeated.
class AgentClient : public QObject
{
    Q_OBJECT
    QML_ELEMENT

    // The bus to use: empty for the session bus, or a D-Bus address (the
    // tests' private bus).
    Q_PROPERTY(QString busAddress READ busAddress WRITE setBusAddress NOTIFY busAddressChanged FINAL)
    // Whether a call may start the service through D-Bus activation. True
    // in the window, which a person opened; false for the stop key, which
    // must never start P@x only to stop it.
    Q_PROPERTY(bool autoStart READ autoStart WRITE setAutoStart NOTIFY autoStartChanged FINAL)
    Q_PROPERTY(Presence presence READ presence NOTIFY presenceChanged FINAL)
    // Why the service is absent, in plain words, when it is.
    Q_PROPERTY(QString presenceDetail READ presenceDetail NOTIFY presenceChanged FINAL)
    // The last answer of GetStatus (pax-service.md, 2.4); empty until one came.
    Q_PROPERTY(QVariantMap status READ status NOTIFY statusChanged FINAL)
    Q_PROPERTY(bool statusKnown READ statusKnown NOTIFY statusChanged FINAL)
    // off, idle, working or paused: the State property, as StateChanged and
    // GetStatus last said.
    Q_PROPERTY(QString serviceState READ serviceState NOTIFY statusChanged FINAL)

public:
    enum Presence {
        Unknown, // not looked for yet
        Absent,  // no bus, or nothing owns or can start org.tur.Agent1
        Present, // running, or startable by the bus
    };
    Q_ENUM(Presence)

    static QString serviceName();
    static QString objectPath();
    static QString interfaceName();

    // The D-Bus types of a method's arguments, one letter (or "as") each;
    // nothing for a name that is not a method of org.tur.Agent1.
    static std::optional<QStringList> argumentTypes(const QString &method);
    // "org.tur.Agent1.Error.TUR_AI_SERVICE_002" -> "TUR-AI-SERVICE-002". A
    // D-Bus error that is not the service's own keeps its D-Bus name.
    static QString codeOfError(const QString &errorName);
    // The service's message is "<code>: <sentence>"; this is the sentence.
    static QString sentenceOfError(const QString &message);
    // A JSON object or array as a JavaScript-friendly value; anything else
    // is returned as the text it was.
    static QVariant parseJson(const QString &text);

    explicit AgentClient(QObject *parent = nullptr);
    ~AgentClient() override;

    QString busAddress() const { return m_busAddress; }
    void setBusAddress(const QString &address);
    bool autoStart() const { return m_autoStart; }
    void setAutoStart(bool autoStart);
    Presence presence() const { return m_presence; }
    QString presenceDetail() const { return m_presenceDetail; }
    QVariantMap status() const { return m_status; }
    bool statusKnown() const { return !m_status.isEmpty(); }
    QString serviceState() const { return m_state; }

    // Connects to the bus, finds out whether the service is there, listens
    // to its signals and asks for its status. Called again after the bus
    // address changed.
    Q_INVOKABLE void start();
    // Asks for the status again. Requests that come while one is on its way
    // are folded into one more.
    Q_INVOKABLE void refresh();

    // Calls `method` of org.tur.Agent1 with `arguments`. `callback`, if it is
    // a function, is called once with an object:
    //   ok        true if the service answered, false if it refused or the
    //             call failed
    //   method    the method
    //   value     the first value of the answer (JSON parsed), or null
    //   values    every value of the answer
    //   code      on failure: the TUR-AI-* code, or the D-Bus error's name
    //   message   on failure: the sentence
    Q_INVOKABLE void call(const QString &method, const QVariantList &arguments, const QJSValue &callback = QJSValue());

    // The same, for C++.
    void callWith(const QString &method, const QVariantList &arguments,
                  const std::function<void(const QVariantMap &)> &done);

    // The connection in use; not connected before start().
    QDBusConnection connection() const;

Q_SIGNALS:
    void busAddressChanged();
    void autoStartChanged();
    void presenceChanged();
    void statusChanged();

    // org.tur.Agent1's signals, their JSON documents parsed.
    void taskStarted(const QString &task, const QVariant &details);
    void textAdded(const QString &task, const QString &kind, const QString &text);
    void actionProposed(const QString &task, const QVariant &step);
    void actionCompleted(const QString &task, const QVariant &step);
    void taskFinished(const QString &task, const QVariant &result);
    void localModelProgress(const QString &model, const QVariant &event);
    void scheduleProposed(const QString &proposal, const QVariant &details);
    void schedulesChanged();
    void memoryChanged();
    void stateChanged(const QString &state);

private Q_SLOTS:
    void onTaskStarted(const QString &task, const QString &details);
    void onTextAdded(const QString &task, const QString &kind, const QString &text);
    void onActionProposed(const QString &task, const QString &step);
    void onActionCompleted(const QString &task, const QString &step);
    void onTaskFinished(const QString &task, const QString &result);
    void onLocalModelProgress(const QString &model, const QString &event);
    void onScheduleProposed(const QString &proposal, const QString &details);
    void onSchedulesChanged();
    void onMemoryChanged();
    void onStateChanged(const QString &state);
    void onServiceRegistered();
    void onServiceUnregistered();

private:
    void disconnectBus();
    void lookForService();
    void setPresence(Presence presence, const QString &detail);
    void sendStatusRequest();
    static int timeoutOf(const QString &method);

    QString m_busAddress;
    bool m_autoStart = true;
    Presence m_presence = Unknown;
    QString m_presenceDetail;
    QVariantMap m_status;
    QString m_state;

    std::optional<QDBusConnection> m_connection;
    QString m_connectionName;
    QDBusServiceWatcher *m_watcher = nullptr;
    bool m_activatable = false;

    bool m_statusInFlight = false;
    bool m_statusWanted = false;
};
