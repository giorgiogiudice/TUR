// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
//
// Runs every tst_*.qml beside this file, or the file given with -input (the
// screenshots in shots/). The same set-up as the kit's own tests, so that a
// picture of the application centre is drawn the way a picture of the kit is.

#include "turkit.h"

#include <QGuiApplication>
#include <QObject>
#include <QQmlContext>
#include <QQmlEngine>
#include <QString>
#include <QStyleHints>
#include <QVariantMap>
#include <QtGlobal>
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
        // Where the recorded answers of tur-catalogue are: the fixtures of
        // this program, in the source tree.
        engine->rootContext()->setContextProperty(QStringLiteral("centreFixtures"),
                                                  QStringLiteral(TUR_CENTRE_FIXTURES));
        // A stand-in for tur-catalogue that misbehaves on request
        // (unit/helper.cpp), for the states only a failing program reaches.
        engine->rootContext()->setContextProperty(QStringLiteral("centreHelper"),
                                                  QStringLiteral(TUR_CENTRE_HELPER));
        // The end-to-end test (tests/e2e, desktop/ci/centre-e2e) is given
        // the real programs and catalogue in the environment; elsewhere
        // these are empty and it is not run.
        engine->rootContext()->setContextProperty(
            QStringLiteral("centreEndToEnd"),
            QVariantMap {
                { QStringLiteral("apps"), qEnvironmentVariable("TUR_CENTRE_E2E_APPS") },
                { QStringLiteral("catalogueProgram"), qEnvironmentVariable("TUR_CENTRE_E2E_CATALOGUE_PROGRAM") },
                { QStringLiteral("catalogue"), qEnvironmentVariable("TUR_CENTRE_E2E_CATALOGUE") },
                { QStringLiteral("updateExpected"), qEnvironmentVariable("TUR_CENTRE_E2E_UPDATE_EXPECTED") == QLatin1String("1") },
            });
    }
};

QUICK_TEST_MAIN_WITH_SETUP(turcentre, Setup)

#include "tst_main.moc"
