// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
//
// Runs every tst_*.qml beside this file, or the file or directory given with
// -input. The kit's QML tests, the gallery's screenshots (shots/) and the
// toolchain proof (desktop/ci/probe) all run through this one program.

#include "turkit.h"

#include <QGuiApplication>
#include <QObject>
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
        // Inter. It does not depend on the machine's theme, so a picture is
        // the same wherever it is rendered.
        TurKit::useStandaloneLook();
        // A text cursor that blinks would make two pictures of one page
        // differ.
        QGuiApplication::styleHints()->setCursorFlashTime(0);
    }

    void qmlEngineAvailable(QQmlEngine *engine)
    {
        engine->addImportPath(QStringLiteral(TUR_QML_IMPORT_DIR));
    }
};

QUICK_TEST_MAIN_WITH_SETUP(turkit, Setup)

#include "tst_main.moc"
