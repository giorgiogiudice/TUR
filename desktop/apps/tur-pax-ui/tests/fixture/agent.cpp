// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
//
// A stand-in for P@x's session service, org.tur.Agent1, for the tests of
// tur-pax-ui: it owns the name on the bus it is given and answers from what
// the real service answered in CI (fixtures/agent1/, recorded by
// desktop/tests/pax-ui/e2e.py from `tur-pax serve` with the scripted
// provider; see fixtures/README.md). It decides nothing itself:
//
//  - a method answers with the recorded reply for the same method and
//    arguments, or the recorded refusal; a setting the recording has no
//    answer for is accepted and noted;
//  - GetStatus answers with one of the recorded statuses, the scenario's;
//    Enable and Disable and a local model that becomes ready move to
//    another recorded status;
//  - StartTask plays one recorded task, signal after signal, as the service
//    sent them; a task the service stopped because it was cancelled holds
//    before its end until CancelTask; UseLocalModel plays the recorded steps
//    of local AI the same way, holding before "cancelled" until
//    CancelLocalModel.
//
// The tests steer it over org.tur.Agent1Fixture at /org/tur/Agent1Fixture:
// Use(scenario), Calls(), KeyMatches(key), Release(). A key given to SetKey
// is kept in memory to answer KeyMatches and never printed; Calls() reports
// only its length.
//
//   tur-pax-ui-fixture-agent --fixture DIR [--bus ADDRESS]
//
// Not part of tur-pax-ui and not shipped.

#include <QCommandLineOption>
#include <QCommandLineParser>
#include <QCoreApplication>
#include <QDBusConnection>
#include <QDBusConnectionInterface>
#include <QDBusContext>
#include <QDBusError>
#include <QDBusMessage>
#include <QDir>
#include <QFile>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QJsonValue>
#include <QObject>
#include <QStringList>
#include <QTextStream>
#include <QTimer>
#include <QVariant>

namespace {

const QString agentName = QStringLiteral("org.tur.Agent1");
const QString agentPath = QStringLiteral("/org/tur/Agent1");
const QString errorPrefix = QStringLiteral("org.tur.Agent1.Error.");

QJsonObject readObject(const QString &path)
{
    QFile file(path);
    if (!file.open(QIODevice::ReadOnly))
        return {};
    return QJsonDocument::fromJson(file.readAll()).object();
}

// A value as it went on the wire: a JSON document is sent as its text.
QVariant wire(const QJsonValue &value)
{
    if (value.isObject())
        return QString::fromUtf8(QJsonDocument(value.toObject()).toJson(QJsonDocument::Compact));
    if (value.isArray())
        return QString::fromUtf8(QJsonDocument(value.toArray()).toJson(QJsonDocument::Compact));
    if (value.isBool())
        return value.toBool();
    if (value.isDouble())
        return value.toDouble();
    if (value.isNull())
        return QString();
    return value.toString();
}

// An argument as the key of a recorded reply: JSON objects in a canonical
// form (keys sorted, no spaces), so that the order a client wrote them in
// does not matter.
QString keyPart(const QVariant &value)
{
    if (value.metaType().id() == QMetaType::Bool)
        return value.toBool() ? QStringLiteral("true") : QStringLiteral("false");
    const QString text = value.toString();
    const QJsonDocument document = QJsonDocument::fromJson(text.toUtf8());
    if (document.isObject())
        return QString::fromUtf8(QJsonDocument(document.object()).toJson(QJsonDocument::Compact));
    return text;
}

QString errorNameOf(const QString &code)
{
    QString name = code;
    name.replace(QLatin1Char('-'), QLatin1Char('_'));
    return errorPrefix + name;
}

} // namespace

// The service, as recorded.
class Agent : public QObject, protected QDBusContext
{
    Q_OBJECT
    Q_CLASSINFO("D-Bus Interface", "org.tur.Agent1")
    Q_PROPERTY(QString State READ state)
    Q_PROPERTY(bool Enabled READ enabled)
    Q_PROPERTY(uint InterfaceVersion READ interfaceVersion)

public:
    explicit Agent(const QString &fixture, QObject *parent = nullptr)
        : QObject(parent)
        , m_fixture(fixture)
    {
        m_replies = readObject(fixture + QStringLiteral("/replies.json")).value(QStringLiteral("replies")).toObject();
        m_timer.setSingleShot(true);
        connect(&m_timer, &QTimer::timeout, this, &Agent::next);
        use(QJsonObject());
    }

