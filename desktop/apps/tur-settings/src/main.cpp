// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
//
// tur-settings: TUR's settings (docs/architecture/desktop-experience.md,
// section 10; docs/architecture/tur-settings.md). The shell, the pages
// Overview, Appearance and About, and the addressing: every page and every
// row has an address, and the program can be opened at one, by the command
// line, by a link or over D-Bus, and says where on screen the row is.
//
// It changes no setting.

#include "address.h"
#include "bustypes.h"
#include "codes.h"
#include "registry.h"
#include "turkit.h"

#include <QCommandLineOption>
#include <QCommandLineParser>
#include <QCoreApplication>
#include <QDBusConnection>
#include <QDBusConnectionInterface>
#include <QDBusMessage>
#include <QFileInfo>
#include <QGuiApplication>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QList>
#include <QQmlApplicationEngine>
#include <QQmlError>
#include <QString>
#include <QStringList>
#include <QTextStream>
#include <QTimer>
#include <QVariantMap>

#include <cstring>
#include <memory>

namespace {

struct Options
{
    QCommandLineOption fixture { QStringLiteral("fixture"),
                                 QStringLiteral("Read what the system says it is from DIR/tur-release and "
                                                "DIR/os-release instead of from the system."),
                                 QStringLiteral("DIR") };
    QCommandLineOption registry { QStringLiteral("registry"),
                                  QStringLiteral("Also read the settings registries (*.json) of other programs in DIR. "
                                                 "Default: /usr/share/tur/settings."),
                                  QStringLiteral("DIR"), QStringLiteral("/usr/share/tur/settings") };
    QCommandLineOption systemSettings { QStringLiteral("system-settings-program"),
                                        QStringLiteral("The program that is KDE System Settings. Default: "
                                                       "systemsettings, on the PATH."),
                                        QStringLiteral("PROGRAM"), QStringLiteral("systemsettings") };
    QCommandLineOption standalone { QStringLiteral("standalone"),
                                    QStringLiteral("Use the look of TUR's programs outside a session: the Basic style "
                                                   "and the Inter font, whatever the desktop says.") };
    QCommandLineOption variant { QStringLiteral("variant"),
                                 QStringLiteral("Start in the dark or the light variant instead of following the "
                                                "desktop."),
                                 QStringLiteral("dark|light") };
    QCommandLineOption rightToLeft { QStringLiteral("rtl"), QStringLiteral("Lay the window out right to left.") };
    QCommandLineOption developer { QStringLiteral("developer"),
                                   QStringLiteral("Developer mode: show the address of what is on screen.") };
    QCommandLineOption noDBus { QStringLiteral("no-dbus"),
                                QStringLiteral("Do not use the session bus: no org.tur.Settings1, and a second window "
                                               "if one is already open.") };
    QCommandLineOption check { QStringLiteral("check"),
                               QStringLiteral("Load the window, report any QML warning, and exit. For tests.") };
    QCommandLineOption search { QStringLiteral("search"),
                                QStringLiteral("Print, as JSON, the settings that match WORDS, and exit. Opens no "
                                               "window."),
                                QStringLiteral("WORDS") };
    QCommandLineOption resolve { QStringLiteral("resolve"),
                                 QStringLiteral("Print, as JSON, the registry entry of ADDRESS, and exit. Opens no "
                                                "window."),
                                 QStringLiteral("ADDRESS") };
    QCommandLineOption list { QStringLiteral("list"),
                              QStringLiteral("Print this program's settings registry, and exit. Opens no window.") };

