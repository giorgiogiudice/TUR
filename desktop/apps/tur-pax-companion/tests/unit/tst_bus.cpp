// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
//
// P@x's character against what it listens to, on a private session bus
// (docs/architecture/tur-pax-companion.md, sections 6 and 10):
//
//  - org.tur.Agent1, answered by the stand-in P@x's window's tests use,
//    whose answers and signals are the real service's, recorded in CI
//    (desktop/apps/tur-pax-ui/fixtures/agent1): on and off, a task running
//    and ending, a refused step, the day's limit, a task the person stopped;
//  - that the character never starts P@x's service, although the bus could;
//  - org.tur.PaxUi1.Show, asked of a stand-in for P@x's window;
//  - a property of another program followed (P@x's indicator's Activity,
//    from a stand-in);
//  - org.tur.PaxCompanion1 as the running program offers it, against the
//    committed XML, with the healing service's Announce: taken only when
//    the bubble is said, refused at once when it cannot be.

#include "agentsource.h"
#include "busproperty.h"
#include "companion.h"
#include "companionservice.h"
#include "companionsettings.h"
#include "fixturebus.h"

#include <QDBusArgument>
#include <QDBusConnection>
#include <QDBusConnectionInterface>
#include <QDBusMessage>
#include <QDBusVariant>
#include <QFile>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QMap>
#include <QProcess>
#include <QProcessEnvironment>
#include <QSignalSpy>
#include <QStringList>
#include <QTemporaryDir>
#include <QTest>
#include <QXmlStreamReader>

#include <memory>

namespace {

// The members of each interface in introspection data: "method Name(in) ->
// (out)", "property Name type access". Argument names are not compared.
QMap<QString, QStringList> shapeOf(const QString &xml)
{
    QMap<QString, QStringList> out;
    QXmlStreamReader reader(xml);
    QString current;
    QString member;
    QString in;
    QString result;
    while (!reader.atEnd()) {
        const QXmlStreamReader::TokenType token = reader.readNext();
        const QString name = reader.name().toString();
        const QXmlStreamAttributes attributes = reader.attributes();
        if (token == QXmlStreamReader::StartElement) {
            if (name == QLatin1String("interface")) {
                current = attributes.value(QLatin1String("name")).toString();
            } else if (name == QLatin1String("method") || name == QLatin1String("signal")) {
                member = attributes.value(QLatin1String("name")).toString();
                in.clear();
                result.clear();
            } else if (name == QLatin1String("arg")) {
                const bool outgoing = attributes.value(QLatin1String("direction")) == QLatin1String("out");
                (outgoing ? result : in) += attributes.value(QLatin1String("type")).toString();
            } else if (name == QLatin1String("property")) {
                out[current].append(QStringLiteral("property %1 %2 %3")
                                        .arg(attributes.value(QLatin1String("name")).toString(),
                                             attributes.value(QLatin1String("type")).toString(),
                                             attributes.value(QLatin1String("access")).toString()));
            }
        } else if (token == QXmlStreamReader::EndElement) {
            if (name == QLatin1String("method"))
                out[current].append(QStringLiteral("method %1(%2) -> (%3)").arg(member, in, result));
            else if (name == QLatin1String("signal"))
                out[current].append(QStringLiteral("signal %1(%2)").arg(member, in + result));
        }
    }
    for (QStringList &members : out)
        members.sort();
    return out;
}

const QString companionName = QStringLiteral("org.tur.PaxCompanion1");
const QString companionPath = QStringLiteral("/org/tur/PaxCompanion1");

} // namespace

// A stand-in for P@x's window: it writes down what it was asked to show.
class WindowStandIn : public QObject
{
    Q_OBJECT
    Q_CLASSINFO("D-Bus Interface", "org.tur.PaxUi1")

public:
    QStringList shown;

public Q_SLOTS:
    void Show(const QString &mode) { shown.append(mode); }
};

// A stand-in for P@x's indicator: its Activity, and nothing else.
class IndicatorStandIn : public QObject
{
    Q_OBJECT
    Q_CLASSINFO("D-Bus Interface", "org.tur.PaxOverlay1")
    Q_PROPERTY(QString Activity READ activity)

public:
    QString activity() const { return said; }
    QString said;
};

