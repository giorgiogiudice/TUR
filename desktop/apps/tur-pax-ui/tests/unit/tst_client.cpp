// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
//
// AgentClient, the program's side of org.tur.Agent1, and the program's stop
// key, on a session bus of the test's own with the stand-in for the service
// (fixture/agent.cpp) answering from the real service's recorded replies.
//
//  - The client's table of methods is the committed interface file's
//    (services/tur-pax/interfaces/org.tur.Agent1.xml), method for method and
//    type for type.
//  - Present, absent; JSON replies parsed; a refusal as its code and the
//    service's sentence; the service's signals passed on, parsed.
//  - A key given to SetKey reaches the service and stays nowhere in the
//    client.
//  - `tur-pax-ui --stop` stops a running task, and never starts P@x's
//    service to do it.

#include "agentclient.h"
#include "conversation.h"
#include "fixturebus.h"

#include <QDir>
#include <QFile>
#include <QJsonDocument>
#include <QMap>
#include <QMetaProperty>
#include <QProcess>
#include <QProcessEnvironment>
#include <QSignalSpy>
#include <QTest>
#include <QXmlStreamReader>

#include <memory>

namespace {

const QString program = QStringLiteral(TUR_PAX_UI_PROGRAM);
const QString interfaceFile = QStringLiteral(TUR_AGENT1_INTERFACE);
// Not a real key: the service is a stand-in.
const QString testKey = QStringLiteral("sk-test-tur-pax-ui-0123456789");

// The methods of org.tur.Agent1 in the committed file, with the types of
// their arguments.
QMap<QString, QStringList> committedMethods()
{
    QFile file(interfaceFile);
    if (!file.open(QIODevice::ReadOnly))
        qFatal("%s cannot be read", qPrintable(interfaceFile));
    QMap<QString, QStringList> out;
    QXmlStreamReader reader(&file);
    QString interface;
    QString method;
    while (!reader.atEnd()) {
        reader.readNext();
        const QString name = reader.name().toString();
        if (reader.isStartElement()) {
            const QXmlStreamAttributes attributes = reader.attributes();
            if (name == QLatin1String("interface")) {
                interface = attributes.value(QLatin1String("name")).toString();
            } else if (name == QLatin1String("method") && interface == QLatin1String("org.tur.Agent1")) {
                method = attributes.value(QLatin1String("name")).toString();
                out.insert(method, {});
            } else if (name == QLatin1String("arg") && !method.isEmpty()
                       && attributes.value(QLatin1String("direction")) == QLatin1String("in")) {
                out[method].append(attributes.value(QLatin1String("type")).toString());
            }
        } else if (reader.isEndElement() && name == QLatin1String("method")) {
            method.clear();
        }
    }
    return out;
}

// Calls and waits for the answer.
QVariantMap callAndWait(AgentClient &client, const QString &method, const QVariantList &arguments)
{
    QVariantMap answer;
    bool answered = false;
    client.callWith(method, arguments, [&answer, &answered](const QVariantMap &reply) {
        answer = reply;
        answered = true;
    });
    if (!QTest::qWaitFor([&answered] { return answered; }, 20000))
        qWarning("no answer to %s", qPrintable(method));
    return answer;
}

} // namespace

class ClientTest : public QObject
{
    Q_OBJECT

private:
    std::unique_ptr<FixtureBus> m_bus;

    std::unique_ptr<AgentClient> client()
    {
        auto made = std::make_unique<AgentClient>();
        made->setBusAddress(m_bus->address());
        made->start();
        return made;
    }

    int runProgram(const QStringList &arguments, QString *output)
    {
        QProcess process;
        QProcessEnvironment environment = QProcessEnvironment::systemEnvironment();
        environment.insert(QStringLiteral("DBUS_SESSION_BUS_ADDRESS"), m_bus->address());
        process.setProcessEnvironment(environment);
        process.setProcessChannelMode(QProcess::MergedChannels);
        process.start(program, arguments);
        if (!process.waitForFinished(30000)) {
            process.kill();
            return -1;
        }
        *output = QString::fromUtf8(process.readAll());
        return process.exitCode();
    }

private Q_SLOTS:
    void initTestCase()
    {
        m_bus = std::make_unique<FixtureBus>(QStringLiteral(TUR_PAX_UI_AGENT), QStringLiteral(TUR_PAX_UI_FIXTURES));
        QString problem;
        QVERIFY2(m_bus->start(&problem), qPrintable(problem));
    }

    void cleanupTestCase()
    {
        m_bus.reset();
    }

    void init()
    {
        QVERIFY(m_bus->startAgent());
        QVERIFY(m_bus->use({ { QStringLiteral("status"), QStringLiteral("off") } }));
    }