    void addTo(QCommandLineParser &parser) const
    {
        parser.setApplicationDescription(
            QStringLiteral("TUR's settings. With an address, opens at that page; if the address names a row, brings "
                           "it into view and highlights it. This version changes no setting."));
        parser.addHelpOption();
        parser.addVersionOption();
        parser.addPositionalArgument(
            QStringLiteral("address"),
            QStringLiteral("What to open: a page, such as tur.appearance, or a row, such as tur.appearance#variant. "
                           "Also as a link: tur-settings:tur.appearance#variant."),
            QStringLiteral("[address]"));
        parser.addOptions({ fixture, registry, systemSettings, standalone, variant, rightToLeft, developer, noDBus,
                            check, search, resolve, list });
    }
};

// True if the command line asks a question that needs no window. Looked at
// before any application object exists, because a program asked only to
// search must not need a display.
bool asksOnly(int argc, char *argv[])
{
    for (int i = 1; i < argc; ++i) {
        if (std::strcmp(argv[i], "--") == 0)
            return false;
        for (const char *name : { "--search", "--resolve", "--list", "--help", "-h", "--help-all", "--version", "-v" }) {
            const std::size_t length = std::strlen(name);
            if (std::strncmp(argv[i], name, length) == 0 && (argv[i][length] == '\0' || argv[i][length] == '='))
                return true;
        }
    }
    return false;
}

// --search, --resolve, --list: answers from the registries and exits.
// Status 0, or 6 when nothing matches (as tur-catalogue), or 2 for an address
// that is not one or is not registered.
int answer(const QCommandLineParser &parser, const Options &options)
{
    QTextStream out(stdout);
    QTextStream err(stderr);
    Registry registry;
    if (parser.isSet(options.registry) || QFileInfo::exists(parser.value(options.registry)))
        registry.setDirectory(parser.value(options.registry));
    for (const QString &problem : registry.problems())
        err << "tur-settings: " << problem << '\n';

    if (parser.isSet(options.list)) {
        out << QString::fromUtf8(registry.data().document(Registry::ownProgram()));
        return 0;
    }
    if (parser.isSet(options.resolve)) {
        const QString text = parser.value(options.resolve);
        const QVariantMap entry = registry.entry(Settings::Address::parse(text).toString());
        if (entry.isEmpty()) {
            const Settings::Code code = Settings::Address::parse(text).isValid() ? Settings::Code::UnknownAddress
                                                                                : Settings::Code::NotAnAddress;
            err << "tur-settings: " << Settings::codeText(code) << ": " << Settings::titleOf(code) << '\n';
            return 2;
        }
        out << QJsonDocument(QJsonObject::fromVariantMap(entry)).toJson(QJsonDocument::Indented);
        return 0;
    }
    const QVariantList hits = registry.search(parser.value(options.search), 20);
    QJsonObject document;
    document.insert(QStringLiteral("query"), parser.value(options.search));
    document.insert(QStringLiteral("hits"), QJsonArray::fromVariantList(hits));
    out << QJsonDocument(document).toJson(QJsonDocument::Indented);
    return hits.isEmpty() ? 6 : 0;
}

// If a Settings window already answers on the session bus, hands it the
// address and returns its verdict: 0 if it went there, 1 if it refused.
// Returns -1 if there is none, and this process should become the window.
int handOver(const QString &address)
{
    QDBusConnection bus = QDBusConnection::sessionBus();
    if (!bus.isConnected() || !bus.interface())
        return -1;
    if (!bus.interface()->isServiceRegistered(Settings::Bus::settingsService()).value())
        return -1;

    const Settings::Address parsed = Settings::Address::parse(address);
    const bool row = parsed.isValid() && parsed.hasRow();
    QDBusMessage call = QDBusMessage::createMethodCall(Settings::Bus::settingsService(), Settings::Bus::settingsPath(),
                                                       Settings::Bus::settingsInterface(),
                                                       row ? QStringLiteral("Reveal") : QStringLiteral("Open"));
    call << address;
    if (row)
        call << QVariantMap();
    const QDBusMessage reply = bus.call(call, QDBus::Block, 20000);
    if (reply.type() == QDBusMessage::ErrorMessage) {
        QTextStream(stderr) << "tur-settings: " << reply.errorMessage() << '\n';
        return 1;
    }
    return 0;
}

} // namespace

