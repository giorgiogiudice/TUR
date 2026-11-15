// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
//
// Runs every tst_*.qml beside this file, or the file given with -input (the
// reference pictures in shots/). The same set-up as the kit's own tests, so
// that a picture of P@x's character is drawn the way a picture of the kit
// is. The tests feed the character's rules (Companion) directly: what the
// services say is the business of unit/tst_bus.cpp.

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
        // Inter, whatever the machine says.
        TurKit::useStandaloneLook();
        QGuiApplication::styleHints()->setCursorFlashTime(0);
        const QString shots = qEnvironmentVariable("TUR_SCREENSHOT_DIR");
        if (!shots.isEmpty())
            QDir().mkpath(shots);
    }

    void qmlEngineAvailable(QQmlEngine *engine)
    {
        engine->addImportPath(QStringLiteral(TUR_QML_IMPORT_DIR));
        // The healing service's notices, as its program prints them.
        engine->rootContext()->setContextProperty(QStringLiteral("healNoticesFile"),
                                                  QStringLiteral(TUR_COMPANION_FIXTURES "/heal/notices.json"));
    }
};

QUICK_TEST_MAIN_WITH_SETUP(turpaxcompanion, Setup)

#include "tst_main.moc"
