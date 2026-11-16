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
// Voice (docs/architecture/pax-voice.md, section 6) is the one part it does
// not answer from a recording: the real service's voice needs a microphone,
// so its documents are written by hand from the design, in fixtures/voice/
// beside the recording (see fixtures/README.md), and the recorder neither
// writes nor compares them. GetVoiceStatus answers with one of
// fixtures/voice/status/; StartListening plays fixtures/voice/turns/<turn>
// and holds where the person would stop speaking until StopListening;
// SendTranscript plays the task the turn names; StartLiveSession plays
// fixtures/voice/live.json and holds before its end until EndLiveSession;
// PrepareVoice plays fixtures/voice/prepare.json.
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
    Q_PROPERTY(QString Voice READ voice)
    Q_PROPERTY(bool MicrophoneOpen READ microphoneOpen)

public:
    explicit Agent(const QString &fixture, QObject *parent = nullptr)
        : QObject(parent)
        , m_fixture(fixture)
        , m_voiceDir(QDir(fixture).absoluteFilePath(QStringLiteral("../voice")))
    {
        m_replies = readObject(fixture + QStringLiteral("/replies.json")).value(QStringLiteral("replies")).toObject();
        m_timer.setSingleShot(true);
        connect(&m_timer, &QTimer::timeout, this, &Agent::next);
        use(QJsonObject());
    }

    QString state() const { return status().value(QStringLiteral("state")).toString(); }
    bool enabled() const { return status().value(QStringLiteral("enabled")).toBool(); }
    uint interfaceVersion() const { return 1; }
    QString voice() const { return voiceStatus().value(QStringLiteral("state")).toString(); }
    bool microphoneOpen() const { return voiceStatus().value(QStringLiteral("microphone_open")).toBool(); }

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
        m_voiceName = scenario.value(QStringLiteral("voice")).toString(QStringLiteral("off"));
        m_voiceState.clear();
        m_microphone = false;
        m_turn.clear();
        m_waitingTurn.clear();
        m_turnFile = QJsonObject();
        m_session.clear();
        m_preparing = false;
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
        // The stop key also closes the microphone and ends a live session
        // (pax-voice.md, 6.1).
        if (m_running.isEmpty() && !m_session.isEmpty()) {
            release();
            return;
        }
        if (m_running.isEmpty() && !m_turn.isEmpty()) {
            dropTurn(QStringLiteral("stop"));
            return;
        }
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

    // --- voice, from the hand-written documents of fixtures/voice -------------

    QString GetVoiceStatus()
    {
        return QString::fromUtf8(QJsonDocument(voiceStatus()).toJson(QJsonDocument::Compact));
    }

    void SetVoiceSettings(const QString &settings)
    {
        note(QStringLiteral("SetVoiceSettings"), { settings });
        const QJsonObject given = QJsonDocument::fromJson(settings.toUtf8()).object();
        if (given.isEmpty() && settings.trimmed() != QLatin1String("{}")) {
            refuse(QStringLiteral("TUR-AI-SERVICE-006"), QStringLiteral("the settings are not one JSON object"));
            return;
        }
        if (given.value(QStringLiteral("wake_word")).toBool()) {
            refuse(QStringLiteral("TUR-AI-VOICE-005"), QStringLiteral("a wake word is not available in this version"));
            return;
        }
        if (given.contains(QStringLiteral("enabled"))) {
            const bool on = given.value(QStringLiteral("enabled")).toBool();
            m_voiceName = on ? m_scenario.value(QStringLiteral("after_voice_enable")).toString(QStringLiteral("ready"))
                             : QStringLiteral("off");
            m_voiceState.clear();
            const QString state = voice();
            Q_EMIT VoiceStateChanged(state, compact(QJsonObject { { QStringLiteral("state"), state },
                                                                  { QStringLiteral("microphone_open"), false } }));
        }
    }

    QString PrepareVoice(const QString &options)
    {
        note(QStringLiteral("PrepareVoice"), { options });
        if (m_preparing) {
            refuse(QStringLiteral("TUR-AI-SERVICE-004"), QStringLiteral("voice is being made ready"));
            return QString();
        }
        const QJsonObject asked = QJsonDocument::fromJson(options.toUtf8()).object();
        const QJsonObject current = voiceStatus();
        qint64 needed = qint64(current.value(QStringLiteral("download_bytes_needed")).toDouble());
        // With the models named (a recognition model the person chose in
        // place of the recommended one), the size is theirs: what of them is
        // not on this computer, as the status lists each.
        if (asked.contains(QStringLiteral("models")))
            needed = bytesOf(current, asked.value(QStringLiteral("models")).toArray());
        if (needed > 0 && qint64(asked.value(QStringLiteral("download_bytes")).toDouble()) != needed) {
            refuse(QStringLiteral("TUR-AI-SERVICE-006"),
                   QStringLiteral("the download size named is not the size of what would be downloaded (%1 bytes)").arg(needed));
            return QString();
        }
        const QJsonArray steps = readObject(m_voiceDir + QStringLiteral("/prepare.json")).value(QStringLiteral("signals")).toArray();
        int cancelled = -1;
        for (qsizetype i = 0; i < steps.size(); ++i) {
            const QJsonObject event = steps.at(i).toObject().value(QStringLiteral("args")).toArray().at(1).toObject();
            if (event.value(QStringLiteral("event")).toString() == QLatin1String("cancelled"))
                cancelled = int(i);
        }
        m_preparing = true;
        const int held = m_scenario.contains(QStringLiteral("voice_prepare_hold_after"))
                             ? m_scenario.value(QStringLiteral("voice_prepare_hold_after")).toInt() + 1
                             : cancelled;
        play(steps, held);
        return QStringLiteral("v-prepare-1");
    }

    void CancelVoicePreparation()
    {
        note(QStringLiteral("CancelVoicePreparation"), {});
        if (!m_preparing) {
            refuse(QStringLiteral("TUR-AI-SERVICE-007"), QStringLiteral("nothing is being made ready"));
            return;
        }
        // What was played up to the hold stays; the end is "cancelled".
        const QJsonObject event { { QStringLiteral("event"), QStringLiteral("cancelled") },
                                  { QStringLiteral("code"), QStringLiteral("TUR-AI-LOCAL-029") },
                                  { QStringLiteral("message"), QStringLiteral("Stopped, as you asked.") } };
        m_timer.stop();
        m_sequence = QJsonArray();
        m_holdAt = -1;
        m_preparing = false;
        Q_EMIT VoiceProgress(QString(), compact(event));
    }

    QString StartListening(const QString &options)
    {
        note(QStringLiteral("StartListening"), { options });
        const QJsonObject current = voiceStatus();
        const QJsonValue problem = current.value(QStringLiteral("problem"));
        if (problem.isObject()) {
            refuse(problem[QStringLiteral("error_code")].toString(), problem[QStringLiteral("message")].toString());
            return QString();
        }
        if (!m_turn.isEmpty() || !m_session.isEmpty()) {
            refuse(QStringLiteral("TUR-AI-SERVICE-004"), QStringLiteral("the microphone is already open"));
            return QString();
        }
        const QString name = m_scenario.value(QStringLiteral("voice_turn")).toString(QStringLiteral("show"));
        m_turnFile = readObject(m_voiceDir + QStringLiteral("/turns/") + name + QStringLiteral(".json"));
        const QJsonArray steps = m_turnFile.value(QStringLiteral("signals")).toArray();
        int stop = -1;
        for (qsizetype i = 0; i < steps.size(); ++i) {
            if (steps.at(i).toObject().value(QStringLiteral("after_stop")).toBool()) {
                stop = int(i);
                break;
            }
        }
        m_turn = m_turnFile.value(QStringLiteral("turn")).toString();
        m_waitingTurn.clear();
        play(steps, stop);
        return m_turn;
    }

    void StopListening(const QString &turn, bool send)
    {
        note(QStringLiteral("StopListening"), { turn, send });
        if (m_turn.isEmpty() || turn != m_turn) {
            refuse(QStringLiteral("TUR-AI-SERVICE-007"), QStringLiteral("no turn with that identifier is listening"));
            return;
        }
        if (!send) {
            dropTurn(QStringLiteral("discarded"));
            return;
        }
        release();
    }

    QString SendTranscript(const QString &turn, const QString &text)
    {
        note(QStringLiteral("SendTranscript"), { turn, text });
        if (m_waitingTurn.isEmpty() || turn != m_waitingTurn) {
            refuse(QStringLiteral("TUR-AI-SERVICE-007"), QStringLiteral("no transcript with that identifier is waiting"));
            return QString();
        }
        if (!m_running.isEmpty()) {
            refuse(QStringLiteral("TUR-AI-SERVICE-004"), QStringLiteral("a task is running"));
            return QString();
        }
        const QString named = m_turnFile.value(QStringLiteral("then_task")).toString();
        const QJsonArray recorded = readObject(m_fixture + QStringLiteral("/tasks/") + named + QStringLiteral(".json"))
                                        .value(QStringLiteral("signals"))
                                        .toArray();
        if (recorded.isEmpty()) {
            refuseUnrecorded(QStringLiteral("SendTranscript ") + text);
            return QString();
        }
        m_waitingTurn.clear();
        QString id;
        for (const QJsonValue &signal : recorded) {
            const QJsonObject object = signal.toObject();
            if (object.value(QStringLiteral("member")).toString() == QLatin1String("TaskStarted")) {
                id = object.value(QStringLiteral("args")).toArray().at(0).toString();
                break;
            }
        }
        play(recorded, -1);
        return id;
    }

    void Speak(const QString &text) { note(QStringLiteral("Speak"), { text }); }
    void StopSpeaking() { note(QStringLiteral("StopSpeaking"), {}); }

    QString StartLiveSession(const QString &options)
    {
        note(QStringLiteral("StartLiveSession"), { options });
        const QJsonObject current = voiceStatus();
        const QJsonValue problem = current.value(QStringLiteral("problem"));
        if (problem.isObject()) {
            refuse(problem[QStringLiteral("error_code")].toString(), problem[QStringLiteral("message")].toString());
            return QString();
        }
        if (!m_session.isEmpty() || !m_turn.isEmpty()) {
            refuse(QStringLiteral("TUR-AI-SERVICE-004"), QStringLiteral("the microphone is already open"));
            return QString();
        }
        const QJsonObject live = readObject(m_voiceDir + QStringLiteral("/live.json"));
        const QJsonArray steps = live.value(QStringLiteral("signals")).toArray();
        int end = -1;
        for (qsizetype i = 0; i < steps.size(); ++i) {
            if (steps.at(i).toObject().value(QStringLiteral("after_end")).toBool()) {
                end = int(i);
                break;
            }
        }
        m_session = live.value(QStringLiteral("session")).toString();
        play(steps, end);
        return m_session;
    }

    void EndLiveSession()
    {
        note(QStringLiteral("EndLiveSession"), {});
        if (m_session.isEmpty()) {
            refuse(QStringLiteral("TUR-AI-SERVICE-007"), QStringLiteral("no live session is running"));
            return;
        }
        release();
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
    void VoiceStateChanged(const QString &state, const QString &details);
    void VoiceLevel(const QString &turn, double level);
    void VoiceTranscript(const QString &turn, const QString &details);
    void VoiceProgress(const QString &model, const QString &event);
    void LiveEvent(const QString &session, const QString &event);

private:
    static QString compact(const QJsonObject &object)
    {
        return QString::fromUtf8(QJsonDocument(object).toJson(QJsonDocument::Compact));
    }

    // What the named speech models download, from the status: every model,
    // detector, engine and voice it lists, by its id.
    static qint64 bytesOf(const QJsonObject &status, const QJsonArray &ids)
    {
        QJsonArray listed;
        listed.append(status.value(QStringLiteral("recognition")));
        listed.append(status.value(QStringLiteral("voice_activity")));
        for (const QString &key : { QStringLiteral("alternatives"), QStringLiteral("engines") }) {
            for (const QJsonValue &item : status.value(key).toArray())
                listed.append(item);
        }
        for (const QJsonValue &item : status.value(QStringLiteral("synthesis")).toObject().value(QStringLiteral("voices")).toArray())
            listed.append(item);
        qint64 total = 0;
        for (const QJsonValue &wanted : ids) {
            for (const QJsonValue &item : listed) {
                const QJsonObject model = item.toObject();
                const QString id = model.value(QStringLiteral("id")).toString(
                    model.value(QStringLiteral("model")).toString(model.value(QStringLiteral("voice")).toString()));
                if (id.isEmpty() || id != wanted.toString())
                    continue;
                const QJsonValue installed = model.value(QStringLiteral("installed"));
                if (installed.toBool() || installed.toString() == QLatin1String("installed"))
                    break;
                total += qint64(model.value(QStringLiteral("download_bytes")).toDouble());
                break;
            }
        }
        return total;
    }

    // The voice status of the scenario, with what is playing now.
    QJsonObject voiceStatus() const
    {
        QJsonObject document = readObject(m_voiceDir + QStringLiteral("/status/") + m_voiceName + QStringLiteral(".json"))
                                   .value(QStringLiteral("status"))
                                   .toObject();
        if (!m_voiceState.isEmpty()) {
            document.insert(QStringLiteral("state"), m_voiceState);
            document.insert(QStringLiteral("microphone_open"), m_microphone);
        }
        document.insert(QStringLiteral("turn"), m_turn.isEmpty() ? QJsonValue(QJsonValue::Null) : QJsonValue(m_turn));
        document.insert(QStringLiteral("session"),
                        m_session.isEmpty() ? QJsonValue(QJsonValue::Null) : QJsonValue(m_session));
        return document;
    }

    // The microphone closes and what was heard is dropped.
    void dropTurn(const QString &reason)
    {
        m_timer.stop();
        m_sequence = QJsonArray();
        m_holdAt = -1;
        const QString turn = m_turn;
        m_turn.clear();
        m_voiceState = QStringLiteral("ready");
        m_microphone = false;
        Q_EMIT VoiceStateChanged(m_voiceState, compact(QJsonObject { { QStringLiteral("state"), m_voiceState },
                                                                     { QStringLiteral("microphone_open"), false },
                                                                     { QStringLiteral("turn"), turn },
                                                                     { QStringLiteral("reason"), reason } }));
    }

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
        } else if (member == QLatin1String("VoiceStateChanged")) {
            const QJsonObject details = arguments.at(1).toObject();
            m_voiceState = text(0);
            m_microphone = details.value(QStringLiteral("microphone_open")).toBool();
            if (m_voiceState == QLatin1String("waiting")) {
                m_waitingTurn = m_turn;
                m_turn.clear();
            } else if (m_voiceState == QLatin1String("ready") || m_voiceState == QLatin1String("off")) {
                if (details.contains(QStringLiteral("session")))
                    m_session.clear();
                m_turn.clear();
            }
            Q_EMIT VoiceStateChanged(text(0), text(1));
        } else if (member == QLatin1String("VoiceLevel")) {
            Q_EMIT VoiceLevel(text(0), arguments.at(1).toDouble());
        } else if (member == QLatin1String("VoiceTranscript")) {
            Q_EMIT VoiceTranscript(text(0), text(1));
        } else if (member == QLatin1String("VoiceProgress")) {
            const QString event = arguments.at(1).toObject().value(QStringLiteral("event")).toString();
            if (event == QLatin1String("ready")) {
                m_preparing = false;
                m_voiceName = m_scenario.value(QStringLiteral("after_voice_prepare")).toString(m_voiceName);
            } else if (event == QLatin1String("failed") || event == QLatin1String("cancelled")) {
                m_preparing = false;
            }
            Q_EMIT VoiceProgress(text(0), text(1));
        } else if (member == QLatin1String("LiveEvent")) {
            if (arguments.at(1).toObject().value(QStringLiteral("kind")).toString() == QLatin1String("ended"))
                m_session.clear();
            Q_EMIT LiveEvent(text(0), text(1));
        } else if (member == QLatin1String("PlayTask")) {
            // A spoken request becomes the recorded task of that name: its
            // signals are played next, in place.
            const QJsonArray recorded = readObject(m_fixture + QStringLiteral("/tasks/") + text(0) + QStringLiteral(".json"))
                                            .value(QStringLiteral("signals"))
                                            .toArray();
            for (qsizetype i = 0; i < recorded.size(); ++i)
                m_sequence.insert(m_position + i, recorded.at(i));
            if (m_holdAt >= m_position)
                m_holdAt += int(recorded.size());
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
    QString m_voiceDir;
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

    // Voice.
    QString m_voiceName = QStringLiteral("off");
    QString m_voiceState;
    bool m_microphone = false;
    QString m_turn;
    QString m_waitingTurn;
    QJsonObject m_turnFile;
    QString m_session;
    bool m_preparing = false;
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
