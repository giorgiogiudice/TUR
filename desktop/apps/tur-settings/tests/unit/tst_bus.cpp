// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
//
// The addressing over D-Bus (desktop-experience.md, 9.5 and 10.3), on a
// session bus of the test's own: a dbus-daemon that it starts, that nothing
// else is on, and that it stops.
//
// In three steps.
//
//  1. The settings window in this process, on the bus, called from a second
//     connection. Here the test can look at the window: the rectangle that
//     Reveal reports is compared with the pixels that belong to the row.
//  2. The program itself, tur-settings, started by hand: called from here,
//     from a second start of itself (the command line and the link) and from
//     busctl, a client that is not Qt; and passing a call on to another
//     program.
//  3. The program started by the bus, from its service file, because a call
//     arrived and no Settings window was open.

#include "bustypes.h"
#include "registry.h"
#include "settingshost.h"
#include "turkit.h"

#include <QColor>
#include <QDBusConnection>
#include <QDBusConnectionInterface>
#include <QDBusMessage>
#include <QDBusPendingCall>
#include <QDBusPendingCallWatcher>
#include <QDBusReply>
#include <QDBusVariant>
#include <QDir>
#include <QFile>
#include <QGuiApplication>
#include <QImage>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QMap>
#include <QProcess>
#include <QProcessEnvironment>
#include <QQmlApplicationEngine>
#include <QQmlContext>
#include <QQmlEngine>
#include <QQmlError>
#include <QQmlExpression>
#include <QQmlProperty>
#include <QQuickItem>
#include <QQuickWindow>
#include <QSignalSpy>
#include <QStyleHints>
#include <QTemporaryDir>
#include <QTest>
#include <QXmlStreamReader>

#include <csignal>
#include <memory>
#include <optional>

#include <sys/types.h>

using Settings::RectD;
using Settings::SearchHit;

namespace {

const QString fixtures = QStringLiteral(TUR_SETTINGS_FIXTURES);
const QString source = QStringLiteral(TUR_SETTINGS_SOURCE);
const QString testData = QStringLiteral(TUR_SETTINGS_TEST_DATA);
const QString program = QStringLiteral(TUR_SETTINGS_PROGRAM);
const QString helper = QStringLiteral(TUR_SETTINGS_HELPER);
const QString peer = QStringLiteral(TUR_SETTINGS_PEER);

const QString settingsName = QStringLiteral("org.tur.Settings1");
const QString settingsPath = QStringLiteral("/org/tur/Settings1");
const QString programName = QStringLiteral("org.tur.Settings");
const QString addressablePath = QStringLiteral("/org/tur/Addressable1");
const QString addressableInterface = QStringLiteral("org.tur.Addressable1");

QByteArray fileContent(const QString &path)
{
    QFile file(path);
    if (!file.open(QIODevice::ReadOnly))
        qFatal("%s cannot be read", qPrintable(path));
    return file.readAll();
}

// What an interface offers, one line for each member, sorted: the names and
// the signatures, which is what a caller depends on. Names of arguments and
// annotations are left out.
QMap<QString, QStringList> shapeOf(const QString &xml)
{
    QMap<QString, QStringList> out;
    QXmlStreamReader reader(xml);
    QString interface;
    QString member;
    QString in;
    QString result;
    while (!reader.atEnd()) {
        const QXmlStreamReader::TokenType token = reader.readNext();
        const QString name = reader.name().toString();
        const QXmlStreamAttributes attributes = reader.attributes();
        if (token == QXmlStreamReader::StartElement) {
            if (name == QLatin1String("interface")) {
                interface = attributes.value(QLatin1String("name")).toString();
            } else if (name == QLatin1String("method") || name == QLatin1String("signal")) {
                member = attributes.value(QLatin1String("name")).toString();
                in.clear();
                result.clear();
            } else if (name == QLatin1String("arg")) {
                const bool outgoing = attributes.value(QLatin1String("direction")) == QLatin1String("out");
                (outgoing ? result : in) += attributes.value(QLatin1String("type")).toString();
            } else if (name == QLatin1String("property")) {
                out[interface].append(QStringLiteral("property %1 %2 %3")
                                          .arg(attributes.value(QLatin1String("name")).toString(),
                                               attributes.value(QLatin1String("type")).toString(),
                                               attributes.value(QLatin1String("access")).toString()));
            }
        } else if (token == QXmlStreamReader::EndElement) {
            if (name == QLatin1String("method"))
                out[interface].append(QStringLiteral("method %1(%2) -> (%3)").arg(member, in, result));
            // The arguments of a signal all go out, whatever is written.
            else if (name == QLatin1String("signal"))
                out[interface].append(QStringLiteral("signal %1(%2)").arg(member, in + result));
        }
    }
    if (reader.hasError())
        qFatal("the XML cannot be read: %s", qPrintable(reader.errorString()));
    for (QStringList &members : out)
        members.sort();
    return out;
}

// The smallest rectangle that holds every pixel in which two pictures of the
// window differ.
QRect difference(const QImage &first, const QImage &second)
{
    QRect box;
    if (first.size() != second.size())
        return first.rect();
    for (int y = 0; y < first.height(); ++y) {
        for (int x = 0; x < first.width(); ++x) {
            if (first.pixel(x, y) != second.pixel(x, y))
                box |= QRect(x, y, 1, 1);
        }
    }
    return box;
}

QQuickItem *itemNamed(QQuickItem *root, const QString &objectName)
{
    if (root->objectName() == objectName)
        return root;
    const QList<QQuickItem *> children = root->childItems();
    for (QQuickItem *child : children) {
        if (QQuickItem *found = itemNamed(child, objectName))
            return found;
    }
    return nullptr;
}

QVariantMap mapOf(const QDBusMessage &reply)
{
    return qdbus_cast<QVariantMap>(reply.arguments().value(0));
}

RectD rectOf(const QVariantMap &result)
{
    return qdbus_cast<RectD>(result.value(QStringLiteral("rect")));
}

QString describe(const RectD &rect)
{
    return QStringLiteral("(%1, %2, %3 x %4)").arg(rect.x).arg(rect.y).arg(rect.width).arg(rect.height);
}

bool sameRect(const RectD &a, const RectD &b)
{
    return qAbs(a.x - b.x) < 0.01 && qAbs(a.y - b.y) < 0.01 && qAbs(a.width - b.width) < 0.01
           && qAbs(a.height - b.height) < 0.01;
}

} // namespace

class TestBus : public QObject
{
    Q_OBJECT

public Q_SLOTS:
    void rowChanged(const QString &address) { m_rowChanges.append(address); }

private:
    // Calls a method from the test's own connection and waits for the
    // answer. The event loop runs meanwhile, which is what lets a window in
    // this process answer.
    QDBusMessage call(const QString &service, const QString &path, const QString &interface, const QString &method,
                      const QVariantList &arguments = QVariantList(), int timeoutMs = 30000)
    {
        QDBusMessage message = QDBusMessage::createMethodCall(service, path, interface, method);
        message.setArguments(arguments);
        QDBusPendingCallWatcher watcher(m_client->asyncCall(message, timeoutMs));
        QSignalSpy finished(&watcher, &QDBusPendingCallWatcher::finished);
        if (!watcher.isFinished())
            finished.wait(timeoutMs + 5000);
        return watcher.reply();
    }

