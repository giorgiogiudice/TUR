// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
//
// The conversation (Conversation), fed what the real org.tur.Agent1 signalled
// in CI, signal by signal, in the order it signalled it
// (fixtures/agent1/tasks/, recorded by desktop/tests/pax-ui/e2e.py). No bus:
// the model is driven directly, the way AgentClient drives it.

#include "conversation.h"

#include <QFile>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QSignalSpy>
#include <QStringList>
#include <QTest>
#include <QVariantList>

namespace {

const QString fixtures = QStringLiteral(TUR_PAX_UI_FIXTURES);

QJsonObject task(const QString &name)
{
    QFile file(fixtures + QStringLiteral("/tasks/") + name + QStringLiteral(".json"));
    if (!file.open(QIODevice::ReadOnly))
        qFatal("the recorded task %s cannot be read", qPrintable(name));
    return QJsonDocument::fromJson(file.readAll()).object();
}

// Feeds `count` recorded signals of a task (all of them if negative) into
// the conversation. Returns how many it fed.
int feed(Conversation &conversation, const QJsonObject &recorded, int count = -1)
{
    const QJsonArray sequence = recorded.value(QStringLiteral("signals")).toArray();
    int fed = 0;
    for (const QJsonValue &value : sequence) {
        if (count >= 0 && fed >= count)
            break;
        const QJsonObject signal = value.toObject();
        const QString member = signal.value(QStringLiteral("member")).toString();
        const QJsonArray arguments = signal.value(QStringLiteral("args")).toArray();
        const QString task = arguments.at(0).toString();
        if (member == QLatin1String("TaskStarted"))
            conversation.taskStarted(task, arguments.at(1).toVariant());
        else if (member == QLatin1String("TextAdded"))
            conversation.textAdded(task, arguments.at(1).toString(), arguments.at(2).toString());
        else if (member == QLatin1String("ActionProposed"))
            conversation.actionProposed(task, arguments.at(1).toVariant());
        else if (member == QLatin1String("ActionCompleted"))
            conversation.actionCompleted(task, arguments.at(1).toVariant());
        else if (member == QLatin1String("TaskFinished"))
            conversation.taskFinished(task, arguments.at(1).toVariant());
        ++fed;
    }
    return fed;
}

// The position, in the recorded signals, of the first `member`.
int indexOf(const QJsonObject &recorded, const QString &member)
{
    const QJsonArray sequence = recorded.value(QStringLiteral("signals")).toArray();
    for (qsizetype i = 0; i < sequence.size(); ++i) {
        if (sequence.at(i).toObject().value(QStringLiteral("member")).toString() == member)
            return int(i);
    }
    return -1;
}

QStringList kinds(const Conversation &conversation)
{
    QStringList out;
    for (int row = 0; row < conversation.count(); ++row)
        out.append(conversation.entry(row).value(QStringLiteral("kind")).toString());
    return out;
}

} // namespace

class ConversationTest : public QObject
{
    Q_OBJECT

private Q_SLOTS:
    // A task that completes: the person's words, P@x's words between the
    // steps, each step once (proposed, then the same line ended), the answer
    // and the end, in the order the service said them.
    void a_task_streams_in_the_order_the_service_signals_it()
    {
        const QJsonObject recorded = task(QStringLiteral("two-tools"));
        Conversation conversation;
        QSignalSpy added(&conversation, &Conversation::countChanged);
        conversation.requested(recorded.value(QStringLiteral("request")).toString());
        QVERIFY(conversation.starting());
        QCOMPARE(conversation.entry(0).value(QStringLiteral("phase")).toString(), QStringLiteral("pending"));

        // Up to the first step proposed: it is shown running, as it runs.
        const int firstStep = indexOf(recorded, QStringLiteral("ActionProposed"));
        QVERIFY(firstStep > 0);
        feed(conversation, recorded, firstStep + 1);
        QVERIFY(conversation.running());
        QVERIFY(!conversation.starting());
        const QVariantList steps = conversation.entriesOf(QStringLiteral("step"));
        QCOMPARE(steps.size(), 1);
        QCOMPARE(steps.first().toMap().value(QStringLiteral("phase")).toString(), QStringLiteral("running"));
        QCOMPARE(conversation.entry(0).value(QStringLiteral("phase")).toString(), QStringLiteral("started"));
        QVERIFY(conversation.entry(0).value(QStringLiteral("time")).toLongLong() > 0);

        // The rest.
        Conversation whole;
        whole.requested(recorded.value(QStringLiteral("request")).toString());
        feed(whole, recorded);
        QCOMPARE(kinds(whole), QStringList({ QStringLiteral("you"), QStringLiteral("pax"), QStringLiteral("step"),
                                             QStringLiteral("step"), QStringLiteral("pax"), QStringLiteral("summary") }));
        QCOMPARE(whole.entry(1).value(QStringLiteral("phase")).toString(), QStringLiteral("interim"));
        QCOMPARE(whole.entry(4).value(QStringLiteral("phase")).toString(), QStringLiteral("answer"));
        for (const QVariant &step : whole.entriesOf(QStringLiteral("step")))
            QCOMPARE(step.toMap().value(QStringLiteral("phase")).toString(), QStringLiteral("done"));
        const QVariantMap detail = whole.entriesOf(QStringLiteral("summary")).first().toMap().value(QStringLiteral("detail")).toMap();
        QCOMPARE(detail.value(QStringLiteral("done")).toInt(), 2);
        QCOMPARE(detail.value(QStringLiteral("refused")).toInt(), 0);
        QVERIFY(detail.value(QStringLiteral("tokens")).toLongLong() > 0);
        QVERIFY(!whole.running());
        QCOMPARE(whole.lastOutcome(), QStringLiteral("completed"));
        QVERIFY(added.count() >= 3);
    }

