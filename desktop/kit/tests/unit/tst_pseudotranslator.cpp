// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#include "pseudotranslator.h"

#include <QChar>
#include <QCoreApplication>
#include <QString>
#include <QTest>

// The pseudo-locale of the direction and text-length harness
// (desktop-experience.md, 13.2, L5 and L6).
class TestPseudoTranslator : public QObject
{
    Q_OBJECT

private:
    static int letters(const QString &text)
    {
        int count = 0;
        for (const QChar ch : text) {
            if (ch.isLetter())
                ++count;
        }
        return count;
    }

private Q_SLOTS:
    void everyStringIsAtLeastFortyPercentLonger_data()
    {
        QTest::addColumn<QString>("source");
        QTest::newRow("one word") << QStringLiteral("Install");
        QTest::newRow("short") << QStringLiteral("Back");
        QTest::newRow("sentence") << QStringLiteral("The update was not applied. Your computer is unchanged.");
        QTest::newRow("placeholders") << QStringLiteral("%1 of %2");
    }

    void everyStringIsAtLeastFortyPercentLonger()
    {
        QFETCH(QString, source);
        for (const bool rightToLeft : { false, true }) {
            const QString out = PseudoTranslator::transform(source, rightToLeft);
            QVERIFY2(letters(out) * 10 >= letters(source) * 14, qPrintable(out));
        }
    }

    void theLengthenedStringStartsWithTheSource()
    {
        QCOMPARE(PseudoTranslator::transform(QStringLiteral("Install"), false), QStringLiteral("Install lorem"));
    }

    void placeholdersSurvive()
    {
        for (const bool rightToLeft : { false, true }) {
            const QString out = PseudoTranslator::transform(QStringLiteral("%1 of %2, %n file(s), %L3"), rightToLeft);
            QVERIFY2(out.contains(QStringLiteral("%1")), qPrintable(out));
            QVERIFY2(out.contains(QStringLiteral("%2")), qPrintable(out));
            QVERIFY2(out.contains(QStringLiteral("%n")), qPrintable(out));
            QVERIFY2(out.contains(QStringLiteral("%L3")), qPrintable(out));
            // The result still formats.
            const QString filled = out.arg(QStringLiteral("2.1 GiB"), QStringLiteral("5.4 GiB"));
            QVERIFY2(filled.contains(QStringLiteral("2.1 GiB")), qPrintable(filled));
        }
    }

    void rightToLeftTextIsRightToLeft()
    {
        const QString out = PseudoTranslator::transform(QStringLiteral("Erase disk and install"), true);
        for (const QChar ch : out) {
            if (!ch.isLetter())
                continue;
            QCOMPARE(ch.direction(), QChar::DirR);
        }
        QVERIFY(out.isRightToLeft());
    }

    void anEmptyStringStaysEmpty()
    {
        QVERIFY(PseudoTranslator::transform(QString(), true).isEmpty());
    }

    void itSaysWhichWayTheLanguageRuns()
    {
        const PseudoTranslator leftToRight(false);
        const PseudoTranslator rightToLeft(true);
        QCOMPARE(leftToRight.translate("QGuiApplication", "QT_LAYOUT_DIRECTION"), QStringLiteral("LTR"));
        QCOMPARE(rightToLeft.translate("QGuiApplication", "QT_LAYOUT_DIRECTION"), QStringLiteral("RTL"));
    }

    void itWorksAsAnInstalledTranslator()
    {
        PseudoTranslator translator(false);
        QVERIFY(!translator.isEmpty());
        QVERIFY(QCoreApplication::installTranslator(&translator));
        QCOMPARE(QCoreApplication::translate("Anything", "Back"), QStringLiteral("Back lorem"));
        QVERIFY(QCoreApplication::removeTranslator(&translator));
        QCOMPARE(QCoreApplication::translate("Anything", "Back"), QStringLiteral("Back"));
    }
};

QTEST_MAIN(TestPseudoTranslator)

#include "tst_pseudotranslator.moc"