    QDBusMessage settings(const QString &method, const QVariantList &arguments = QVariantList(), int timeoutMs = 30000)
    {
        return call(settingsName, settingsPath, settingsName, method, arguments, timeoutMs);
    }

    QDBusMessage reveal(const QString &address, const QVariantMap &options = QVariantMap())
    {
        return settings(QStringLiteral("Reveal"), { address, options });
    }

    // The property Address of org.tur.Settings1: where the window is.
    QString whereItIs()
    {
        const QDBusMessage reply = call(settingsName, settingsPath, QStringLiteral("org.freedesktop.DBus.Properties"),
                                        QStringLiteral("Get"), { settingsName, QStringLiteral("Address") });
        if (reply.type() != QDBusMessage::ReplyMessage)
            return QStringLiteral("(no answer: ") + reply.errorMessage() + QLatin1Char(')');
        return qdbus_cast<QDBusVariant>(reply.arguments().value(0)).variant().toString();
    }

    bool owned(const QString &name) { return m_client->interface()->isServiceRegistered(name).value(); }

    QJsonArray helperArguments()
    {
        QJsonArray seen;
        const bool started = QTest::qWaitFor(
            [&] {
                QFile file(m_helperOut);
                if (!file.open(QIODevice::ReadOnly))
                    return false;
                const QJsonDocument document = QJsonDocument::fromJson(file.readAll());
                seen = document.array();
                return document.isArray();
            },
            15000);
        if (!started)
            seen.append(QStringLiteral("(the stand-in for System Settings was not started)"));
        QFile::remove(m_helperOut);
        return seen;
    }

    // Lets the window draw what it has been asked to show.
    void drawn()
    {
        QTest::qWait(60);
        m_window->grabWindow();
        QTest::qWait(60);
    }

    // A picture of the window once it has stopped changing: two pictures in
    // a row that are the same. Whatever fades or settles has done so.
    QImage settledPicture()
    {
        QImage last = m_window->grabWindow();
        for (int attempt = 0; attempt < 40; ++attempt) {
            QTest::qWait(80);
            const QImage next = m_window->grabWindow();
            if (next == last)
                return next;
            last = next;
        }
        return last;
    }

    // Goes to another page than the one an address is on, so that what is
    // asked for next is shown on a page that begins at its top, whatever was
    // looked at before.
    void startElsewhere(const QString &address)
    {
        const QString elsewhere = address.startsWith(QLatin1String("tur.about")) ? QStringLiteral("tur.overview")
                                                                                 : QStringLiteral("tur.about");
        const QDBusMessage reply = settings(QStringLiteral("Open"), { elsewhere });
        if (reply.type() != QDBusMessage::ReplyMessage)
            qWarning("%s could not be opened: %s", qPrintable(elsewhere), qPrintable(reply.errorMessage()));
    }

    void stop(QProcess &process)
    {
        if (process.state() == QProcess::NotRunning)
            return;
        process.terminate();
        if (!process.waitForFinished(10000)) {
            process.kill();
            process.waitForFinished(5000);
        }
    }

private Q_SLOTS:
    void initTestCase()
    {
        Settings::registerBusTypes();
        QVERIFY(m_dir.isValid());
        m_helperOut = m_dir.path() + QStringLiteral("/system-settings-was-asked.json");
        qputenv("TUR_SETTINGS_HELPER_OUT", m_helperOut.toLocal8Bit());

        // The service file that starts the program on demand: the one that
        // is shipped, with the program that was just built in the place of
        // the installed one, on a fixture.
        const QString services = m_dir.path() + QStringLiteral("/data/dbus-1/services");
        QVERIFY(QDir().mkpath(services));
        QVERIFY(QDir().mkpath(m_dir.path() + QStringLiteral("/runtime")));
        QString service = QString::fromUtf8(fileContent(source + QStringLiteral("/data/org.tur.Settings1.service")));
        QVERIFY(service.contains(QLatin1String("\nName=org.tur.Settings1\n")));
        QVERIFY(service.contains(QLatin1String("\nExec=/usr/bin/tur-settings\n")));
        service.replace(QLatin1String("Exec=/usr/bin/tur-settings"),
                        QStringLiteral("Exec=%1 --standalone --fixture %2/default --system-settings-program %3")
                            .arg(program, fixtures, helper));
        QFile installed(services + QStringLiteral("/org.tur.Settings1.service"));
        QVERIFY(installed.open(QIODevice::WriteOnly));
        installed.write(service.toUtf8());
        installed.close();

        // A session bus of the test's own.
        // Its address is chosen here, so that the programs it starts are
        // told where it is.
        const QString chosen = QStringLiteral("unix:path=") + m_dir.path() + QStringLiteral("/bus");
        QProcessEnvironment environment = QProcessEnvironment::systemEnvironment();
        environment.insert(QStringLiteral("DBUS_SESSION_BUS_ADDRESS"), chosen);
        environment.insert(QStringLiteral("XDG_DATA_HOME"), m_dir.path() + QStringLiteral("/data"));
        environment.insert(QStringLiteral("XDG_DATA_DIRS"), m_dir.path() + QStringLiteral("/no-data"));
        environment.insert(QStringLiteral("XDG_RUNTIME_DIR"), m_dir.path() + QStringLiteral("/runtime"));
        m_daemon.setProcessEnvironment(environment);
        m_daemon.setProcessChannelMode(QProcess::ForwardedErrorChannel);
        m_daemon.start(QStringLiteral("dbus-daemon"),
                       { QStringLiteral("--session"), QStringLiteral("--nofork"), QStringLiteral("--nopidfile"),
                         QStringLiteral("--print-address=1"), QStringLiteral("--address=") + chosen });
        QVERIFY2(m_daemon.waitForStarted(15000),
                 "dbus-daemon could not be started; the builder image must have it (desktop/ci/builder/Containerfile)");
        QVERIFY2(m_daemon.waitForReadyRead(15000), "dbus-daemon did not print its address");
        m_address = QString::fromUtf8(m_daemon.readLine()).trimmed();
        QVERIFY2(m_address.startsWith(chosen), qPrintable(m_address));
        qputenv("DBUS_SESSION_BUS_ADDRESS", m_address.toUtf8());

        m_client.emplace(QDBusConnection::connectToBus(m_address, QStringLiteral("tur-settings-test-client")));
        QVERIFY2(m_client->isConnected(), qPrintable(m_client->lastError().message()));
        QVERIFY(!owned(settingsName));

        // The window, in this process, drawn the way the pictures are.
        TurKit::useStandaloneLook();
        QGuiApplication::styleHints()->setCursorFlashTime(0);
        QCoreApplication::setApplicationVersion(QStringLiteral(TUR_VERSION));
        m_engine = std::make_unique<QQmlApplicationEngine>();
        m_engine->setInitialProperties(QVariantMap {
            { QStringLiteral("fixture"), fixtures + QStringLiteral("/default") },
            { QStringLiteral("dbus"), true },
            { QStringLiteral("systemSettingsProgram"), helper },
        });
        m_engine->loadFromModule("Tur.Settings", "Main");
        QVERIFY(!m_engine->rootObjects().isEmpty());
        m_window = qobject_cast<QQuickWindow *>(m_engine->rootObjects().constFirst());
        QVERIFY(m_window);
        QVERIFY(QTest::qWaitForWindowExposed(m_window));
        QCOMPARE(m_window->property("busProblem").toString(), QString());
        m_host = m_window->findChild<SettingsHost *>();
        QVERIFY(m_host);
        QVERIFY(m_host->onBus());

        // It is on the bus under both of its names, on a connection that is
        // not the test's.
        QVERIFY(owned(settingsName));
        QVERIFY(owned(programName));
        QVERIFY(m_client->baseService() != m_host->bus().baseService());
    }

