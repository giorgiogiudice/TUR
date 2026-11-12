// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
//
// org.tur.PaxOverlay1 as the running program offers it, on a session bus of
// the test's own (a dbus-daemon it starts), under the offscreen platform:
// what it offers is what the XML states; a lease is answered only for the
// connection that took it, and its signals go to that connection alone; the
// overlay is shown and says so; it vanishes when its owner leaves the bus,
// when Hide is called, and when `tur-pax-overlay --hide` is run.
//
// Where the windows of other programs are, it cannot know here (no
// compositor): that, and everything else that needs KWin, is in
// desktop/tests/compositor.

#include "bustypes.h"

#include <QDBusConnection>
#include <QDBusConnectionInterface>
#include <QDBusMessage>
#include <QDBusVariant>
#include <QFile>
#include <QMap>
#include <QProcess>
#include <QProcessEnvironment>
#include <QSignalSpy>
#include <QStringList>
#include <QTemporaryDir>
#include <QTest>
#include <QXmlStreamReader>

#include <optional>

using PaxOverlay::RectD;

namespace {

const QString service = QStringLiteral("org.tur.PaxOverlay1");
const QString path = QStringLiteral("/org/tur/PaxOverlay1");
const QString interface = QStringLiteral("org.tur.PaxOverlay1");

// The members of each interface in an introspection document, as text that
// two documents can be compared by: "method Name(in) -> (out)", "signal
// Name(args)", "property Name type access". Argument names are not compared.
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
    if (reader.hasError())
        qFatal("the XML cannot be read: %s", qPrintable(reader.errorString()));
    for (QStringList &members : out)
        members.sort();
    return out;
}

} // namespace

class BusTest : public QObject
{
    Q_OBJECT

private:
    QTemporaryDir m_dir;
    QProcess m_daemon;
    QProcess m_overlay;
    QString m_address;
    QProcessEnvironment m_environment;
    std::optional<QDBusConnection> m_a;
    std::optional<QDBusConnection> m_b;

    QDBusMessage call(QDBusConnection &bus, const QString &method, const QVariantList &arguments = {})
    {
        QDBusMessage message = QDBusMessage::createMethodCall(service, path, interface, method);
        message.setArguments(arguments);
        return bus.call(message, QDBus::Block, 20000);
    }

    QVariant property(const QString &name)
    {
        QDBusMessage get = QDBusMessage::createMethodCall(service, path, QStringLiteral("org.freedesktop.DBus.Properties"),
                                                          QStringLiteral("Get"));
        get << interface << name;
        const QDBusMessage reply = m_a->call(get, QDBus::Block, 20000);
        if (reply.type() != QDBusMessage::ReplyMessage)
            return QVariant();
        return reply.arguments().value(0).value<QDBusVariant>().variant();
    }

    bool shownBecomes(bool wanted)
    {
        return QTest::qWaitFor([&] { return property(QStringLiteral("Shown")).toBool() == wanted; }, 15000);
    }

    QString acquire(QDBusConnection &bus)
    {
        const QDBusMessage reply = call(bus, QStringLiteral("Acquire"), { QStringLiteral("bus test") });
        if (reply.type() != QDBusMessage::ReplyMessage)
            return QString();
        return reply.arguments().value(0).toString();
    }

    void startOverlay()
    {
        m_overlay.setProcessEnvironment(m_environment);
        m_overlay.setProcessChannelMode(QProcess::ForwardedChannels);
        m_overlay.start(QStringLiteral(TUR_PAX_OVERLAY_PROGRAM), {});
        QVERIFY(m_overlay.waitForStarted(15000));
        QVERIFY2(QTest::qWaitFor([&] { return m_a->interface()->isServiceRegistered(service).value(); }, 20000),
                 "tur-pax-overlay took its name on the bus");
    }

private Q_SLOTS:
    void initTestCase()
    {
        PaxOverlay::registerBusTypes();
        QVERIFY(m_dir.isValid());
        const QString chosen = QStringLiteral("unix:path=") + m_dir.path() + QStringLiteral("/bus");
        m_environment = QProcessEnvironment::systemEnvironment();
        m_environment.insert(QStringLiteral("DBUS_SESSION_BUS_ADDRESS"), chosen);
        m_environment.insert(QStringLiteral("XDG_RUNTIME_DIR"), m_dir.path());
        m_environment.insert(QStringLiteral("QT_QPA_PLATFORM"), QStringLiteral("offscreen"));
        m_daemon.setProcessEnvironment(m_environment);
        m_daemon.setProcessChannelMode(QProcess::ForwardedErrorChannel);
        m_daemon.start(QStringLiteral("dbus-daemon"),
                       { QStringLiteral("--session"), QStringLiteral("--nofork"), QStringLiteral("--nopidfile"),
                         QStringLiteral("--print-address=1"), QStringLiteral("--address=") + chosen });
        QVERIFY2(m_daemon.waitForStarted(15000), "dbus-daemon could not be started");
        QVERIFY(m_daemon.waitForReadyRead(15000));
        m_address = QString::fromUtf8(m_daemon.readLine()).trimmed();
        QVERIFY2(m_address.startsWith(chosen), qPrintable(m_address));
        m_environment.insert(QStringLiteral("DBUS_SESSION_BUS_ADDRESS"), m_address);

        m_a.emplace(QDBusConnection::connectToBus(m_address, QStringLiteral("a")));
        m_b.emplace(QDBusConnection::connectToBus(m_address, QStringLiteral("b")));
        QVERIFY(m_a->isConnected() && m_b->isConnected());
        startOverlay();
    }