    QString state() const { return status().value(QStringLiteral("state")).toString(); }
    bool enabled() const { return status().value(QStringLiteral("enabled")).toBool(); }
    uint interfaceVersion() const { return 1; }

    // The control side (org.tur.Agent1Fixture).
    void use(const QJsonObject &scenario)
    {
        m_timer.stop();
        m_sequence = QJsonArray();
        m_position = 0;
        m_holdAt = -1;
        m_running.clear();
        m_paused = false;
        m_localRunning = false;
        m_calls = QJsonArray();
        m_key.clear();
        m_scenario = scenario;
        m_statusName = scenario.value(QStringLiteral("status")).toString(QStringLiteral("off"));
    }

    QJsonArray calls() const { return m_calls; }
    bool keyMatches(const QString &key) const { return !m_key.isEmpty() && key == m_key; }

    void release()
    {
        if (m_holdAt < 0)
            return;
        m_holdAt = -1;
        m_timer.start(pace());
    }

public Q_SLOTS:
    QString StartTask(const QString &request, const QString &options)
    {
        note(QStringLiteral("StartTask"), { request, options });
        if (!m_running.isEmpty()) {
            refuse(QStringLiteral("TUR-AI-SERVICE-004"), QStringLiteral("a task is running"));
            return QString();
        }
        const QJsonObject current = status();
        const QJsonValue problem = current.value(QStringLiteral("problem"));
        if (problem.isObject()) {
            refuse(problem[QStringLiteral("error_code")].toString(), problem[QStringLiteral("message")].toString());
            return QString();
        }
        const QJsonObject task = taskFor(request);
        if (task.isEmpty()) {
            refuseUnrecorded(QStringLiteral("StartTask ") + request);
            return QString();
        }
        const QJsonArray recorded = task.value(QStringLiteral("signals")).toArray();
        QString id;
        int finish = -1;
        for (qsizetype i = 0; i < recorded.size(); ++i) {
            const QJsonObject signal = recorded.at(i).toObject();
            const QString member = signal.value(QStringLiteral("member")).toString();
            const QJsonArray arguments = signal.value(QStringLiteral("args")).toArray();
            if (member == QLatin1String("TaskStarted") && id.isEmpty())
                id = arguments.at(0).toString();
            if (member == QLatin1String("TaskFinished")
                && arguments.at(1).toObject().value(QStringLiteral("outcome")).toString() == QLatin1String("cancelled"))
                finish = int(i);
        }
        play(recorded, holdFor(recorded, finish));
        return id;
    }

    void CancelTask(const QString &task)
    {
        note(QStringLiteral("CancelTask"), { task });
        if (m_running.isEmpty()) {
            refuse(QStringLiteral("TUR-AI-SERVICE-007"), QStringLiteral("no task with that identifier is running"));
            return;
        }
        release();
    }

    void Pause()
    {
        note(QStringLiteral("Pause"), {});
        if (m_running.isEmpty()) {
            refuse(QStringLiteral("TUR-AI-SERVICE-007"), QStringLiteral("no task is running"));
            return;
        }
        m_paused = true;
        Q_EMIT StateChanged(QStringLiteral("paused"));
    }

    void Resume()
    {
        note(QStringLiteral("Resume"), {});
        if (m_running.isEmpty()) {
            refuse(QStringLiteral("TUR-AI-SERVICE-007"), QStringLiteral("no task is running"));
            return;
        }
        m_paused = false;
        Q_EMIT StateChanged(QStringLiteral("working"));
    }

    QString GetTask(const QString &task) { return recordedText(QStringLiteral("GetTask"), { task }); }
    QString ListActivity(uint limit) { return recordedText(QStringLiteral("ListActivity"), { QString::number(limit) }); }