    void cleanupTestCase()
    {
        m_engine.reset();
        stop(m_settings);
        stop(m_peer);
        if (m_activated > 0)
            ::kill(pid_t(m_activated), SIGTERM);
        stop(m_daemon);
    }

    // --- 1. The window in this process ------------------------------------------

    // What the program offers is what the two XML files state.
    void the_interfaces_are_the_ones_the_xml_states_data()
    {
        QTest::addColumn<QString>("service");
        QTest::addColumn<QString>("path");
        QTest::addColumn<QString>("interface");
        QTest::addColumn<int>("members");

        QTest::newRow("org.tur.Settings1") << settingsName << settingsPath << settingsName << 6;
        QTest::newRow("org.tur.Addressable1") << programName << addressablePath << addressableInterface << 4;
    }

    void the_interfaces_are_the_ones_the_xml_states()
    {
        QFETCH(QString, service);
        QFETCH(QString, path);
        QFETCH(QString, interface);
        QFETCH(int, members);

        const QDBusMessage reply =
            call(service, path, QStringLiteral("org.freedesktop.DBus.Introspectable"), QStringLiteral("Introspect"));
        QCOMPARE(reply.type(), QDBusMessage::ReplyMessage);
        const QStringList offered = shapeOf(reply.arguments().value(0).toString()).value(interface);
        const QStringList stated =
            shapeOf(QString::fromUtf8(fileContent(source + QStringLiteral("/dbus/") + interface + QStringLiteral(".xml"))))
                .value(interface);
        QCOMPARE(stated.size(), members);
        QCOMPARE(offered, stated);
    }

    // The path of one interface does not offer the other.
    void each_interface_has_its_own_path()
    {
        const QDBusMessage wrong = call(settingsName, addressablePath, settingsName, QStringLiteral("Search"),
                                        { QStringLiteral("dark"), uint(5) });
        QCOMPARE(wrong.type(), QDBusMessage::ErrorMessage);
        const QDBusMessage other = call(settingsName, settingsPath, addressableInterface, QStringLiteral("List"));
        QCOMPARE(other.type(), QDBusMessage::ErrorMessage);
    }

    void search()
    {
        const QDBusMessage reply = settings(QStringLiteral("Search"), { QStringLiteral("dark mode"), uint(5) });
        QCOMPARE(reply.type(), QDBusMessage::ReplyMessage);
        QCOMPARE(reply.signature(), QStringLiteral("a(ssd)"));
        const auto hits = qdbus_cast<QList<SearchHit>>(reply.arguments().value(0));
        QVERIFY(!hits.isEmpty());
        QVERIFY(hits.size() <= 5);
        QCOMPARE(hits.constFirst().address, QStringLiteral("tur.appearance#variant"));
        QCOMPARE(hits.constFirst().title, QStringLiteral("Dark, light, or light by day and dark at night"));
        for (qsizetype i = 0; i < hits.size(); ++i) {
            QVERIFY(hits.at(i).score > 0 && hits.at(i).score <= 1);
            if (i > 0)
                QVERIFY(hits.at(i).score <= hits.at(i - 1).score);
        }

        // A limit is kept; no limit means twenty; nothing asked finds nothing.
        const auto two = qdbus_cast<QList<SearchHit>>(
            settings(QStringLiteral("Search"), { QStringLiteral("version"), uint(2) }).arguments().value(0));
        QCOMPARE(two.size(), 2);
        const auto unlimited = qdbus_cast<QList<SearchHit>>(
            settings(QStringLiteral("Search"), { QStringLiteral("version"), uint(0) }).arguments().value(0));
        QVERIFY(unlimited.size() >= 3 && unlimited.size() <= 20);
        const QDBusMessage nothing = settings(QStringLiteral("Search"), { QString(), uint(5) });
        QCOMPARE(nothing.type(), QDBusMessage::ReplyMessage);
        QVERIFY(qdbus_cast<QList<SearchHit>>(nothing.arguments().value(0)).isEmpty());
    }

    void resolve()
    {
        const QDBusMessage reply = settings(QStringLiteral("Resolve"), { QStringLiteral("tur.about#version") });
        QCOMPARE(reply.type(), QDBusMessage::ReplyMessage);
        const QJsonObject entry = QJsonDocument::fromJson(reply.arguments().value(0).toString().toUtf8()).object();
        QCOMPARE(entry.value(QStringLiteral("address")).toString(), QStringLiteral("tur.about#version"));
        QCOMPARE(entry.value(QStringLiteral("program")).toString(), programName);
        const QJsonObject page = entry.value(QStringLiteral("page")).toObject();
        QCOMPARE(page.value(QStringLiteral("page")).toString(), QStringLiteral("tur.about"));
        QCOMPARE(page.value(QStringLiteral("title")).toString(), QStringLiteral("About"));
        QVERIFY(!page.contains(QStringLiteral("rows")));
        const QJsonObject row = entry.value(QStringLiteral("row")).toObject();
        QCOMPARE(row.value(QStringLiteral("id")).toString(), QStringLiteral("version"));
        QCOMPARE(row.value(QStringLiteral("kind")).toString(), QStringLiteral("information"));
        QCOMPARE(row.value(QStringLiteral("source")).toObject().value(QStringLiteral("path")).toString(),
                 QStringLiteral("/usr/lib/tur/release"));

        // A page that is reserved resolves, and says that it is.
        const QJsonObject reserved =
            QJsonDocument::fromJson(settings(QStringLiteral("Resolve"), { QStringLiteral("tur.pax.privacy") })
                                        .arguments()
                                        .value(0)
                                        .toString()
                                        .toUtf8())
                .object();
        QCOMPARE(reserved.value(QStringLiteral("page")).toObject().value(QStringLiteral("status")).toString(),
                 QStringLiteral("planned"));
        QVERIFY(!reserved.contains(QStringLiteral("row")));

        // A page of KDE's resolves to its owner and to how it is opened.
        const QJsonObject kde =
            QJsonDocument::fromJson(settings(QStringLiteral("Resolve"), { QStringLiteral("kde.kcm_nightlight") })
                                        .arguments()
                                        .value(0)
                                        .toString()
                                        .toUtf8())
                .object();
        QCOMPARE(kde.value(QStringLiteral("program")).toString(), QStringLiteral("org.kde.systemsettings"));
        QCOMPARE(kde.value(QStringLiteral("page")).toObject().value(QStringLiteral("open")).toObject()
                     .value(QStringLiteral("module")).toString(),
                 QStringLiteral("kcm_nightlight"));
    }