    void the_table_of_methods_is_the_committed_interface()
    {
        const QMap<QString, QStringList> committed = committedMethods();
        QVERIFY(committed.size() >= 30);
        for (auto it = committed.constBegin(); it != committed.constEnd(); ++it) {
            const std::optional<QStringList> types = AgentClient::argumentTypes(it.key());
            QVERIFY2(types.has_value(), qPrintable(it.key() + QStringLiteral(" is missing from the client's table")));
            QCOMPARE(*types, it.value());
        }
        QVERIFY(!AgentClient::argumentTypes(QStringLiteral("BecomeRoot")).has_value());
    }

    void errors_become_codes_and_sentences()
    {
        QCOMPARE(AgentClient::codeOfError(QStringLiteral("org.tur.Agent1.Error.TUR_AI_SERVICE_002")),
                 QStringLiteral("TUR-AI-SERVICE-002"));
        QCOMPARE(AgentClient::codeOfError(QStringLiteral("org.freedesktop.DBus.Error.ServiceUnknown")),
                 QStringLiteral("org.freedesktop.DBus.Error.ServiceUnknown"));
        QCOMPARE(AgentClient::sentenceOfError(QStringLiteral("TUR-AI-KEY-004: the text is empty")),
                 QStringLiteral("the text is empty"));
        QCOMPARE(AgentClient::parseJson(QStringLiteral("t-1-2")).toString(), QStringLiteral("t-1-2"));
        QCOMPARE(AgentClient::parseJson(QStringLiteral("{\"a\":1}")).toMap().value(QStringLiteral("a")).toInt(), 1);
    }

    void the_service_is_found_and_its_status_read()
    {
        auto agent = client();
        QCOMPARE(agent->presence(), AgentClient::Present);
        QTRY_VERIFY_WITH_TIMEOUT(agent->statusKnown(), 10000);
        QCOMPARE(agent->serviceState(), QStringLiteral("off"));
        QCOMPARE(agent->status().value(QStringLiteral("enabled")).toBool(), false);
        QCOMPARE(agent->status().value(QStringLiteral("interface_version")).toInt(), 1);
    }

    void replies_are_parsed_and_refusals_coded()
    {
        auto agent = client();
        const QVariantMap consent = callAndWait(*agent, QStringLiteral("GetConsentText"), { QStringLiteral("anthropic") });
        QVERIFY(consent.value(QStringLiteral("ok")).toBool());
        const QVariantList values = consent.value(QStringLiteral("values")).toList();
        QCOMPARE(values.size(), 2);
        QVERIFY(values.at(1).toString().startsWith(QLatin1String("What P@x sends to Anthropic")));

        const QVariantMap models = callAndWait(*agent, QStringLiteral("ListModels"), {});
        QVERIFY(models.value(QStringLiteral("ok")).toBool());
        QVERIFY(!models.value(QStringLiteral("value")).toMap().value(QStringLiteral("providers")).toList().isEmpty());

        const QVariantMap refused = callAndWait(*agent, QStringLiteral("Enable"), { QStringLiteral("not-the-version") });
        QVERIFY(!refused.value(QStringLiteral("ok")).toBool());
        QCOMPARE(refused.value(QStringLiteral("code")).toString(), QStringLiteral("TUR-AI-SERVICE-002"));
        QVERIFY(!refused.value(QStringLiteral("message")).toString().startsWith(QLatin1String("TUR-")));

        const QVariantMap wrong = callAndWait(*agent, QStringLiteral("Enable"), {});
        QCOMPARE(wrong.value(QStringLiteral("code")).toString(), QStringLiteral("org.freedesktop.DBus.Error.InvalidArgs"));
        const QVariantMap unknown = callAndWait(*agent, QStringLiteral("BecomeRoot"), {});
        QCOMPARE(unknown.value(QStringLiteral("code")).toString(), QStringLiteral("org.freedesktop.DBus.Error.UnknownMethod"));
    }

    void signals_arrive_parsed_and_drive_the_conversation()
    {
        QVERIFY(m_bus->use({ { QStringLiteral("status"), QStringLiteral("ready") },
                             { QStringLiteral("task"), QStringLiteral("two-tools") },
                             { QStringLiteral("pace_ms"), 20 } }));
        auto agent = client();
        QTRY_VERIFY_WITH_TIMEOUT(agent->statusKnown(), 10000);
        Conversation conversation;
        conversation.setClient(agent.get());
        QSignalSpy started(agent.get(), &AgentClient::taskStarted);
        QSignalSpy finished(agent.get(), &AgentClient::taskFinished);
        conversation.ask(QStringLiteral("Set the value for the night."));
        QTRY_COMPARE_WITH_TIMEOUT(finished.count(), 1, 20000);
        QCOMPARE(started.count(), 1);
        QCOMPARE(started.first().at(1).toMap().value(QStringLiteral("request")).toString(),
                 QStringLiteral("Set the value for the night."));
        QCOMPARE(finished.first().at(1).toMap().value(QStringLiteral("outcome")).toString(), QStringLiteral("completed"));
        QCOMPARE(conversation.lastOutcome(), QStringLiteral("completed"));
        QCOMPARE(conversation.entriesOf(QStringLiteral("step")).size(), 2);
        QVERIFY(m_bus->callNames().contains(QStringLiteral("StartTask")));
    }

