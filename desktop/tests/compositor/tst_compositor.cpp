// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
//
// TUR's desktop programs under a real KWin (desktop-experience.md, 15.3 and
// 15.4; docs/architecture/tur-pax-overlay.md, section 9).
//
// desktop/ci/compositor.sh starts kwin_wayland headless (the virtual backend)
// and a private session bus, then this program, which starts tur-settings
// and tur-pax-overlay as Wayland clients and checks what only a compositor
// can show:
//
//  - KWin's view of the settings window: its application identifier, and
//    whether Reveal brings it forward.
//  - The overlay has a surface on every output, at every scale.
//  - The chain: search a setting, reveal it, P@x's pointer travels to the
//    row, the highlight appears round it. Where the pointer and the
//    highlight are is measured in screenshots KWin takes, against where
//    Reveal said the row is plus where KWin says the window is.
//  - Every pixel lets input through: a click injected where P@x's pointer
//    and highlight are drawn reaches the button under them.
//  - The overlay never takes the keyboard: keys typed while it is shown
//    reach the window that had them, which never loses focus.
//  - It disappears when told to, when its owner leaves, and when it exits.
//
// Input is injected with KWin's fake-input protocol (org_kde_kwin_fake_input,
// which KWin 6.8 offers to any client that is not sandboxed, and whose
// authentication is a no-op). Screenshots come from KWin's
// org.kde.KWin.ScreenShot2 D-Bus interface. Both are read with KWayland.
//
// Every screenshot is written to TUR_COMPOSITOR_OUT, named after the
// arrangement (TUR_COMPOSITOR_LABEL), for the job's artifact.

#include "bustypes.h"
#include "probewindow.h"

#include <KWayland/Client/connection_thread.h>
#include <KWayland/Client/fakeinput.h>
#include <KWayland/Client/plasmawindowmanagement.h>
#include <KWayland/Client/registry.h>

#include <QColor>
#include <QCoreApplication>
#include <QDBusConnection>
#include <QDBusConnectionInterface>
#include <QDBusMessage>
#include <QDBusPendingCall>
#include <QDBusUnixFileDescriptor>
#include <QDBusVariant>
#include <QDir>
#include <QElapsedTimer>
#include <QFile>
#include <QGuiApplication>
#include <QImage>
#include <QJsonArray>
#include <QJsonDocument>
#include <QList>
#include <QMap>
#include <QProcess>
#include <QProcessEnvironment>
#include <QScreen>
#include <QStringList>
#include <QTest>
#include <QTextStream>
#include <QXmlStreamReader>

#include <linux/input-event-codes.h>

#include <algorithm>
#include <cerrno>
#include <cmath>
#include <csignal>
#include <cstdlib>
#include <functional>
#include <optional>
#include <fcntl.h>
#include <unistd.h>

using namespace KWayland::Client;
using PaxOverlay::RectD;

namespace {

const QString overlayService = QStringLiteral("org.tur.PaxOverlay1");
const QString overlayPath = QStringLiteral("/org/tur/PaxOverlay1");
const QString settingsService = QStringLiteral("org.tur.Settings1");
const QString settingsPath = QStringLiteral("/org/tur/Settings1");

// P@x's gold, as the overlay draws it, and its dark line. Close enough to
// allow for blending at the edge of a line, and no closer: the system's
// warning orange (#FF9A52) is not gold.
bool isGold(const QColor &colour)
{
    return std::abs(colour.red() - 0xE8) <= 24 && std::abs(colour.green() - 0xB4) <= 24
           && std::abs(colour.blue() - 0x4C) <= 24;
}

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

QString describe(const QRectF &rect)
{
    return QStringLiteral("(%1, %2, %3 x %4)").arg(rect.x()).arg(rect.y()).arg(rect.width()).arg(rect.height());
}

void report(const QString &line)
{
    QTextStream(stdout) << "TUR-KWIN " << line << '\n';
}

// --owner: a process of its own that takes a lease, shows P@x as acting, says
// "ready" and waits to be killed. Its death is the owner leaving the bus.
int ownerMain(int argc, char *argv[])
{
    QCoreApplication app(argc, argv);
    QDBusConnection bus = QDBusConnection::sessionBus();
    QDBusMessage acquire = QDBusMessage::createMethodCall(overlayService, overlayPath, overlayService,
                                                          QStringLiteral("Acquire"));
    acquire << QStringLiteral("a process that will be killed");
    const QDBusMessage lease = bus.call(acquire, QDBus::Block, 20000);
    if (lease.type() != QDBusMessage::ReplyMessage)
        return 2;
    QDBusMessage acting = QDBusMessage::createMethodCall(overlayService, overlayPath, overlayService,
                                                         QStringLiteral("SetActivity"));
    acting << lease.arguments().value(0).toString() << QStringLiteral("acting")
           << QVariantMap { { QStringLiteral("scope"), QStringLiteral("screen") } };
    if (bus.call(acting, QDBus::Block, 20000).type() != QDBusMessage::ReplyMessage)
        return 3;
    QTextStream(stdout) << "ready" << Qt::endl;
    return app.exec();
}

} // namespace

class CompositorTest : public QObject
{
    Q_OBJECT

private:
    QString m_out;
    QString m_label;
    QString m_variant;
    QString m_helperOut;
    ConnectionThread *m_connection = nullptr;
    Registry *m_registry = nullptr;
    PlasmaWindowManagement *m_windows = nullptr;
    FakeInput *m_input = nullptr;
    QProcess m_settings;
    QProcess m_overlay;
    QString m_lease;
    int m_arrivals = 0;
    int m_shots = 0;
    // Gold pixels on each output before the overlay drew anything: KWin's
    // decoration shows Wayland's generic window icon, which is gold.
    QMap<QString, int> m_baseline;

    static QDBusConnection bus() { return QDBusConnection::sessionBus(); }

    QDBusMessage overlay(const QString &method, const QVariantList &arguments = {})
    {
        QDBusMessage message = QDBusMessage::createMethodCall(overlayService, overlayPath, overlayService, method);
        message.setArguments(arguments);
        return bus().call(message, QDBus::Block, 20000);
    }