class BusTest : public QObject
{
    Q_OBJECT

private:
    std::unique_ptr<FixtureBus> m_bus;
    std::unique_ptr<QDBusConnection> m_standIns;
    QTemporaryDir m_dir;

    struct Character {
        CompanionSettings settings;
        Companion companion;
        AgentSource source;
    };

    // A character listening to the stand-in for P@x's service.
    std::unique_ptr<Character> character()
    {
        auto made = std::make_unique<Character>();
        made->companion.setSettings(&made->settings);
        made->companion.setBubbleSeconds(0);
        made->companion.setMoodSeconds(0);
        made->source.setCompanion(&made->companion);
        made->source.setBusAddress(m_bus->address());
        made->source.start();
        return made;
    }

    bool scenario(const QString &status, const QString &task = QString(), const QString &holdBefore = QString(),
                  int holdCount = 1)
    {
        m_bus->startAgent();
        QVariantMap wanted { { QStringLiteral("status"), status }, { QStringLiteral("pace_ms"), 20 } };
        if (!task.isEmpty())
            wanted.insert(QStringLiteral("task"), task);
        if (!holdBefore.isEmpty()) {
            wanted.insert(QStringLiteral("hold_before"), holdBefore);
            wanted.insert(QStringLiteral("hold_count"), holdCount);
        }
        return m_bus->use(wanted);
    }

    bool ask(Character &who, const QString &request)
    {
        bool answered = false;
        bool ok = false;
        who.source.client()->callWith(QStringLiteral("StartTask"), { request, QString() },
                                      [&](const QVariantMap &reply) {
                                          answered = true;
                                          ok = reply.value(QStringLiteral("ok")).toBool();
                                      });
        return QTest::qWaitFor([&] { return answered; }, 20000) && ok;
    }

    QDBusMessage call(const QString &method, const QVariantList &arguments = {})
    {
        QDBusMessage message = QDBusMessage::createMethodCall(companionName, companionPath, companionName, method);
        message.setArguments(arguments);
        message.setAutoStartService(false);
        return m_standIns->call(message, QDBus::Block, 20000);
    }

    QVariant property(const QString &name)
    {
        QDBusMessage get = QDBusMessage::createMethodCall(companionName, companionPath,
                                                          QStringLiteral("org.freedesktop.DBus.Properties"),
                                                          QStringLiteral("Get"));
        get << companionName << name;
        get.setAutoStartService(false);
        const QDBusMessage reply = m_standIns->call(get, QDBus::Block, 20000);
        if (reply.type() != QDBusMessage::ReplyMessage)
            return QVariant();
        return reply.arguments().value(0).value<QDBusVariant>().variant();
    }

    bool owned(const QString &name) { return m_standIns->interface()->isServiceRegistered(name).value(); }

    void startProgram(QProcess &program, const QStringList &extra)
    {
        QProcessEnvironment environment = QProcessEnvironment::systemEnvironment();
        environment.insert(QStringLiteral("DBUS_SESSION_BUS_ADDRESS"), m_bus->address());
        program.setProcessEnvironment(environment);
        program.setProcessChannelMode(QProcess::ForwardedChannels);
        QStringList arguments { QStringLiteral("--standalone"), QStringLiteral("--bus"), m_bus->address(),
                                QStringLiteral("--settings"), m_dir.path() + QStringLiteral("/pax-companion.json"),
                                QStringLiteral("--heal-command"), QStringLiteral("true") };
        program.start(QStringLiteral(TUR_COMPANION_PROGRAM), arguments + extra);
    }

    int runProgram(const QStringList &arguments)
    {
        QProcess program;
        startProgram(program, arguments);
        if (!program.waitForFinished(30000)) {
            program.kill();
            program.waitForFinished(5000);
            return -1;
        }
        return program.exitCode();
    }

private Q_SLOTS:
    void initTestCase()
    {
        QVERIFY(m_dir.isValid());
        m_bus = std::make_unique<FixtureBus>(QStringLiteral(TUR_PAX_UI_AGENT), QStringLiteral(TUR_PAX_UI_FIXTURES));
        QString problem;
        QVERIFY2(m_bus->start(&problem), qPrintable(problem));
        m_standIns = std::make_unique<QDBusConnection>(
            QDBusConnection::connectToBus(m_bus->address(), QStringLiteral("tur-pax-companion-stand-ins")));
        QVERIFY(m_standIns->isConnected());
    }

