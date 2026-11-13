// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
//
// Runs every tst_*.qml beside this file, or the file given with -input (the
// pictures in shots/, and the end-to-end test of desktop/tests/pax-ui). The
// same set-up as the kit's own tests, so that a picture of P@x's window is
// drawn the way a picture of the kit is.
//
// Before the first test it starts a session bus of its own with the
// stand-in for org.tur.Agent1 on it (fixturebus.h), which the tests reach
// as `fixture`. With TUR_PAX_UI_E2E_BUS set, it starts nothing: the tests
// are the end-to-end test, against the real service on that bus, which
// they reach as `e2eBus`.

#include "fixturebus.h"
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
        // A text cursor that blinks would make two pictures of one view
        // differ.
        QGuiApplication::styleHints()->setCursorFlashTime(0);
        const QString shots = qEnvironmentVariable("TUR_SCREENSHOT_DIR");
        if (!shots.isEmpty())
            QDir().mkpath(shots);
        if (qEnvironmentVariableIsEmpty("TUR_PAX_UI_E2E_BUS")) {
            m_fixture = new FixtureBus(QStringLiteral(TUR_PAX_UI_AGENT), QStringLiteral(TUR_PAX_UI_FIXTURES), this);
            QString problem;
            if (!m_fixture->start(&problem))
                qFatal("the stand-in for org.tur.Agent1 could not be started: %s", qPrintable(problem));
        }
    }

    void qmlEngineAvailable(QQmlEngine *engine)
    {
        engine->addImportPath(QStringLiteral(TUR_QML_IMPORT_DIR));
        engine->rootContext()->setContextProperty(QStringLiteral("fixture"), m_fixture);
        engine->rootContext()->setContextProperty(QStringLiteral("e2eBus"), qEnvironmentVariable("TUR_PAX_UI_E2E_BUS"));
        // The one source of P@x's keys, which the tests read as it is.
        engine->rootContext()->setContextProperty(QStringLiteral("paxCommandsFile"), QStringLiteral(TUR_PAX_COMMANDS));
    }

    void cleanupTestCase()
    {
        delete m_fixture;
        m_fixture = nullptr;
    }

private:
    FixtureBus *m_fixture = nullptr;
};

QUICK_TEST_MAIN_WITH_SETUP(turpaxui, Setup)

#include "tst_main.moc"