    QDBusMessage settings(const QString &method, const QVariantList &arguments = {})
    {
        QDBusMessage message = QDBusMessage::createMethodCall(settingsService, settingsPath, settingsService, method);
        message.setArguments(arguments);
        return bus().call(message, QDBus::Block, 30000);
    }

    QVariant overlayProperty(const QString &name)
    {
        QDBusMessage get = QDBusMessage::createMethodCall(overlayService, overlayPath,
                                                          QStringLiteral("org.freedesktop.DBus.Properties"),
                                                          QStringLiteral("Get"));
        get << overlayService << name;
        const QDBusMessage reply = bus().call(get, QDBus::Block, 20000);
        if (reply.type() != QDBusMessage::ReplyMessage)
            return QVariant();
        return reply.arguments().value(0).value<QDBusVariant>().variant();
    }

    bool shownBecomes(bool wanted)
    {
        return QTest::qWaitFor([&] { return overlayProperty(QStringLiteral("Shown")).toBool() == wanted; }, 20000);
    }

    bool arrives(int before)
    {
        return QTest::qWaitFor([&] { return m_arrivals > before; }, 10000);
    }

    PlasmaWindow *windowOf(qint64 pid, const QString &title = QString()) const
    {
        if (!m_windows)
            return nullptr;
        const auto windows = m_windows->windows();
        for (PlasmaWindow *window : windows) {
            if (window->isValid() && window->pid() == quint32(pid) && (title.isEmpty() || window->title() == title))
                return window;
        }
        return nullptr;
    }

    QScreen *screenAt(const QPointF &point) const
    {
        const auto screens = QGuiApplication::screens();
        for (QScreen *screen : screens) {
            if (QRectF(screen->geometry()).contains(point))
                return screen;
        }
        return QGuiApplication::primaryScreen();
    }

    // A screenshot of one output, from KWin, at the output's own resolution.
    QImage capture(QScreen *screen)
    {
        int fds[2];
        if (::pipe2(fds, O_CLOEXEC) != 0)
            return QImage();
        QDBusPendingCall pending = [&] {
            QDBusMessage message = QDBusMessage::createMethodCall(
                QStringLiteral("org.kde.KWin"), QStringLiteral("/org/kde/KWin/ScreenShot2"),
                QStringLiteral("org.kde.KWin.ScreenShot2"), QStringLiteral("CaptureScreen"));
            message << screen->name()
                    << QVariantMap { { QStringLiteral("native-resolution"), true },
                                     { QStringLiteral("include-cursor"), false },
                                     { QStringLiteral("hide-caller-windows"), false } }
                    << QVariant::fromValue(QDBusUnixFileDescriptor(fds[1]));
            return bus().asyncCall(message, 30000);
        }();
        ::close(fds[1]);
        pending.waitForFinished();
        const QDBusMessage reply = pending.reply();
        if (reply.type() != QDBusMessage::ReplyMessage) {
            qWarning() << "ScreenShot2 refused:" << reply.errorName() << reply.errorMessage();
            ::close(fds[0]);
            return QImage();
        }
        // a{sv} arrives as a QDBusArgument from an asynchronous call.
        const QVariantMap results = qdbus_cast<QVariantMap>(reply.arguments().value(0));
        const int width = int(results.value(QStringLiteral("width")).toUInt());
        const int height = int(results.value(QStringLiteral("height")).toUInt());
        const int stride = int(results.value(QStringLiteral("stride")).toUInt());
        const auto format = QImage::Format(results.value(QStringLiteral("format")).toUInt());
        QByteArray data;
        data.resize(qsizetype(stride) * height);
        qsizetype got = 0;
        while (got < data.size()) {
            const ssize_t n = ::read(fds[0], data.data() + got, size_t(data.size() - got));
            if (n > 0)
                got += n;
            else if (n < 0 && errno == EINTR)
                continue;
            else
                break;
        }
        ::close(fds[0]);
        if (width <= 0 || height <= 0 || got != data.size()) {
            qWarning() << "ScreenShot2 gave" << results << "and" << got << "bytes";
            return QImage();
        }
        return QImage(reinterpret_cast<const uchar *>(data.constData()), width, height, stride, format)
            .convertToFormat(QImage::Format_RGB32);
    }

    // Screenshots until one shows what `drawn` looks for, or 10 s have
    // passed: the overlay draws its next frame in its own time, and at a
    // fractional scale in software that takes a while.
    QImage captureUntil(QScreen *screen, const std::function<bool(const QImage &)> &drawn)
    {
        QImage picture;
        QElapsedTimer clock;
        clock.start();
        do {
            picture = capture(screen);
            if (!picture.isNull() && drawn(picture))
                return picture;
            QTest::qWait(200);
        } while (clock.elapsed() < 10000);
        return picture;
    }

    // Keeps a screenshot for the artifact and the report.
    void keep(const QImage &image, const QString &what)
    {
        const QString name = QStringLiteral("%1-%2-%3.png").arg(m_label).arg(++m_shots, 2, 10, QLatin1Char('0')).arg(what);
        image.save(m_out + QLatin1Char('/') + name);
        report(QStringLiteral("screenshot %1 (%2 x %3)").arg(name).arg(image.width()).arg(image.height()));
    }

    // A point of the global layout, in the picture of `screen`.
    static QPoint inPicture(QScreen *screen, const QImage &image, const QPointF &global)
    {
        const double scale = double(image.width()) / screen->geometry().width();
        const QPointF local = global - QPointF(screen->geometry().topLeft());
        return QPoint(int(std::floor(local.x() * scale)), int(std::floor(local.y() * scale)));
    }

    static QColor colourAt(QScreen *screen, const QImage &image, const QPointF &global)
    {
        const QPoint at = inPicture(screen, image, global);
        if (!image.rect().contains(at))
            return QColor();
        return image.pixelColor(at);
    }