    void list()
    {
        const QDBusMessage reply = call(programName, addressablePath, addressableInterface, QStringLiteral("List"));
        QCOMPARE(reply.type(), QDBusMessage::ReplyMessage);
        const QJsonDocument listed = QJsonDocument::fromJson(reply.arguments().value(0).toString().toUtf8());
        const QJsonDocument shipped =
            QJsonDocument::fromJson(fileContent(source + QStringLiteral("/registry/org.tur.Settings.json")));
        QVERIFY(shipped.isObject());
        QCOMPARE(listed, shipped);
    }

    // The heart of it: a row is asked for over the bus, and the rectangle in
    // the answer is where the row is drawn.
    void a_row_is_revealed_where_it_is_drawn_data()
    {
        QTest::addColumn<QString>("address");

        // At the top of its page.
        QTest::newRow("a row in sight") << "tur.overview#build";
        // With controls of its own, which cannot be used.
        QTest::newRow("a row with a control") << "tur.appearance#variant";
        // Below the window's edge, in a group that is closed.
        QTest::newRow("a row out of sight in a closed group") << "tur.about#source-revision";
        // With a control that takes the keyboard.
        QTest::newRow("a row that leads elsewhere") << "tur.appearance#night-light";
    }

    void a_row_is_revealed_where_it_is_drawn()
    {
        QFETCH(QString, address);
        const QString page = address.section(QLatin1Char('#'), 0, 0);

        // Without a highlight first, so that what is looked at is the row
        // and nothing drawn round it.
        startElsewhere(address);
        const QDBusMessage reply = reveal(address, { { QStringLiteral("highlight"), false } });
        QVERIFY2(reply.type() == QDBusMessage::ReplyMessage, qPrintable(reply.errorMessage()));
        QCOMPARE(reply.signature(), QStringLiteral("a{sv}"));
        const QVariantMap result = mapOf(reply);
        QCOMPARE(result.value(QStringLiteral("address")).toString(), address);
        QCOMPARE(result.value(QStringLiteral("page")).toString(), page);
        QCOMPARE(result.value(QStringLiteral("row")).toString(), address.section(QLatin1Char('#'), 1, 1));
        QCOMPARE(result.value(QStringLiteral("program")).toString(), programName);
        QCOMPARE(result.value(QStringLiteral("located")).toBool(), true);
        QCOMPARE(result.value(QStringLiteral("highlighted")).toBool(), false);
        QCOMPARE(result.value(QStringLiteral("focused")).toBool(), true);
        // What the overlay needs to find the window (PaxOverlay1.HighlightInWindow).
        QCOMPARE(result.value(QStringLiteral("pid")).toUInt(), uint(QCoreApplication::applicationPid()));
        QCOMPARE(result.value(QStringLiteral("app_id")).toString(), programName);
        QCOMPARE(result.value(QStringLiteral("window_title")).toString(), m_window->title());
        QCOMPARE(m_window->title(), QStringLiteral("Settings"));
        QCOMPARE(result.value(QStringLiteral("scale")).toDouble(), m_window->devicePixelRatio());
        const RectD rect = rectOf(result);
        m_rects.insert(address, rect);
        QCOMPARE(whereItIs(), page);

        // The row itself, and where it is once the window has drawn.
        QQuickItem *row = m_host->findRow(m_window->contentItem(), address);
        QVERIFY(row);
        QVERIFY(row->isVisible());
        drawn();
        const QRectF placed = row->mapRectToScene(QRectF(0, 0, row->width(), row->height()));
        QVERIFY2(sameRect(rect, RectD { placed.x(), placed.y(), placed.width(), placed.height() }),
                 qPrintable(QStringLiteral("reported %1; the row is at %2")
                                .arg(describe(rect),
                                     describe(RectD { placed.x(), placed.y(), placed.width(), placed.height() }))));
        // The row's control, where it has one: where the control is drawn,
        // and inside the row (what P@x's pointer points at).
        QQuickItem *control = row->property("focusTarget").value<QQuickItem *>();
        if (control && control->isVisible()) {
            QVERIFY2(result.contains(QStringLiteral("control")), "a row with a control says where it is");
            const RectD reportedControl = qdbus_cast<RectD>(result.value(QStringLiteral("control")));
            const QRectF drawnControl = control->mapRectToScene(QRectF(0, 0, control->width(), control->height()));
            QVERIFY2(sameRect(reportedControl,
                              RectD { drawnControl.x(), drawnControl.y(), drawnControl.width(), drawnControl.height() }),
                     qPrintable(describe(reportedControl)));
            QVERIFY(QRectF(rect.x, rect.y, rect.width, rect.height)
                        .contains(QRectF(reportedControl.x, reportedControl.y, reportedControl.width,
                                         reportedControl.height)));
        } else {
            QVERIFY2(!result.contains(QStringLiteral("control")), "a row without a control has no control rectangle");
        }

        // In the window, and in sight.
        QVERIFY(rect.width > 100 && rect.height >= 36);
        QVERIFY(rect.x >= 0 && rect.y >= 0);
        QVERIFY(rect.x + rect.width <= m_window->width());
        QVERIFY2(rect.y + rect.height <= m_window->height(), "the row was not brought into sight");

        // And in the picture. The pixels that belong to the row are the ones
        // that change when the row is made invisible. All of them lie inside
        // the rectangle, and they begin at its left and top edges: the
        // rectangle is the row's, not something near it.
        // The keyboard is put on the search field meanwhile: a focus ring,
        // round the row or round its control, reaches a few pixels outside
        // what it is round. (Taking the keyboard away from the row with
        // nowhere to go would give it to the card the row stands on, and the
        // page would scroll to show the whole card.)
        QQuickItem *field = itemNamed(m_window->contentItem(), QStringLiteral("shellSearch"));
        QVERIFY(field);
        field->forceActiveFocus();
        QVERIFY(field->hasActiveFocus());
        drawn();
        const QRectF still = row->mapRectToScene(QRectF(0, 0, row->width(), row->height()));
        QVERIFY2(sameRect(rect, RectD { still.x(), still.y(), still.width(), still.height() }),
                 "the row moved when the keyboard left it");
        const QImage with = settledPicture();
        row->setOpacity(0);
        const QImage without = settledPicture();
        row->setOpacity(1);
        const QRect changed = difference(with, without);
        const QRect reported = QRectF(rect.x, rect.y, rect.width, rect.height).toAlignedRect();
        const QString seen = QStringLiteral("the row's pixels are in (%1, %2, %3 x %4); reported %5")
                                 .arg(changed.x()).arg(changed.y()).arg(changed.width()).arg(changed.height())
                                 .arg(describe(rect));
        QVERIFY2(!changed.isEmpty(), "making the row invisible changed nothing in the picture");
        QVERIFY2(reported.contains(changed), qPrintable(seen));
        QVERIFY2(changed.left() - reported.left() <= 4, qPrintable(seen));
        QVERIFY2(changed.top() - reported.top() <= 20, qPrintable(seen));
        QVERIFY2(reported.bottom() - changed.bottom() <= 30, qPrintable(seen));
        QVERIFY2(changed.width() >= 40 && changed.height() >= 8, qPrintable(seen));

        // Now with the highlight, which is the default: the same rectangle,
        // and a line round it, 4 px clear and 3 px wide.
        const QVariantMap marked = mapOf(reveal(address));
        QCOMPARE(marked.value(QStringLiteral("highlighted")).toBool(), true);
        QVERIFY2(sameRect(rectOf(marked), rect), qPrintable(describe(rectOf(marked))));
        QCOMPARE(whereItIs(), address);
        drawn();
        QQuickItem *mark = itemNamed(row, QStringLiteral("rowHighlight"));
        QVERIFY(mark);
        QVERIFY(mark->isVisible());
        const QColor blue = QQmlProperty(mark, QStringLiteral("border.color")).read().value<QColor>();
        QVERIFY(blue.isValid());
        const int left = reported.left();
        const int right = reported.left() + reported.width(); // the first pixel after the row
        const int middle = reported.top() + reported.height() / 2;
        const int centre = reported.left() + reported.width() / 2;
        const int bottom = reported.top() + reported.height();
        QImage picture = m_window->grabWindow();
        QCOMPARE(picture.pixelColor(left - 6, middle), blue);
        QCOMPARE(picture.pixelColor(right + 5, middle), blue);
        QCOMPARE(picture.pixelColor(centre, reported.top() - 6), blue);
        QCOMPARE(picture.pixelColor(centre, bottom + 5), blue);
        // Between the line and the row there is no line.
        QVERIFY(picture.pixelColor(left - 2, middle) != blue);
        QVERIFY(picture.pixelColor(right + 1, middle) != blue);
        // The line was not there before.
        QVERIFY(with.pixelColor(left - 6, middle) != blue);

        // Asked for by the assistant, the line is P@x's gold.
        const QVariantMap byPax = mapOf(reveal(address, { { QStringLiteral("pax"), true } }));
        QCOMPARE(byPax.value(QStringLiteral("highlighted")).toBool(), true);
        // A row that is asked for again stays where it is.
        QVERIFY2(sameRect(rectOf(byPax), rect), qPrintable(describe(rectOf(byPax))));
        drawn();
        const QColor gold = QQmlProperty(mark, QStringLiteral("border.color")).read().value<QColor>();
        QVERIFY(gold != blue);
        // Gold: more red than blue in it; the system's colour is the reverse.
        QVERIFY2(gold.red() > gold.blue() && blue.blue() > blue.red(),
                 qPrintable(gold.name() + QStringLiteral(" and ") + blue.name()));
        picture = m_window->grabWindow();
        QCOMPARE(picture.pixelColor(left - 6, middle), gold);
        QCOMPARE(picture.pixelColor(centre, bottom + 5), gold);

        // Cleared, it is gone, and the window is at the page.
        QCOMPARE(settings(QStringLiteral("ClearHighlight")).type(), QDBusMessage::ReplyMessage);
        drawn();
        QVERIFY(!mark->isVisible());
        picture = m_window->grabWindow();
        QVERIFY(picture.pixelColor(left - 6, middle) != gold);
        QVERIFY(picture.pixelColor(left - 6, middle) != blue);
        QCOMPARE(whereItIs(), page);
    }