    // A step the policy refuses is a step, marked refused, with the
    // service's sentence and its code; it is counted as refused.
    void a_refused_step_carries_the_reason_and_the_code()
    {
        const QJsonObject recorded = task(QStringLiteral("refused"));
        Conversation conversation;
        conversation.requested(recorded.value(QStringLiteral("request")).toString());
        feed(conversation, recorded);
        QVariantMap refused;
        for (const QVariant &step : conversation.entriesOf(QStringLiteral("step"))) {
            if (step.toMap().value(QStringLiteral("phase")).toString() == QLatin1String("refused"))
                refused = step.toMap();
        }
        QVERIFY2(!refused.isEmpty(), "the recorded task has a refused step");
        const QVariantMap step = refused.value(QStringLiteral("step")).toMap();
        QCOMPARE(step.value(QStringLiteral("decision")).toString(), QStringLiteral("refused"));
        QVERIFY(!step.value(QStringLiteral("message")).toString().isEmpty());
        QVERIFY(refused.value(QStringLiteral("code")).toString().startsWith(QLatin1String("TUR-AI-POLICY-")));
        QVERIFY(step.value(QStringLiteral("class")).toInt() >= 2);
        const QVariantMap detail = conversation.entriesOf(QStringLiteral("summary")).first().toMap().value(QStringLiteral("detail")).toMap();
        QCOMPARE(detail.value(QStringLiteral("refused")).toInt(), 1);
    }

    // Stopped while a step runs: the step is shown as stopped, the system
    // says the task stopped as asked, with the service's code, and nothing
    // runs any more.
    void a_task_stopped_mid_step_says_so()
    {
        const QJsonObject recorded = task(QStringLiteral("stop"));
        Conversation conversation;
        conversation.requested(recorded.value(QStringLiteral("request")).toString());
        const int finish = indexOf(recorded, QStringLiteral("TaskFinished"));
        QVERIFY(finish > 0);
        feed(conversation, recorded, finish);
        QVERIFY(conversation.running());
        QCOMPARE(conversation.entriesOf(QStringLiteral("step")).first().toMap().value(QStringLiteral("phase")).toString(),
                 QStringLiteral("running"));
        feed(conversation, recorded);
        QVERIFY(!conversation.running());
        QCOMPARE(conversation.lastOutcome(), QStringLiteral("cancelled"));
        QCOMPARE(conversation.entriesOf(QStringLiteral("step")).first().toMap().value(QStringLiteral("phase")).toString(),
                 QStringLiteral("stopped"));
        const QVariantList cards = conversation.entriesOf(QStringLiteral("system"));
        QCOMPARE(cards.size(), 1);
        QCOMPARE(cards.first().toMap().value(QStringLiteral("phase")).toString(), QStringLiteral("cancelled"));
        QCOMPARE(cards.first().toMap().value(QStringLiteral("code")).toString(), QStringLiteral("TUR-AI-TASK-003"));
        QCOMPARE(conversation.entriesOf(QStringLiteral("summary")).size(), 0);
    }