    // The share of the pixels in `area` (global, logical) that are gold.
    static double goldShare(QScreen *screen, const QImage &image, const QRectF &area)
    {
        const QPoint a = inPicture(screen, image, area.topLeft());
        const QPoint b = inPicture(screen, image, area.bottomRight());
        int all = 0;
        int gold = 0;
        for (int y = a.y(); y < b.y(); ++y) {
            for (int x = a.x(); x < b.x(); ++x) {
                if (!image.rect().contains(x, y))
                    continue;
                ++all;
                if (isGold(image.pixelColor(x, y)))
                    ++gold;
            }
        }
        return all > 0 ? double(gold) / all : 0;
    }

    static int goldCount(const QImage &image)
    {
        int count = 0;
        for (int y = 0; y < image.height(); ++y) {
            for (int x = 0; x < image.width(); ++x) {
                if (isGold(image.pixelColor(x, y)))
                    ++count;
            }
        }
        return count;
    }

    // The first gold pixel met going from `from` towards `to`, in global
    // logical coordinates (found in device pixels, given back in logical).
    static std::optional<QPointF> firstGold(QScreen *screen, const QImage &image, const QPointF &from, const QPointF &to)
    {
        const QPoint a = inPicture(screen, image, from);
        const QPoint b = inPicture(screen, image, to);
        const int steps = std::max(std::abs(b.x() - a.x()), std::abs(b.y() - a.y()));
        const double scale = double(image.width()) / screen->geometry().width();
        for (int i = 0; i <= steps; ++i) {
            const QPoint at(a.x() + (b.x() - a.x()) * i / std::max(steps, 1),
                            a.y() + (b.y() - a.y()) * i / std::max(steps, 1));
            if (image.rect().contains(at) && isGold(image.pixelColor(at))) {
                // The pixel's edge that faces where the scan came from.
                const double x = at.x() + (b.x() < a.x() ? 1 : 0);
                const double y = at.y() + (b.y() < a.y() ? 1 : 0);
                return QPointF(x / scale, y / scale) + QPointF(screen->geometry().topLeft());
            }
        }
        return std::nullopt;
    }

    void moveTo(const QPointF &global)
    {
        m_input->requestPointerMoveAbsolute(global);
        m_connection->flush();
        QTest::qWait(150);
    }

    void click(const QPointF &global)
    {
        moveTo(global);
        m_input->requestPointerButtonClick(Qt::LeftButton);
        m_connection->flush();
        QTest::qWait(300);
    }

    void typeKeys(const QList<quint32> &keys)
    {
        for (quint32 key : keys) {
            m_input->requestKeyboardKeyPress(key);
            m_connection->flush();
            QTest::qWait(30);
            m_input->requestKeyboardKeyRelease(key);
            m_connection->flush();
            QTest::qWait(30);
        }
        QTest::qWait(300);
    }

    QString startSettings()
    {
        QProcessEnvironment environment = QProcessEnvironment::systemEnvironment();
        environment.insert(QStringLiteral("QT_QPA_PLATFORM"), QStringLiteral("wayland"));
        environment.insert(QStringLiteral("TUR_SETTINGS_HELPER_OUT"), m_helperOut);
        m_settings.setProcessEnvironment(environment);
        m_settings.setProcessChannelMode(QProcess::ForwardedChannels);
        m_settings.start(QStringLiteral(TUR_SETTINGS_PROGRAM),
                         { QStringLiteral("--standalone"), QStringLiteral("--variant"), m_variant,
                           QStringLiteral("--fixture"), QStringLiteral(TUR_SETTINGS_FIXTURES "/default"),
                           QStringLiteral("--system-settings-program"), QStringLiteral(TUR_SETTINGS_HELPER),
                           QStringLiteral("--registry"), m_out + QStringLiteral("/no-registries") });
        if (!m_settings.waitForStarted(15000))
            return QStringLiteral("tur-settings did not start");
        if (!QTest::qWaitFor([] { return bus().interface()->isServiceRegistered(settingsService).value(); }, 30000))
            return QStringLiteral("tur-settings did not take org.tur.Settings1");
        if (!QTest::qWaitFor([&] { return windowOf(m_settings.processId()) != nullptr; }, 30000))
            return QStringLiteral("KWin does not list a window of tur-settings");
        return QString();
    }

private Q_SLOTS:
    void pointerArrived(const QString &lease)
    {
        if (lease == m_lease)
            ++m_arrivals;
    }

