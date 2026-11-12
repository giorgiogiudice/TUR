// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
//
// Runs every tst_*.qml beside this file, or the file given with -input (the
// screenshots in shots/). The same set-up as the kit's own tests, so that a
// picture of the settings is drawn the way a picture of the kit is.

#include "turkit.h"

#include <QByteArray>
#include <QCoreApplication>
#include <QDir>
#include <QFile>
#include <QGuiApplication>
#include <QObject>
#include <QQmlContext>
#include <QQmlEngine>
#include <QString>
#include <QStyleHints>
#include <QtQuickTest/quicktest.h>

// What a QML test cannot do by itself: put away the file the stand-in for
// KDE System Settings wrote, so that the next thing read from it is new.
class HelperRecord : public QObject
{
    Q_OBJECT

public:
    explicit HelperRecord(const QString &path, QObject *parent = nullptr)
        : QObject(parent)
        , m_path(path)
    {
    }

    Q_INVOKABLE void forget() { QFile::remove(m_path); }

private:
    QString m_path;
};

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
        // What About shows as the version of this program.
        QCoreApplication::setApplicationVersion(QStringLiteral(TUR_VERSION));

        // Where the stand-in for KDE System Settings (unit/helper.cpp) writes
        // what it was asked to open. Every program this one starts inherits
        // the variable.
        m_helperOut = QDir::tempPath() + QStringLiteral("/tur-settings-helper-%1.json")
                                             .arg(QCoreApplication::applicationPid());
        QFile::remove(m_helperOut);
        qputenv("TUR_SETTINGS_HELPER_OUT", m_helperOut.toLocal8Bit());
    }

    void qmlEngineAvailable(QQmlEngine *engine)
    {
        engine->addImportPath(QStringLiteral(TUR_QML_IMPORT_DIR));
        QQmlContext *context = engine->rootContext();
        // The fixture sets of this program, in the source tree.
        context->setContextProperty(QStringLiteral("settingsFixtures"), QStringLiteral(TUR_SETTINGS_FIXTURES));
        // The registry of a program that does not exist, for what
        // tur-settings does with an address that is not its own.
        context->setContextProperty(QStringLiteral("settingsOtherRegistries"),
                                    QStringLiteral(TUR_SETTINGS_TEST_DATA) + QStringLiteral("/registry"));
        // The stand-in for KDE System Settings, and where it writes.
        context->setContextProperty(QStringLiteral("settingsHelper"), QStringLiteral(TUR_SETTINGS_HELPER));
        context->setContextProperty(QStringLiteral("settingsHelperOut"), m_helperOut);
        context->setContextProperty(QStringLiteral("settingsHelperRecord"), new HelperRecord(m_helperOut, engine));
    }

    void cleanupTestCase() { QFile::remove(m_helperOut); }

private:
    QString m_helperOut;
};

QUICK_TEST_MAIN_WITH_SETUP(tursettings, Setup)

#include "tst_main.moc"
