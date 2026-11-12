// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
//
// tur-pax-overlay: P@x's presence on the screen (docs/architecture/
// desktop-experience.md, sections 9.3 to 9.6; docs/architecture/
// tur-pax-overlay.md). The activity indicator, P@x's own pointer and its
// highlights, drawn over everything on every screen, driven over
// org.tur.PaxOverlay1.
//
// It draws and does nothing else. Every pixel lets input through to what is
// under it, it never takes the keyboard, it holds no authority, and it is not
// a security boundary: any program in the session could draw the same.

#include "bustypes.h"
#include "kwinlocator.h"
#include "overlayservice.h"
#include "overlaystate.h"
#include "surfaces.h"
#include "turkit.h"
#include "windowlocator.h"

#include <QCommandLineOption>
#include <QCommandLineParser>
#include <QCoreApplication>
#include <QDBusConnection>
#include <QDBusMessage>
#include <QDBusVariant>
#include <QGuiApplication>
#include <QList>
#include <QQmlEngine>
#include <QQmlError>
#include <QTextStream>
#include <QTimer>

#include <cstring>
#include <memory>

namespace {

// --hide: tells the overlay that is running to switch off, and exits. Needs
// no display.
int hideRunning()
{
    QDBusConnection bus = QDBusConnection::sessionBus();
    if (!bus.isConnected()) {
        QTextStream(stderr) << "tur-pax-overlay: there is no session bus\n";
        return 1;
    }
    const QDBusMessage call = QDBusMessage::createMethodCall(PaxOverlay::Bus::service(), PaxOverlay::Bus::path(),
                                                             PaxOverlay::Bus::interface(), QStringLiteral("Hide"));
    const QDBusMessage reply = bus.call(call, QDBus::Block, 10000);
    if (reply.type() == QDBusMessage::ErrorMessage) {
        // Not running is as off as it gets.
        if (reply.errorName() == QLatin1String("org.freedesktop.DBus.Error.ServiceUnknown"))
            return 0;
        QTextStream(stderr) << "tur-pax-overlay: " << reply.errorMessage() << '\n';
        return 1;
    }
    return 0;
}

// --commands: shows the list of P@x's keys on the running overlay, or takes
// it away if it is shown; a binding can run it until tur-pax-ui calls
// ShowCommands itself.
int toggleCommands()
{
    QDBusConnection bus = QDBusConnection::sessionBus();
    if (!bus.isConnected()) {
        QTextStream(stderr) << "tur-pax-overlay: there is no session bus\n";
        return 1;
    }
    QDBusMessage get = QDBusMessage::createMethodCall(PaxOverlay::Bus::service(), PaxOverlay::Bus::path(),
                                                      QStringLiteral("org.freedesktop.DBus.Properties"),
                                                      QStringLiteral("Get"));
    get << PaxOverlay::Bus::interface() << QStringLiteral("CommandsShown");
    const QDBusMessage now = bus.call(get, QDBus::Block, 10000);
    if (now.type() == QDBusMessage::ErrorMessage) {
        QTextStream(stderr) << "tur-pax-overlay: " << now.errorMessage() << '\n';
        return 1;
    }
    const bool shown = now.arguments().value(0).value<QDBusVariant>().variant().toBool();
    QDBusMessage call = QDBusMessage::createMethodCall(PaxOverlay::Bus::service(), PaxOverlay::Bus::path(),
                                                       PaxOverlay::Bus::interface(), QStringLiteral("ShowCommands"));
    call << !shown;
    const QDBusMessage reply = bus.call(call, QDBus::Block, 10000);
    if (reply.type() == QDBusMessage::ErrorMessage) {
        QTextStream(stderr) << "tur-pax-overlay: " << reply.errorMessage() << '\n';
        return 1;
    }
    return 0;
}

bool asks(int argc, char *argv[], const char *option)
{
    for (int i = 1; i < argc; ++i) {
        if (std::strcmp(argv[i], option) == 0)
            return true;
    }
    return false;
}

} // namespace