    void initTestCase()
    {
        m_out = qEnvironmentVariable("TUR_COMPOSITOR_OUT", QDir::currentPath());
        m_label = qEnvironmentVariable("TUR_COMPOSITOR_LABEL", QStringLiteral("kwin"));
        m_variant = qEnvironmentVariable("TUR_COMPOSITOR_VARIANT", QStringLiteral("dark"));
        QDir().mkpath(m_out + QStringLiteral("/no-registries"));
        m_helperOut = m_out + QStringLiteral("/") + m_label + QStringLiteral("-system-settings.json");
        QFile::remove(m_helperOut);
        PaxOverlay::registerBusTypes();

        QCOMPARE(QGuiApplication::platformName(), QStringLiteral("wayland"));
        const auto screens = QGuiApplication::screens();
        for (const QScreen *screen : screens) {
            report(QStringLiteral("output %1 at %2, scale %3").arg(screen->name(), describe(QRectF(screen->geometry())))
                       .arg(screen->devicePixelRatio()));
        }

        // KWin's window list and its fake input, through KWayland.
        m_connection = ConnectionThread::fromApplication(this);
        QVERIFY(m_connection);
        m_registry = new Registry(this);
        m_registry->create(m_connection);
        connect(m_registry, &Registry::plasmaWindowManagementAnnounced, this, [this](quint32 name, quint32 version) {
            m_windows = m_registry->createPlasmaWindowManagement(name, version, this);
        });
        connect(m_registry, &Registry::fakeInputAnnounced, this, [this](quint32 name, quint32 version) {
            m_input = m_registry->createFakeInput(name, version, this);
        });
        m_registry->setup();
        QVERIFY2(QTest::qWaitFor([&] { return m_windows && m_input; }, 15000),
                 "KWin offers org_kde_plasma_window_management and org_kde_kwin_fake_input to this client");
        m_input->authenticate(QStringLiteral("TUR compositor test"), QStringLiteral("drives the overlay's tests"));
        m_connection->flush();

        QVERIFY2(QTest::qWaitFor(
                     [] { return bus().interface()->isServiceRegistered(QStringLiteral("org.kde.KWin")).value(); }, 20000),
                 "KWin is on the session bus");

        const QString problem = startSettings();
        QVERIFY2(problem.isEmpty(), qPrintable(problem));

        QTest::qWait(500);
        for (QScreen *screen : QGuiApplication::screens()) {
            const QImage picture = capture(screen);
            QVERIFY2(!picture.isNull(), "KWin took a screenshot");
            m_baseline.insert(screen->name(), goldCount(picture));
            report(QStringLiteral("output %1: %2 gold pixels before the overlay started (window icons)")
                       .arg(screen->name()).arg(m_baseline.value(screen->name())));
        }

        QProcessEnvironment environment = QProcessEnvironment::systemEnvironment();
        environment.insert(QStringLiteral("QT_QPA_PLATFORM"), QStringLiteral("wayland"));
        m_overlay.setProcessEnvironment(environment);
        m_overlay.setProcessChannelMode(QProcess::ForwardedChannels);
        m_overlay.start(QStringLiteral(TUR_PAX_OVERLAY_PROGRAM), {});
        QVERIFY(m_overlay.waitForStarted(15000));
        QVERIFY2(QTest::qWaitFor([] { return bus().interface()->isServiceRegistered(overlayService).value(); }, 30000),
                 "tur-pax-overlay took org.tur.PaxOverlay1");

        QVERIFY(bus().connect(overlayService, overlayPath, overlayService, QStringLiteral("PointerArrived"), this,
                              SLOT(pointerArrived(QString))));
        const QDBusMessage lease = overlay(QStringLiteral("Acquire"), { QStringLiteral("compositor test") });
        QCOMPARE(lease.type(), QDBusMessage::ReplyMessage);
        m_lease = lease.arguments().value(0).toString();
        QVERIFY(!m_lease.isEmpty());
    }

    void cleanupTestCase()
    {
        for (QProcess *process : { &m_overlay, &m_settings }) {
            if (process->state() != QProcess::NotRunning) {
                process->terminate();
                process->waitForFinished(10000);
            }
        }
    }

    // What KWin knows of tur-settings' window: issue #51 lists the
    // application identifier as not verified.
    void kwin_sees_the_settings_window()
    {
        PlasmaWindow *window = windowOf(m_settings.processId());
        QVERIFY(window);
        report(QStringLiteral("tur-settings window: app id \"%1\", resource name \"%2\", title \"%3\", pid %4, "
                              "client %5, frame %6, active %7")
                   .arg(window->appId(), window->resourceName(), window->title())
                   .arg(window->pid())
                   .arg(describe(QRectF(window->clientGeometry())), describe(QRectF(window->geometry())))
                   .arg(window->isActive() ? QStringLiteral("yes") : QStringLiteral("no")));
        QCOMPARE(window->appId(), QStringLiteral("org.tur.Settings"));
        QCOMPARE(window->title(), QStringLiteral("Settings"));
        QVERIFY(window->clientGeometry().isValid());
        // The overlay has no window of the kind KWin lists: its surfaces are
        // layer-shell surfaces.
        QVERIFY(windowOf(m_overlay.processId()) == nullptr);
    }

    void the_interface_is_the_one_the_xml_states()
    {
        QDBusMessage introspect = QDBusMessage::createMethodCall(
            overlayService, overlayPath, QStringLiteral("org.freedesktop.DBus.Introspectable"),
            QStringLiteral("Introspect"));
        const QDBusMessage reply = bus().call(introspect, QDBus::Block, 20000);
        QCOMPARE(reply.type(), QDBusMessage::ReplyMessage);
        QFile file(QStringLiteral(TUR_PAX_OVERLAY_XML));
        QVERIFY(file.open(QIODevice::ReadOnly));
        QCOMPARE(shapeOf(reply.arguments().value(0).toString()).value(overlayService),
                 shapeOf(QString::fromUtf8(file.readAll())).value(overlayService));
    }

    // P@x on and idle: the pill is drawn with the stop combination (the
    // owner's answer, desktop-experience.md 17.1.2); the list of P@x's keys
    // under it when asked for.
    void on_and_idle_the_stop_keys_are_shown()
    {
        QCOMPARE(overlay(QStringLiteral("SetActivity"), { m_lease, QStringLiteral("idle"), QVariantMap() }).type(),
                 QDBusMessage::ReplyMessage);
        QVERIFY2(shownBecomes(true), "P@x is on: the indicator is shown while idle");
        QScreen *screen = QGuiApplication::primaryScreen();
        const QRectF area(screen->geometry());
        const QRectF top(area.center().x() - 200, area.top() + 8, 400, 44);
        const QImage idle = captureUntil(screen, [&](const QImage &image) { return goldShare(screen, image, top) > 0.01; });
        keep(idle, QStringLiteral("on-idle"));
        // The quiet pill: a dark pill with a gold outline at the top centre.
        QVERIFY2(goldShare(screen, idle, top) > 0.01,
                 "the pill is drawn while P@x is idle");
        QCOMPARE(overlay(QStringLiteral("ShowCommands"), { true }).type(), QDBusMessage::ReplyMessage);
        QCOMPARE(overlayProperty(QStringLiteral("CommandsShown")).toBool(), true);
        QTest::qWait(300);
        keep(capture(screen), QStringLiteral("on-idle-keys"));
        QCOMPARE(overlay(QStringLiteral("ShowCommands"), { false }).type(), QDBusMessage::ReplyMessage);
    }

