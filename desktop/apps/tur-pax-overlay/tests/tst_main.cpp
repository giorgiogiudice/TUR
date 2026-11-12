// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
//
// Runs every tst_*.qml beside this file, or the file given with -input (the
// reference pictures in shots/). The same set-up as the kit's own tests, so
// that a picture of the overlay is drawn the way a picture of the kit is.

#include "turkit.h"

#include <QGuiApplication>
#include <QObject>
#include <QQmlEngine>
#include <QStyleHints>
#include <QtQuickTest/quicktest.h>

class Setup : public QObject
{
    Q_OBJECT

public Q_SLOTS:
    void applicationAvailable()
    {
        // The overlay's own look: the Basic style and Inter, whatever the
        // machine says (design, 5.7).
        TurKit::useStandaloneLook();
        QGuiApplication::styleHints()->setCursorFlashTime(0);
    }

    void qmlEngineAvailable(QQmlEngine *engine) { engine->addImportPath(QStringLiteral(TUR_QML_IMPORT_DIR)); }
};

QUICK_TEST_MAIN_WITH_SETUP(turpaxoverlay, Setup)

#include "tst_main.moc"