    QString GetStatus()
    {
        return QString::fromUtf8(QJsonDocument(status()).toJson(QJsonDocument::Compact));
    }

    QString GetConsentText(const QString &provider, QString &text)
    {
        note(QStringLiteral("GetConsentText"), { provider });
        const QJsonObject recorded = recordedFor(QStringLiteral("GetConsentText"), { provider });
        if (recorded.isEmpty()) {
            refuseUnrecorded(keyOf(QStringLiteral("GetConsentText"), { provider }));
            return QString();
        }
        const QJsonArray values = recorded.value(QStringLiteral("reply")).toArray();
        text = values.at(1).toString();
        return values.at(0).toString();
    }

    void Enable(const QString &version)
    {
        note(QStringLiteral("Enable"), { version });
        const QString current = status().value(QStringLiteral("consent")).toObject().value(QStringLiteral("text_version")).toString();
        if (version != current) {
            if (!answerRecordedError(QStringLiteral("Enable"), { version }))
                refuse(QStringLiteral("TUR-AI-SERVICE-002"), QStringLiteral("the version named is not the version of the text in use"));
            return;
        }
        m_statusName = m_scenario.value(QStringLiteral("after_enable")).toString(QStringLiteral("ready"));
        Q_EMIT StateChanged(state());
    }

    void Disable()
    {
        note(QStringLiteral("Disable"), {});
        m_statusName = m_scenario.value(QStringLiteral("after_disable")).toString(QStringLiteral("off"));
        Q_EMIT StateChanged(state());
    }

    void SetProvider(const QString &provider, const QString &model) { setting(QStringLiteral("SetProvider"), { provider, model }); }
    void SetLimits(const QString &limits) { setting(QStringLiteral("SetLimits"), { limits }); }
    QString ListModels() { return recordedText(QStringLiteral("ListModels"), {}); }

    QString ListProviderModels(const QString &provider, bool refresh)
    {
        return recordedText(QStringLiteral("ListProviderModels"), { provider, refresh });
    }

    QString GetGenerationSettings(const QString &provider, const QString &model)
    {
        return recordedText(QStringLiteral("GetGenerationSettings"), { provider, model });
    }

    void SetGenerationSettings(const QString &provider, const QString &model, const QString &settings)
    {
        setting(QStringLiteral("SetGenerationSettings"), { provider, model, settings });
    }

    QString ListLocalModels() { return recordedText(QStringLiteral("ListLocalModels"), {}); }

    QString UseLocalModel(const QString &model, const QString &options)
    {
        const QString job = recordedText(QStringLiteral("UseLocalModel"), { model, options });
        if (job.isEmpty())
            return QString();
        if (m_localRunning) {
            refuse(QStringLiteral("TUR-AI-SERVICE-004"), QStringLiteral("a model is being made ready"));
            return QString();
        }
        const QJsonObject steps = readObject(m_fixture + QStringLiteral("/local/") + model + QStringLiteral(".json"));
        const QJsonArray recorded = steps.value(QStringLiteral("signals")).toArray();
        int cancelled = -1;
        for (qsizetype i = 0; i < recorded.size(); ++i) {
            const QJsonObject event = recorded.at(i).toObject().value(QStringLiteral("args")).toArray().at(1).toObject();
            if (event.value(QStringLiteral("event")).toString() == QLatin1String("cancelled"))
                cancelled = int(i);
        }
        m_localRunning = true;
        const int held = m_scenario.contains(QStringLiteral("local_hold_after"))
                             ? m_scenario.value(QStringLiteral("local_hold_after")).toInt() + 1
                             : cancelled;
        play(recorded, held);
        return job;
    }

    void CancelLocalModel()
    {
        note(QStringLiteral("CancelLocalModel"), {});
        if (!m_localRunning) {
            refuse(QStringLiteral("TUR-AI-SERVICE-007"), QStringLiteral("nothing is being made ready"));
            return;
        }
        release();
    }