    void cleanupTestCase()
    {
        m_standIns.reset();
        QDBusConnection::disconnectFromBus(QStringLiteral("tur-pax-companion-stand-ins"));
        m_bus.reset();
    }

    // --- org.tur.Agent1 ------------------------------------------------------

    void pax_off_is_asleep_and_pax_on_is_awake()
    {
        QVERIFY(scenario(QStringLiteral("off")));
        auto off = character();
        QTest::qWait(500);
        QVERIFY(!off->companion.paxOn());
        QCOMPARE(off->companion.state(), QStringLiteral("asleep"));
        // Off, the character is not on the bar unless the person keeps it.
        QVERIFY(!off->companion.shown());

        QVERIFY(scenario(QStringLiteral("ready")));
        auto on = character();
        QTRY_VERIFY_WITH_TIMEOUT(on->companion.paxOn(), 10000);
        QCOMPARE(on->companion.state(), QStringLiteral("idle"));
        QVERIFY(on->companion.shown());
        // The recorded status has a limit in tokens and nothing used.
        QCOMPARE(on->companion.fuel(), 1.0);
        QVERIFY(on->companion.spent() >= 0);
    }

    void on_without_a_key_is_an_error()
    {
        QVERIFY(scenario(QStringLiteral("no-key")));
        auto who = character();
        QTRY_VERIFY_WITH_TIMEOUT(who->companion.paxOn(), 10000);
        QCOMPARE(who->companion.state(), QStringLiteral("error"));
        // The recorded status says "not ready" and gives no sentence: the
        // character adds none of its own to the rules.
        QCOMPARE(who->companion.problem().value(QStringLiteral("ready")).toBool(), false);
        QVERIFY(who->companion.problem().contains(QStringLiteral("message")));
        // A standing condition is not something it speaks first about.
        QVERIFY(!who->companion.speaking());
    }

    void a_task_is_thinking_then_idle_and_a_short_one_is_not_announced()
    {
        QVERIFY(scenario(QStringLiteral("ready"), QStringLiteral("two-tools"), QStringLiteral("ActionCompleted"), 2));
        auto who = character();
        QTRY_VERIFY_WITH_TIMEOUT(who->companion.paxOn(), 10000);
        QVERIFY(ask(*who, QStringLiteral("Set the value for the night.")));
        QTRY_COMPARE_WITH_TIMEOUT(who->companion.state(), QStringLiteral("thinking"), 10000);
        // The step it is on, in the service's words, for "What is P@x
        // watching?".
        QTRY_COMPARE_WITH_TIMEOUT(who->companion.step().value(QStringLiteral("title")).toString(),
                                  QStringLiteral("Set the test value"), 10000);
        QCOMPARE(who->companion.step().value(QStringLiteral("reason")).toString(),
                 QStringLiteral("reversible_permitted"));
        m_bus->release();
        QTRY_COMPARE_WITH_TIMEOUT(who->companion.state(), QStringLiteral("idle"), 10000);
        QVERIFY(who->companion.step().isEmpty());
        // The person was there and it took a moment: nothing is said.
        QVERIFY(!who->companion.speaking());
        QCOMPARE(who->companion.heldCount(), 0);
    }

