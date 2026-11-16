// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
//
// tur-centre: the application centre (docs/architecture/desktop-experience.md,
// section 8; docs/architecture/tur-centre.md). It reads the catalogue and the
// router (part 1, D-10), and installs, updates and removes applications per
// user through tur-apps, which it runs as a program of the session (part 2,
// D-11).

#include "turkit.h"

#include <QCommandLineOption>
#include <QCommandLineParser>
#include <QCoreApplication>
#include <QGuiApplication>
#include <QList>
#include <QQmlApplicationEngine>
#include <QQmlError>
#include <QString>
#include <QStringList>
#include <QTextStream>
#include <QTimer>
#include <QVariantMap>

int main(int argc, char *argv[])
{
    // A QGuiApplication, never a QApplication: that would load the GPL
    // widget style (desktop/README.md).
    QGuiApplication app(argc, argv);
    QCoreApplication::setApplicationName(QStringLiteral("tur-centre"));
    QCoreApplication::setApplicationVersion(QStringLiteral(TUR_VERSION));
    // The launcher entry the compositor takes the window's name and icon from.
    QGuiApplication::setDesktopFileName(QStringLiteral("org.tur.Centre"));

    QCommandLineParser parser;
    parser.setApplicationDescription(
        QStringLiteral("The TUR application centre. It finds applications, says how each will run on this "
                       "computer, and installs, updates and removes them for you through tur-apps."));
    parser.addHelpOption();
    parser.addVersionOption();
    parser.addPositionalArgument(
        QStringLiteral("address"),
        QStringLiteral("What to open: tur-centre:home, tur-centre:search?q=WORDS, tur-centre:app/ID, "
                       "tur-centre:installed, tur-centre:updates or tur-centre:sources."),
        QStringLiteral("[address]"));

    const QCommandLineOption fixture(
        QStringLiteral("fixture"),
        QStringLiteral("Answer from recorded answers of tur-catalogue in DIR instead of running it."),
        QStringLiteral("DIR"));
    const QCommandLineOption program(QStringLiteral("catalogue-program"),
                                     QStringLiteral("The catalogue program to run. Default: tur-catalogue, on the PATH."),
                                     QStringLiteral("PROGRAM"), QStringLiteral("tur-catalogue"));
    const QCommandLineOption catalogue(
        QStringLiteral("catalogue"),
        QStringLiteral("The catalogue directory, passed to the catalogue program as --catalogue."),
        QStringLiteral("DIR"));
    const QCommandLineOption hostFacts(
        QStringLiteral("host-facts"),
        QStringLiteral("A JSON file of facts about this computer, passed to the router as --host. Without it "
                       "the router assumes a computer, and the application centre says so."),
        QStringLiteral("FILE"));
    const QCommandLineOption preferences(QStringLiteral("preferences"),
                                         QStringLiteral("A JSON file of routing preferences, passed to the router as "
                                                        "--prefs."),
                                         QStringLiteral("FILE"));
    const QCommandLineOption appsProgram(
        QStringLiteral("apps-program"),
        QStringLiteral("The install backend to run. Default: tur-apps, on the PATH. It installs per user."),
        QStringLiteral("PROGRAM"), QStringLiteral("tur-apps"));
    const QCommandLineOption appsFixture(
        QStringLiteral("apps-fixture"),
        QStringLiteral("Answer installing, listing and sources from recorded runs of tur-apps in DIR instead of "
                       "running it. Nothing is installed."),
        QStringLiteral("DIR"));
    const QCommandLineOption standalone(QStringLiteral("standalone"),
                                        QStringLiteral("Use the look of TUR's programs outside a session: the "
                                                       "Basic style and the Inter font, whatever the desktop says."));
    const QCommandLineOption variant(QStringLiteral("variant"),
                                     QStringLiteral("Start in the dark or the light variant instead of following "
                                                    "the desktop."),
                                     QStringLiteral("dark|light"));
    const QCommandLineOption rightToLeft(QStringLiteral("rtl"), QStringLiteral("Lay the window out right to left."));
    const QCommandLineOption check(QStringLiteral("check"),
                                   QStringLiteral("Load the window, report any QML warning, and exit. For tests."));
    parser.addOptions({ fixture, program, catalogue, hostFacts, preferences, appsProgram, appsFixture, standalone,
                        variant, rightToLeft, check });
    parser.process(app);

    if (parser.isSet(standalone))
        TurKit::useStandaloneLook();
    if (parser.isSet(rightToLeft))
        QGuiApplication::setLayoutDirection(Qt::RightToLeft);

    const QStringList positional = parser.positionalArguments();

    QQmlApplicationEngine engine;
    int warnings = 0;
    QObject::connect(&engine, &QQmlEngine::warnings, &app, [&warnings](const QList<QQmlError> &list) {
        warnings += int(list.size());
        // Connecting to the signal replaces the engine's own printing.
        QTextStream err(stderr);
        for (const QQmlError &error : list)
            err << "tur-centre: QML warning: " << error.toString() << '\n';
    });
    engine.setInitialProperties(QVariantMap {
        { QStringLiteral("variant"), parser.value(variant) },
        { QStringLiteral("fixture"), parser.value(fixture) },
        { QStringLiteral("program"), parser.value(program) },
        { QStringLiteral("catalogueDirectory"), parser.value(catalogue) },
        { QStringLiteral("hostFacts"), parser.value(hostFacts) },
        { QStringLiteral("preferences"), parser.value(preferences) },
        { QStringLiteral("appsProgram"), parser.value(appsProgram) },
        { QStringLiteral("appsFixture"), parser.value(appsFixture) },
        { QStringLiteral("address"), positional.isEmpty() ? QString() : positional.first() },
    });
    engine.loadFromModule("Tur.Centre", "Main");
    if (engine.rootObjects().isEmpty()) {
        QTextStream(stderr) << "tur-centre: the window could not be loaded\n";
        return 2;
    }

    if (parser.isSet(check)) {
        // Long enough for the first frame, for the answers of a fixture, and
        // for anything that fails late.
        QTimer::singleShot(800, &app, [&app, &warnings] {
            if (warnings > 0)
                QTextStream(stderr) << "tur-centre: " << warnings << " QML warning(s)\n";
            app.exit(warnings > 0 ? 3 : 0);
        });
    }
    return app.exec();
}