    void cleanupTestCase()
    {
        m_overlay.terminate();
        m_overlay.waitForFinished(10000);
        m_a.reset();
        m_b.reset();
        QDBusConnection::disconnectFromBus(QStringLiteral("a"));
        QDBusConnection::disconnectFromBus(QStringLiteral("b"));
        m_daemon.terminate();
        m_daemon.waitForFinished(10000);
    }

    void cleanup()
    {
        // Whatever a test left on screen.
        call(*m_a, QStringLiteral("Hide"));
    }

    void the_interface_is_the_one_the_xml_states()
    {
        QDBusMessage introspect = QDBusMessage::createMethodCall(
            service, path, QStringLiteral("org.freedesktop.DBus.Introspectable"), QStringLiteral("Introspect"));
        const QDBusMessage reply = m_a->call(introspect, QDBus::Block, 20000);
        QCOMPARE(reply.type(), QDBusMessage::ReplyMessage);
        const QStringList offered = shapeOf(reply.arguments().value(0).toString()).value(interface);
        QFile file(QStringLiteral(TUR_PAX_OVERLAY_SOURCE "/dbus/org.tur.PaxOverlay1.xml"));
        QVERIFY(file.open(QIODevice::ReadOnly));
        const QStringList stated = shapeOf(QString::fromUtf8(file.readAll())).value(interface);
        QCOMPARE(stated.size(), 22);
        QCOMPARE(offered, stated);
    }

    void it_is_shown_and_says_so()
    {
        QCOMPARE(property(QStringLiteral("Shown")).toBool(), false);
        const QString lease = acquire(*m_a);
        QCOMPARE(lease.size(), 32);
        QCOMPARE(call(*m_a, QStringLiteral("SetActivity"), { lease, QStringLiteral("acting"), QVariantMap() }).type(),
                 QDBusMessage::ReplyMessage);
        QVERIFY2(shownBecomes(true), "Shown is true once the surface has been drawn on every screen");
        QVERIFY(!property(QStringLiteral("Screens")).toStringList().isEmpty());

        QCOMPARE(call(*m_a, QStringLiteral("SetActivity"), { lease, QStringLiteral("idle"), QVariantMap() }).type(),
                 QDBusMessage::ReplyMessage);
        QVERIFY2(shownBecomes(false), "idle is not shown");
    }

    void a_lease_is_only_its_holders()
    {
        const QString lease = acquire(*m_a);
        const QDBusMessage stolen = call(*m_b, QStringLiteral("SetActivity"), { lease, QStringLiteral("acting"), QVariantMap() });
        QCOMPARE(stolen.type(), QDBusMessage::ErrorMessage);
        QCOMPARE(stolen.errorName(), QStringLiteral("org.tur.PaxOverlay1.Error.NoLease"));
        QVERIFY(stolen.errorMessage().startsWith(QStringLiteral("TUR-DESKTOP-PAXOVERLAY-001: ")));

        const QDBusMessage wrong = call(*m_a, QStringLiteral("SetActivity"), { lease, QStringLiteral("dancing"), QVariantMap() });
        QCOMPARE(wrong.errorName(), QStringLiteral("org.tur.PaxOverlay1.Error.InvalidArgument"));

        const QDBusMessage highlight =
            call(*m_a, QStringLiteral("HighlightRect"),
                 { lease, QString(), QVariant::fromValue(RectD { 10, 10, 100, 30 }), QVariantMap() });
        QCOMPARE(highlight.type(), QDBusMessage::ReplyMessage);
        QVERIFY(highlight.arguments().value(0).toUInt() > 0);
    }

    // Under the offscreen platform there is no compositor to ask: the error
    // says so, and nothing is made up.
    void a_window_that_cannot_be_found_is_an_error()
    {
        const QString lease = acquire(*m_a);
        const QDBusMessage reply =
            call(*m_a, QStringLiteral("HighlightInWindow"),
                 { lease, 1234u, QStringLiteral("org.tur.Settings"), QStringLiteral("Settings"),
                   QVariant::fromValue(RectD { 10, 10, 100, 30 }), QVariantMap() });
        QCOMPARE(reply.type(), QDBusMessage::ErrorMessage);
        QCOMPARE(reply.errorName(), QStringLiteral("org.tur.PaxOverlay1.Error.WindowNotFound"));
        QVERIFY2(reply.errorMessage().contains(QStringLiteral("not a Wayland session")), qPrintable(reply.errorMessage()));
    }