    void SetKey(const QString &provider, const QString &key)
    {
        // The key is not noted, only its length.
        QJsonObject call { { QStringLiteral("method"), QStringLiteral("SetKey") },
                           { QStringLiteral("args"), QJsonArray { provider, QJsonObject { { QStringLiteral("key_length"), int(key.size()) } } } } };
        m_calls.append(call);
        if (answerRecordedError(QStringLiteral("SetKey"), { provider, key }, false))
            return;
        m_key = key;
    }

    bool RemoveKey(const QString &provider)
    {
        note(QStringLiteral("RemoveKey"), { provider });
        m_key.clear();
        return true;
    }

    bool HasKey(const QString &provider)
    {
        note(QStringLiteral("HasKey"), { provider });
        if (!m_key.isEmpty())
            return true;
        const QJsonObject recorded = recordedFor(QStringLiteral("HasKey"), { provider }, false);
        return !recorded.isEmpty() && recorded.value(QStringLiteral("reply")).toArray().at(0).toBool();
    }

Q_SIGNALS:
    void TaskStarted(const QString &task, const QString &details);
    void TextAdded(const QString &task, const QString &kind, const QString &text);
    void ActionProposed(const QString &task, const QString &step);
    void ActionCompleted(const QString &task, const QString &step);
    void TaskFinished(const QString &task, const QString &result);
    void ScheduleProposed(const QString &proposal, const QString &details);
    void SchedulesChanged();
    void MemoryChanged();
    void StateChanged(const QString &state);
    void LocalModelProgress(const QString &model, const QString &event);

private:
    QJsonObject status() const
    {
        QJsonObject document = readObject(m_fixture + QStringLiteral("/status/") + m_statusName + QStringLiteral(".json"))
                                   .value(QStringLiteral("recorded"))
                                   .toObject();
        // While a recorded task plays, the service would say so.
        if (!m_running.isEmpty()) {
            document.insert(QStringLiteral("state"), m_paused ? QStringLiteral("paused") : QStringLiteral("working"));
            document.insert(QStringLiteral("running"),
                            QJsonObject { { QStringLiteral("task"), m_running },
                                          { QStringLiteral("attended"), true },
                                          { QStringLiteral("schedule"), QJsonValue::Null },
                                          { QStringLiteral("paused"), m_paused } });
        }
        return document;
    }

    int pace() const { return m_scenario.value(QStringLiteral("pace_ms")).toInt(60); }

    // Where a task holds: before the scenario's `hold_count`-th signal
    // `hold_before` ("ActionCompleted", 2: while the second step runs), or
    // after the scenario's `hold_after`-th signal, or where the recording
    // says (before the end of a task that was cancelled).
    int holdFor(const QJsonArray &recorded, int recordedHold) const
    {
        const QString member = m_scenario.value(QStringLiteral("hold_before")).toString();
        if (!member.isEmpty()) {
            int seen = 0;
            const int wanted = m_scenario.value(QStringLiteral("hold_count")).toInt(1);
            for (qsizetype i = 0; i < recorded.size(); ++i) {
                if (recorded.at(i).toObject().value(QStringLiteral("member")).toString() == member && ++seen == wanted)
                    return int(i);
            }
        }
        if (m_scenario.contains(QStringLiteral("hold_after")))
            return m_scenario.value(QStringLiteral("hold_after")).toInt() + 1;
        return recordedHold;
    }

    QJsonObject taskFor(const QString &request) const
    {
        const QString named = m_scenario.value(QStringLiteral("task")).toString();
        if (!named.isEmpty())
            return readObject(m_fixture + QStringLiteral("/tasks/") + named + QStringLiteral(".json"));
        const QStringList files = QDir(m_fixture + QStringLiteral("/tasks")).entryList({ QStringLiteral("*.json") }, QDir::Files, QDir::Name);
        for (const QString &file : files) {
            const QJsonObject task = readObject(m_fixture + QStringLiteral("/tasks/") + file);
            if (task.value(QStringLiteral("request")).toString() == request)
                return task;
        }
        return {};
    }

    void play(const QJsonArray &recorded, int holdAt)
    {
        m_sequence = recorded;
        m_position = 0;
        m_holdAt = holdAt;
        m_timer.start(pace());
    }

