// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
//
// tur-installer-wizard: TUR's graphical installer (desktop-experience.md,
// section 11.5; task D-23), in the live session of the installer medium
// (ADR-0018, amendment of 13 November 2026). It runs as the live session's
// user. Everything that touches a disk is done by the installation engine,
// which it starts through pkexec for each step; the engine checks the disk
// and the confirmation again itself.

#include "transcript.h"
#include "turkit.h"

#include <QCommandLineOption>
#include <QCommandLineParser>
#include <QCoreApplication>
#include <QGuiApplication>
#include <QList>
#include <QQmlApplicationEngine>
#include <QQmlError>
#include <QString>
#include <QTextStream>
#include <QTimer>
#include <QVariantMap>

int main(int argc, char *argv[])
{
    // A QGuiApplication, never a QApplication: that would load the GPL
    // widget style (desktop/README.md).
    QGuiApplication app(argc, argv);
    QCoreApplication::setApplicationName(QStringLiteral("tur-installer-wizard"));
    QCoreApplication::setApplicationVersion(QStringLiteral(TUR_VERSION));
    QGuiApplication::setDesktopFileName(QStringLiteral("org.tur.Installer"));

    QCommandLineParser parser;
    parser.setApplicationDescription(
        QStringLiteral("TUR's installer. It erases one disk, after a typed confirmation, and installs TUR on it."));
    parser.addHelpOption();
    parser.addVersionOption();
    const QCommandLineOption fixture(QStringLiteral("fixture"),
                                     QStringLiteral("Answer from the engine's recorded answers in DIR; run nothing."),
                                     QStringLiteral("DIR"));
    const QCommandLineOption installFixture(QStringLiteral("install-fixture"),
                                            QStringLiteral("The recorded installation to replay (default: install)."),
                                            QStringLiteral("NAME"), QStringLiteral("install"));
    const QCommandLineOption program(QStringLiteral("engine"), QStringLiteral("The installation engine to run."),
                                     QStringLiteral("PROGRAM"),
                                     QStringLiteral("/usr/libexec/tur-installer/tur-install-engine"));
    const QCommandLineOption launcher(QStringLiteral("launcher"),
                                      QStringLiteral("What runs the engine as root (default pkexec); empty: run it directly."),
                                      QStringLiteral("PROGRAM"), QStringLiteral("pkexec"));
    const QCommandLineOption standalone(QStringLiteral("standalone"),
                                        QStringLiteral("Use the look of TUR's programs outside a session."));
    const QCommandLineOption variant(QStringLiteral("variant"),
                                     QStringLiteral("Start in the dark or the light variant instead of following the desktop."),
                                     QStringLiteral("dark|light"));
    const QCommandLineOption session(QStringLiteral("started-by-session"),
                                     QStringLiteral("Started when the live session started (recorded only)."));
    const QCommandLineOption check(QStringLiteral("check"),
                                   QStringLiteral("Load the window, report any QML warning, and exit. For tests."));
    parser.addOptions({ fixture, installFixture, program, launcher, standalone, variant, session, check });
    parser.process(app);

    if (parser.isSet(standalone))
        TurKit::useStandaloneLook();

    TranscriptFile::write(QStringLiteral("start version=%1 started_by_session=%2 fixture=%3")
                              .arg(QStringLiteral(TUR_VERSION),
                                   parser.isSet(session) ? QStringLiteral("yes") : QStringLiteral("no"),
                                   parser.isSet(fixture) ? QStringLiteral("yes") : QStringLiteral("no")));

    QQmlApplicationEngine engine;
    int warnings = 0;
    QObject::connect(&engine, &QQmlEngine::warnings, &app, [&warnings](const QList<QQmlError> &list) {
        warnings += int(list.size());
        QTextStream err(stderr);
        for (const QQmlError &error : list) {
            err << "tur-installer-wizard: QML warning: " << error.toString() << '\n';
            TranscriptFile::write(QStringLiteral("qml-warning %1").arg(error.toString()));
        }
    });
    engine.setInitialProperties(QVariantMap {
        { QStringLiteral("variant"), parser.value(variant) },
        { QStringLiteral("fixture"), parser.value(fixture) },
        { QStringLiteral("installFixture"), parser.value(installFixture) },
        { QStringLiteral("engineProgram"), parser.value(program) },
        { QStringLiteral("engineLauncher"), parser.value(launcher) },
    });
    engine.loadFromModule("Tur.Installer", "Main");
    if (engine.rootObjects().isEmpty()) {
        QTextStream(stderr) << "tur-installer-wizard: the window could not be loaded\n";
        TranscriptFile::write(QStringLiteral("load-failed"));
        return 2;
    }

    if (parser.isSet(check)) {
        // Long enough for the first frame and the recorded answers.
        QTimer::singleShot(1500, &app, [&app, &warnings] {
            if (warnings > 0)
                QTextStream(stderr) << "tur-installer-wizard: " << warnings << " QML warning(s)\n";
            app.exit(warnings > 0 ? 3 : 0);
        });
    }
    return app.exec();
}