    // A surface on every output; the frame at every output's edge and the
    // pill at the top centre of each.
    void the_overlay_covers_every_output()
    {
        QCOMPARE(overlay(QStringLiteral("SetActivity"),
                         { m_lease, QStringLiteral("acting"),
                           QVariantMap { { QStringLiteral("scope"), QStringLiteral("screen") } } })
                     .type(),
                 QDBusMessage::ReplyMessage);
        QVERIFY2(shownBecomes(true), "Shown: the overlay was drawn on every output");
        QStringList names;
        const auto screens = QGuiApplication::screens();
        for (const QScreen *screen : screens)
            names.append(screen->name());
        names.sort();
        QCOMPARE(overlayProperty(QStringLiteral("Screens")).toStringList(), names);

        for (QScreen *screen : screens) {
            const QRectF area(screen->geometry());
            const QImage picture = captureUntil(screen, [&](const QImage &image) {
                return isGold(colourAt(screen, image, QPointF(area.left() + 1, area.center().y())));
            });
            QVERIFY2(!picture.isNull(), "KWin took a screenshot");
            keep(picture, QStringLiteral("acting-") + screen->name());
            report(QStringLiteral("output %1: picture %2 x %3 for %4 x %5 logical")
                       .arg(screen->name()).arg(picture.width()).arg(picture.height())
                       .arg(area.width()).arg(area.height()));
            // The 3 px acting frame at the edge: the first logical pixel of
            // every side is gold.
            QVERIFY2(isGold(colourAt(screen, picture, QPointF(area.left() + 1, area.center().y()))), "left edge");
            QVERIFY2(isGold(colourAt(screen, picture, QPointF(area.right() - 1.5, area.center().y()))), "right edge");
            QVERIFY2(isGold(colourAt(screen, picture, QPointF(area.center().x() + 300, area.top() + 1))), "top edge");
            QVERIFY2(isGold(colourAt(screen, picture, QPointF(area.center().x(), area.bottom() - 1.5))), "bottom edge");
            // The loud pill: solid gold at the top centre, 12 px down and
            // 36 px high.
            const double pill = goldShare(screen, picture,
                                          QRectF(area.center().x() - 120, area.top() + 14, 240, 32));
            report(QStringLiteral("output %1: %2 % of the pill's area is gold").arg(screen->name()).arg(pill * 100, 0, 'f', 1));
            QVERIFY2(pill > 0.3, "the pill is at the top centre");
        }
    }

