// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
//
// Runs every tst_*.qml beside this file, or the file given with -input (the
// pictures in shots/). The same set-up as the kit's own tests, so that a
// picture of the installer is drawn the way a picture of the kit is.

#include "turkit.h"

#include <QDir>
#include <QGuiApplication>
#include <QObject>
#include <QQmlContext>
#include <QQmlEngine>
#include <QString>
#include <QStyleHints>
#include <QtQuickTest/quicktest.h>

class Setup : public QObject
{
    Q_OBJECT

public Q_SLOTS:
    void applicationAvailable()
    {
        // The look of TUR's programs outside a session: the Basic style and
        // Inter. The installer runs inside the live session, under its
        // style, but its own controls draw themselves either way.
        TurKit::useStandaloneLook();
        // A text cursor that blinks would make two pictures of one page
        // differ.
        QGuiApplication::styleHints()->setCursorFlashTime(0);
        const QString shots = qEnvironmentVariable("TUR_SCREENSHOT_DIR");
        if (!shots.isEmpty())
            QDir().mkpath(shots);
        QDir().mkpath(QStringLiteral(TUR_INSTALLER_SCRATCH));
    }

    void qmlEngineAvailable(QQmlEngine *engine)
    {
        engine->addImportPath(QStringLiteral(TUR_QML_IMPORT_DIR));
        // The engine's recorded answers, in the source tree.
        engine->rootContext()->setContextProperty(QStringLiteral("installerFixtures"),
                                                  QStringLiteral(TUR_INSTALLER_FIXTURES));
        // Where the tests may write (the keyboard's kxkbrc).
        engine->rootContext()->setContextProperty(QStringLiteral("installerScratch"),
                                                  QStringLiteral(TUR_INSTALLER_SCRATCH));
    }
};

QUICK_TEST_MAIN_WITH_SETUP(turinstaller, Setup)

#include "tst_main.moc"
