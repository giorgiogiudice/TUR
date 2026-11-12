// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#include "systemappearance.h"

#include <QByteArray>
#include <QDir>
#include <QFile>
#include <QGuiApplication>
#include <QSignalSpy>
#include <QString>
#include <QStyleHints>
#include <QTemporaryDir>
#include <QTest>

// SystemAppearance reads what the desktop says: the animation speed from
// kdeglobals, and whether a light scheme is preferred.
class TestSystemAppearance : public QObject
{
    Q_OBJECT

private:
    void write(const QString &directory, const QByteArray &content)
    {
        QVERIFY(QDir().mkpath(directory));
        QFile file(directory + QStringLiteral("/kdeglobals"));
        QVERIFY(file.open(QIODevice::WriteOnly | QIODevice::Truncate));
        file.write(content);
        // QSettings reads a file again only if its size or its time stamp
        // changed. Two writes can share a time stamp, so give every one a
        // different size.
        file.write(";" + QByteArray(++m_writes, '.') + "\n");
    }

    QTemporaryDir m_home;
    QString m_user;
    QString m_system;
    int m_writes = 0;

private Q_SLOTS:
    void initTestCase()
    {
        QVERIFY(m_home.isValid());
        m_user = m_home.path() + QStringLiteral("/user");
        m_system = m_home.path() + QStringLiteral("/system");
        qputenv("XDG_CONFIG_HOME", m_user.toUtf8());
        qputenv("XDG_CONFIG_DIRS", m_system.toUtf8());
    }

    void init()
    {
        qunsetenv("TUR_ANIMATION_FACTOR");
        QFile::remove(m_user + QStringLiteral("/kdeglobals"));
        QFile::remove(m_system + QStringLiteral("/kdeglobals"));
        SystemAppearance::setStandalone(false);
        QGuiApplication::styleHints()->unsetColorScheme();
    }

    void normalSpeedWhenNothingSaysOtherwise()
    {
        QCOMPARE(SystemAppearance::readAnimationFactor(), 1.0);
        const SystemAppearance appearance;
        QCOMPARE(appearance.animationFactor(), 1.0);
    }

    void readsTheDesktopsAnimationSetting()
    {
        write(m_user, "[General]\nColorScheme=TurDark\n\n[KDE]\nAnimationDurationFactor=0\nSingleClick=false\n");
        QCOMPARE(SystemAppearance::readAnimationFactor(), 0.0);

        write(m_user, "[KDE]\nAnimationDurationFactor=0.5\n");
        QCOMPARE(SystemAppearance::readAnimationFactor(), 0.5);
    }

    void theUsersFileWinsOverTheSystems()
    {
        write(m_system, "[KDE]\nAnimationDurationFactor=2\n");
        QCOMPARE(SystemAppearance::readAnimationFactor(), 2.0);

        write(m_user, "[KDE]\nAnimationDurationFactor=0.25\n");
        QCOMPARE(SystemAppearance::readAnimationFactor(), 0.25);
    }

    void aFileWithoutTheKeyIsPassedOver()
    {
        write(m_user, "[KDE]\nSingleClick=false\n");
        write(m_system, "[KDE]\nAnimationDurationFactor=0\n");
        QCOMPARE(SystemAppearance::readAnimationFactor(), 0.0);
    }

    void nonsenseIsIgnored()
    {
        write(m_user, "[KDE]\nAnimationDurationFactor=-3\n");
        QCOMPARE(SystemAppearance::readAnimationFactor(), 1.0);

        write(m_user, "[KDE]\nAnimationDurationFactor=fast\n");
        QCOMPARE(SystemAppearance::readAnimationFactor(), 1.0);

        write(m_user, "[KDE]\nAnimationDurationFactor=5000\n");
        QCOMPARE(SystemAppearance::readAnimationFactor(), 10.0);
    }

    void theEnvironmentOverrides()
    {
        write(m_user, "[KDE]\nAnimationDurationFactor=2\n");
        qputenv("TUR_ANIMATION_FACTOR", "0");
        QCOMPARE(SystemAppearance::readAnimationFactor(), 0.0);
    }

    void reloadNotices()
    {
        SystemAppearance appearance;
        QCOMPARE(appearance.animationFactor(), 1.0);
        QSignalSpy changed(&appearance, &SystemAppearance::animationFactorChanged);

        write(m_user, "[KDE]\nAnimationDurationFactor=0\n");
        appearance.reload();
        QCOMPARE(appearance.animationFactor(), 0.0);
        QCOMPARE(changed.count(), 1);

        appearance.reload();
        QCOMPARE(changed.count(), 1);
    }

    void lightOnlyWhenTheDesktopSaysLight()
    {
        QVERIFY(SystemAppearance::lightFor(Qt::ColorScheme::Light, false));
        QVERIFY(!SystemAppearance::lightFor(Qt::ColorScheme::Dark, false));
        QVERIFY2(!SystemAppearance::lightFor(Qt::ColorScheme::Unknown, false), "no stated preference means dark");
    }

    void aStandaloneProgramDoesNotAskTheDesktop()
    {
        QVERIFY(!SystemAppearance::lightFor(Qt::ColorScheme::Light, true));
        QVERIFY(!SystemAppearance::lightFor(Qt::ColorScheme::Dark, true));

        SystemAppearance::setStandalone(true);
        const SystemAppearance appearance;
        QVERIFY(!appearance.prefersLight());
    }

    void itFollowsTheDesktopWhileRunning()
    {
        QStyleHints *hints = QGuiApplication::styleHints();
        hints->setColorScheme(Qt::ColorScheme::Dark);
        if (hints->colorScheme() != Qt::ColorScheme::Dark)
            QSKIP("this platform cannot be told a colour scheme, so there is no change to follow");

        SystemAppearance appearance;
        QVERIFY(!appearance.prefersLight());
        QSignalSpy changed(&appearance, &SystemAppearance::prefersLightChanged);

        hints->setColorScheme(Qt::ColorScheme::Light);
        QVERIFY(appearance.prefersLight());
        QCOMPARE(changed.count(), 1);

        hints->setColorScheme(Qt::ColorScheme::Dark);
        QVERIFY(!appearance.prefersLight());
        QCOMPARE(changed.count(), 2);
    }
};

QTEST_MAIN(TestSystemAppearance)

#include "tst_systemappearance.moc"