    // The day's limit: the service says it as a notice and ends the task
    // with the code; the panel says it once, as the system, with the code.
    void a_spending_limit_is_said_once_by_the_system()
    {
        const QJsonObject recorded = task(QStringLiteral("spend-limit"));
        Conversation conversation;
        conversation.requested(recorded.value(QStringLiteral("request")).toString());
        feed(conversation, recorded);
        const QVariantList cards = conversation.entriesOf(QStringLiteral("system"));
        QCOMPARE(cards.size(), 1);
        const QVariantMap card = cards.first().toMap();
        QCOMPARE(card.value(QStringLiteral("phase")).toString(), QStringLiteral("spend_limit"));
        QCOMPARE(card.value(QStringLiteral("code")).toString(), QStringLiteral("TUR-AI-SPEND-001"));
        QVERIFY(card.value(QStringLiteral("text")).toString().contains(QLatin1String("limit")));
        QCOMPARE(conversation.lastOutcome(), QStringLiteral("spend_limit"));
        // No P@x words claim it: the notice is not P@x's.
        for (const QVariant &words : conversation.entriesOf(QStringLiteral("pax")))
            QVERIFY(!words.toMap().value(QStringLiteral("text")).toString().contains(QLatin1String("limit")));
    }

    // A request the service refuses before it starts: the person's words are
    // marked, and the system says why, with the code.
    void a_refusal_before_the_start_is_a_system_card()
    {
        Conversation conversation;
        conversation.requested(QStringLiteral("Do something"));
        conversation.startRefused(QStringLiteral("TUR-AI-SERVICE-001"),
                                  QStringLiteral("P@x is switched off. Switch it on in its settings first."));
        QCOMPARE(kinds(conversation), QStringList({ QStringLiteral("you"), QStringLiteral("system") }));
        QCOMPARE(conversation.entry(0).value(QStringLiteral("phase")).toString(), QStringLiteral("refused"));
        QCOMPARE(conversation.entry(1).value(QStringLiteral("code")).toString(), QStringLiteral("TUR-AI-SERVICE-001"));
        QVERIFY(!conversation.starting());
        QVERIFY(!conversation.running());
    }

    // A task started by something other than this window is shown as such;
    // a scheduled one as the system's.
    void a_task_asked_elsewhere_is_marked()
    {
        const QJsonObject recorded = task(QStringLiteral("two-tools"));
        Conversation conversation;
        feed(conversation, recorded, 1);
        QCOMPARE(conversation.entry(0).value(QStringLiteral("kind")).toString(), QStringLiteral("you"));
        QCOMPARE(conversation.entry(0).value(QStringLiteral("phase")).toString(), QStringLiteral("elsewhere"));

        Conversation scheduled;
        QJsonObject started = recorded.value(QStringLiteral("signals")).toArray().at(0).toObject();
        QJsonObject details = started.value(QStringLiteral("args")).toArray().at(1).toObject();
        details.insert(QStringLiteral("attended"), false);
        scheduled.taskStarted(QStringLiteral("t-1"), details.toVariantMap());
        QCOMPARE(scheduled.entry(0).value(QStringLiteral("kind")).toString(), QStringLiteral("system"));
        QCOMPARE(scheduled.entry(0).value(QStringLiteral("phase")).toString(), QStringLiteral("scheduled"));
    }

    // A spoken request that a task follows ("show") is the person's words,
    // marked spoken, and the task's TaskStarted finds it as it finds a typed
    // one. One that waits ("wait") is not a row until it is sent; nothing
    // heard is never a row. (Voice documents: fixtures/voice/, written by
    // hand from docs/architecture/pax-voice.md, 6.2.)
    void a_spoken_request_is_the_persons_words_marked_spoken()
    {
        const QJsonObject recorded = task(QStringLiteral("two-tools"));
        const QString request = recorded.value(QStringLiteral("request")).toString();
        Conversation conversation;
        conversation.voiceTranscript(QStringLiteral("v-1"),
                                     QVariantMap { { QStringLiteral("text"), request },
                                                   { QStringLiteral("final"), true },
                                                   { QStringLiteral("check"), QStringLiteral("show") },
                                                   { QStringLiteral("empty"), false } });
        QCOMPARE(kinds(conversation), QStringList({ QStringLiteral("you") }));
        QVERIFY(conversation.entry(0).value(QStringLiteral("spoken")).toBool());
        QCOMPARE(conversation.entry(0).value(QStringLiteral("phase")).toString(), QStringLiteral("pending"));
        QVERIFY(conversation.starting());
        feed(conversation, recorded);
        QCOMPARE(conversation.entry(0).value(QStringLiteral("phase")).toString(), QStringLiteral("started"));
        QVERIFY(conversation.entry(0).value(QStringLiteral("spoken")).toBool());
        QCOMPARE(conversation.entriesOf(QStringLiteral("you")).size(), 1);
        QCOMPARE(conversation.lastOutcome(), QStringLiteral("completed"));

        Conversation waiting;
        waiting.voiceTranscript(QStringLiteral("v-2"),
                                QVariantMap { { QStringLiteral("text"), request },
                                              { QStringLiteral("final"), true },
                                              { QStringLiteral("check"), QStringLiteral("wait") },
                                              { QStringLiteral("reason"), QStringLiteral("other_sound") } });
        QCOMPARE(waiting.count(), 0);
        QVERIFY(!waiting.starting());

        Conversation nothing;
        nothing.voiceTranscript(QStringLiteral("v-3"),
                                QVariantMap { { QStringLiteral("text"), QString() },
                                              { QStringLiteral("final"), true },
                                              { QStringLiteral("check"), QStringLiteral("show") },
                                              { QStringLiteral("empty"), true } });
        QCOMPARE(nothing.count(), 0);
    }

