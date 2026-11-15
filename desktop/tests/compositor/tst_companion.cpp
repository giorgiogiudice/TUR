// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
//
// P@x's character on the bar under a real KWin (docs/architecture/
// tur-pax-companion.md, sections 2, 5 and 10).
//
// desktop/ci/compositor.sh starts kwin_wayland headless and a private
// session bus, then this program, which draws a stand-in for the bar (a
// layer-shell surface of its own on the top layer, along the bottom edge,
// that reserves its thickness as Plasma's panel does), starts the stand-in
// for org.tur.Agent1 that P@x's window's tests use, tur-pax-companion and
// tur-pax-ui as Wayland clients, and checks what only a compositor can
// show:
//
//  - the character is drawn inside the bar, at its end, in the middle of the
//    bar's thickness: so the thickness was found, from the layer-shell
//    protocol alone;
//  - it never takes the keyboard: keys typed while it is there, and while
//    it speaks, reach the window that had them, which never loses focus;
//  - it never blocks a click: a click on the bar beside the character
//    reaches the bar, and a click beside its bubble reaches the window
//    under the strip;
//  - asked for (org.tur.PaxCompanion1.Attend) it has the keyboard, and Esc
//    gives it back;
//  - one press opens P@x's ask bar beside it, just above the bar;
//  - dragged, it moves along the bar and stays;
//  - hidden it is gone, and it comes back;
//  - over a full-screen window it stays visible; told to step aside, it
//    leaves with the bar and comes back while P@x's indicator says P@x is
//    controlling.
//
// The bar is a stand-in: Plasma's own panel is not in the builder image and
// was not run (docs/reports/desktop-8.md).
//
// Input is injected with KWin's fake-input protocol and screenshots come
// from org.kde.KWin.ScreenShot2, as in tst_compositor.cpp.

#include "probewindow.h"

#include <KWayland/Client/connection_thread.h>
#include <KWayland/Client/fakeinput.h>
#include <KWayland/Client/plasmawindowmanagement.h>
#include <KWayland/Client/registry.h>

#include <LayerShellQt/window.h>

#include <QColor>
#include <QCoreApplication>
#include <QDBusArgument>
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
#include <QJsonObject>
#include <QMouseEvent>
#include <QPainter>
#include <QProcess>
#include <QProcessEnvironment>
#include <QRasterWindow>
#include <QScreen>
#include <QTest>
#include <QTextStream>

#include <linux/input-event-codes.h>

#include <algorithm>
#include <cerrno>
#include <cmath>
#include <functional>
#include <fcntl.h>
#include <unistd.h>

using namespace KWayland::Client;

namespace {

const QString companionService = QStringLiteral("org.tur.PaxCompanion1");
const QString companionPath = QStringLiteral("/org/tur/PaxCompanion1");
const QString windowService = QStringLiteral("org.tur.PaxUi1");
const QString windowPath = QStringLiteral("/org/tur/PaxUi1");
const QString indicatorService = QStringLiteral("org.tur.PaxOverlay1");
const QString indicatorPath = QStringLiteral("/org/tur/PaxOverlay1");
const QString agentService = QStringLiteral("org.tur.Agent1");

// The stand-in bar's thickness. Thicker than the character's box, so that
// "in the middle of the bar" and "just above the screen's edge" are places
// eight pixels apart and a picture can tell which it is.
constexpr int barThickness = 64;

// P@x's gold, as the character draws it.
bool isGold(const QColor &colour)
{
    return std::abs(colour.red() - 0xE8) <= 24 && std::abs(colour.green() - 0xB4) <= 24
           && std::abs(colour.blue() - 0x4C) <= 24;
}

bool isNear(const QColor &colour, const QColor &wanted, int tolerance = 10)
{
    return colour.isValid() && std::abs(colour.red() - wanted.red()) <= tolerance
           && std::abs(colour.green() - wanted.green()) <= tolerance
           && std::abs(colour.blue() - wanted.blue()) <= tolerance;
}

QString describe(const QRectF &rect)
{
    return QStringLiteral("(%1, %2, %3 x %4)").arg(rect.x()).arg(rect.y()).arg(rect.width()).arg(rect.height());
}

void report(const QString &line)
{
    QTextStream(stdout) << "TUR-KWIN companion: " << line << '\n';
}

} // namespace

// A stand-in for the bar: a layer-shell surface on the top layer along the
// bottom edge that reserves its thickness, in a plain colour that is not
// gold, and writes down every click it is given.
class BarStandIn : public QRasterWindow
{
    Q_OBJECT

public:
    explicit BarStandIn(const QColor &colour)
        : m_colour(colour)
    {
        setTitle(QStringLiteral("TUR stand-in bar"));
        setFlags(Qt::FramelessWindowHint);
        resize(QGuiApplication::primaryScreen()->geometry().width(), barThickness);
        LayerShellQt::Window *layer = LayerShellQt::Window::get(this);
        layer->setLayer(LayerShellQt::Window::LayerTop);
        layer->setAnchors(LayerShellQt::Window::Anchors(LayerShellQt::Window::AnchorBottom)
                          | LayerShellQt::Window::AnchorLeft | LayerShellQt::Window::AnchorRight);
        layer->setExclusiveZone(barThickness);
        layer->setKeyboardInteractivity(LayerShellQt::Window::KeyboardInteractivityNone);
        layer->setActivateOnShow(false);
        layer->setWantsToBeOnActiveScreen(false);
        layer->setScope(QStringLiteral("tur-stand-in-bar"));
    }

