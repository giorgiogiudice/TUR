// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#include "agentclient.h"

#include <QCoreApplication>
#include <QDBusConnectionInterface>
#include <QDBusMessage>
#include <QDBusPendingCall>
#include <QDBusPendingCallWatcher>
#include <QDBusReply>
#include <QDBusServiceWatcher>
#include <QHash>
#include <QJSEngine>
#include <QJsonDocument>
#include <QJsonParseError>
#include <QMetaType>
#include <QPointer>
#include <QRegularExpression>
#include <QTimer>
#include <QtQml/qqml.h>

namespace {

// The service's signals and the slots that receive them.
struct Hook
{
    const char *name;
    const char *slot;
};

const Hook hooks[] = {
    { "TaskStarted", SLOT(onTaskStarted(QString,QString)) },
    { "TextAdded", SLOT(onTextAdded(QString,QString,QString)) },
    { "ActionProposed", SLOT(onActionProposed(QString,QString)) },
    { "ActionCompleted", SLOT(onActionCompleted(QString,QString)) },
    { "TaskFinished", SLOT(onTaskFinished(QString,QString)) },
    { "LocalModelProgress", SLOT(onLocalModelProgress(QString,QString)) },
    { "ScheduleProposed", SLOT(onScheduleProposed(QString,QString)) },
    { "SchedulesChanged", SLOT(onSchedulesChanged()) },
    { "MemoryChanged", SLOT(onMemoryChanged()) },
    { "StateChanged", SLOT(onStateChanged(QString)) },
};

// Every method of org.tur.Agent1, version 1, with the D-Bus types of its
// arguments (services/tur-pax/interfaces/org.tur.Agent1.xml). A test holds
// this table and the committed file together.
const QHash<QString, QStringList> &methods()
{
    static const QHash<QString, QStringList> table = {
        { QStringLiteral("StartTask"), { QStringLiteral("s"), QStringLiteral("s") } },
        { QStringLiteral("CancelTask"), { QStringLiteral("s") } },
        { QStringLiteral("Pause"), {} },
        { QStringLiteral("Resume"), {} },
        { QStringLiteral("GetTask"), { QStringLiteral("s") } },
        { QStringLiteral("ListActivity"), { QStringLiteral("u") } },
        { QStringLiteral("GetStatus"), {} },
        { QStringLiteral("GetConsentText"), { QStringLiteral("s") } },
        { QStringLiteral("Enable"), { QStringLiteral("s") } },
        { QStringLiteral("Disable"), {} },
        { QStringLiteral("SetProvider"), { QStringLiteral("s"), QStringLiteral("s") } },
        { QStringLiteral("SetLimits"), { QStringLiteral("s") } },
        { QStringLiteral("ListModels"), {} },
        { QStringLiteral("ListProviderModels"), { QStringLiteral("s"), QStringLiteral("b") } },
        { QStringLiteral("GetGenerationSettings"), { QStringLiteral("s"), QStringLiteral("s") } },
        { QStringLiteral("SetGenerationSettings"), { QStringLiteral("s"), QStringLiteral("s"), QStringLiteral("s") } },
        { QStringLiteral("ListLocalModels"), {} },
        { QStringLiteral("UseLocalModel"), { QStringLiteral("s"), QStringLiteral("s") } },
        { QStringLiteral("CancelLocalModel"), {} },
        { QStringLiteral("SetKey"), { QStringLiteral("s"), QStringLiteral("s") } },
        { QStringLiteral("RemoveKey"), { QStringLiteral("s") } },
        { QStringLiteral("HasKey"), { QStringLiteral("s") } },
        { QStringLiteral("ListProposals"), {} },
        { QStringLiteral("ConfirmProposal"), { QStringLiteral("s") } },
        { QStringLiteral("DismissProposal"), { QStringLiteral("s") } },
        { QStringLiteral("CreateSchedule"), { QStringLiteral("s") } },
        { QStringLiteral("RemoveSchedule"), { QStringLiteral("s") } },
        { QStringLiteral("ListSchedules"), {} },
        { QStringLiteral("GetAway"), {} },
        { QStringLiteral("MarkAwaySeen"), { QStringLiteral("as") } },
        { QStringLiteral("Wake"), {} },
        { QStringLiteral("ListMemory"), {} },
        { QStringLiteral("SetMemoryEnabled"), { QStringLiteral("b") } },
        { QStringLiteral("ConfirmMemory"), { QStringLiteral("s") } },
        { QStringLiteral("ForgetMemory"), { QStringLiteral("s") } },
    };
    return table;
}

QVariantMap failure(const QString &method, const QString &code, const QString &message)
{
    return {
        { QStringLiteral("ok"), false },
        { QStringLiteral("method"), method },
        { QStringLiteral("value"), QVariant::fromValue(nullptr) },
        { QStringLiteral("values"), QVariantList() },
        { QStringLiteral("code"), code },
        { QStringLiteral("message"), message },
    };
}

} // namespace