    void the_same_through_addressable()
    {
        const QString address = QStringLiteral("tur.appearance#variant");
        startElsewhere(address);
        const QDBusMessage reply = call(programName, addressablePath, addressableInterface, QStringLiteral("Reveal"),
                                        { address, QVariantMap { { QStringLiteral("dim"), true } } });
        QVERIFY2(reply.type() == QDBusMessage::ReplyMessage, qPrintable(reply.errorMessage()));
        const QVariantMap result = mapOf(reply);
        QVERIFY2(sameRect(rectOf(result), m_rects.value(address)), qPrintable(describe(rectOf(result))));
        QCOMPARE(result.value(QStringLiteral("highlighted")).toBool(), true);
        QCOMPARE(whereItIs(), address);
        drawn();
        QQuickItem *shade = itemNamed(m_window->contentItem(), QStringLiteral("shellShade"));
        QVERIFY(shade);
        QVERIFY(shade->isVisible());

        QCOMPARE(call(programName, addressablePath, addressableInterface, QStringLiteral("ClearHighlight")).type(),
                 QDBusMessage::ReplyMessage);
        QVERIFY(!shade->isVisible());
        QCOMPARE(whereItIs(), QStringLiteral("tur.appearance"));

        // It speaks for this program's pages only: a page of KDE's is asked
        // of org.tur.Settings1, and nothing is opened here.
        const QDBusMessage notMine = call(programName, addressablePath, addressableInterface, QStringLiteral("Reveal"),
                                          { QStringLiteral("kde.kcm_colors"), QVariantMap() });
        QCOMPARE(notMine.type(), QDBusMessage::ErrorMessage);
        QCOMPARE(notMine.errorName(), QStringLiteral("org.tur.Settings1.Error.UnknownAddress"));
        QVERIFY(!QFile::exists(m_helperOut));
    }