    void next()
    {
        if (m_position >= m_sequence.size())
            return;
        if (m_holdAt >= 0 && m_position >= m_holdAt)
            return;
        const QJsonObject signal = m_sequence.at(m_position++).toObject();
        emitRecorded(signal.value(QStringLiteral("member")).toString(), signal.value(QStringLiteral("args")).toArray());
        if (m_position < m_sequence.size())
            m_timer.start(pace());
    }

    void emitRecorded(const QString &member, const QJsonArray &arguments)
    {
        const auto text = [&arguments](int i) { return wire(arguments.at(i)).toString(); };
        if (member == QLatin1String("TaskStarted")) {
            m_running = text(0);
            Q_EMIT TaskStarted(text(0), text(1));
        } else if (member == QLatin1String("TextAdded")) {
            Q_EMIT TextAdded(text(0), text(1), text(2));
        } else if (member == QLatin1String("ActionProposed")) {
            Q_EMIT ActionProposed(text(0), text(1));
        } else if (member == QLatin1String("ActionCompleted")) {
            Q_EMIT ActionCompleted(text(0), text(1));
        } else if (member == QLatin1String("TaskFinished")) {
            m_running.clear();
            m_paused = false;
            Q_EMIT TaskFinished(text(0), text(1));
        } else if (member == QLatin1String("ScheduleProposed")) {
            Q_EMIT ScheduleProposed(text(0), text(1));
        } else if (member == QLatin1String("SchedulesChanged")) {
            Q_EMIT SchedulesChanged();
        } else if (member == QLatin1String("MemoryChanged")) {
            Q_EMIT MemoryChanged();
        } else if (member == QLatin1String("StateChanged")) {
            Q_EMIT StateChanged(text(0));
        } else if (member == QLatin1String("LocalModelProgress")) {
            const QString event = arguments.at(1).toObject().value(QStringLiteral("event")).toString();
            if (event == QLatin1String("in-use")) {
                m_localRunning = false;
                m_statusName = m_scenario.value(QStringLiteral("after_local")).toString(QStringLiteral("local-consent"));
            } else if (event == QLatin1String("cancelled") || event == QLatin1String("failed")) {
                m_localRunning = false;
            }
            Q_EMIT LocalModelProgress(text(0), text(1));
        }
    }

    void note(const QString &method, const QVariantList &arguments)
    {
        QJsonArray values;
        for (const QVariant &argument : arguments)
            values.append(QJsonValue::fromVariant(argument));
        m_calls.append(QJsonObject { { QStringLiteral("method"), method }, { QStringLiteral("args"), values } });
    }

    static QString keyOf(const QString &method, const QVariantList &arguments)
    {
        QStringList parts { method };
        for (const QVariant &argument : arguments)
            parts.append(keyPart(argument));
        return parts.join(QLatin1Char(' '));
    }

    // The recorded answer to this call, or empty. `noted` false: the call
    // is noted by the caller (SetKey, which notes no key).
    QJsonObject recordedFor(const QString &method, const QVariantList &arguments, bool noted = true)
    {
        Q_UNUSED(noted)
        const QJsonValue found = m_replies.value(keyOf(method, arguments));
        return found.toObject();
    }

    bool answerRecordedError(const QString &method, const QVariantList &arguments, bool noteIt = true)
    {
        const QJsonObject recorded = recordedFor(method, arguments, noteIt);
        if (!recorded.contains(QStringLiteral("error")))
            return false;
        sendErrorReply(recorded.value(QStringLiteral("error")).toString(), recorded.value(QStringLiteral("message")).toString());
        return true;
    }

    // A call whose answer is a text: the recorded one, or a refusal.
    QString recordedText(const QString &method, const QVariantList &arguments)
    {
        note(method, arguments);
        const QJsonObject recorded = recordedFor(method, arguments);
        if (recorded.isEmpty()) {
            refuseUnrecorded(keyOf(method, arguments));
            return QString();
        }
        if (recorded.contains(QStringLiteral("error"))) {
            sendErrorReply(recorded.value(QStringLiteral("error")).toString(), recorded.value(QStringLiteral("message")).toString());
            return QString();
        }
        return wire(recorded.value(QStringLiteral("reply")).toArray().at(0)).toString();
    }