int main(int argc, char *argv[])
{
    const Options options;

    if (asksOnly(argc, argv)) {
        QCoreApplication app(argc, argv);
        QCoreApplication::setApplicationName(QStringLiteral("tur-settings"));
        QCoreApplication::setApplicationVersion(QStringLiteral(TUR_VERSION));
        QCommandLineParser parser;
        options.addTo(parser);
        parser.process(app);
        return answer(parser, options);
    }

    // A QGuiApplication, never a QApplication: that would load the GPL
    // widget style (desktop/README.md).
    QGuiApplication app(argc, argv);
    QCoreApplication::setApplicationName(QStringLiteral("tur-settings"));
    QCoreApplication::setApplicationVersion(QStringLiteral(TUR_VERSION));
    // The identifier of the desktop file, which is the window's application
    // identifier under Wayland and the name of the program in the registry.
    QGuiApplication::setDesktopFileName(Registry::ownProgram());

    QCommandLineParser parser;
    options.addTo(parser);
    parser.process(app);

    const QStringList positional = parser.positionalArguments();
    const QString address = positional.isEmpty() ? QString() : positional.constFirst();
    const bool useBus = !parser.isSet(options.noDBus) && !parser.isSet(options.check);

    // One Settings window: a second start goes to the first.
    if (useBus) {
        const int handed = handOver(address);
        if (handed >= 0)
            return handed;
    }

    if (parser.isSet(options.standalone))
        TurKit::useStandaloneLook();
    if (parser.isSet(options.rightToLeft))
        QGuiApplication::setLayoutDirection(Qt::RightToLeft);

    QQmlApplicationEngine engine;
    int warnings = 0;
    QObject::connect(&engine, &QQmlEngine::warnings, &app, [&warnings](const QList<QQmlError> &list) {
        warnings += int(list.size());
        // Connecting to the signal replaces the engine's own printing.
        QTextStream err(stderr);
        for (const QQmlError &error : list)
            err << "tur-settings: QML warning: " << error.toString() << '\n';
    });

    // The registries of other programs, where a system has them.
    if (auto *registry = engine.singletonInstance<Registry *>("Tur.Settings", "Registry")) {
        if (parser.isSet(options.registry) || QFileInfo::exists(parser.value(options.registry)))
            registry->setDirectory(parser.value(options.registry));
        QTextStream err(stderr);
        for (const QString &problem : registry->problems())
            err << "tur-settings: " << problem << '\n';
    }

    engine.setInitialProperties(QVariantMap {
        { QStringLiteral("variant"), parser.value(options.variant) },
        { QStringLiteral("fixture"), parser.value(options.fixture) },
        { QStringLiteral("address"), address },
        { QStringLiteral("developer"), parser.isSet(options.developer) },
        { QStringLiteral("dbus"), useBus },
        { QStringLiteral("systemSettingsProgram"), parser.value(options.systemSettings) },
    });
    engine.loadFromModule("Tur.Settings", "Main");
    if (engine.rootObjects().isEmpty()) {
        QTextStream(stderr) << "tur-settings: the window could not be loaded\n";
        return 2;
    }
    const QObject *window = engine.rootObjects().constFirst();
    const QString busProblem = window->property("busProblem").toString();
    if (useBus && !busProblem.isEmpty())
        QTextStream(stderr) << "tur-settings: " << busProblem << " Running without D-Bus.\n";

    if (parser.isSet(options.check)) {
        // Long enough for the first frame and for anything that fails late.
        QTimer::singleShot(800, &app, [&app, &warnings, window] {
            QTextStream err(stderr);
            if (warnings > 0)
                err << "tur-settings: " << warnings << " QML warning(s)\n";
            const QString refused = window->property("refused").toString();
            if (!refused.isEmpty())
                err << "tur-settings: " << refused << '\n';
            app.exit(warnings > 0 ? 3 : refused.isEmpty() ? 0 : 4);
        });
    }
    return app.exec();
}