    // A live conversation: started, the person's words and P@x's as they
    // grow (one row each until final), P@x interrupted, ended; the session
    // is known while it runs and gone when it ends.
    void a_live_conversation_is_shown_as_it_happens()
    {
        Conversation conversation;
        QSignalSpy live(&conversation, &Conversation::liveChanged);
        const QString session = QStringLiteral("s-1");
        const auto event = [&conversation, &session](const QVariantMap &said) {
            conversation.liveEvent(session, said);
        };
        event({ { QStringLiteral("kind"), QStringLiteral("started") }, { QStringLiteral("name"), QStringLiteral("On this computer") } });
        QVERIFY(conversation.live());
        QCOMPARE(conversation.liveSession(), session);
        event({ { QStringLiteral("kind"), QStringLiteral("heard") }, { QStringLiteral("text"), QStringLiteral("What is the") },
                { QStringLiteral("final"), false } });
        event({ { QStringLiteral("kind"), QStringLiteral("heard") }, { QStringLiteral("text"), QStringLiteral("What is the value?") },
                { QStringLiteral("final"), true } });
        event({ { QStringLiteral("kind"), QStringLiteral("said") }, { QStringLiteral("text"), QStringLiteral("The value") },
                { QStringLiteral("final"), false } });
        event({ { QStringLiteral("kind"), QStringLiteral("said") }, { QStringLiteral("text"), QStringLiteral("The value is 21.") },
                { QStringLiteral("final"), true } });
        event({ { QStringLiteral("kind"), QStringLiteral("interrupted") } });
        event({ { QStringLiteral("kind"), QStringLiteral("ended") }, { QStringLiteral("reason"), QStringLiteral("person") },
                { QStringLiteral("message"), QStringLiteral("The conversation ended.") } });

        QCOMPARE(kinds(conversation), QStringList({ QStringLiteral("system"), QStringLiteral("you"), QStringLiteral("pax"),
                                                    QStringLiteral("system") }));
        const QVariantMap heard = conversation.entry(1);
        QCOMPARE(heard.value(QStringLiteral("text")).toString(), QStringLiteral("What is the value?"));
        QCOMPARE(heard.value(QStringLiteral("phase")).toString(), QStringLiteral("heard"));
        QVERIFY(heard.value(QStringLiteral("spoken")).toBool());
        const QVariantMap said = conversation.entry(2);
        QCOMPARE(said.value(QStringLiteral("text")).toString(), QStringLiteral("The value is 21."));
        QCOMPARE(said.value(QStringLiteral("phase")).toString(), QStringLiteral("said"));
        QVERIFY(said.value(QStringLiteral("detail")).toMap().value(QStringLiteral("interrupted")).toBool());
        QCOMPARE(conversation.entry(0).value(QStringLiteral("phase")).toString(), QStringLiteral("live-started"));
        QCOMPARE(conversation.entry(3).value(QStringLiteral("phase")).toString(), QStringLiteral("live-ended"));
        QVERIFY(!conversation.live());
        QVERIFY(live.count() >= 2);
    }

    // What was heard in a live conversation becomes a task's request when
    // the service starts one with it: the row is found, not repeated.
    void words_heard_live_become_the_request_of_a_task()
    {
        const QJsonObject recorded = task(QStringLiteral("two-tools"));
        Conversation conversation;
        conversation.liveEvent(QStringLiteral("s-1"), QVariantMap { { QStringLiteral("kind"), QStringLiteral("started") } });
        conversation.liveEvent(QStringLiteral("s-1"),
                               QVariantMap { { QStringLiteral("kind"), QStringLiteral("heard") },
                                             { QStringLiteral("text"), recorded.value(QStringLiteral("request")).toString() },
                                             { QStringLiteral("final"), true } });
        feed(conversation, recorded);
        QCOMPARE(conversation.entriesOf(QStringLiteral("you")).size(), 1);
        const QVariantMap you = conversation.entriesOf(QStringLiteral("you")).first().toMap();
        QCOMPARE(you.value(QStringLiteral("phase")).toString(), QStringLiteral("started"));
        QVERIFY(you.value(QStringLiteral("spoken")).toBool());
    }
};

QTEST_GUILESS_MAIN(ConversationTest)

#include "tst_conversation.moc"