    // A call that changes a setting: refused as recorded, otherwise accepted.
    void setting(const QString &method, const QVariantList &arguments)
    {
        note(method, arguments);
        answerRecordedError(method, arguments, false);
    }

    void refuse(const QString &code, const QString &sentence)
    {
        sendErrorReply(errorNameOf(code), code + QStringLiteral(": ") + sentence);
    }

    void refuseUnrecorded(const QString &key)
    {
        sendErrorReply(QStringLiteral("org.tur.Agent1Fixture.Error.NotRecorded"),
                       QStringLiteral("the fixture has no recorded answer for: ") + key);
    }

    QString m_fixture;
    QJsonObject m_replies;
    QJsonObject m_scenario;
    QString m_statusName;
    QJsonArray m_calls;
    QString m_key;

    QTimer m_timer;
    QJsonArray m_sequence;
    qsizetype m_position = 0;
    int m_holdAt = -1;
    QString m_running;
    bool m_paused = false;
    bool m_localRunning = false;
};

// How the tests steer the stand-in.
class Control : public QObject
{
    Q_OBJECT
    Q_CLASSINFO("D-Bus Interface", "org.tur.Agent1Fixture")

public:
    explicit Control(Agent *agent, QObject *parent = nullptr)
        : QObject(parent)
        , m_agent(agent)
    {
    }

public Q_SLOTS:
    void Use(const QString &scenario)
    {
        m_agent->use(QJsonDocument::fromJson(scenario.toUtf8()).object());
    }

    QString Calls()
    {
        return QString::fromUtf8(QJsonDocument(m_agent->calls()).toJson(QJsonDocument::Compact));
    }

    bool KeyMatches(const QString &key) { return m_agent->keyMatches(key); }
    void Release() { m_agent->release(); }

private:
    Agent *m_agent;
};

int main(int argc, char *argv[])
{
    QCoreApplication app(argc, argv);
    QCommandLineParser parser;
    parser.setApplicationDescription(QStringLiteral("A stand-in for org.tur.Agent1, answering from recorded replies."));
    parser.addHelpOption();
    const QCommandLineOption fixture(QStringLiteral("fixture"), QStringLiteral("The recorded answers."), QStringLiteral("DIR"));
    const QCommandLineOption bus(QStringLiteral("bus"), QStringLiteral("The bus to own the name on."), QStringLiteral("ADDRESS"));
    parser.addOptions({ fixture, bus });
    parser.process(app);

    QTextStream err(stderr);
    if (!parser.isSet(fixture) || !QFile::exists(parser.value(fixture) + QStringLiteral("/replies.json"))) {
        err << "tur-pax-ui-fixture-agent: --fixture names no directory with replies.json\n";
        return 2;
    }
    QDBusConnection connection = parser.isSet(bus)
                                     ? QDBusConnection::connectToBus(parser.value(bus), QStringLiteral("fixture-agent"))
                                     : QDBusConnection::sessionBus();
    if (!connection.isConnected()) {
        err << "tur-pax-ui-fixture-agent: no bus: " << connection.lastError().message() << '\n';
        return 1;
    }
    Agent agent(parser.value(fixture));
    Control control(&agent);
    const auto options = QDBusConnection::ExportAllSlots | QDBusConnection::ExportAllSignals | QDBusConnection::ExportAllProperties;
    if (!connection.registerObject(agentPath, &agent, options)
        || !connection.registerObject(QStringLiteral("/org/tur/Agent1Fixture"), &control, QDBusConnection::ExportAllSlots)
        || !connection.registerService(agentName)) {
        err << "tur-pax-ui-fixture-agent: could not take " << agentName << ": " << connection.lastError().message() << '\n';
        return 1;
    }
    err << "tur-pax-ui-fixture-agent: serving " << agentName << " from " << parser.value(fixture) << '\n';
    err.flush();
    return app.exec();
}

#include "agent.moc"
