// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
//
// tur-pax-ui: the window where a person talks to P@x (docs/architecture/
// desktop-experience.md, sections 9.1, 9.2 and 9.12; docs/architecture/
// tur-pax-ui.md). The ask bar, the conversation panel with every step as it
// happens, and P@x's setup, over org.tur.Agent1 on the session bus.
//
// It holds no authority of its own: everything it does is a call that any
// program of the person could make (pax-service.md, section 3). It keeps no
// key and no conversation on disk.

#include "agentclient.h"
#include "surface.h"
#include "turkit.h"
#include "uiservice.h"

#include <QCommandLineOption>
#include <QCommandLineParser>
#include <QCoreApplication>
#include <QDBusConnection>
#include <QDBusConnectionInterface>
#include <QDBusMessage>
#include <QDBusVariant>
#include <QGuiApplication>
#include <QList>
#include <QMetaObject>
#include <QQmlApplicationEngine>
#include <QQmlError>
#include <QString>
#include <QTextStream>
#include <QTimer>
#include <QVariantMap>

#include <cstring>

namespace {

bool asks(int argc, char *argv[], const char *option)
{
    for (int i = 1; i < argc; ++i) {
        if (std::strcmp(argv[i], option) == 0)
            return true;
    }
    return false;
}

QString valueOf(int argc, char *argv[], const char *option)
{
    for (int i = 1; i + 1 < argc; ++i) {
        if (std::strcmp(argv[i], option) == 0)
            return QString::fromLocal8Bit(argv[i + 1]);
    }
    return QString();
}

QDBusConnection busFor(const QString &address)
{
    if (address.isEmpty())
        return QDBusConnection::sessionBus();
    return QDBusConnection::connectToBus(address, QStringLiteral("tur-pax-ui-command"));
}

// --stop: what P@x's stop key runs (data/org.tur.PaxUi.desktop). Stops the
// running task, if P@x is running; never starts P@x's service to do it.
int stopTask(const QString &address)
{
    QTextStream err(stderr);
    QDBusConnection bus = busFor(address);
    if (!bus.isConnected()) {
        err << "tur-pax-ui: there is no session bus, so there is no P@x to stop\n";
        return 1;
    }
    QDBusMessage call = QDBusMessage::createMethodCall(AgentClient::serviceName(), AgentClient::objectPath(),
                                                       AgentClient::interfaceName(), QStringLiteral("CancelTask"));
    call << QString();
    call.setAutoStartService(false);
    const QDBusMessage reply = bus.call(call, QDBus::Block, 10000);
    if (reply.type() != QDBusMessage::ErrorMessage) {
        err << "tur-pax-ui: P@x's task was stopped\n";
        return 0;
    }
    const QString name = reply.errorName();
    if (name == QLatin1String("org.freedesktop.DBus.Error.ServiceUnknown")
        || name == QLatin1String("org.freedesktop.DBus.Error.NameHasNoOwner")) {
        err << "tur-pax-ui: P@x is not running; nothing to stop\n";
        return 0;
    }
    if (AgentClient::codeOfError(name) == QLatin1String("TUR-AI-SERVICE-007")) {
        err << "tur-pax-ui: P@x is doing nothing; nothing to stop\n";
        return 0;
    }
    err << "tur-pax-ui: " << reply.errorMessage() << '\n';
    return 1;
}

// --show-keys: what the key that shows P@x's keys runs. Shows the list on
// P@x's indicator (org.tur.PaxOverlay1.ShowCommands), or takes it away.
int showKeys(const QString &address)
{
    QTextStream err(stderr);
    QDBusConnection bus = busFor(address);
    if (!bus.isConnected()) {
        err << "tur-pax-ui: there is no session bus\n";
        return 1;
    }
    const QString service = QStringLiteral("org.tur.PaxOverlay1");
    const QString path = QStringLiteral("/org/tur/PaxOverlay1");
    QDBusMessage get = QDBusMessage::createMethodCall(service, path, QStringLiteral("org.freedesktop.DBus.Properties"),
                                                      QStringLiteral("Get"));
    get << service << QStringLiteral("CommandsShown");
    get.setAutoStartService(false);
    const QDBusMessage now = bus.call(get, QDBus::Block, 10000);
    if (now.type() == QDBusMessage::ErrorMessage) {
        err << "tur-pax-ui: P@x's indicator is not running: " << now.errorMessage() << '\n';
        return 1;
    }
    const bool shown = now.arguments().value(0).value<QDBusVariant>().variant().toBool();
    QDBusMessage call = QDBusMessage::createMethodCall(service, path, service, QStringLiteral("ShowCommands"));
    call << !shown;
    call.setAutoStartService(false);
    const QDBusMessage reply = bus.call(call, QDBus::Block, 10000);
    if (reply.type() == QDBusMessage::ErrorMessage) {
        err << "tur-pax-ui: " << reply.errorMessage() << '\n';
        return 1;
    }
    return 0;
}

} // namespace