    void a_key_reaches_the_service_and_stays_nowhere_here()
    {
        auto agent = client();
        const QVariantMap stored = callAndWait(*agent, QStringLiteral("SetKey"), { QStringLiteral("anthropic"), testKey });
        QVERIFY(stored.value(QStringLiteral("ok")).toBool());
        QVERIFY(m_bus->keyMatches(testKey));
        // Nothing the client holds or offers has it.
        agent->refresh();
        QTest::qWait(300);
        const QMetaObject *meta = agent->metaObject();
        for (int i = 0; i < meta->propertyCount(); ++i) {
            const QVariant value = meta->property(i).read(agent.get());
            const QString text = QString::fromUtf8(QJsonDocument::fromVariant(value).toJson()) + value.toString();
            QVERIFY2(!text.contains(testKey), meta->property(i).name());
        }
        // And what the stand-in noted of the call is only its length.
        const QString noted = QString::fromUtf8(QJsonDocument::fromVariant(m_bus->calls()).toJson());
        QVERIFY(!noted.contains(testKey));
        QVERIFY(noted.contains(QLatin1String("key_length")));
    }

    void the_stop_key_stops_a_running_task()
    {
        QVERIFY(m_bus->use({ { QStringLiteral("status"), QStringLiteral("ready") },
                             { QStringLiteral("task"), QStringLiteral("stop") },
                             { QStringLiteral("pace_ms"), 20 } }));
        auto agent = client();
        QTRY_VERIFY_WITH_TIMEOUT(agent->statusKnown(), 10000);
        QSignalSpy proposed(agent.get(), &AgentClient::actionProposed);
        QSignalSpy finished(agent.get(), &AgentClient::taskFinished);
        const QVariantMap started = callAndWait(*agent, QStringLiteral("StartTask"),
                                                { QStringLiteral("Wait for a while."), QString() });
        QVERIFY2(started.value(QStringLiteral("ok")).toBool(), qPrintable(started.value(QStringLiteral("message")).toString()));
        QTRY_COMPARE_WITH_TIMEOUT(proposed.count(), 1, 10000);
        QTest::qWait(300);
        QCOMPARE(finished.count(), 0);

        QString output;
        QCOMPARE(runProgram({ QStringLiteral("--stop") }, &output), 0);
        QTRY_COMPARE_WITH_TIMEOUT(finished.count(), 1, 10000);
        QCOMPARE(finished.first().at(1).toMap().value(QStringLiteral("outcome")).toString(), QStringLiteral("cancelled"));
        QVERIFY(m_bus->callNames().contains(QStringLiteral("CancelTask")));
    }

    void the_stop_key_never_starts_the_service()
    {
        // The service can be started by the bus, as on the desktop.
        const QString file = m_bus->servicesDirectory() + QStringLiteral("/org.tur.Agent1.service");
        {
            QFile service(file);
            QVERIFY(service.open(QIODevice::WriteOnly));
            service.write(QStringLiteral("[D-BUS Service]\nName=org.tur.Agent1\nExec=%1 --fixture %2\n")
                              .arg(m_bus->agentProgram(), m_bus->fixtureDirectory())
                              .toUtf8());
        }
        m_bus->reloadServices();
        m_bus->stopAgent();
        QVERIFY(!m_bus->agentOwned());

        QString output;
        QCOMPARE(runProgram({ QStringLiteral("--stop") }, &output), 0);
        QVERIFY2(output.contains(QLatin1String("not running")), qPrintable(output));
        QTest::qWait(500);
        QVERIFY2(!m_bus->agentOwned(), "the stop key started P@x's service");

        // The window, which a person opened, may start it.
        auto agent = client();
        QCOMPARE(agent->presence(), AgentClient::Present);
        QTRY_VERIFY_WITH_TIMEOUT(agent->statusKnown(), 15000);
        QVERIFY(m_bus->agentOwned());

        // Started by the bus, not by the test: stop it and remove the file.
        agent.reset();
        QFile::remove(file);
        m_bus->reloadServices();
        m_bus->killOwner();
        QVERIFY(!m_bus->agentOwned());
    }

    void an_absent_service_is_said_to_be_absent()
    {
        m_bus->stopAgent();
        auto agent = client();
        QCOMPARE(agent->presence(), AgentClient::Absent);
        QVERIFY(!agent->presenceDetail().isEmpty());
        QVERIFY(!agent->statusKnown());
        const QVariantMap answer = callAndWait(*agent, QStringLiteral("GetStatus"), {});
        QVERIFY(!answer.value(QStringLiteral("ok")).toBool());
    }
};

QTEST_GUILESS_MAIN(ClientTest)

#include "tst_client.moc"