    void a_page_is_opened()
    {
        QVERIFY(reveal(QStringLiteral("tur.about#licence")).type() == QDBusMessage::ReplyMessage);
        const QDBusMessage reply = settings(QStringLiteral("Open"), { QStringLiteral("tur.appearance") });
        QCOMPARE(reply.type(), QDBusMessage::ReplyMessage);
        QVERIFY(reply.arguments().isEmpty());
        QCOMPARE(whereItIs(), QStringLiteral("tur.appearance"));

        // A row, opened: its page, and no highlight.
        QCOMPARE(settings(QStringLiteral("Open"), { QStringLiteral("tur.about#base") }).type(), QDBusMessage::ReplyMessage);
        QCOMPARE(whereItIs(), QStringLiteral("tur.about"));

        // Nothing to go to: the window is only asked to come forward.
        QCOMPARE(settings(QStringLiteral("Open"), { QString() }).type(), QDBusMessage::ReplyMessage);
        QCOMPARE(whereItIs(), QStringLiteral("tur.about"));

        // Revealing a page says where the page is shown.
        const QVariantMap result = mapOf(reveal(QStringLiteral("tur.overview")));
        QCOMPARE(result.value(QStringLiteral("row")).toString(), QString());
        QCOMPARE(result.value(QStringLiteral("highlighted")).toBool(), false);
        QQuickItem *content = itemNamed(m_window->contentItem(), QStringLiteral("shellContent"));
        QVERIFY(content);
        drawn();
        const QRectF area = content->mapRectToScene(QRectF(0, 0, content->width(), content->height()));
        QVERIFY2(sameRect(rectOf(result), RectD { area.x(), area.y(), area.width(), area.height() }),
                 qPrintable(describe(rectOf(result))));
    }

    void what_cannot_be_shown_is_an_error_with_its_code_data()
    {
        QTest::addColumn<QString>("address");
        QTest::addColumn<QString>("error");
        QTest::addColumn<QString>("code");

        QTest::newRow("not an address") << "Appearance" << "NotAnAddress" << "TUR-DESKTOP-SETTINGS-004";
        QTest::newRow("empty") << "" << "NotAnAddress" << "TUR-DESKTOP-SETTINGS-004";
        QTest::newRow("a page nobody has") << "tur.nothing" << "UnknownAddress" << "TUR-DESKTOP-SETTINGS-001";
        QTest::newRow("a row its page does not have") << "tur.about#nothing" << "UnknownAddress"
                                                      << "TUR-DESKTOP-SETTINGS-001";
        QTest::newRow("a page that is reserved") << "tur.pax.privacy" << "PageNotAvailable" << "TUR-DESKTOP-SETTINGS-003";
        QTest::newRow("a row of a page that is reserved") << "tur.pax.privacy#private-mode" << "PageNotAvailable"
                                                          << "TUR-DESKTOP-SETTINGS-003";
    }

    void what_cannot_be_shown_is_an_error_with_its_code()
    {
        QFETCH(QString, address);
        QFETCH(QString, error);
        QFETCH(QString, code);
        QCOMPARE(settings(QStringLiteral("Open"), { QStringLiteral("tur.appearance") }).type(), QDBusMessage::ReplyMessage);

        QList<QDBusMessage> replies = { reveal(address),
                                        settings(QStringLiteral("Resolve"), { address }),
                                        call(programName, addressablePath, addressableInterface, QStringLiteral("Reveal"),
                                             { address, QVariantMap() }) };
        if (!address.isEmpty())
            replies.append(settings(QStringLiteral("Open"), { address }));
        // A page that is reserved does resolve: its entry says so.
        if (error == QLatin1String("PageNotAvailable"))
            replies.removeAt(1);
        for (const QDBusMessage &reply : std::as_const(replies)) {
            QCOMPARE(reply.type(), QDBusMessage::ErrorMessage);
            QCOMPARE(reply.errorName(), QStringLiteral("org.tur.Settings1.Error.") + error);
            // The code, a colon, and a sentence.
            QVERIFY2(reply.errorMessage().startsWith(code + QStringLiteral(": ")), qPrintable(reply.errorMessage()));
            QVERIFY(reply.errorMessage().size() > code.size() + 10);
        }
        // The window stayed where it was.
        QCOMPARE(whereItIs(), QStringLiteral("tur.appearance"));
    }

    // A row's state changes: the bus hears of it.
    void a_change_of_a_row_is_signalled()
    {
        QVERIFY(m_client->connect(programName, addressablePath, addressableInterface, QStringLiteral("RowChanged"), this,
                                  SLOT(rowChanged(QString))));
        QCOMPARE(settings(QStringLiteral("Open"), { QStringLiteral("tur.appearance") }).type(), QDBusMessage::ReplyMessage);
        m_rowChanges.clear();

        // The window is put in the other variant: the row that says what the
        // desktop is set to now also says that the window does not follow it.
        QQmlExpression lighten(qmlContext(m_window), m_window, QStringLiteral("Kit.Theme.light = true"));
        lighten.evaluate();
        QVERIFY2(!lighten.hasError(), qPrintable(lighten.error().toString()));
        QTRY_VERIFY_WITH_TIMEOUT(m_rowChanges.contains(QStringLiteral("tur.appearance#current")), 10000);
        // Once, and no other row of the page claimed to have changed.
        QTest::qWait(200);
        QCOMPARE(m_rowChanges, QStringList({ QStringLiteral("tur.appearance#current") }));

        m_rowChanges.clear();
        QQmlExpression darken(qmlContext(m_window), m_window, QStringLiteral("Kit.Theme.light = false"));
        darken.evaluate();
        QVERIFY2(!darken.hasError(), qPrintable(darken.error().toString()));
        QTRY_VERIFY_WITH_TIMEOUT(m_rowChanges.contains(QStringLiteral("tur.appearance#current")), 10000);
        QVERIFY(m_client->disconnect(programName, addressablePath, addressableInterface, QStringLiteral("RowChanged"),
                                     this, SLOT(rowChanged(QString))));
    }

    // A page of KDE System Settings is opened by starting System Settings at
    // its module. A row of one cannot be pointed at yet, and the answer
    // says so.
    void a_page_of_system_settings_is_opened_there()
    {
        QFile::remove(m_helperOut);
        QCOMPARE(settings(QStringLiteral("Open"), { QStringLiteral("kde.kcm_colors") }).type(), QDBusMessage::ReplyMessage);
        QCOMPARE(helperArguments(), QJsonArray({ QStringLiteral("kcm_colors") }));

        const QDBusMessage reply = reveal(QStringLiteral("kde.kcm_nightlight"));
        QVERIFY2(reply.type() == QDBusMessage::ReplyMessage, qPrintable(reply.errorMessage()));
        const QVariantMap result = mapOf(reply);
        QCOMPARE(helperArguments(), QJsonArray({ QStringLiteral("kcm_nightlight") }));
        QCOMPARE(result.value(QStringLiteral("address")).toString(), QStringLiteral("kde.kcm_nightlight"));
        QCOMPARE(result.value(QStringLiteral("program")).toString(), QStringLiteral("org.kde.systemsettings"));
        QCOMPARE(result.value(QStringLiteral("located")).toBool(), false);
        QCOMPARE(result.value(QStringLiteral("highlighted")).toBool(), false);
        // No rectangle is made up.
        QVERIFY(!result.contains(QStringLiteral("rect")));
        // The Settings window did not move.
        QCOMPARE(whereItIs(), QStringLiteral("tur.appearance"));

        // System Settings is not there: an error, with the code.
        m_host->setSystemSettingsProgram(QStringLiteral("/nonexistent/systemsettings"));
        const QDBusMessage missing = settings(QStringLiteral("Open"), { QStringLiteral("kde.kcm_fonts") });
        m_host->setSystemSettingsProgram(helper);
        QCOMPARE(missing.type(), QDBusMessage::ErrorMessage);
        QCOMPARE(missing.errorName(), QStringLiteral("org.tur.Settings1.Error.OwnerUnreachable"));
        QVERIFY2(missing.errorMessage().startsWith(QLatin1String("TUR-DESKTOP-SETTINGS-005: ")),
                 qPrintable(missing.errorMessage()));
    }

