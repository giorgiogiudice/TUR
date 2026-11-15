// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
//
// tur-pax-companion: P@x's character on the bar (docs/architecture/
// tur-pax-companion.md). A small flame that shows what P@x is doing, says
// the one sentence that matters when something happened that the person
// should hear of, and opens P@x's ask bar beside itself when pressed.
//
// It is a standalone program: it draws its own layer-shell surface over the
// bar and loads nothing into Plasma. It holds no authority: it listens to
// org.tur.Agent1 and reads the healing service's notices, and what it does
// (open P@x's window, stop P@x's task) any program of the person could do.
// It never takes the keyboard unless the person asks, never covers a
// window, never plays a sound, and never starts P@x.

#include "companion.h"
#include "companionservice.h"
#include "companionsettings.h"
#include "surfaces.h"
#include "turkit.h"

#include <QCommandLineOption>
#include <QCommandLineParser>
#include <QCoreApplication>
#include <QDBusConnection>
#include <QDBusConnectionInterface>
#include <QDateTime>
#include <QGuiApplication>
#include <QList>
#include <QMetaObject>
#include <QProcess>
#include <QQmlApplicationEngine>
#include <QQmlError>
#include <QString>
#include <QStringList>
#include <QTextStream>
#include <QTimer>
#include <QVariantMap>

#include <cstring>

namespace {

QString valueOf(int argc, char *argv[], const char *option)
{
    for (int i = 1; i + 1 < argc; ++i) {
        if (std::strcmp(argv[i], option) == 0)
            return QString::fromLocal8Bit(argv[i + 1]);
    }
    return QString();
}

bool asks(int argc, char *argv[], const char *option)
{
    for (int i = 1; i < argc; ++i) {
        if (std::strcmp(argv[i], option) == 0)
            return true;
    }
    return false;
}

QDBusConnection busFor(const QString &address, const QString &name)
{
    if (address.isEmpty())
        return QDBusConnection::sessionBus();
    return QDBusConnection::connectToBus(address, name);
}

bool running(const QDBusConnection &bus)
{
    QDBusConnectionInterface *names = bus.isConnected() ? bus.interface() : nullptr;
    return names && names->isServiceRegistered(CompanionService::serviceName()).value();
}

// --hide and --quiet: what the person chose is told to the character that
// is running, or written down for the next one. Needs no display.
int choose(int argc, char *argv[], const QString &method, const QVariantList &arguments)
{
    QCoreApplication app(argc, argv);
    QTextStream err(stderr);
    const QDBusConnection bus = busFor(valueOf(argc, argv, "--bus"), QStringLiteral("tur-pax-companion-command"));
    if (running(bus)) {
        QString problem;
        if (CompanionService::forward(bus, method, arguments, &problem))
            return 0;
        err << "tur-pax-companion: " << problem << '\n';
        return 1;
    }
    CompanionSettings settings;
    const QString path = asks(argc, argv, "--settings") ? valueOf(argc, argv, "--settings")
                                                        : CompanionSettings::defaultPath();
    settings.setPath(path);
    if (method == QLatin1String("Hide")) {
        settings.setHidden(true);
    } else if (!settings.setQuiet(arguments.value(0).toString(), double(QDateTime::currentMSecsSinceEpoch()))) {
        err << "tur-pax-companion: --quiet takes one of: " << CompanionService::quietModes().join(QStringLiteral(", "))
            << '\n';
        return 2;
    }
    return 0;
}

} // namespace