QString AgentClient::serviceName()
{
    return QStringLiteral("org.tur.Agent1");
}

QString AgentClient::objectPath()
{
    return QStringLiteral("/org/tur/Agent1");
}

QString AgentClient::interfaceName()
{
    return QStringLiteral("org.tur.Agent1");
}

std::optional<QStringList> AgentClient::argumentTypes(const QString &method)
{
    const auto found = methods().constFind(method);
    if (found == methods().constEnd())
        return std::nullopt;
    return *found;
}

QString AgentClient::codeOfError(const QString &errorName)
{
    const QString prefix = QStringLiteral("org.tur.Agent1.Error.");
    if (errorName.startsWith(prefix)) {
        QString code = errorName.mid(prefix.size());
        code.replace(QLatin1Char('_'), QLatin1Char('-'));
        return code;
    }
    return errorName;
}

QString AgentClient::sentenceOfError(const QString &message)
{
    static const QRegularExpression coded(QStringLiteral("^TUR-[A-Z0-9-]+: "));
    const QRegularExpressionMatch match = coded.match(message);
    return match.hasMatch() ? message.mid(match.capturedLength()) : message;
}

QVariant AgentClient::parseJson(const QString &text)
{
    const QString trimmed = text.trimmed();
    if (!trimmed.startsWith(QLatin1Char('{')) && !trimmed.startsWith(QLatin1Char('[')))
        return text;
    QJsonParseError error;
    const QJsonDocument document = QJsonDocument::fromJson(trimmed.toUtf8(), &error);
    if (error.error != QJsonParseError::NoError || document.isNull())
        return text;
    return document.toVariant();
}

AgentClient::AgentClient(QObject *parent)
    : QObject(parent)
{
}

AgentClient::~AgentClient()
{
    disconnectBus();
}

void AgentClient::setBusAddress(const QString &address)
{
    if (address == m_busAddress)
        return;
    m_busAddress = address;
    Q_EMIT busAddressChanged();
    if (m_connection) {
        disconnectBus();
        start();
    }
}

void AgentClient::setAutoStart(bool autoStart)
{
    if (autoStart == m_autoStart)
        return;
    m_autoStart = autoStart;
    Q_EMIT autoStartChanged();
}

QDBusConnection AgentClient::connection() const
{
    return m_connection ? *m_connection : QDBusConnection(QString());
}

void AgentClient::start()
{
    if (!m_connection) {
        if (m_busAddress.isEmpty()) {
            m_connection.emplace(QDBusConnection::sessionBus());
        } else {
            m_connectionName = QStringLiteral("tur-pax-ui-%1").arg(quintptr(this), 0, 16);
            m_connection.emplace(QDBusConnection::connectToBus(m_busAddress, m_connectionName));
        }
        if (m_connection->isConnected()) {
            for (const Hook &hook : hooks) {
                m_connection->connect(serviceName(), objectPath(), interfaceName(), QString::fromLatin1(hook.name),
                                      this, hook.slot);
            }
            m_watcher = new QDBusServiceWatcher(serviceName(), *m_connection,
                                                QDBusServiceWatcher::WatchForRegistration
                                                    | QDBusServiceWatcher::WatchForUnregistration,
                                                this);
            connect(m_watcher, &QDBusServiceWatcher::serviceRegistered, this, &AgentClient::onServiceRegistered);
            connect(m_watcher, &QDBusServiceWatcher::serviceUnregistered, this, &AgentClient::onServiceUnregistered);
        }
    }
    lookForService();
    if (m_presence == Present)
        refresh();
}

void AgentClient::disconnectBus()
{
    if (!m_connection)
        return;
    if (m_connection->isConnected()) {
        for (const Hook &hook : hooks) {
            m_connection->disconnect(serviceName(), objectPath(), interfaceName(), QString::fromLatin1(hook.name),
                                     this, hook.slot);
        }
    }
    delete m_watcher;
    m_watcher = nullptr;
    m_connection.reset();
    if (!m_connectionName.isEmpty())
        QDBusConnection::disconnectFromBus(m_connectionName);
    m_connectionName.clear();
    m_statusInFlight = false;
    m_statusWanted = false;
}