    // --- 2. The program itself -----------------------------------------------------

    void the_program_takes_the_name_when_started()
    {
        // The window in this process goes, and its names with it.
        m_engine.reset();
        m_window = nullptr;
        m_host = nullptr;
        QTRY_VERIFY_WITH_TIMEOUT(!owned(settingsName) && !owned(programName), 10000);

        m_settings.setProcessChannelMode(QProcess::ForwardedChannels);
        m_settings.start(program, { QStringLiteral("--standalone"), QStringLiteral("--fixture"),
                                    fixtures + QStringLiteral("/default"), QStringLiteral("--system-settings-program"),
                                    helper, QStringLiteral("--registry"), testData + QStringLiteral("/registry") });
        QVERIFY(m_settings.waitForStarted(15000));
        QTRY_VERIFY_WITH_TIMEOUT(owned(settingsName) && owned(programName), 60000);
        QCOMPARE(m_client->interface()->servicePid(settingsName).value(), uint(m_settings.processId()));
        QCOMPARE(whereItIs(), QStringLiteral("tur.overview"));
    }

    // The program, asked over the bus, puts its rows where the window in the
    // test's process put them: the same program, the same fixture, the same
    // size of window. So what was seen in the picture there holds here.
    void the_program_reports_the_same_rectangles_data()
    {
        QTest::addColumn<QString>("address");
        for (auto it = m_rects.constBegin(); it != m_rects.constEnd(); ++it)
            QTest::newRow(qPrintable(it.key())) << it.key();
    }

    void the_program_reports_the_same_rectangles()
    {
        QFETCH(QString, address);
        startElsewhere(address);
        const QDBusMessage reply = reveal(address);
        QVERIFY2(reply.type() == QDBusMessage::ReplyMessage, qPrintable(reply.errorMessage()));
        const QVariantMap result = mapOf(reply);
        QVERIFY2(sameRect(rectOf(result), m_rects.value(address)),
                 qPrintable(QStringLiteral("the program says %1; the window in the test said %2")
                                .arg(describe(rectOf(result)), describe(m_rects.value(address)))));
        QCOMPARE(result.value(QStringLiteral("highlighted")).toBool(), true);
        QCOMPARE(result.value(QStringLiteral("pid")).toUInt(), uint(m_settings.processId()));
        // The identifier the program gives its window: its desktop file's.
        QCOMPARE(result.value(QStringLiteral("app_id")).toString(), programName);
        QCOMPARE(result.value(QStringLiteral("window_title")).toString(), QStringLiteral("Settings"));
        QCOMPARE(whereItIs(), address);
    }

    // The command line and the link: a second start hands its address to the
    // window that is open, and ends.
    void a_second_start_goes_to_the_window_that_is_open_data()
    {
        QTest::addColumn<QString>("argument");
        QTest::addColumn<int>("status");
        QTest::addColumn<QString>("where");
        QTest::addColumn<QString>("says");

        QTest::newRow("a row") << "tur.about#licence" << 0 << "tur.about#licence" << "";
        QTest::newRow("a page") << "tur.appearance" << 0 << "tur.appearance" << "";
        QTest::newRow("a link to a row") << "tur-settings:tur.appearance#fonts" << 0 << "tur.appearance#fonts" << "";
        QTest::newRow("a link to a page") << "tur-settings:tur.overview" << 0 << "tur.overview" << "";
        QTest::newRow("nothing") << "" << 0 << "tur.overview" << "";
        QTest::newRow("an address nobody has") << "tur.nothing#at-all" << 1 << "tur.overview"
                                               << "TUR-DESKTOP-SETTINGS-001: ";
        QTest::newRow("a page that is reserved") << "tur.pax.privacy" << 1 << "tur.overview"
                                                 << "TUR-DESKTOP-SETTINGS-003: ";
        QTest::newRow("not an address") << "Appearance" << 1 << "tur.overview" << "TUR-DESKTOP-SETTINGS-004: ";
    }

    void a_second_start_goes_to_the_window_that_is_open()
    {
        QFETCH(QString, argument);
        QFETCH(int, status);
        QFETCH(QString, where);
        QFETCH(QString, says);

        QProcess second;
        QStringList arguments = { QStringLiteral("--standalone") };
        if (!argument.isEmpty())
            arguments.append(argument);
        second.start(program, arguments);
        QVERIFY(second.waitForStarted(15000));
        // It does not become a window: it ends by itself.
        QVERIFY2(second.waitForFinished(60000), "the second start did not end: it opened a window of its own");
        QCOMPARE(second.exitStatus(), QProcess::NormalExit);
        const QString said = QString::fromUtf8(second.readAllStandardError());
        QVERIFY2(second.exitCode() == status, qPrintable(said));
        if (!says.isEmpty())
            QVERIFY2(said.contains(says), qPrintable(said));
        QCOMPARE(whereItIs(), where);
        // Still one window: the name is where it was.
        QCOMPARE(m_client->interface()->servicePid(settingsName).value(), uint(m_settings.processId()));
    }

    // A caller that is not Qt: systemd's busctl.
    void a_caller_that_is_not_qt()
    {
        const QString address = QStringLiteral("tur.appearance#variant");
        startElsewhere(address);
        QProcess busctl;
        busctl.start(QStringLiteral("busctl"),
                     { QStringLiteral("--address=") + m_address, QStringLiteral("--json=short"), QStringLiteral("call"),
                       settingsName, settingsPath, settingsName, QStringLiteral("Reveal"), QStringLiteral("sa{sv}"),
                       address, QStringLiteral("2"), QStringLiteral("highlight"), QStringLiteral("b"),
                       QStringLiteral("true"), QStringLiteral("duration"), QStringLiteral("d"), QStringLiteral("30") });
        QVERIFY2(busctl.waitForStarted(15000), "busctl could not be started");
        QVERIFY(busctl.waitForFinished(60000));
        const QByteArray printed = busctl.readAllStandardOutput();
        QVERIFY2(busctl.exitCode() == 0, busctl.readAllStandardError().constData());

        // {"type":"a{sv}","data":[{"address":{"type":"s","data":"..."},"rect":{"type":"(dddd)","data":[...]},...}]}
        const QJsonObject answer = QJsonDocument::fromJson(printed).object();
        QCOMPARE(answer.value(QStringLiteral("type")).toString(), QStringLiteral("a{sv}"));
        const QJsonObject result = answer.value(QStringLiteral("data")).toArray().at(0).toObject();
        QVERIFY2(!result.isEmpty(), printed.constData());
        QCOMPARE(result.value(QStringLiteral("address")).toObject().value(QStringLiteral("data")).toString(), address);
        QCOMPARE(result.value(QStringLiteral("pid")).toObject().value(QStringLiteral("type")).toString(),
                 QStringLiteral("u"));
        const QJsonObject rect = result.value(QStringLiteral("rect")).toObject();
        QCOMPARE(rect.value(QStringLiteral("type")).toString(), QStringLiteral("(dddd)"));
        const QJsonArray numbers = rect.value(QStringLiteral("data")).toArray();
        QCOMPARE(numbers.size(), 4);
        const RectD seen { numbers.at(0).toDouble(), numbers.at(1).toDouble(), numbers.at(2).toDouble(),
                           numbers.at(3).toDouble() };
        QVERIFY2(sameRect(seen, m_rects.value(address)), printed.constData());
        QCOMPARE(whereItIs(), address);
    }