    QColor colour() const { return m_colour; }
    QList<QPointF> clicks;

protected:
    void paintEvent(QPaintEvent *) override
    {
        QPainter painter(this);
        painter.fillRect(QRect(QPoint(0, 0), size()), m_colour);
    }

    void mousePressEvent(QMouseEvent *event) override { clicks.append(event->position()); }

private:
    QColor m_colour;
};

// A stand-in for P@x's indicator: its Activity and nothing else.
class IndicatorStandIn : public QObject
{
    Q_OBJECT
    Q_CLASSINFO("D-Bus Interface", "org.tur.PaxOverlay1")
    Q_PROPERTY(QString Activity READ activity)

public:
    QString activity() const { return QStringLiteral("acting"); }
};

class CompanionCompositorTest : public QObject
{
    Q_OBJECT

private:
    QString m_out;
    QString m_label;
    QString m_variant;
    QString m_work;
    ConnectionThread *m_connection = nullptr;
    Registry *m_registry = nullptr;
    PlasmaWindowManagement *m_windows = nullptr;
    FakeInput *m_input = nullptr;
    QProcess m_agent;
    QProcess m_companion;
    QProcess m_window;
    BarStandIn *m_bar = nullptr;
    ProbeWindow *m_probe = nullptr;
    QScreen *m_screen = nullptr;
    int m_shots = 0;

    static QDBusConnection bus() { return QDBusConnection::sessionBus(); }

    double width() const { return m_screen->geometry().width(); }
    double height() const { return m_screen->geometry().height(); }
    // The bar's strip of the screen.
    QRectF band() const { return QRectF(0, height() - barThickness, width(), barThickness); }
    // What opens above the bar, and P@x's window, in the variant in use
    // (Tur.Kit's surface.overlay).
    QColor overlayColour() const
    {
        return m_variant == QLatin1String("light") ? QColor(0xFF, 0xFF, 0xFF) : QColor(0x1B, 0x24, 0x34);
    }

    static bool owned(const QString &name) { return bus().interface()->isServiceRegistered(name).value(); }

    static QVariant property(const QString &service, const QString &path, const QString &name)
    {
        QDBusMessage get = QDBusMessage::createMethodCall(service, path, QStringLiteral("org.freedesktop.DBus.Properties"),
                                                          QStringLiteral("Get"));
        get << service << name;
        get.setAutoStartService(false);
        const QDBusMessage reply = bus().call(get, QDBus::Block, 20000);
        if (reply.type() != QDBusMessage::ReplyMessage)
            return QVariant();
        return reply.arguments().value(0).value<QDBusVariant>().variant();
    }

    static QString state() { return property(companionService, companionPath, QStringLiteral("State")).toString(); }

    static bool stateBecomes(const QString &wanted)
    {
        return QTest::qWaitFor([&] { return state() == wanted; }, 20000);
    }

    static QVariantMap anchor()
    {
        return qdbus_cast<QVariantMap>(property(companionService, companionPath, QStringLiteral("Anchor")));
    }

    static QDBusMessage companion(const QString &method, const QVariantList &arguments = {})
    {
        QDBusMessage message = QDBusMessage::createMethodCall(companionService, companionPath, companionService, method);
        message.setArguments(arguments);
        message.setAutoStartService(false);
        return bus().call(message, QDBus::Block, 20000);
    }

