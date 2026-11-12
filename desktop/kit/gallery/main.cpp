// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
//
// tur-kit-gallery: every component of tur-kit in every state, to look at.
// The screenshot job renders the same pages through the kit's tests.

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
    QGuiApplication app(argc, argv);
    QCoreApplication::setApplicationName(QStringLiteral("tur-kit-gallery"));
    QCoreApplication::setApplicationVersion(QStringLiteral(TUR_VERSION));

    QCommandLineParser parser;
    parser.setApplicationDescription(QStringLiteral("Shows every component of TUR's UI kit in every state."));
    parser.addHelpOption();
    parser.addVersionOption();
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
    parser.addOptions({ standalone, variant, rightToLeft, check });
    parser.process(app);

    if (parser.isSet(standalone))
        TurKit::useStandaloneLook();
    if (parser.isSet(rightToLeft))
        QGuiApplication::setLayoutDirection(Qt::RightToLeft);

    QQmlApplicationEngine engine;
    int warnings = 0;
    QObject::connect(&engine, &QQmlEngine::warnings, &app, [&warnings](const QList<QQmlError> &list) {
        warnings += int(list.size());
        // Connecting to the signal replaces the engine's own printing.
        QTextStream err(stderr);
        for (const QQmlError &error : list)
            err << "tur-kit-gallery: QML warning: " << error.toString() << '\n';
    });
    engine.setInitialProperties(QVariantMap { { QStringLiteral("variant"), parser.value(variant) } });
    engine.loadFromModule("Tur.Kit.Gallery", "Main");
    if (engine.rootObjects().isEmpty()) {
        QTextStream(stderr) << "tur-kit-gallery: the window could not be loaded\n";
        return 2;
    }

    if (parser.isSet(check)) {
        // Long enough for the first frame and for anything that fails late.
        QTimer::singleShot(500, &app, [&app, &warnings] {
            if (warnings > 0)
                QTextStream(stderr) << "tur-kit-gallery: " << warnings << " QML warning(s)\n";
            app.exit(warnings > 0 ? 3 : 0);
        });
    }
    return app.exec();
}