int main(int argc, char *argv[])
{
    if (asks(argc, argv, "--hide"))
        return choose(argc, argv, QStringLiteral("Hide"), {});
    if (asks(argc, argv, "--quiet"))
        return choose(argc, argv, QStringLiteral("SetQuiet"), { valueOf(argc, argv, "--quiet") });

    // A QGuiApplication, never a QApplication: that would load the GPL
    // widget style (desktop/README.md).
    QGuiApplication app(argc, argv);
    QCoreApplication::setApplicationName(QStringLiteral("tur-pax-companion"));
    QCoreApplication::setApplicationVersion(QStringLiteral(TUR_VERSION));
    QGuiApplication::setDesktopFileName(QStringLiteral("org.tur.PaxCompanion"));
    // The character comes and goes with P@x; that is not a reason to end.
    QGuiApplication::setQuitOnLastWindowClosed(false);

    QCommandLineParser parser;
    parser.setApplicationDescription(
        QStringLiteral("P@x's character on the bar: what P@x is doing, the one sentence that matters, and P@x's ask "
                       "bar one press away. A second start brings the running character back."));
    parser.addHelpOption();
    parser.addVersionOption();
    const QCommandLineOption show(QStringLiteral("show"),
                                  QStringLiteral("Put the character back on the bar if it was hidden (the default)."));
    const QCommandLineOption hide(QStringLiteral("hide"), QStringLiteral("Take the character off the bar, and exit."));
    const QCommandLineOption attend(QStringLiteral("attend"),
                                    QStringLiteral("Give the character the keyboard: what it is saying, or its "
                                                   "menu. Esc gives it back."));
    const QCommandLineOption quiet(QStringLiteral("quiet"),
                                   QStringLiteral("Make the character quiet: hour, tomorrow, always, or off. Exits."),
                                   QStringLiteral("MODE"));
    const QCommandLineOption background(QStringLiteral("background"),
                                        QStringLiteral("Started with the session: change nothing the person chose."));
    const QCommandLineOption bus(QStringLiteral("bus"),
                                 QStringLiteral("Use this D-Bus address instead of the session bus (tests)."),
                                 QStringLiteral("ADDRESS"));
    const QCommandLineOption variant(QStringLiteral("variant"),
                                     QStringLiteral("Start in the dark or the light variant instead of following the "
                                                    "desktop."),
                                     QStringLiteral("dark|light"));
    const QCommandLineOption standalone(QStringLiteral("standalone"),
                                        QStringLiteral("Use the look of TUR's programs outside a session: the Basic "
                                                       "style and the Inter font."));
    const QCommandLineOption settings(QStringLiteral("settings"),
                                      QStringLiteral("Keep the person's choices in this file; empty: keep nothing."),
                                      QStringLiteral("FILE"));
    const QCommandLineOption healCommand(QStringLiteral("heal-command"),
                                         QStringLiteral("The program that prints the healing service's notices, "
                                                        "instead of \"tur-diag heal notices --json\" (tests)."),
                                         QStringLiteral("COMMAND"));
    const QCommandLineOption healState(QStringLiteral("heal-state"),
                                       QStringLiteral("The healing service's state file to watch (tests)."),
                                       QStringLiteral("FILE"));
    const QCommandLineOption barInset(QStringLiteral("bar-inset"),
                                      QStringLiteral("The bar's thickness, where the compositor cannot be asked."),
                                      QStringLiteral("PIXELS"));
    const QCommandLineOption noDBus(QStringLiteral("no-dbus"),
                                    QStringLiteral("Do not offer org.tur.PaxCompanion1, and start even if a "
                                                   "character is running."));
    const QCommandLineOption check(QStringLiteral("check"),
                                   QStringLiteral("Load the character, report any QML warning, and exit. For tests."));
    parser.addOptions({ show, hide, attend, quiet, background, bus, variant, standalone, settings, healCommand,
                        healState, barInset, noDBus, check });
    parser.process(app);

    QTextStream err(stderr);
    if (parser.isSet(standalone))
        TurKit::useStandaloneLook();

    // One character per person: a second start speaks to the first.
    CompanionService service;
    const bool offered = !parser.isSet(noDBus) && !parser.isSet(check);
    if (offered) {
        const QDBusConnection connection = busFor(parser.value(bus), QStringLiteral("tur-pax-companion"));
        QString problem;
        if (running(connection)) {
            if (parser.isSet(background))
                return 0;
            const QString method = parser.isSet(attend) ? QStringLiteral("Attend") : QStringLiteral("Show");
            if (CompanionService::forward(connection, method, {}, &problem))
                return 0;
            err << "tur-pax-companion: the running character did not answer (" << problem << "); starting another\n";
        } else if (!service.registerOn(connection, &problem)) {
            err << "tur-pax-companion: " << problem << "; other programs cannot reach the character\n";
        }
    }

    QQmlApplicationEngine engine;
    int warnings = 0;
    QObject::connect(&engine, &QQmlEngine::warnings, &app, [&warnings](const QList<QQmlError> &list) {
        warnings += int(list.size());
        QTextStream out(stderr);
        for (const QQmlError &error : list)
            out << "tur-pax-companion: QML warning: " << error.toString() << '\n';
    });
    const QString settingsPath = parser.isSet(settings) ? parser.value(settings)
                                 : parser.isSet(check) ? QString() : CompanionSettings::defaultPath();
    QVariantMap initial {
        { QStringLiteral("busAddress"), parser.value(bus) },
        { QStringLiteral("variant"), parser.value(variant) },
        { QStringLiteral("settingsPath"), settingsPath },
        { QStringLiteral("healStateFile"), parser.value(healState) },
        { QStringLiteral("standInInset"), parser.value(barInset).toDouble() },
    };
    if (parser.isSet(healCommand))
        initial.insert(QStringLiteral("healCommand"), QProcess::splitCommand(parser.value(healCommand)));
    engine.setInitialProperties(initial);
    engine.loadFromModule("Tur.PaxCompanion", "Main");
    if (engine.rootObjects().isEmpty()) {
        err << "tur-pax-companion: the character could not be loaded\n";
        return 2;
    }
    QObject *root = engine.rootObjects().constFirst();
    auto *companion = root->property("companion").value<Companion *>();
    service.setCompanion(companion);

    // Started by the person, not with the session: they want it on the bar.
    if (!parser.isSet(background) && !parser.isSet(check) && companion && companion->settings())
        companion->settings()->setHidden(false);

    Surfaces surfaces(root, &service);
    err << "tur-pax-companion: platform " << QGuiApplication::platformName() << ", "
        << (surfaces.layerShell() ? "a layer-shell surface on the bar" : "an ordinary window") << '\n';
    err.flush();

    QObject::connect(&service, &CompanionService::attendRequested, root,
                     [root] { QMetaObject::invokeMethod(root, "attend"); });
    if (parser.isSet(attend))
        QTimer::singleShot(0, root, [root] { QMetaObject::invokeMethod(root, "attend"); });

    if (parser.isSet(check)) {
        // Long enough for the first frame and the sources' first answers.
        QTimer::singleShot(1500, &app, [&app, &warnings] {
            if (warnings > 0)
                QTextStream(stderr) << "tur-pax-companion: " << warnings << " QML warning(s)\n";
            app.exit(warnings > 0 ? 3 : 0);
        });
    }
    return app.exec();
}