int main(int argc, char *argv[])
{
    // The keys' commands need no display: they run, act and end.
    if (asks(argc, argv, "--stop")) {
        QCoreApplication app(argc, argv);
        return stopTask(valueOf(argc, argv, "--bus"));
    }
    if (asks(argc, argv, "--show-keys")) {
        QCoreApplication app(argc, argv);
        return showKeys(valueOf(argc, argv, "--bus"));
    }

    // A QGuiApplication, never a QApplication: that would load the GPL
    // widget style (desktop/README.md).
    QGuiApplication app(argc, argv);
    QCoreApplication::setApplicationName(QStringLiteral("tur-pax-ui"));
    QCoreApplication::setApplicationVersion(QStringLiteral(TUR_VERSION));
    QGuiApplication::setDesktopFileName(QStringLiteral("org.tur.PaxUi"));
    // The surface comes and goes; hiding it is not a reason to end.
    QGuiApplication::setQuitOnLastWindowClosed(false);

    QCommandLineParser parser;
    parser.setApplicationDescription(
        QStringLiteral("P@x's window: the ask bar, the conversation panel and P@x's setup, over org.tur.Agent1 on "
                       "the session bus. A second start shows the window that is open."));
    parser.addHelpOption();
    parser.addVersionOption();
    const QCommandLineOption mode(QStringLiteral("mode"),
                                  QStringLiteral("What to show: ask (the default), panel, activity or setup."),
                                  QStringLiteral("MODE"), QStringLiteral("ask"));
    const QCommandLineOption background(QStringLiteral("background"),
                                        QStringLiteral("Start with nothing shown and wait to be asked over "
                                                       "org.tur.PaxUi1 (D-Bus activation)."));
    const QCommandLineOption bus(QStringLiteral("bus"),
                                 QStringLiteral("Talk to P@x's service on this D-Bus address instead of the "
                                                "session bus (tests)."),
                                 QStringLiteral("ADDRESS"));
    const QCommandLineOption variant(QStringLiteral("variant"),
                                     QStringLiteral("Start in the dark or the light variant instead of following the "
                                                    "desktop."),
                                     QStringLiteral("dark|light"));
    const QCommandLineOption standalone(QStringLiteral("standalone"),
                                        QStringLiteral("Use the look of TUR's programs outside a session: the Basic "
                                                       "style and the Inter font."));
    const QCommandLineOption noDBus(QStringLiteral("no-dbus"),
                                    QStringLiteral("Do not offer org.tur.PaxUi1, and open a window even if one is "
                                                   "open."));
    const QCommandLineOption check(QStringLiteral("check"),
                                   QStringLiteral("Load the window, report any QML warning, and exit. For tests."));
    const QCommandLineOption stop(QStringLiteral("stop"),
                                  QStringLiteral("Stop P@x's running task, if P@x is running, and exit. Never starts "
                                                 "P@x. What P@x's stop key runs."));
    const QCommandLineOption keys(QStringLiteral("show-keys"),
                                  QStringLiteral("Show P@x's keys on its indicator, or take them away, and exit."));
    parser.addOptions({ mode, background, bus, variant, standalone, noDBus, check, stop, keys });
    parser.process(app);

    QTextStream err(stderr);
    const QString shown = parser.isSet(background) ? QString() : parser.value(mode);
    if (!shown.isEmpty() && !UiService::modes().contains(shown)) {
        err << "tur-pax-ui: --mode takes one of: " << UiService::modes().join(QStringLiteral(", ")) << '\n';
        return 2;
    }

    if (parser.isSet(standalone))
        TurKit::useStandaloneLook();

    // One window per person: a second start shows the first one's.
    UiService service;
    if (!parser.isSet(noDBus) && !parser.isSet(check)) {
        QDBusConnection session = QDBusConnection::sessionBus();
        QString problem;
        QDBusConnectionInterface *names = session.isConnected() ? session.interface() : nullptr;
        if (names && names->isServiceRegistered(UiService::serviceName()).value()) {
            if (shown.isEmpty())
                return 0;
            if (UiService::forward(session, shown, &problem))
                return 0;
            err << "tur-pax-ui: the open window did not answer (" << problem << "); opening another\n";
        } else if (!service.registerOn(session, &problem)) {
            err << "tur-pax-ui: " << problem << "; other programs cannot open this window\n";
        }
    }

    QQmlApplicationEngine engine;
    int warnings = 0;
    QObject::connect(&engine, &QQmlEngine::warnings, &app, [&warnings](const QList<QQmlError> &list) {
        warnings += int(list.size());
        QTextStream out(stderr);
        for (const QQmlError &error : list)
            out << "tur-pax-ui: QML warning: " << error.toString() << '\n';
    });
    engine.setInitialProperties(QVariantMap {
        { QStringLiteral("mode"), shown },
        { QStringLiteral("busAddress"), parser.value(bus) },
        { QStringLiteral("variant"), parser.value(variant) },
    });
    engine.loadFromModule("Tur.PaxUi", "Main");
    if (engine.rootObjects().isEmpty()) {
        err << "tur-pax-ui: the window could not be loaded\n";
        return 2;
    }
    QObject *root = engine.rootObjects().constFirst();
    Surface surface(root);
    err << "tur-pax-ui: platform " << QGuiApplication::platformName() << ", "
        << (surface.layerShell() ? "a layer-shell surface" : "an ordinary window") << '\n';
    err.flush();

    const auto showMode = [root](const QString &asked) {
        if (!QMetaObject::invokeMethod(root, "show", Q_ARG(QString, asked)))
            root->setProperty("mode", asked);
    };
    QObject::connect(&service, &UiService::showRequested, root, showMode);
    QObject::connect(&service, &UiService::hideRequested, root, [root] { root->setProperty("mode", QString()); });

    if (parser.isSet(check)) {
        // Long enough for the first frame and the service's first answers.
        QTimer::singleShot(1500, &app, [&app, &warnings] {
            if (warnings > 0)
                QTextStream(stderr) << "tur-pax-ui: " << warnings << " QML warning(s)\n";
            app.exit(warnings > 0 ? 3 : 0);
        });
    }
    return app.exec();
}
