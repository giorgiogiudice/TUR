// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
//
// The keyboard layout of the live session: the list read from
// xkeyboard-config's evdev.lst, and the kxkbrc written for KWin, which keeps
// every other group of the file.

#include "keyboard.h"

#include <QDir>
#include <QFile>
#include <QSignalSpy>
#include <QTemporaryDir>
#include <QTest>

class KeyboardTest : public QObject
{
    Q_OBJECT

private Q_SLOTS:
    void parsesTheLayoutSection()
    {
        const QString rules = QStringLiteral(
            "! model\n  pc105           Generic 105-key PC\n\n"
            "! layout\n  us              English (US)\n  de              German\n  fr              French\n\n"
            "! variant\n  intl            us: English (US, intl., with dead keys)\n");
        const QVariantList layouts = KeyboardLayouts::parseRules(rules);
        QCOMPARE(layouts.size(), 3);
        // By name.
        QCOMPARE(layouts.at(0).toMap().value(QStringLiteral("code")).toString(), QStringLiteral("us"));
        QCOMPARE(layouts.at(1).toMap().value(QStringLiteral("code")).toString(), QStringLiteral("fr"));
        QCOMPARE(layouts.at(2).toMap().value(QStringLiteral("name")).toString(), QStringLiteral("German"));
    }

    void keepsTheOtherGroups()
    {
        const QString before = QStringLiteral("[Layout]\nLayoutList=us\nUse=false\nOptions=caps:none\n\n[Other]\nKey=1\n");
        const QString after = KeyboardLayouts::withGroup(before, KeyboardLayouts::layoutGroup(QStringLiteral("de")));
        QVERIFY(after.contains(QStringLiteral("[Other]\nKey=1")));
        QVERIFY(after.contains(QStringLiteral("[Layout]\nLayoutList=de\nUse=true\n")));
        QCOMPARE(after.count(QStringLiteral("[Layout]")), 1);
        QVERIFY(!after.contains(QStringLiteral("LayoutList=us")));
    }

    void writesKxkbrc()
    {
        QTemporaryDir directory;
        QVERIFY(directory.isValid());
        QFile rules(directory.filePath(QStringLiteral("evdev.lst")));
        QVERIFY(rules.open(QIODevice::WriteOnly));
        rules.write("! layout\n  us              English (US)\n  it              Italian\n");
        rules.close();

        KeyboardLayouts layouts;
        layouts.setRulesFile(rules.fileName());
        layouts.setConfigDirectory(directory.path());
        QCOMPARE(layouts.layouts().size(), 2);
        QCOMPARE(layouts.current(), QStringLiteral("us"));

        QSignalSpy changed(&layouts, &KeyboardLayouts::currentChanged);
        QVERIFY(!layouts.apply(QStringLiteral("xx")));
        QCOMPARE(changed.count(), 0);
        QVERIFY(layouts.apply(QStringLiteral("it")));
        QCOMPARE(layouts.current(), QStringLiteral("it"));
        QCOMPARE(changed.count(), 1);

        QFile written(QDir(directory.path()).filePath(QStringLiteral("kxkbrc")));
        QVERIFY(written.open(QIODevice::ReadOnly));
        QCOMPARE(QString::fromUtf8(written.readAll()), QStringLiteral("[Layout]\nLayoutList=it\nUse=true\n"));

        // Read back as the current layout by a new instance.
        KeyboardLayouts again;
        again.setRulesFile(rules.fileName());
        again.setConfigDirectory(directory.path());
        QCOMPARE(again.current(), QStringLiteral("it"));
    }
};

QTEST_GUILESS_MAIN(KeyboardTest)

#include "tst_keyboard.moc"