    void a_task_that_took_long_is_announced_once()
    {
        QVERIFY(scenario(QStringLiteral("ready"), QStringLiteral("two-tools")));
        auto who = character();
        who->source.setLongTaskSeconds(0);
        QTRY_VERIFY_WITH_TIMEOUT(who->companion.paxOn(), 10000);
        QVERIFY(ask(*who, QStringLiteral("Set the value for the night.")));
        QTRY_VERIFY_WITH_TIMEOUT(who->companion.speaking(), 10000);
        const QVariantMap bubble = who->companion.bubble();
        QCOMPARE(bubble.value(QStringLiteral("kind")).toString(), QStringLiteral("task"));
        // The system speaks about P@x: TUR, not P@x.
        QCOMPARE(bubble.value(QStringLiteral("sender")).toString(), QStringLiteral("tur"));
        QCOMPARE(bubble.value(QStringLiteral("sentence")).toString(), QStringLiteral("P@x finished what you asked."));
        QCOMPARE(bubble.value(QStringLiteral("opens")).toString(), QStringLiteral("panel"));
        QVERIFY(bubble.value(QStringLiteral("actions")).toStringList().size() <= 2);
    }

    void a_refused_step_is_told_with_the_services_reason()
    {
        QVERIFY(scenario(QStringLiteral("ready"), QStringLiteral("refused")));
        auto who = character();
        QTRY_VERIFY_WITH_TIMEOUT(who->companion.paxOn(), 10000);
        QVERIFY(ask(*who, QStringLiteral("Read the value and send the report to Marta.")));
        QTRY_VERIFY_WITH_TIMEOUT(who->companion.speaking(), 10000);
        QCOMPARE(who->companion.bubble().value(QStringLiteral("sentence")).toString(),
                 QStringLiteral("P@x finished, but one step was refused."));
        // The reason is the service's own sentence, word for word.
        QCOMPARE(who->companion.bubble().value(QStringLiteral("detail")).toString(),
                 QStringLiteral("The assistant cannot ask for your approval in a way that is safe on this system, so "
                                "it does only things that can be undone."));
        QTRY_COMPARE_WITH_TIMEOUT(who->companion.state(), QStringLiteral("refused"), 10000);
        who->companion.dismiss();
        QCOMPARE(who->companion.state(), QStringLiteral("idle"));
    }

    void the_days_limit_is_told_in_the_services_words_with_its_code()
    {
        QVERIFY(scenario(QStringLiteral("limit-reached"), QStringLiteral("spend-limit")));
        auto who = character();
        QTRY_VERIFY_WITH_TIMEOUT(who->companion.paxOn(), 10000);
        QVERIFY(ask(*who, QStringLiteral("Read the value again and again.")));
        QTRY_VERIFY_WITH_TIMEOUT(who->companion.speaking(), 10000);
        QCOMPARE(who->companion.bubble().value(QStringLiteral("sentence")).toString(),
                 QStringLiteral("P@x stopped: today's spending limit was reached."));
        QCOMPARE(who->companion.bubble().value(QStringLiteral("code")).toString(), QStringLiteral("TUR-AI-SPEND-001"));
        QCOMPARE(who->companion.bubble().value(QStringLiteral("sender")).toString(), QStringLiteral("tur"));
    }

    void a_task_the_person_stopped_is_not_announced()
    {
        QVERIFY(scenario(QStringLiteral("ready"), QStringLiteral("stop")));
        auto who = character();
        who->source.setLongTaskSeconds(0);
        QTRY_VERIFY_WITH_TIMEOUT(who->companion.paxOn(), 10000);
        QSignalSpy finished(who->source.client(), &AgentClient::taskFinished);
        QVERIFY(ask(*who, QStringLiteral("Wait for a while.")));
        QTRY_COMPARE_WITH_TIMEOUT(who->companion.state(), QStringLiteral("thinking"), 10000);
        // "Stop P@x" on the character's menu.
        who->source.stop();
        QTRY_COMPARE_WITH_TIMEOUT(finished.count(), 1, 10000);
        QVERIFY(m_bus->callNames().contains(QStringLiteral("CancelTask")));
        QTRY_COMPARE_WITH_TIMEOUT(who->companion.state(), QStringLiteral("idle"), 10000);
        QVERIFY(!who->companion.speaking());
        QCOMPARE(who->companion.heldCount(), 0);
    }

    void the_character_never_starts_pax()
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