    // The chain of the design (9.6, "show me"): search, reveal, P@x's pointer
    // travels to the row, the highlight appears round it; then the pointer
    // goes to the row's control, and a click there reaches the button under
    // the pointer, the highlight and the overlay.
    void search_reveal_point_highlight_and_click_through()
    {
        // 1. Search.
        const QDBusMessage found = settings(QStringLiteral("Search"), { QStringLiteral("night light"), uint(5) });
        QCOMPARE(found.type(), QDBusMessage::ReplyMessage);
        const QDBusArgument hits = found.arguments().value(0).value<QDBusArgument>();
        QString address;
        hits.beginArray();
        if (!hits.atEnd()) {
            hits.beginStructure();
            double score = 0;
            QString title;
            hits >> address >> title >> score;
            hits.endStructure();
        }
        QCOMPARE(address, QStringLiteral("tur.appearance#night-light"));

        // 2. Reveal, without the program's own highlight: the overlay draws it.
        const QDBusMessage revealed =
            settings(QStringLiteral("Reveal"), { address, QVariantMap { { QStringLiteral("highlight"), false } } });
        QVERIFY2(revealed.type() == QDBusMessage::ReplyMessage, qPrintable(revealed.errorMessage()));
        const QVariantMap result = qdbus_cast<QVariantMap>(revealed.arguments().value(0));
        const RectD rect = qdbus_cast<RectD>(result.value(QStringLiteral("rect")));
        const RectD control = qdbus_cast<RectD>(result.value(QStringLiteral("control")));
        const uint pid = result.value(QStringLiteral("pid")).toUInt();
        const QString appId = result.value(QStringLiteral("app_id")).toString();
        const QString title = result.value(QStringLiteral("window_title")).toString();
        report(QStringLiteral("Reveal: rect %1, control %2, pid %3, app id \"%4\", title \"%5\", scale %6")
                   .arg(describe(rect.toRect()), describe(control.toRect())).arg(pid)
                   .arg(appId, title).arg(result.value(QStringLiteral("scale")).toDouble()));
        QVERIFY(control.width > 0 && control.height > 0);
        QCOMPARE(qint64(pid), m_settings.processId());

        QTest::qWait(500);
        PlasmaWindow *window = windowOf(pid);
        QVERIFY(window);
        const QRectF client(window->clientGeometry());
        const QRectF row = rect.toRect().translated(client.topLeft());
        const QRectF button = control.toRect().translated(client.topLeft());
        report(QStringLiteral("KWin: client area %1; so the row is at %2 and its button at %3")
                   .arg(describe(client), describe(row), describe(button)));
        QScreen *screen = screenAt(row.center());
        QCOMPARE(overlay(QStringLiteral("SetActivity"), { m_lease, QStringLiteral("working"), QVariantMap() }).type(),
                 QDBusMessage::ReplyMessage);
        QTest::qWait(300);
        const QImage before = capture(screen);
        keep(before, QStringLiteral("e2e-1-revealed"));
        QVERIFY(!isGold(colourAt(screen, before, row.center() + QPointF(18, 18))));

        // 3. The pointer travels to the row; the highlight appears when it
        // gets there.
        const int arrivals = m_arrivals;
        const QDBusMessage highlighted = overlay(
            QStringLiteral("HighlightInWindow"),
            { m_lease, pid, appId, title, QVariant::fromValue(rect),
              QVariantMap { { QStringLiteral("point"), true },
                            { QStringLiteral("dim"), true },
                            { QStringLiteral("step"), QStringLiteral("Step 1 of 1") },
                            { QStringLiteral("caption"), QStringLiteral("Night Light is in KDE's settings.") } } });
        QVERIFY2(highlighted.type() == QDBusMessage::ReplyMessage, qPrintable(highlighted.errorMessage()));
        QTest::qWait(150);
        keep(capture(screen), QStringLiteral("e2e-2-travelling"));
        QVERIFY2(arrives(arrivals), "PointerArrived");
        const QImage arrived = captureUntil(screen, [&](const QImage &image) {
            return isGold(colourAt(screen, image, row.center() + QPointF(18, 18)));
        });
        keep(arrived, QStringLiteral("e2e-3-arrived"));

        // Where the pointer is: its tip is the row's centre, and the core of
        // the flame (18 px right and 18 px down from the tip, for a flame 36
        // px long, 1.5 times a 24 px cursor) is gold.
        QVERIFY2(isGold(colourAt(screen, arrived, row.center() + QPointF(18, 18))),
                 "P@x's pointer is at the row's centre");
        // Where the highlight is: its gold line's outer edge is 7 px outside
        // the row (4 px clear, 3 px of gold). Found by scanning towards the
        // row from outside it, on each side.
        const double third = row.left() + row.width() / 4;
        const auto left = firstGold(screen, arrived, QPointF(row.left() - 14, row.center().y()),
                                    QPointF(row.left(), row.center().y()));
        const auto right = firstGold(screen, arrived, QPointF(row.right() + 14, row.center().y()),
                                     QPointF(row.right(), row.center().y()));
        const auto top = firstGold(screen, arrived, QPointF(third, row.top() - 14), QPointF(third, row.top()));
        const auto bottom = firstGold(screen, arrived, QPointF(third, row.bottom() + 14), QPointF(third, row.bottom()));
        QVERIFY2(left && right && top && bottom, "the highlight's line is found on all four sides");
        const QRectF measured(QPointF(left->x(), top->y()), QPointF(right->x(), bottom->y()));
        const QRectF expected = row.adjusted(-7, -7, 7, 7);
        const double slack = 1.5;
        report(QStringLiteral("highlight: expected outer edge %1, measured %2 (differences %3, %4, %5, %6 px)")
                   .arg(describe(expected), describe(measured))
                   .arg(measured.left() - expected.left()).arg(measured.top() - expected.top())
                   .arg(measured.right() - expected.right()).arg(measured.bottom() - expected.bottom()));
        QVERIFY2(std::abs(measured.left() - expected.left()) <= slack && std::abs(measured.top() - expected.top()) <= slack
                     && std::abs(measured.right() - expected.right()) <= slack
                     && std::abs(measured.bottom() - expected.bottom()) <= slack,
                 "the highlight is round the row Reveal reported, to within 1.5 px");
        // Between the line and the row: 4 px clear, not gold.
        QVERIFY(!isGold(colourAt(screen, arrived, QPointF(row.left() - 2, row.center().y()))));

        // 4. To the row's control, and a highlight round that too.
        const int again = m_arrivals;
        QCOMPARE(overlay(QStringLiteral("SetActivity"), { m_lease, QStringLiteral("acting"), QVariantMap() }).type(),
                 QDBusMessage::ReplyMessage);
        QCOMPARE(overlay(QStringLiteral("HighlightInWindow"),
                         { m_lease, pid, appId, title, QVariant::fromValue(control), QVariantMap() })
                     .type(),
                 QDBusMessage::ReplyMessage);
        QCOMPARE(overlay(QStringLiteral("PointInWindow"),
                         { m_lease, pid, appId, title, QVariant::fromValue(control), QVariantMap() })
                     .type(),
                 QDBusMessage::ReplyMessage);
        QVERIFY(arrives(again));
        QTest::qWait(400);
        const QImage pointing = capture(screen);
        keep(pointing, QStringLiteral("e2e-4-at-the-button"));
        // The click goes where P@x's pointer is drawn: on the flame's body,
        // which lies over the button.
        // 8.5 px from the tip along the flame's axis: gold, short of the
        // inner flame.
        const QPointF target = button.center() + QPointF(6, 6);
        QVERIFY2(button.contains(target), "the point clicked is on the button");
        QVERIFY2(isGold(colourAt(screen, pointing, target)) || isGold(colourAt(screen, pointing, target + QPointF(1, 1))),
                 "the point clicked is covered by P@x's pointer");

        // 5. The click.
        QFile::remove(m_helperOut);
        QCOMPARE(overlay(QStringLiteral("PulsePointer"), { m_lease, QStringLiteral("click") }).type(),
                 QDBusMessage::ReplyMessage);
        click(target);
        QVERIFY2(QTest::qWaitFor([&] { return QFile::exists(m_helperOut); }, 10000),
                 "the click reached the button under the overlay: tur-settings started System Settings");
        QFile record(m_helperOut);
        QVERIFY(record.open(QIODevice::ReadOnly));
        const QJsonArray arguments = QJsonDocument::fromJson(record.readAll()).array();
        report(QStringLiteral("the button under P@x's pointer was pressed: System Settings asked for %1")
                   .arg(QString::fromUtf8(QJsonDocument(arguments).toJson(QJsonDocument::Compact))));
        QVERIFY(!arguments.isEmpty());
        keep(capture(screen), QStringLiteral("e2e-5-clicked"));

        QCOMPARE(overlay(QStringLiteral("Clear"), { m_lease, 0u }).type(), QDBusMessage::ReplyMessage);
        QCOMPARE(overlay(QStringLiteral("HidePointer"), { m_lease }).type(), QDBusMessage::ReplyMessage);
    }