    // Signals about a lease go to its holder and to nobody else.
    void its_signals_go_to_its_holder_alone()
    {
        const QString lease = acquire(*m_a);
        QSignalSpy toA(this, SIGNAL(endedForA(QString, QString)));
        QSignalSpy toB(this, SIGNAL(endedForB(QString, QString)));
        QVERIFY(m_a->connect(service, path, interface, QStringLiteral("LeaseEnded"), this,
                             SIGNAL(endedForA(QString, QString))));
        QVERIFY(m_b->connect(QString(), path, interface, QStringLiteral("LeaseEnded"), this,
                             SIGNAL(endedForB(QString, QString))));
        QCOMPARE(call(*m_a, QStringLiteral("Release"), { lease }).type(), QDBusMessage::ReplyMessage);
        QVERIFY(toA.wait(10000));
        QCOMPARE(toA.at(0).at(0).toString(), lease);
        QCOMPARE(toA.at(0).at(1).toString(), QStringLiteral("released"));
        QTest::qWait(500);
        QCOMPARE(toB.size(), 0);
        m_a->disconnect(service, path, interface, QStringLiteral("LeaseEnded"), this, SIGNAL(endedForA(QString, QString)));
        m_b->disconnect(QString(), path, interface, QStringLiteral("LeaseEnded"), this, SIGNAL(endedForB(QString, QString)));
    }

    // Its owner leaves the bus: everything it drew goes.
    void it_vanishes_when_its_owner_leaves()
    {
        {
            QDBusConnection owner = QDBusConnection::connectToBus(m_address, QStringLiteral("owner"));
            QVERIFY(owner.isConnected());
            const QString lease = acquire(owner);
            QVERIFY(!lease.isEmpty());
            QCOMPARE(call(owner, QStringLiteral("SetActivity"), { lease, QStringLiteral("observing"), QVariantMap() }).type(),
                     QDBusMessage::ReplyMessage);
            QVERIFY(shownBecomes(true));
        }
        QDBusConnection::disconnectFromBus(QStringLiteral("owner"));
        QVERIFY2(shownBecomes(false), "the overlay is gone once the lease's holder has left the bus");
    }

    // Anyone may switch it off, at once.
    void hide_switches_it_off()
    {
        const QString lease = acquire(*m_a);
        QCOMPARE(call(*m_a, QStringLiteral("SetActivity"), { lease, QStringLiteral("acting"), QVariantMap() }).type(),
                 QDBusMessage::ReplyMessage);
        QVERIFY(shownBecomes(true));
        QCOMPARE(call(*m_b, QStringLiteral("Hide")).type(), QDBusMessage::ReplyMessage);
        QCOMPARE(property(QStringLiteral("Shown")).toBool(), false);
        const QDBusMessage after = call(*m_a, QStringLiteral("SetActivity"), { lease, QStringLiteral("acting"), QVariantMap() });
        QCOMPARE(after.errorName(), QStringLiteral("org.tur.PaxOverlay1.Error.NoLease"));
    }

    void the_command_line_switches_it_off()
    {
        const QString lease = acquire(*m_a);
        QCOMPARE(call(*m_a, QStringLiteral("SetActivity"), { lease, QStringLiteral("acting"), QVariantMap() }).type(),
                 QDBusMessage::ReplyMessage);
        QVERIFY(shownBecomes(true));
        QProcess hide;
        hide.setProcessEnvironment(m_environment);
        hide.start(QStringLiteral(TUR_PAX_OVERLAY_PROGRAM), { QStringLiteral("--hide") });
        QVERIFY(hide.waitForFinished(20000));
        QCOMPARE(hide.exitCode(), 0);
        QCOMPARE(property(QStringLiteral("Shown")).toBool(), false);
    }

    // Last: with nothing running, --hide is not an error.
    void the_command_line_with_nothing_running()
    {
        m_overlay.terminate();
        QVERIFY(m_overlay.waitForFinished(10000));
        QProcess hide;
        hide.setProcessEnvironment(m_environment);
        hide.start(QStringLiteral(TUR_PAX_OVERLAY_PROGRAM), { QStringLiteral("--hide") });
        QVERIFY(hide.waitForFinished(20000));
        QCOMPARE(hide.exitCode(), 0);
    }

Q_SIGNALS:
    void endedForA(const QString &lease, const QString &reason);
    void endedForB(const QString &lease, const QString &reason);
};

QTEST_GUILESS_MAIN(BusTest)

#include "tst_bus.moc"