        auto who = character();
        QTest::qWait(1000);
        QVERIFY2(!m_bus->agentOwned(), "the character started P@x's service");
        QVERIFY(!who->companion.paxOn());
        QCOMPARE(who->companion.state(), QStringLiteral("asleep"));
        // Not even to stop it.
        who->source.stop();
        QTest::qWait(500);
        QVERIFY2(!m_bus->agentOwned(), "stopping started P@x's service");
        QVERIFY(!who->source.problem().isEmpty());

        // When P@x is started by someone else, the character wakes. The
        // stand-in takes its name before it is told its scenario and does
        // not signal the change, where the service signals StateChanged
        // when it is switched on; the client asks again, as it does on
        // that signal.
        QVERIFY(scenario(QStringLiteral("ready")));
        who->source.client()->refresh();
        QTRY_VERIFY_WITH_TIMEOUT(who->companion.paxOn(), 15000);
        QCOMPARE(who->companion.state(), QStringLiteral("idle"));
        // And sleeps again when it goes.
        m_bus->stopAgent();
        QTRY_COMPARE_WITH_TIMEOUT(who->companion.state(), QStringLiteral("asleep"), 15000);

        QFile::remove(file);
        m_bus->reloadServices();
    }

    // --- P@x's window and P@x's indicator -----------------------------------

    void pressing_the_character_asks_pax_window_for_the_ask_bar()
    {
        WindowStandIn window;
        QVERIFY(m_standIns->registerObject(QStringLiteral("/org/tur/PaxUi1"), QStringLiteral("org.tur.PaxUi1"), &window,
                                           QDBusConnection::ExportAllSlots));
        QVERIFY(m_standIns->registerService(QStringLiteral("org.tur.PaxUi1")));
        QVERIFY(scenario(QStringLiteral("ready")));
        auto who = character();
        who->source.showWindow(QStringLiteral("ask"));
        QTRY_COMPARE_WITH_TIMEOUT(window.shown, QStringList { QStringLiteral("ask") }, 10000);
        QVERIFY(who->source.problem().isEmpty());
        who->source.showWindow(QStringLiteral("panel"));
        QTRY_COMPARE_WITH_TIMEOUT(window.shown.size(), 2, 10000);
        QCOMPARE(window.shown.constLast(), QStringLiteral("panel"));
        m_standIns->unregisterService(QStringLiteral("org.tur.PaxUi1"));
        m_standIns->unregisterObject(QStringLiteral("/org/tur/PaxUi1"));
    }

    void what_the_indicator_shows_is_followed()
    {
        const QString service = QStringLiteral("org.tur.PaxOverlay1");
        const QString path = QStringLiteral("/org/tur/PaxOverlay1");
        BusProperty followed;
        followed.setBusAddress(m_bus->address());
        followed.setService(service);
        followed.setPath(path);
        followed.setIface(service);
        followed.setName(QStringLiteral("Activity"));
        followed.start();
        // The indicator is not running: nothing is known, nothing is started.
        QTest::qWait(300);
        QVERIFY(!followed.present());

        IndicatorStandIn indicator;
        indicator.said = QStringLiteral("working");
        QVERIFY(m_standIns->registerObject(path, service, &indicator, QDBusConnection::ExportAllProperties));
        QVERIFY(m_standIns->registerService(service));
        QTRY_VERIFY_WITH_TIMEOUT(followed.present(), 10000);
        QCOMPARE(followed.value().toString(), QStringLiteral("working"));

        // It changes: the character's rules hear of it.
        CompanionSettings settings;
        Companion companion;
        companion.setSettings(&settings);
        companion.setPaxOn(true);
        connect(&followed, &BusProperty::valueChanged, &companion,
                [&] { companion.setOverlayActivity(followed.present() ? followed.value().toString() : QString()); });
        indicator.said = QStringLiteral("acting");
        QDBusMessage changed = QDBusMessage::createSignal(path, QStringLiteral("org.freedesktop.DBus.Properties"),
                                                          QStringLiteral("PropertiesChanged"));
        changed << service << QVariantMap { { QStringLiteral("Activity"), indicator.said } } << QStringList();
        QVERIFY(m_standIns->send(changed));
        QTRY_COMPARE_WITH_TIMEOUT(followed.value().toString(), QStringLiteral("acting"), 10000);
        QCOMPARE(companion.state(), QStringLiteral("acting"));

        // The indicator goes: the character no longer says "controlling".
        m_standIns->unregisterService(service);
        QTRY_VERIFY_WITH_TIMEOUT(!followed.present(), 10000);
        QCOMPARE(companion.state(), QStringLiteral("idle"));
        m_standIns->unregisterObject(path);
    }

    // --- org.tur.PaxCompanion1, as the program offers it -----------------------

    void the_program_offers_what_the_xml_states()
    {
        QVERIFY(scenario(QStringLiteral("ready")));
        // What the healing service's program prints: the fixture's notices.
        const QString printed = m_dir.path() + QStringLiteral("/printed.json");
        {
            QFile fixture(QStringLiteral(TUR_COMPANION_FIXTURES "/heal/notices.json"));
            QVERIFY(fixture.open(QIODevice::ReadOnly));
            QFile out(printed);
            QVERIFY(out.open(QIODevice::WriteOnly));
            out.write(QJsonDocument(QJsonDocument::fromJson(fixture.readAll()).object().value(QLatin1String("printed"))
                                        .toArray())
                          .toJson());
        }
        QProcess program;
        startProgram(program, { QStringLiteral("--heal-command"), QStringLiteral("cat ") + printed });
        QVERIFY(program.waitForStarted(15000));
        QVERIFY2(QTest::qWaitFor([&] { return owned(companionName); }, 30000),
                 "tur-pax-companion took org.tur.PaxCompanion1");

        QDBusMessage introspect = QDBusMessage::createMethodCall(
            companionName, companionPath, QStringLiteral("org.freedesktop.DBus.Introspectable"),
            QStringLiteral("Introspect"));
        const QDBusMessage reply = m_standIns->call(introspect, QDBus::Block, 20000);
        QCOMPARE(reply.type(), QDBusMessage::ReplyMessage);
        const QStringList offered = shapeOf(reply.arguments().value(0).toString()).value(companionName);
        QFile file(QStringLiteral(TUR_COMPANION_XML));
        QVERIFY(file.open(QIODevice::ReadOnly));
        const QStringList stated = shapeOf(QString::fromUtf8(file.readAll())).value(companionName);
        QCOMPARE(stated.size(), 10);
        QCOMPARE(offered, stated);
        QCOMPARE(property(QStringLiteral("InterfaceVersion")).toUInt(), 1u);

        // P@x is on: the character is on the bar, awake.
        QTRY_COMPARE_WITH_TIMEOUT(property(QStringLiteral("State")).toString(), QStringLiteral("idle"), 15000);
        QCOMPARE(property(QStringLiteral("Shown")).toBool(), true);
        QCOMPARE(property(QStringLiteral("Quiet")).toBool(), false);

        // Where it is, for P@x's ask bar.
        const QVariantMap anchor = qdbus_cast<QVariantMap>(property(QStringLiteral("Anchor")));
        QVERIFY(anchor.contains(QStringLiteral("screen")));
        QVERIFY(anchor.value(QStringLiteral("x")).toDouble() > 0);
        QVERIFY(anchor.value(QStringLiteral("lift")).toDouble() >= 8);
        QCOMPARE(anchor.value(QStringLiteral("shown")).toBool(), true);

        // Hidden, and brought back.
        QCOMPARE(call(QStringLiteral("Hide")).type(), QDBusMessage::ReplyMessage);
        QTRY_COMPARE_WITH_TIMEOUT(property(QStringLiteral("Shown")).toBool(), false, 10000);
        QCOMPARE(call(QStringLiteral("Show")).type(), QDBusMessage::ReplyMessage);
        QTRY_COMPARE_WITH_TIMEOUT(property(QStringLiteral("Shown")).toBool(), true, 10000);

        // Quiet in one call; a mode it does not know is refused.
        QCOMPARE(call(QStringLiteral("SetQuiet"), { QStringLiteral("hour") }).type(), QDBusMessage::ReplyMessage);
        QTRY_COMPARE_WITH_TIMEOUT(property(QStringLiteral("Quiet")).toBool(), true, 10000);
        QCOMPARE(call(QStringLiteral("SetQuiet"), { QStringLiteral("off") }).type(), QDBusMessage::ReplyMessage);
        QTRY_COMPARE_WITH_TIMEOUT(property(QStringLiteral("Quiet")).toBool(), false, 10000);
        QCOMPARE(call(QStringLiteral("SetQuiet"), { QStringLiteral("for ever") }).type(), QDBusMessage::ErrorMessage);

        // The healing service asks the character to say its notice 3 (the
        // handshake, design section 7): taken, once the bubble is drawn.
        const auto announce = [&](const QString &source, qulonglong id) {
            return call(QStringLiteral("Announce"), { source, QVariant::fromValue(id) });
        };
        const auto taken = [&](qulonglong id) {
            const QDBusMessage reply = announce(QStringLiteral("heal"), id);
            return reply.type() == QDBusMessage::ReplyMessage && reply.arguments().value(0).toBool();
        };
        QVERIFY2(taken(3), "the notice was not taken");
        QCOMPARE(property(QStringLiteral("State")).toString(), QStringLiteral("telling"));
        // While it speaks, the next one is the desktop notification's.
        QVERIFY(!taken(4));
        // Quiet: not taken; what was being said waits as a mark.
        QCOMPARE(call(QStringLiteral("SetQuiet"), { QStringLiteral("always") }).type(), QDBusMessage::ReplyMessage);
        QTRY_COMPARE_WITH_TIMEOUT(property(QStringLiteral("State")).toString(), QStringLiteral("noticing"), 10000);
        QVERIFY(!taken(4));
        QCOMPARE(call(QStringLiteral("SetQuiet"), { QStringLiteral("off") }).type(), QDBusMessage::ReplyMessage);
        QTRY_COMPARE_WITH_TIMEOUT(property(QStringLiteral("Quiet")).toBool(), false, 10000);
        // Said before, or not waiting at all: not taken.
        QVERIFY(!taken(3));
        QVERIFY(!taken(99));
        // An event the person is not told of is not a notice.
        QVERIFY(!taken(7));
        // One source only.
        QCOMPARE(announce(QStringLiteral("tur-pax"), 1).type(), QDBusMessage::ErrorMessage);
        // Hidden: not taken.
        QCOMPARE(call(QStringLiteral("Hide")).type(), QDBusMessage::ReplyMessage);
        QTRY_COMPARE_WITH_TIMEOUT(property(QStringLiteral("Shown")).toBool(), false, 10000);
        QVERIFY(!taken(4));
        QCOMPARE(call(QStringLiteral("Show")).type(), QDBusMessage::ReplyMessage);
        QTRY_COMPARE_WITH_TIMEOUT(property(QStringLiteral("Shown")).toBool(), true, 10000);
        // Asking for the keyboard is answered.
        QCOMPARE(call(QStringLiteral("Attend")).type(), QDBusMessage::ReplyMessage);

        // A second start speaks to the first and ends: hide, then show.
        QCOMPARE(runProgram({ QStringLiteral("--hide") }), 0);
        QTRY_COMPARE_WITH_TIMEOUT(property(QStringLiteral("Shown")).toBool(), false, 10000);
        // Started with the session, it changes nothing the person chose.
        QCOMPARE(runProgram({ QStringLiteral("--background") }), 0);
        QCOMPARE(property(QStringLiteral("Shown")).toBool(), false);
        QCOMPARE(runProgram({ QStringLiteral("--show") }), 0);
        QTRY_COMPARE_WITH_TIMEOUT(property(QStringLiteral("Shown")).toBool(), true, 10000);

        // P@x goes: the character is asleep and, by default, off the bar.
        m_bus->stopAgent();
        QTRY_COMPARE_WITH_TIMEOUT(property(QStringLiteral("State")).toString(), QStringLiteral("asleep"), 15000);
        QCOMPARE(property(QStringLiteral("Shown")).toBool(), false);

        program.terminate();
        QVERIFY(program.waitForFinished(10000));
        QVERIFY(!owned(companionName));
    }
};

QTEST_GUILESS_MAIN(BusTest)

#include "tst_bus.moc"