    // A window of the test's own has the keyboard. P@x shows itself over it,
    // points at it and highlights it; keys typed meanwhile reach the window,
    // which never loses the keyboard; a click where P@x's pointer is drawn
    // reaches the window at that point.
    void the_overlay_never_takes_the_keyboard()
    {
        ProbeWindow probe;
        probe.show();
        QVERIFY(QTest::qWaitForWindowExposed(&probe));
        PlasmaWindow *window = nullptr;
        QVERIFY(QTest::qWaitFor([&] {
            window = windowOf(QCoreApplication::applicationPid(), probe.title());
            return window != nullptr && window->clientGeometry().isValid();
        }, 15000));
        if (!window->isActive()) {
            // Give it the keyboard the way a person would.
            click(QRectF(window->clientGeometry()).center());
        }
        QVERIFY2(QTest::qWaitFor([&] { return window->isActive() && probe.isActive(); }, 10000),
                 "the probe window has the keyboard");
        const QRectF client(window->clientGeometry());
        probe.deactivations = 0;
        probe.typed.clear();
        probe.clicks.clear();

        const int arrivals = m_arrivals;
        QCOMPARE(overlay(QStringLiteral("SetActivity"),
                         { m_lease, QStringLiteral("acting"),
                           QVariantMap { { QStringLiteral("scope"), QStringLiteral("window") },
                                         { QStringLiteral("pid"), uint(QCoreApplication::applicationPid()) },
                                         { QStringLiteral("title"), probe.title() } } })
                     .type(),
                 QDBusMessage::ReplyMessage);
        const RectD inside { 40, 40, 300, 60 };
        QCOMPARE(overlay(QStringLiteral("HighlightInWindow"),
                         { m_lease, uint(QCoreApplication::applicationPid()), QString(), probe.title(),
                           QVariant::fromValue(inside),
                           QVariantMap { { QStringLiteral("point"), true }, { QStringLiteral("dim"), true },
                                         { QStringLiteral("caption"), QStringLiteral("Type here.") } } })
                     .type(),
                 QDBusMessage::ReplyMessage);
        QVERIFY(arrives(arrivals));
        QVERIFY(shownBecomes(true));
        QScreen *screen = screenAt(client.center());
        const QPointF tipAt = client.topLeft() + QPointF(190, 70);
        const QImage shown = captureUntil(screen, [&](const QImage &image) {
            return isGold(colourAt(screen, image, tipAt + QPointF(18, 18)));
        });
        keep(shown, QStringLiteral("focus-probe"));
        const QPointF tip = client.topLeft() + QPointF(190, 70);
        QVERIFY2(isGold(colourAt(screen, shown, tip + QPointF(18, 18))), "P@x's pointer is over the probe");

        typeKeys({ KEY_T, KEY_U, KEY_R });
        QCOMPARE(probe.typed, QStringLiteral("tur"));
        QVERIFY2(probe.isActive() && window->isActive(), "the probe still has the keyboard");
        QCOMPARE(probe.deactivations, 0);

        // A click on P@x's pointer lands on the probe, at that point.
        const QPointF target = tip + QPointF(6, 6);
        QVERIFY(isGold(colourAt(screen, shown, target)) || isGold(colourAt(screen, shown, target + QPointF(1, 1))));
        click(target);
        QCOMPARE(probe.clicks.size(), 1);
        const QPointF local = probe.clicks.constFirst();
        const QPointF wanted = target - client.topLeft();
        report(QStringLiteral("a click at %1,%2 on P@x's pointer reached the probe at %3,%4 (expected %5,%6)")
                   .arg(target.x()).arg(target.y()).arg(local.x()).arg(local.y()).arg(wanted.x()).arg(wanted.y()));
        QVERIFY(std::abs(local.x() - wanted.x()) <= 1.5 && std::abs(local.y() - wanted.y()) <= 1.5);
        QCOMPARE(probe.deactivations, 0);

        // What Reveal does to a window that does not have the keyboard: an
        // observation for issue #51, not a requirement.
        PlasmaWindow *settingsWindow = windowOf(m_settings.processId());
        QVERIFY(settingsWindow);
        settings(QStringLiteral("Reveal"), { QStringLiteral("tur.appearance#variant"),
                                             QVariantMap { { QStringLiteral("highlight"), false } } });
        QTest::qWait(1500);
        report(QStringLiteral("Reveal while another window has the keyboard: tur-settings %1 activated (active: "
                              "settings %2, probe %3, demands attention %4)")
                   .arg(settingsWindow->isActive() ? QStringLiteral("was") : QStringLiteral("was not"))
                   .arg(settingsWindow->isActive() ? QStringLiteral("yes") : QStringLiteral("no"))
                   .arg(window->isActive() ? QStringLiteral("yes") : QStringLiteral("no"))
                   .arg(settingsWindow->isDemandingAttention() ? QStringLiteral("yes") : QStringLiteral("no")));

        QCOMPARE(overlay(QStringLiteral("Clear"), { m_lease, 0u }).type(), QDBusMessage::ReplyMessage);
        QCOMPARE(overlay(QStringLiteral("HidePointer"), { m_lease }).type(), QDBusMessage::ReplyMessage);
        probe.close();
    }

    // With two outputs: the pointer and a highlight on the second.
    void the_second_output()
    {
        const auto screens = QGuiApplication::screens();
        if (screens.size() < 2)
            QSKIP("one output in this arrangement");
        QScreen *second = screens.at(1);
        const QRectF area(second->geometry());
        const int arrivals = m_arrivals;
        QCOMPARE(overlay(QStringLiteral("MovePointer"),
                         { m_lease, second->name(), 200.0, 150.0, QVariantMap { { QStringLiteral("slow"), true } } })
                     .type(),
                 QDBusMessage::ReplyMessage);
        QTest::qWait(500);
        keep(capture(screens.at(0)), QStringLiteral("crossing-first-output"));
        keep(capture(second), QStringLiteral("crossing-second-output"));
        QVERIFY(arrives(arrivals));
        const RectD rect { 300, 300, 400, 50 };
        QCOMPARE(overlay(QStringLiteral("HighlightRect"),
                         { m_lease, second->name(), QVariant::fromValue(rect), QVariantMap() })
                     .type(),
                 QDBusMessage::ReplyMessage);
        QTest::qWait(400);
        const QImage picture = capture(second);
        keep(picture, QStringLiteral("second-output"));
        const QPointF origin = area.topLeft();
        QVERIFY2(isGold(colourAt(second, picture, origin + QPointF(200 + 21, 150 + 23))), "the pointer is on the second output");
        QVERIFY2(isGold(colourAt(second, picture, origin + QPointF(300 - 5.5, 325))), "the highlight is on the second output");
        QVERIFY(!isGold(colourAt(second, picture, origin + QPointF(300 - 2, 325))));
        QCOMPARE(overlay(QStringLiteral("Clear"), { m_lease, 0u }).type(), QDBusMessage::ReplyMessage);
        QCOMPARE(overlay(QStringLiteral("HidePointer"), { m_lease }).type(), QDBusMessage::ReplyMessage);
    }