    // A screenshot of the output, from KWin, at the output's own resolution.
    QImage capture()
    {
        int fds[2];
        if (::pipe2(fds, O_CLOEXEC) != 0)
            return QImage();
        QDBusPendingCall pending = [&] {
            QDBusMessage message = QDBusMessage::createMethodCall(
                QStringLiteral("org.kde.KWin"), QStringLiteral("/org/kde/KWin/ScreenShot2"),
                QStringLiteral("org.kde.KWin.ScreenShot2"), QStringLiteral("CaptureScreen"));
            message << m_screen->name()
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
        const QVariantMap results = qdbus_cast<QVariantMap>(reply.arguments().value(0));
        const int imageWidth = int(results.value(QStringLiteral("width")).toUInt());
        const int imageHeight = int(results.value(QStringLiteral("height")).toUInt());
        const int stride = int(results.value(QStringLiteral("stride")).toUInt());
        const auto format = QImage::Format(results.value(QStringLiteral("format")).toUInt());
        QByteArray data;
        data.resize(qsizetype(stride) * imageHeight);
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
        if (imageWidth <= 0 || imageHeight <= 0 || got != data.size()) {
            qWarning() << "ScreenShot2 gave" << results << "and" << got << "bytes";
            return QImage();
        }
        return QImage(reinterpret_cast<const uchar *>(data.constData()), imageWidth, imageHeight, stride, format)
            .convertToFormat(QImage::Format_RGB32);
    }

    // Screenshots until one shows what `drawn` looks for, or 15 s passed.
    QImage captureUntil(const std::function<bool(const QImage &)> &drawn)
    {
        QImage picture;
        QElapsedTimer clock;
        clock.start();
        do {
            picture = capture();
            if (!picture.isNull() && drawn(picture))
                return picture;
            QTest::qWait(250);
        } while (clock.elapsed() < 15000);
        return picture;
    }

    void keep(const QImage &image, const QString &what)
    {
        const QString name =
            QStringLiteral("%1-companion-%2-%3.png").arg(m_label).arg(++m_shots, 2, 10, QLatin1Char('0')).arg(what);
        image.save(m_out + QLatin1Char('/') + name);
        report(QStringLiteral("screenshot %1 (%2 x %3)").arg(name).arg(image.width()).arg(image.height()));
    }

    double scaleOf(const QImage &image) const { return double(image.width()) / width(); }

    QColor colourAt(const QImage &image, const QPointF &logical) const
    {
        const double scale = scaleOf(image);
        const QPoint at(int(std::floor(logical.x() * scale)), int(std::floor(logical.y() * scale)));
        return image.rect().contains(at) ? image.pixelColor(at) : QColor();
    }

    // The box round the gold pixels in `area`, in logical pixels; empty if
    // there are none. `count` is how many there are, in device pixels.
    QRectF goldBox(const QImage &image, const QRectF &area, int *count = nullptr) const
    {
        const double scale = scaleOf(image);
        const QRect device = QRectF(area.x() * scale, area.y() * scale, area.width() * scale, area.height() * scale)
                                 .toAlignedRect()
                                 .intersected(image.rect());
        int found = 0;
        int left = image.width();
        int top = image.height();
        int right = -1;
        int bottom = -1;
        for (int y = device.top(); y <= device.bottom(); ++y) {
            for (int x = device.left(); x <= device.right(); ++x) {
                if (!isGold(image.pixelColor(x, y)))
                    continue;
                ++found;
                left = std::min(left, x);
                top = std::min(top, y);
                right = std::max(right, x);
                bottom = std::max(bottom, y);
            }
        }
        if (count)
            *count = found;
        if (found == 0)
            return QRectF();
        return QRectF(left / scale, top / scale, (right - left + 1) / scale, (bottom - top + 1) / scale);
    }

    int goldIn(const QImage &image, const QRectF &area) const
    {
        int count = 0;
        goldBox(image, area, &count);
        return count;
    }

    // Enough gold for a flame, not a stray pixel at an edge.
    bool characterIn(const QImage &image, const QRectF &area) const
    {
        return goldIn(image, area) >= int(60 * scaleOf(image) * scaleOf(image));
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
        QTest::qWait(400);
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
        QTest::qWait(400);
    }

    PlasmaWindow *probeWindow() const
    {
        if (!m_windows)
            return nullptr;
        const auto windows = m_windows->windows();
        for (PlasmaWindow *window : windows) {
            if (window->isValid() && window->pid() == quint32(QCoreApplication::applicationPid())
                && window->title() == m_probe->title())
                return window;
        }
        return nullptr;
    }

    bool probeHasTheKeyboard()
    {
        PlasmaWindow *window = nullptr;
        if (!QTest::qWaitFor([&] {
                window = probeWindow();
                return window != nullptr && window->clientGeometry().isValid();
            }, 15000))
            return false;
        if (!window->isActive())
            click(QRectF(window->clientGeometry()).center());
        return QTest::qWaitFor([&] { return window->isActive() && m_probe->isActive(); }, 10000);
    }

    void writeFile(const QString &path, const QByteArray &content)
    {
        QFile file(path);
        if (file.open(QIODevice::WriteOnly | QIODevice::Truncate))
            file.write(content);
    }

    // The healing service repaired something and asks the character to say
    // it (org.tur.PaxCompanion1.Announce, as `tur-diag heal notify` will):
    // the stand-in for its program prints one notice, the first of the
    // fixture's, and the character must answer that it took it.
    void healingServiceRepairsSomething()
    {
        QFile fixture(QStringLiteral(TUR_COMPANION_FIXTURES "/heal/notices.json"));
        QVERIFY(fixture.open(QIODevice::ReadOnly));
        const QJsonArray printed =
            QJsonDocument::fromJson(fixture.readAll()).object().value(QLatin1String("printed")).toArray();
        QVERIFY(!printed.isEmpty());
        writeFile(m_work + QStringLiteral("/printed.json"), QJsonDocument(QJsonArray { printed.at(0) }).toJson());
        const qulonglong id = qulonglong(printed.at(0).toObject().value(QLatin1String("id")).toInteger());
        QElapsedTimer clock;
        clock.start();
        const QDBusMessage reply = companion(QStringLiteral("Announce"), { QStringLiteral("heal"), QVariant::fromValue(id) });
        QCOMPARE(reply.type(), QDBusMessage::ReplyMessage);
        QVERIFY2(reply.arguments().value(0).toBool(), "the character took the healing service's notice");
        report(QStringLiteral("Announce(heal, %1) was answered \"taken\" after %2 ms").arg(id).arg(clock.elapsed()));
    }

    QString startCompanion(bool stayOverFullScreen)
    {
        writeFile(m_work + QStringLiteral("/pax-companion.json"),
                  stayOverFullScreen ? QByteArray("{\"version\": 1, \"over_full_screen\": true}\n")
                                     : QByteArray("{\"version\": 1}\n"));
        QProcessEnvironment environment = QProcessEnvironment::systemEnvironment();
        environment.insert(QStringLiteral("QT_QPA_PLATFORM"), QStringLiteral("wayland"));
        m_companion.setProcessEnvironment(environment);
        m_companion.setProcessChannelMode(QProcess::ForwardedChannels);
        m_companion.start(QStringLiteral(TUR_COMPANION_PROGRAM),
                          { QStringLiteral("--standalone"), QStringLiteral("--variant"), m_variant,
                            QStringLiteral("--settings"), m_work + QStringLiteral("/pax-companion.json"),
                            QStringLiteral("--heal-command"),
                            QStringLiteral("cat ") + m_work + QStringLiteral("/printed.json") });
        if (!m_companion.waitForStarted(15000))
            return QStringLiteral("tur-pax-companion did not start");
        if (!QTest::qWaitFor([] { return owned(companionService); }, 30000))
            return QStringLiteral("tur-pax-companion did not take org.tur.PaxCompanion1");
        if (!stateBecomes(QStringLiteral("idle")))
            return QStringLiteral("the character is not idle with P@x on: ") + state();
        return QString();
    }

    void stopCompanion()
    {
        if (m_companion.state() == QProcess::NotRunning)
            return;
        m_companion.terminate();
        if (!m_companion.waitForFinished(10000)) {
            m_companion.kill();
            m_companion.waitForFinished(5000);
        }
        if (!QTest::qWaitFor([] { return !owned(companionService); }, 10000))
            report(QStringLiteral("the stopped character still owned org.tur.PaxCompanion1 after 10 s"));
    }

private Q_SLOTS:
    void initTestCase()
    {
        m_out = qEnvironmentVariable("TUR_COMPOSITOR_OUT", QDir::currentPath());
        m_label = qEnvironmentVariable("TUR_COMPOSITOR_LABEL", QStringLiteral("kwin"));
        m_variant = qEnvironmentVariable("TUR_COMPOSITOR_VARIANT", QStringLiteral("dark"));
        m_work = m_out + QStringLiteral("/") + m_label + QStringLiteral("-companion-work");
        QDir(m_work).removeRecursively();
        QVERIFY(QDir().mkpath(m_work));
        writeFile(m_work + QStringLiteral("/printed.json"), "[]\n");

        QCOMPARE(QGuiApplication::platformName(), QStringLiteral("wayland"));
        if (QGuiApplication::screens().size() != 1)
            QSKIP("the character's first slice is on one screen: the screen the bar is on");
        m_screen = QGuiApplication::primaryScreen();
        report(QStringLiteral("output %1 at %2, scale %3")
                   .arg(m_screen->name(), describe(QRectF(m_screen->geometry())))
                   .arg(m_screen->devicePixelRatio()));

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
        m_input->authenticate(QStringLiteral("TUR compositor test"), QStringLiteral("drives the character's tests"));
        m_connection->flush();
        QVERIFY2(QTest::qWaitFor([] { return owned(QStringLiteral("org.kde.KWin")); }, 20000),
                 "KWin is on the session bus");

        // The stand-in bar, and under it a window that fills what is left.
        m_bar = new BarStandIn(m_variant == QLatin1String("light") ? QColor(0xD8, 0xDD, 0xE6) : QColor(0x2C, 0x36, 0x48));
        m_bar->show();
        QVERIFY(QTest::qWaitForWindowExposed(m_bar));
        m_probe = new ProbeWindow;
        m_probe->showMaximized();
        QVERIFY(QTest::qWaitForWindowExposed(m_probe));
        QVERIFY2(probeHasTheKeyboard(), "the probe window has the keyboard");
        const QRectF client(probeWindow()->clientGeometry());
        report(QStringLiteral("stand-in bar %1 thick; the window beside it is at %2").arg(barThickness).arg(describe(client)));
        // KWin gives a maximised window what the bar leaves.
        QVERIFY2(client.bottom() <= height() - barThickness + 1, "the stand-in bar reserves its strip");

        const QImage before = capture();
        QVERIFY2(!before.isNull(), "KWin took a screenshot");
        QVERIFY2(isNear(colourAt(before, QPointF(100, height() - barThickness / 2.0)), m_bar->colour()),
                 "the stand-in bar is along the bottom edge");
        QCOMPARE(goldIn(before, band()), 0);

        // P@x's service: the stand-in that answers as the real one did.
        m_agent.setProcessChannelMode(QProcess::ForwardedChannels);
        m_agent.start(QStringLiteral(TUR_PAX_UI_AGENT), { QStringLiteral("--fixture"), QStringLiteral(TUR_PAX_UI_FIXTURES) });
        QVERIFY(m_agent.waitForStarted(15000));
        QVERIFY2(QTest::qWaitFor([] { return owned(agentService); }, 30000), "the stand-in for org.tur.Agent1 is there");
        QDBusMessage use = QDBusMessage::createMethodCall(agentService, QStringLiteral("/org/tur/Agent1Fixture"),
                                                          QStringLiteral("org.tur.Agent1Fixture"), QStringLiteral("Use"));
        use << QStringLiteral("{\"status\": \"ready\"}");
        QCOMPARE(bus().call(use, QDBus::Block, 20000).type(), QDBusMessage::ReplyMessage);

        const QString problem = startCompanion(true);
        QVERIFY2(problem.isEmpty(), qPrintable(problem));
    }

    void cleanupTestCase()
    {
        // As in tst_compositor.cpp: KWayland's FakeInput is left to the end
        // of the process.
        if (m_input)
            m_input->setParent(nullptr);
        for (QProcess *process : { &m_window, &m_companion, &m_agent }) {
            if (process->state() != QProcess::NotRunning) {
                process->terminate();
                if (!process->waitForFinished(10000))
                    process->kill();
                process->waitForFinished(5000);
            }
        }
        delete m_probe;
        delete m_bar;
    }

    // In the bar, at its end, in the middle of its thickness.
    void it_sits_in_the_bar()
    {
        const QImage picture = captureUntil([&](const QImage &image) { return characterIn(image, band()); });
        keep(picture, QStringLiteral("on-the-bar"));
        int count = 0;
        const QRectF box = goldBox(picture, band(), &count);
        QVERIFY2(count > 0, "the character is drawn");
        // Nothing of it is above the bar, over the window there.
        QCOMPARE(goldIn(picture, QRectF(width() - 200, height() - barThickness - 120, 200, 120)), 0);
        const QVariantMap where = anchor();
        report(QStringLiteral("the character's gold is at %1 (%2 device pixels); Anchor: x %3, lift %4, screen %5")
                   .arg(describe(box))
                   .arg(count)
                   .arg(where.value(QStringLiteral("x")).toDouble())
                   .arg(where.value(QStringLiteral("lift")).toDouble())
                   .arg(where.value(QStringLiteral("screen")).toString()));
        // Inside the bar's strip, clear of its top edge and of the screen's.
        QVERIFY2(box.top() >= band().top() + 4 && box.bottom() <= band().bottom() - 4, qPrintable(describe(box)));
        // At the bar's end: the box is 40 wide, 8 from the screen's edge.
        QVERIFY2(std::abs(box.center().x() - (width() - 28)) <= 4, qPrintable(describe(box)));
        // In the middle of a bar 64 thick, not at the screen's edge: the
        // bar's thickness was found.
        QVERIFY2(std::abs(box.center().y() - (height() - barThickness / 2.0)) <= 4, qPrintable(describe(box)));
        QVERIFY2(std::abs(where.value(QStringLiteral("x")).toDouble() - box.center().x()) <= 4,
                 "Anchor says where the character is");
        // What opens above it starts 8 above the bar's top edge.
        QCOMPARE(where.value(QStringLiteral("lift")).toDouble(), 8.0);
        QCOMPARE(where.value(QStringLiteral("screen")).toString(), m_screen->name());
        // It is not a window KWin manages: it cannot be switched to, and it
        // is not in the task bar.
        const auto windows = m_windows->windows();
        for (PlasmaWindow *window : windows)
            QVERIFY2(window->pid() != quint32(m_companion.processId()), "the character is a managed window");
    }

    void it_never_takes_the_keyboard_and_never_blocks_a_click()
    {
        QVERIFY(probeHasTheKeyboard());
        m_probe->deactivations = 0;
        m_probe->typed.clear();
        m_probe->clicks.clear();
        m_bar->clicks.clear();

        // Typing while it is there.
        typeKeys({ KEY_A, KEY_B });
        QCOMPARE(m_probe->typed, QStringLiteral("ab"));

        // A click on the bar, 100 px along from the character: the bar's.
        const QPointF onTheBar(width() - 128, height() - barThickness / 2.0);
        click(onTheBar);
        QCOMPARE(m_bar->clicks.size(), 1);
        report(QStringLiteral("a click on the bar at %1,%2, in the character's row, reached the bar at %3,%4")
                   .arg(onTheBar.x()).arg(onTheBar.y())
                   .arg(m_bar->clicks.constFirst().x()).arg(m_bar->clicks.constFirst().y()));
        QVERIFY(std::abs(m_bar->clicks.constFirst().x() - onTheBar.x()) <= 1.5);
        QVERIFY2(probeHasTheKeyboard(), "the window under the strip has the keyboard");
        m_probe->deactivations = 0;

        // It speaks: TUR repaired something.
        healingServiceRepairsSomething();
        QVERIFY2(stateBecomes(QStringLiteral("telling")), qPrintable(state()));
        // The bubble: 320 wide, its right edge 8 from the screen's, its
        // bottom edge 8 above the bar.
        const QPointF inBubble(width() - 8 - 320 + 12, height() - barThickness - 8 - 6);
        const QImage speaking =
            captureUntil([&](const QImage &image) { return isNear(colourAt(image, inBubble), overlayColour()); });
        keep(speaking, QStringLiteral("bubble"));
        QVERIFY2(isNear(colourAt(speaking, inBubble), overlayColour()), "the bubble is above the bar, beside the character");
        // Nothing of the bubble is over the bar.
        QVERIFY(isNear(colourAt(speaking, QPointF(width() - 200, height() - barThickness + 4)), m_bar->colour()));
        QVERIFY2(characterIn(speaking, band()), "the character is still in the bar");

        // Typing while it speaks: the window still has the keyboard, and
        // never lost it.
        typeKeys({ KEY_C });
        QCOMPARE(m_probe->typed, QStringLiteral("abc"));
        QCOMPARE(m_probe->deactivations, 0);

        // A click beside the bubble, at its height, goes to the window.
        m_probe->clicks.clear();
        const QRectF client(probeWindow()->clientGeometry());
        const QPointF beside(200, height() - barThickness - 40);
        click(beside);
        QCOMPARE(m_probe->clicks.size(), 1);
        const QPointF wanted = beside - client.topLeft();
        report(QStringLiteral("a click beside the bubble at %1,%2 reached the window at %3,%4 (expected %5,%6)")
                   .arg(beside.x()).arg(beside.y())
                   .arg(m_probe->clicks.constFirst().x()).arg(m_probe->clicks.constFirst().y())
                   .arg(wanted.x()).arg(wanted.y()));
        QVERIFY(std::abs(m_probe->clicks.constFirst().x() - wanted.x()) <= 1.5);
        QVERIFY(std::abs(m_probe->clicks.constFirst().y() - wanted.y()) <= 1.5);
        QCOMPARE(m_probe->deactivations, 0);
        QCOMPARE(state(), QStringLiteral("telling"));
    }

    void asked_for_it_has_the_keyboard_and_esc_gives_it_back()
    {
        QCOMPARE(state(), QStringLiteral("telling"));
        m_probe->typed.clear();
        QCOMPARE(companion(QStringLiteral("Attend")).type(), QDBusMessage::ReplyMessage);
        QTest::qWait(1200);
        // The keyboard is the character's now: this key does not reach the
        // window.
        typeKeys({ KEY_X });
        QVERIFY2(m_probe->typed.isEmpty(), qPrintable(QStringLiteral("the window was typed into: ") + m_probe->typed));
        // Esc: the bubble waits as a mark, and the keyboard goes back.
        typeKeys({ KEY_ESC });
        QVERIFY2(stateBecomes(QStringLiteral("noticing")), qPrintable(state()));
        QTest::qWait(800);
        if (!m_probe->isActive())
            report(QStringLiteral("after Esc the window did not have the keyboard again by itself"));
        QVERIFY2(probeHasTheKeyboard(), "the window has the keyboard again");
        typeKeys({ KEY_D });
        QCOMPARE(m_probe->typed, QStringLiteral("d"));
        const QImage marked = capture();
        keep(marked, QStringLiteral("a-mark-waits"));
    }

    void one_press_opens_the_ask_bar_beside_it()
    {
        QProcessEnvironment environment = QProcessEnvironment::systemEnvironment();
        environment.insert(QStringLiteral("QT_QPA_PLATFORM"), QStringLiteral("wayland"));
        m_window.setProcessEnvironment(environment);
        m_window.setProcessChannelMode(QProcess::ForwardedChannels);
        m_window.start(QStringLiteral(TUR_PAX_UI_PROGRAM),
                       { QStringLiteral("--background"), QStringLiteral("--standalone"), QStringLiteral("--variant"),
                         m_variant });
        QVERIFY(m_window.waitForStarted(15000));
        QVERIFY2(QTest::qWaitFor([] { return owned(windowService); }, 30000), "tur-pax-ui took org.tur.PaxUi1");
        QCOMPARE(property(windowService, windowPath, QStringLiteral("Mode")).toString(), QString());

        const QVariantMap where = anchor();
        const QPointF onTheCharacter(where.value(QStringLiteral("x")).toDouble(), height() - barThickness / 2.0);
        // Something waits: the first press says it.
        QCOMPARE(state(), QStringLiteral("noticing"));
        click(onTheCharacter);
        QVERIFY2(stateBecomes(QStringLiteral("telling")), qPrintable(state()));
        QCOMPARE(property(windowService, windowPath, QStringLiteral("Mode")).toString(), QString());
        // The next press opens P@x's ask bar.
        click(onTheCharacter);
        QVERIFY2(QTest::qWaitFor([] {
                     return property(windowService, windowPath, QStringLiteral("Mode")).toString() == QLatin1String("ask");
                 }, 20000),
                 "P@x's window shows its ask bar");
        // The character listens, and what it was saying made room.
        QVERIFY2(stateBecomes(QStringLiteral("listening")), qPrintable(state()));

        // The bar is 640 wide, centred on the character as far as the
        // screen allows, its bottom edge 8 above the bar: where the bubble
        // was.
        const double left = std::clamp(where.value(QStringLiteral("x")).toDouble() - 320.0, 8.0, width() - 648.0);
        const QPointF nearItsRightCorner(left + 640 - 12, height() - barThickness - 8 - 6);
        const QPointF nearItsLeftCorner(left + 12, height() - barThickness - 8 - 6);
        const QImage asking = captureUntil([&](const QImage &image) {
            return isNear(colourAt(image, nearItsRightCorner), overlayColour())
                   && isNear(colourAt(image, nearItsLeftCorner), overlayColour());
        });
        keep(asking, QStringLiteral("ask-bar-beside-the-character"));
        QVERIFY2(isNear(colourAt(asking, nearItsRightCorner), overlayColour()), "the ask bar reaches the character's side");
        QVERIFY2(isNear(colourAt(asking, nearItsLeftCorner), overlayColour()), "the ask bar is 640 wide from there");
        // Not over the bar, and the character is still in it.
        QVERIFY(isNear(colourAt(asking, QPointF(left + 320, height() - barThickness + 4)), m_bar->colour()));
        QVERIFY2(characterIn(asking, band()), "the character is still in the bar");
        report(QStringLiteral("the ask bar opened from x %1 to %2, its bottom edge %3 above the bar")
                   .arg(left).arg(left + 640).arg(8));

        // The person opened it, so it has the keyboard; an observation.
        m_probe->typed.clear();
        typeKeys({ KEY_H });
        report(QStringLiteral("a key typed while the ask bar is open %1 the window under it")
                   .arg(m_probe->typed.isEmpty() ? QStringLiteral("did not reach") : QStringLiteral("reached")));

        QDBusMessage hide = QDBusMessage::createMethodCall(windowService, windowPath, windowService, QStringLiteral("Hide"));
        QCOMPARE(bus().call(hide, QDBus::Block, 20000).type(), QDBusMessage::ReplyMessage);
        QVERIFY(QTest::qWaitFor([] {
            return property(windowService, windowPath, QStringLiteral("Mode")).toString().isEmpty();
        }, 20000));
        // The notice it folded still waits.
        QVERIFY2(stateBecomes(QStringLiteral("noticing")), qPrintable(state()));
    }

    void dragged_it_moves_along_the_bar_and_stays()
    {
        const QImage before = captureUntil([&](const QImage &image) { return characterIn(image, band()); });
        const QRectF was = goldBox(before, band());
        const QPointF from = was.center();
        m_input->requestPointerMoveAbsolute(from);
        m_connection->flush();
        QTest::qWait(200);
        m_input->requestPointerButtonPress(Qt::LeftButton);
        m_connection->flush();
        QTest::qWait(150);
        for (const double moved : { 14.0, 60.0, 120.0, 180.0, 240.0 }) {
            m_input->requestPointerMoveAbsolute(from - QPointF(moved, 0));
            m_connection->flush();
            QTest::qWait(150);
        }
        m_input->requestPointerButtonRelease(Qt::LeftButton);
        m_connection->flush();
        QTest::qWait(600);

        const QRectF wanted(from.x() - 240 - 30, band().y(), 60, band().height());
        const QImage after = captureUntil([&](const QImage &image) { return characterIn(image, wanted); });
        keep(after, QStringLiteral("dragged-along-the-bar"));
        const QRectF is = goldBox(after, band());
        report(QStringLiteral("dragged 240 to the left: the character went from %1 to %2").arg(describe(was), describe(is)));
        QVERIFY2(std::abs(is.center().x() - (from.x() - 240)) <= 6, qPrintable(describe(is)));
        QVERIFY2(std::abs(is.center().y() - was.center().y()) <= 2, "it stays in the bar's row");
        QVERIFY2(std::abs(anchor().value(QStringLiteral("x")).toDouble() - is.center().x()) <= 4, "Anchor follows it");
        // A drag is not a press: nothing opened.
        QCOMPARE(property(windowService, windowPath, QStringLiteral("Mode")).toString(), QString());
        QCOMPARE(state(), QStringLiteral("noticing"));
    }

    void hidden_it_is_gone_and_it_comes_back()
    {
        QCOMPARE(companion(QStringLiteral("Hide")).type(), QDBusMessage::ReplyMessage);
        const QImage hidden = captureUntil([&](const QImage &image) { return goldIn(image, band()) == 0; });
        keep(hidden, QStringLiteral("hidden"));
        QCOMPARE(goldIn(hidden, band()), 0);
        QCOMPARE(property(companionService, companionPath, QStringLiteral("Shown")).toBool(), false);
        QCOMPARE(companion(QStringLiteral("Show")).type(), QDBusMessage::ReplyMessage);
        const QImage back = captureUntil([&](const QImage &image) { return characterIn(image, band()); });
        QVERIFY2(characterIn(back, band()), "the character is back on the bar");
        QCOMPARE(property(companionService, companionPath, QStringLiteral("Shown")).toBool(), true);
    }

    void over_a_full_screen_window_it_stays_or_steps_aside_as_the_person_chose()
    {
        // The person's choice to stay (the switch "Stay over full-screen
        // windows", on for the tests above): visible over a full-screen
        // window, where the bar itself is not.
        m_probe->showFullScreen();
        const QPointF onTheBar(100, height() - barThickness / 2.0);
        const QImage full = captureUntil([&](const QImage &image) {
            return !isNear(colourAt(image, onTheBar), m_bar->colour()) && characterIn(image, band());
        });
        keep(full, QStringLiteral("over-a-full-screen-window"));
        QVERIFY2(!isNear(colourAt(full, onTheBar), m_bar->colour()), "the full-screen window covers the bar");
        QVERIFY2(characterIn(full, band()), "the character stays visible over the full-screen window");
        // And the full-screen window keeps the keyboard.
        QVERIFY2(probeHasTheKeyboard(), "the full-screen window has the keyboard");
        m_probe->typed.clear();
        m_probe->deactivations = 0;
        typeKeys({ KEY_F });
        QCOMPARE(m_probe->typed, QStringLiteral("f"));
        QCOMPARE(m_probe->deactivations, 0);

        // The default, stepping aside (the owner's decision of 15 November
        // 2026; the settings file says nothing about it): it leaves with the
        // bar.
        writeFile(m_work + QStringLiteral("/printed.json"), "[]\n");
        stopCompanion();
        const QString problem = startCompanion(false);
        QVERIFY2(problem.isEmpty(), qPrintable(problem));
        QTest::qWait(2500);
        const QImage aside = capture();
        keep(aside, QStringLiteral("stepped-aside-with-the-bar"));
        QCOMPARE(goldIn(aside, band()), 0);
        QCOMPARE(property(companionService, companionPath, QStringLiteral("Shown")).toBool(), true);

        // P@x's indicator says P@x is controlling: the character says the
        // same, and is where the person can see it.
        IndicatorStandIn indicator;
        QVERIFY(bus().registerObject(indicatorPath, indicatorService, &indicator, QDBusConnection::ExportAllProperties));
        QVERIFY(bus().registerService(indicatorService));
        QVERIFY2(stateBecomes(QStringLiteral("acting")), qPrintable(state()));
        const QImage acting = captureUntil([&](const QImage &image) { return characterIn(image, band()); });
        keep(acting, QStringLiteral("controlling-over-a-full-screen-window"));
        QVERIFY2(characterIn(acting, band()), "while P@x controls, the character is over the full-screen window");
        QCOMPARE(m_probe->deactivations, 0);

        // The indicator goes: the character steps aside again.
        bus().unregisterService(indicatorService);
        bus().unregisterObject(indicatorPath);
        QVERIFY2(stateBecomes(QStringLiteral("idle")), qPrintable(state()));
        const QImage again = captureUntil([&](const QImage &image) { return goldIn(image, band()) == 0; });
        QCOMPARE(goldIn(again, band()), 0);

        // Out of full screen: an observation of the top layer, where the
        // bar is too.
        m_probe->showMaximized();
        QTest::qWait(2000);
        const QImage beside = capture();
        keep(beside, QStringLiteral("on-the-top-layer-with-the-bar"));
        report(QStringLiteral("stepped aside (top layer), with the bar showing: the character %1 drawn above the "
                              "stand-in bar")
                   .arg(characterIn(beside, band()) ? QStringLiteral("is") : QStringLiteral("is not")));
    }
};

int main(int argc, char *argv[])
{
    QGuiApplication app(argc, argv);
    QGuiApplication::setDesktopFileName(QStringLiteral("org.tur.CompositorTest"));
    CompanionCompositorTest test;
    return QTest::qExec(&test, argc, argv);
}

#include "tst_companion.moc"