void AgentClient::lookForService()
{
    if (!m_connection || !m_connection->isConnected()) {
        setPresence(Absent, tr("There is no session bus, so P@x cannot be reached."));
        return;
    }
    bool owned = false;
    if (QDBusConnectionInterface *bus = m_connection->interface()) {
        const QDBusReply<bool> registered = bus->isServiceRegistered(serviceName());
        owned = registered.isValid() && registered.value();
    }
    const QDBusMessage list = QDBusMessage::createMethodCall(
        QStringLiteral("org.freedesktop.DBus"), QStringLiteral("/org/freedesktop/DBus"),
        QStringLiteral("org.freedesktop.DBus"), QStringLiteral("ListActivatableNames"));
    const QDBusMessage names = m_connection->call(list, QDBus::Block, 10000);
    m_activatable = names.type() == QDBusMessage::ReplyMessage
                    && names.arguments().value(0).toStringList().contains(serviceName());
    if (owned || (m_activatable && m_autoStart))
        setPresence(Present, QString());
    else if (m_activatable)
        setPresence(Absent, tr("P@x is not running."));
    else
        setPresence(Absent, tr("P@x's service is not on this computer."));
}

void AgentClient::setPresence(Presence presence, const QString &detail)
{
    if (presence == m_presence && detail == m_presenceDetail)
        return;
    m_presence = presence;
    m_presenceDetail = detail;
    if (presence != Present && !m_status.isEmpty()) {
        m_status.clear();
        m_state.clear();
        Q_EMIT statusChanged();
    }
    Q_EMIT presenceChanged();
}

void AgentClient::refresh()
{
    if (!m_connection || m_presence != Present)
        return;
    m_statusWanted = true;
    if (m_statusInFlight)
        return;
    QTimer::singleShot(0, this, [this] {
        if (m_statusWanted && !m_statusInFlight)
            sendStatusRequest();
    });
}

void AgentClient::sendStatusRequest()
{
    m_statusInFlight = true;
    m_statusWanted = false;
    callWith(QStringLiteral("GetStatus"), {}, [this](const QVariantMap &reply) {
        m_statusInFlight = false;
        if (reply.value(QStringLiteral("ok")).toBool()) {
            const QVariantMap status = reply.value(QStringLiteral("value")).toMap();
            if (!status.isEmpty()) {
                m_status = status;
                m_state = status.value(QStringLiteral("state")).toString();
                Q_EMIT statusChanged();
            }
        }
        if (m_statusWanted)
            sendStatusRequest();
    });
}

int AgentClient::timeoutOf(const QString &method)
{
    // The password store may ask the person before it answers, and P@x
    // waits up to two minutes for it (pax-service.md, section 5); local AI
    // runs a program; asking a provider for its list goes over the network.
    static const QStringList slow = {
        QStringLiteral("SetKey"), QStringLiteral("RemoveKey"), QStringLiteral("HasKey"),
        QStringLiteral("StartTask"), QStringLiteral("ListLocalModels"), QStringLiteral("ListProviderModels"),
    };
    return slow.contains(method) ? 180000 : 30000;
}

void AgentClient::call(const QString &method, const QVariantList &arguments, const QJSValue &callback)
{
    const QJSValue function = callback;
    const QPointer<AgentClient> self(this);
    callWith(method, arguments, [self, function](const QVariantMap &reply) {
        if (!self || !function.isCallable())
            return;
        QJSEngine *engine = qjsEngine(self.data());
        if (!engine)
            return;
        const QJSValue result = function.call({ engine->toScriptValue(reply) });
        if (result.isError())
            qWarning("tur-pax-ui: a reply handler failed: %s", qPrintable(result.toString()));
    });
}