int main(int argc, char *argv[])
{
    if (asks(argc, argv, "--hide")) {
        QCoreApplication app(argc, argv);
        return hideRunning();
    }
    if (asks(argc, argv, "--commands")) {
        QCoreApplication app(argc, argv);
        return toggleCommands();
    }

    // A QGuiApplication, never a QApplication: that would load the GPL
    // widget style (desktop/README.md).
    QGuiApplication app(argc, argv);
    QCoreApplication::setApplicationName(QStringLiteral("tur-pax-overlay"));
    QCoreApplication::setApplicationVersion(QStringLiteral(TUR_VERSION));
    QGuiApplication::setDesktopFileName(QStringLiteral("org.tur.PaxOverlay"));
    // It has no window most of the time; hiding the last one is not a reason
    // to exit.
    QGuiApplication::setQuitOnLastWindowClosed(false);

    QCommandLineParser parser;
    parser.setApplicationDescription(
        QStringLiteral("P@x's indicator, pointer and highlights, over every screen, driven over "
                       "org.tur.PaxOverlay1 on the session bus. Draws only; takes no input."));
    parser.addHelpOption();
    parser.addVersionOption();
    const QCommandLineOption hide(QStringLiteral("hide"),
                                  QStringLiteral("Switch the running overlay off at once, and exit."));
    const QCommandLineOption commands(QStringLiteral("commands"),
                                      QStringLiteral("Show the list of P@x's keys on the running overlay, or take it "
                                                     "away, and exit."));
    const QCommandLineOption noDBus(QStringLiteral("no-dbus"), QStringLiteral("Do not use the session bus."));
    const QCommandLineOption check(QStringLiteral("check"),
                                   QStringLiteral("Load the surfaces, report any QML warning, and exit. For tests."));
    parser.addOptions({ hide, commands, noDBus, check });
    parser.process(app);

    // The look of TUR's programs outside the session's theme: the overlay
    // sits over content TUR does not control and has a fixed palette
    // (design, 5.7).
    TurKit::useStandaloneLook();

    QTextStream err(stderr);
    QQmlEngine engine;
    int warnings = 0;
    QObject::connect(&engine, &QQmlEngine::warnings, &app, [&warnings](const QList<QQmlError> &list) {
        warnings += int(list.size());
        QTextStream out(stderr);
        for (const QQmlError &error : list)
            out << "tur-pax-overlay: QML warning: " << error.toString() << '\n';
    });

    OverlayState state;
    std::unique_ptr<WindowLocator> locator;
    if (QGuiApplication::platformName() == QLatin1String("wayland"))
        locator = std::make_unique<KWinWindowLocator>();
    else
        locator = std::make_unique<NoWindowLocator>(
            QStringLiteral("windows cannot be found: not a Wayland session (platform %1)")
                .arg(QGuiApplication::platformName()));

    OverlayService service(&state, locator.get());
    QObject::connect(&service, &OverlayService::leaseBegan, &app, [](const QString &owner, const QString &purpose) {
        QTextStream(stderr) << "tur-pax-overlay: lease taken by " << owner << " (" << purpose << ")\n";
    });
    QObject::connect(&service, &OverlayService::leaseEnded, &app, [](const QString &owner, const QString &reason) {
        QTextStream(stderr) << "tur-pax-overlay: lease of " << owner << " ended: " << reason << '\n';
    });
    QObject::connect(&service, &OverlayService::shownChanged, &app, [](bool shown) {
        QTextStream(stderr) << "tur-pax-overlay: shown " << (shown ? "yes" : "no") << '\n';
    });

    SurfaceHost surfaces(&engine, &state, &service);
    for (const QString &problem : surfaces.problems())
        err << "tur-pax-overlay: " << problem << '\n';
    err << "tur-pax-overlay: platform " << QGuiApplication::platformName() << ", "
        << (surfaces.layerShell() ? "layer-shell surfaces on the overlay layer" : "plain windows") << ", "
        << QGuiApplication::screens().size() << " screen(s); " << locator->mechanism() << '\n';
    err.flush();

    if (!parser.isSet(noDBus) && !parser.isSet(check)) {
        QString problem;
        if (!service.registerOn(QDBusConnection::sessionBus(), &problem)) {
            err << "tur-pax-overlay: " << problem << '\n';
            return 1;
        }
    }

    if (parser.isSet(check)) {
        QTimer::singleShot(800, &app, [&app, &warnings, &surfaces] {
            const int problems = int(surfaces.problems().size());
            if (warnings + problems > 0)
                QTextStream(stderr) << "tur-pax-overlay: " << warnings + problems << " problem(s)\n";
            app.exit(warnings + problems > 0 ? 3 : 0);
        });
    }
    return app.exec();
}