    // Told to go, it goes: nothing gold is left anywhere.
    void it_vanishes_when_told()
    {
        const int arrivals = m_arrivals;
        QCOMPARE(overlay(QStringLiteral("SetActivity"),
                         { m_lease, QStringLiteral("acting"),
                           QVariantMap { { QStringLiteral("scope"), QStringLiteral("screen") } } })
                     .type(),
                 QDBusMessage::ReplyMessage);
        QCOMPARE(overlay(QStringLiteral("HighlightRect"),
                         { m_lease, QString(), QVariant::fromValue(RectD { 100, 200, 300, 50 }),
                           QVariantMap { { QStringLiteral("point"), true } } })
                     .type(),
                 QDBusMessage::ReplyMessage);
        QVERIFY(arrives(arrivals));
        QVERIFY(shownBecomes(true));
        QTest::qWait(300);
        const auto screens = QGuiApplication::screens();
        int before = 0;
        for (QScreen *screen : screens)
            before += goldCount(capture(screen));

        QElapsedTimer clock;
        clock.start();
        QCOMPARE(overlay(QStringLiteral("Hide")).type(), QDBusMessage::ReplyMessage);
        QCOMPARE(overlayProperty(QStringLiteral("Shown")).toBool(), false);
        QTest::qWait(200);
        const qint64 waited = clock.elapsed();
        int after = 0;
        int baseline = 0;
        for (QScreen *screen : screens) {
            const int icons = m_baseline.value(screen->name());
            const QImage picture = captureUntil(screen, [&](const QImage &image) { return goldCount(image) <= icons; });
            keep(picture, QStringLiteral("after-hide-") + screen->name());
            after += goldCount(picture);
            baseline += m_baseline.value(screen->name());
        }
        report(QStringLiteral("Hide: gold pixels %1 before, %2 after (%3 before the overlay started), screenshots "
                              "begun %4 ms after the call")
                   .arg(before).arg(after).arg(baseline).arg(waited));
        // The lease ended with Hide; a new one for what follows.
        const QDBusMessage lease = overlay(QStringLiteral("Acquire"), { QStringLiteral("compositor test, again") });
        QCOMPARE(lease.type(), QDBusMessage::ReplyMessage);
        m_lease = lease.arguments().value(0).toString();
        QVERIFY(before > 1000);
        QVERIFY2(after <= baseline, "no gold of the overlay's is left");
    }

    // Its owner dies: it goes.
    void it_vanishes_when_its_owner_dies()
    {
        // The test's own lease would keep P@x on, and the pill shown.
        QCOMPARE(overlay(QStringLiteral("Release"), { m_lease }).type(), QDBusMessage::ReplyMessage);
        QVERIFY(shownBecomes(false));
        QProcess owner;
        owner.start(QCoreApplication::applicationFilePath(), { QStringLiteral("--owner") });
        QVERIFY(owner.waitForStarted(15000));
        QVERIFY(owner.waitForReadyRead(20000));
        QCOMPARE(owner.readLine().trimmed(), QByteArray("ready"));
        QVERIFY(shownBecomes(true));
        QTest::qWait(300);
        const auto screens = QGuiApplication::screens();
        QVERIFY(goldCount(capture(screens.constFirst())) > 1000);
        ::kill(pid_t(owner.processId()), SIGKILL);
        QVERIFY(owner.waitForFinished(10000));
        QVERIFY2(shownBecomes(false), "the overlay noticed its owner had gone");
        const int icons = m_baseline.value(screens.constFirst()->name());
        const QImage picture =
            captureUntil(screens.constFirst(), [&](const QImage &image) { return goldCount(image) <= icons; });
        keep(picture, QStringLiteral("after-owner-died"));
        QVERIFY(goldCount(picture) <= m_baseline.value(screens.constFirst()->name()));

        const QDBusMessage lease = overlay(QStringLiteral("Acquire"), { QStringLiteral("compositor test, last") });
        QCOMPARE(lease.type(), QDBusMessage::ReplyMessage);
        m_lease = lease.arguments().value(0).toString();
    }

    // The overlay itself exits: its surfaces go with it.
    void it_vanishes_when_it_exits()
    {
        QCOMPARE(overlay(QStringLiteral("SetActivity"),
                         { m_lease, QStringLiteral("acting"),
                           QVariantMap { { QStringLiteral("scope"), QStringLiteral("screen") } } })
                     .type(),
                 QDBusMessage::ReplyMessage);
        QVERIFY(shownBecomes(true));
        QTest::qWait(300);
        const auto screens = QGuiApplication::screens();
        QVERIFY(goldCount(capture(screens.constFirst())) > 1000);
        ::kill(pid_t(m_overlay.processId()), SIGKILL);
        QVERIFY(m_overlay.waitForFinished(10000));
        const int icons = m_baseline.value(screens.constFirst()->name());
        const QImage picture =
            captureUntil(screens.constFirst(), [&](const QImage &image) { return goldCount(image) <= icons; });
        keep(picture, QStringLiteral("after-overlay-killed"));
        QVERIFY(goldCount(picture) <= m_baseline.value(screens.constFirst()->name()));
    }
};

int main(int argc, char *argv[])
{
    if (argc > 1 && qstrcmp(argv[1], "--owner") == 0)
        return ownerMain(argc, argv);
    QGuiApplication app(argc, argv);
    QGuiApplication::setDesktopFileName(QStringLiteral("org.tur.CompositorTest"));
    CompositorTest test;
    return QTest::qExec(&test, argc, argv);
}

#include "tst_compositor.moc"