void AgentClient::callWith(const QString &method, const QVariantList &arguments,
                           const std::function<void(const QVariantMap &)> &done)
{
    // A view may ask before the window has started the client.
    if (!m_connection)
        start();
    const std::optional<QStringList> types = argumentTypes(method);
    QVariantMap refused;
    if (!types) {
        refused = failure(method, QStringLiteral("org.freedesktop.DBus.Error.UnknownMethod"),
                          tr("%1 is not a method of P@x's service.").arg(method));
    } else if (types->size() != arguments.size()) {
        refused = failure(method, QStringLiteral("org.freedesktop.DBus.Error.InvalidArgs"),
                          tr("%1 takes %2 values, not %3.").arg(method).arg(types->size()).arg(arguments.size()));
    } else if (!m_connection || !m_connection->isConnected()) {
        refused = failure(method, QStringLiteral("org.freedesktop.DBus.Error.Disconnected"),
                          tr("There is no session bus, so P@x cannot be reached."));
    }
    if (!refused.isEmpty()) {
        // Answered later, like every other call, so that a caller never
        // sees its callback run before call() returns.
        QTimer::singleShot(0, this, [refused, done] {
            if (done)
                done(refused);
        });
        return;
    }

    QDBusMessage message = QDBusMessage::createMethodCall(serviceName(), objectPath(), interfaceName(), method);
    message.setAutoStartService(m_autoStart);
    QVariantList values;
    for (qsizetype i = 0; i < arguments.size(); ++i) {
        const QString &type = types->at(i);
        const QVariant &argument = arguments.at(i);
        if (type == QLatin1String("u"))
            values.append(QVariant::fromValue(uint(argument.toUInt())));
        else if (type == QLatin1String("b"))
            values.append(QVariant(argument.toBool()));
        else if (type == QLatin1String("as"))
            values.append(QVariant(argument.toStringList()));
        else
            values.append(QVariant(argument.toString()));
    }
    message.setArguments(values);

    const QDBusPendingCall pending = m_connection->asyncCall(message, timeoutOf(method));
    auto *watcher = new QDBusPendingCallWatcher(pending, this);
    connect(watcher, &QDBusPendingCallWatcher::finished, this, [this, method, done](QDBusPendingCallWatcher *call) {
        call->deleteLater();
        const QDBusMessage reply = call->reply();
        QVariantMap out;
        if (reply.type() == QDBusMessage::ErrorMessage) {
            out = failure(method, codeOfError(reply.errorName()), sentenceOfError(reply.errorMessage()));
            const QString name = reply.errorName();
            if (name == QLatin1String("org.freedesktop.DBus.Error.ServiceUnknown")
                || name == QLatin1String("org.freedesktop.DBus.Error.NameHasNoOwner")
                || name == QLatin1String("org.freedesktop.DBus.Error.Spawn.ChildExited")) {
                lookForService();
            }
        } else {
            QVariantList values;
            const QVariantList arguments = reply.arguments();
            for (const QVariant &argument : arguments) {
                if (argument.metaType().id() == QMetaType::QString)
                    values.append(parseJson(argument.toString()));
                else
                    values.append(argument);
            }
            out = {
                { QStringLiteral("ok"), true },
                { QStringLiteral("method"), method },
                { QStringLiteral("value"), values.isEmpty() ? QVariant::fromValue(nullptr) : values.constFirst() },
                { QStringLiteral("values"), values },
                { QStringLiteral("code"), QString() },
                { QStringLiteral("message"), QString() },
            };
            if (m_presence != Present)
                setPresence(Present, QString());
        }
        if (done)
            done(out);
    });
}

void AgentClient::onTaskStarted(const QString &task, const QString &details)
{
    Q_EMIT taskStarted(task, parseJson(details));
}

void AgentClient::onTextAdded(const QString &task, const QString &kind, const QString &text)
{
    Q_EMIT textAdded(task, kind, text);
    // A response came: the day's spending has changed.
    refresh();
}

void AgentClient::onActionProposed(const QString &task, const QString &step)
{
    Q_EMIT actionProposed(task, parseJson(step));
}

void AgentClient::onActionCompleted(const QString &task, const QString &step)
{
    Q_EMIT actionCompleted(task, parseJson(step));
    refresh();
}

void AgentClient::onTaskFinished(const QString &task, const QString &result)
{
    Q_EMIT taskFinished(task, parseJson(result));
    refresh();
}

void AgentClient::onLocalModelProgress(const QString &model, const QString &event)
{
    const QVariant parsed = parseJson(event);
    Q_EMIT localModelProgress(model, parsed);
    const QString name = parsed.toMap().value(QStringLiteral("event")).toString();
    if (name == QLatin1String("in-use") || name == QLatin1String("failed") || name == QLatin1String("cancelled"))
        refresh();
}

void AgentClient::onScheduleProposed(const QString &proposal, const QString &details)
{
    Q_EMIT scheduleProposed(proposal, parseJson(details));
}

void AgentClient::onSchedulesChanged()
{
    Q_EMIT schedulesChanged();
}

void AgentClient::onMemoryChanged()
{
    Q_EMIT memoryChanged();
}

void AgentClient::onStateChanged(const QString &state)
{
    if (state != m_state) {
        m_state = state;
        if (!m_status.isEmpty())
            m_status.insert(QStringLiteral("state"), state);
        Q_EMIT statusChanged();
    }
    Q_EMIT stateChanged(state);
    refresh();
}

void AgentClient::onServiceRegistered()
{
    setPresence(Present, QString());
    refresh();
}

void AgentClient::onServiceUnregistered()
{
    lookForService();
    if (m_presence == Present)
        refresh();
}