    // An address of another program: the call is passed on to the program
    // that owns it, and its answer comes back.
    void an_address_of_another_program_is_passed_on()
    {
        const QString address = QStringLiteral("tur.example#thing");

        // The program is not running, and nothing can start it.
        const QDBusMessage alone = reveal(address);
        QCOMPARE(alone.type(), QDBusMessage::ErrorMessage);
        QCOMPARE(alone.errorName(), QStringLiteral("org.tur.Settings1.Error.OwnerUnreachable"));
        QVERIFY2(alone.errorMessage().startsWith(QLatin1String("TUR-DESKTOP-SETTINGS-005: ")),
                 qPrintable(alone.errorMessage()));
        QVERIFY2(alone.errorMessage().contains(QLatin1String("org.tur.Example")), qPrintable(alone.errorMessage()));

        m_peer.setProcessChannelMode(QProcess::ForwardedChannels);
        m_peer.start(peer, QStringList());
        QVERIFY(m_peer.waitForStarted(15000));
        QTRY_VERIFY_WITH_TIMEOUT(owned(QStringLiteral("org.tur.Example")), 30000);

        const QDBusMessage reply = reveal(address, { { QStringLiteral("highlight"), true } });
        QVERIFY2(reply.type() == QDBusMessage::ReplyMessage, qPrintable(reply.errorMessage()));
        const QVariantMap result = mapOf(reply);
        // The other program's answer, as it gave it.
        QCOMPARE(result.value(QStringLiteral("address")).toString(), address);
        QCOMPARE(result.value(QStringLiteral("program")).toString(), QStringLiteral("org.tur.Example"));
        QCOMPARE(result.value(QStringLiteral("pid")).toUInt(), uint(m_peer.processId()));
        QVERIFY2(sameRect(rectOf(result), RectD { 1, 2, 3, 4 }), qPrintable(describe(rectOf(result))));
        QCOMPARE(result.value(QStringLiteral("highlighted")).toBool(), true);

        // Open asks for no highlight, and gives nothing back.
        const QDBusMessage opened = settings(QStringLiteral("Open"), { QStringLiteral("tur.example") });
        QVERIFY2(opened.type() == QDBusMessage::ReplyMessage, qPrintable(opened.errorMessage()));
        QVERIFY(opened.arguments().isEmpty());

        // The registry of the other program is searched with this one's.
        const auto hits = qdbus_cast<QList<SearchHit>>(
            settings(QStringLiteral("Search"), { QStringLiteral("widget"), uint(5) }).arguments().value(0));
        QCOMPARE(hits.size(), 1);
        QCOMPARE(hits.constFirst().address, address);

        // What was removed is refused here, with its successor.
        const QDBusMessage removed = reveal(QStringLiteral("tur.example#old-thing"));
        QCOMPARE(removed.errorName(), QStringLiteral("org.tur.Settings1.Error.RemovedAddress"));
        QVERIFY2(removed.errorMessage().startsWith(QLatin1String("TUR-DESKTOP-SETTINGS-002: ")),
                 qPrintable(removed.errorMessage()));
        QVERIFY(removed.errorMessage().contains(QLatin1String("tur.example#thing")));
        // And it still resolves, to an entry that says it was removed.
        const QJsonObject entry =
            QJsonDocument::fromJson(settings(QStringLiteral("Resolve"), { QStringLiteral("tur.example#old-thing") })
                                        .arguments()
                                        .value(0)
                                        .toString()
                                        .toUtf8())
                .object();
        QCOMPARE(entry.value(QStringLiteral("program")).toString(), QStringLiteral("org.tur.Example"));
        QCOMPARE(entry.value(QStringLiteral("row")).toObject().value(QStringLiteral("replaced_by")).toString(), address);

        stop(m_peer);
    }

    // --- 3. Started by the bus -------------------------------------------------------

    void the_bus_starts_the_program_when_it_is_called()
    {
        stop(m_settings);
        QTRY_VERIFY_WITH_TIMEOUT(!owned(settingsName), 15000);

        // No Settings window is open. A call starts one, from the service
        // file, and is answered by it.
        const QDBusMessage reply = reveal(QStringLiteral("tur.overview#build"));
        QVERIFY2(reply.type() == QDBusMessage::ReplyMessage, qPrintable(reply.errorMessage()));
        QVERIFY(owned(settingsName));
        m_activated = m_client->interface()->servicePid(settingsName).value();
        QVERIFY(m_activated > 0);
        QVERIFY(m_activated != uint(QCoreApplication::applicationPid()));

        const QVariantMap result = mapOf(reply);
        QCOMPARE(result.value(QStringLiteral("pid")).toUInt(), m_activated);
        QCOMPARE(result.value(QStringLiteral("highlighted")).toBool(), true);
        // Asked the moment it started, before it had drawn anything, it put
        // the row where the others did.
        const QString address = QStringLiteral("tur.overview#build");
        QVERIFY2(sameRect(rectOf(result), m_rects.value(address)),
                 qPrintable(QStringLiteral("the started program says %1; the window in the test said %2")
                                .arg(describe(rectOf(result)), describe(m_rects.value(address)))));
        QCOMPARE(whereItIs(), address);

        ::kill(pid_t(m_activated), SIGTERM);
        QTRY_VERIFY_WITH_TIMEOUT(!owned(settingsName), 15000);
        m_activated = 0;
    }

private:
    QTemporaryDir m_dir;
    QString m_helperOut;
    QString m_address;
    QProcess m_daemon;
    QProcess m_settings;
    QProcess m_peer;
    uint m_activated = 0;
    std::optional<QDBusConnection> m_client;
    std::unique_ptr<QQmlApplicationEngine> m_engine;
    QQuickWindow *m_window = nullptr;
    SettingsHost *m_host = nullptr;
    QStringList m_rowChanges;
    QMap<QString, RectD> m_rects;
};

QTEST_MAIN(TestBus)

#include "tst_bus.moc"
