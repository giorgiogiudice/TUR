// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
//
// What the system says it is: reading the two files the image build writes,
// from each fixture set and from a place where they are not; the syntax of
// os-release; the texts the About page shows word for word; and the codes.

#include "codes.h"
#include "documents.h"
#include "systemfacts.h"

#include <QFile>
#include <QSignalSpy>
#include <QTemporaryDir>
#include <QTest>

namespace {

const QString fixtures = QStringLiteral(TUR_SETTINGS_FIXTURES);
const QString repository = QStringLiteral(TUR_SETTINGS_SOURCE) + QStringLiteral("/../../..");

QByteArray fileContent(const QString &path)
{
    QFile file(path);
    if (!file.open(QIODevice::ReadOnly))
        qFatal("%s cannot be read", qPrintable(path));
    return file.readAll();
}

} // namespace

class TestFacts : public QObject
{
    Q_OBJECT

private Q_SLOTS:
    void the_syntax_of_os_release_data()
    {
        QTest::addColumn<QByteArray>("text");
        QTest::addColumn<QString>("key");
        QTest::addColumn<QString>("value");

        QTest::newRow("plain") << QByteArray("ID=tur\n") << "ID" << "tur";
        QTest::newRow("double quotes") << QByteArray("PRETTY_NAME=\"TUR 0.4.0\"\n") << "PRETTY_NAME" << "TUR 0.4.0";
        QTest::newRow("single quotes") << QByteArray("OSTREE_VERSION='45.1'\n") << "OSTREE_VERSION" << "45.1";
        QTest::newRow("escapes in double quotes")
            << QByteArray("NAME=\"a \\\"b\\\" \\$c \\\\ \\`d\"\n") << "NAME" << "a \"b\" $c \\ `d";
        QTest::newRow("a backslash before another letter stays")
            << QByteArray("NAME=\"a\\nb\"\n") << "NAME" << "a\\nb";
        QTest::newRow("brackets inside quotes")
            << QByteArray("PRETTY_NAME=\"TUR 0.1.0-dev (based on Fedora Linux 45 (Kinoite))\"\n") << "PRETTY_NAME"
            << "TUR 0.1.0-dev (based on Fedora Linux 45 (Kinoite))";
        QTest::newRow("an equals sign in the value") << QByteArray("A=b=c\n") << "A" << "b=c";
        QTest::newRow("an empty value") << QByteArray("VERSION_CODENAME=\"\"\n") << "VERSION_CODENAME" << "";
        QTest::newRow("spaces round the line") << QByteArray("  ID=tur  \r\n") << "ID" << "tur";
        QTest::newRow("no line break at the end") << QByteArray("ID=tur") << "ID" << "tur";
        QTest::newRow("the last one wins") << QByteArray("ID=one\nID=two\n") << "ID" << "two";
        QTest::newRow("after a comment") << QByteArray("# ID=no\nID=yes\n") << "ID" << "yes";
        QTest::newRow("text outside ASCII") << QByteArray("NAME=\"Caf\xc3\xa9\"\n") << "NAME" << QStringLiteral("Caf\u00e9");
    }

    void the_syntax_of_os_release()
    {
        QFETCH(QByteArray, text);
        QFETCH(QString, key);
        QFETCH(QString, value);
        const QHash<QString, QString> read = SystemFacts::parse(text);
        QVERIFY(read.contains(key));
        QCOMPARE(read.value(key), value);
    }

    void what_is_not_a_line_of_os_release_is_left_out()
    {
        const QHash<QString, QString> read = SystemFacts::parse(
            "# a comment\n"
            "\n"
            "no equals sign\n"
            "=value without a key\n"
            "1KEY=starts with a digit\n"
            "KEY WITH SPACE=x\n"
            "export ID=tur\n"
            "GOOD=1\n");
        QCOMPARE(read.size(), 1);
        QCOMPARE(read.value(QStringLiteral("GOOD")), QStringLiteral("1"));
    }

    void a_tur_image_as_it_is_today()
    {
        SystemFacts facts;
        QSignalSpy changed(&facts, &SystemFacts::changed);
        facts.setFixture(fixtures + QStringLiteral("/default"));
        QCOMPARE(changed.count(), 1);
        QVERIFY(facts.failure().isEmpty());
        QCOMPARE(facts.version(), QStringLiteral("0.1.0-dev"));
        QCOMPARE(facts.build(), QStringLiteral("20261112.g387ac4f62f6c"));
        QCOMPARE(facts.sourceRevision(), QStringLiteral("387ac4f62f6cc07a6d0a3430c64634ca2b72fc02"));
        QCOMPARE(facts.baseImage(), QStringLiteral("quay.io/fedora/fedora-kinoite"));
        QCOMPARE(facts.baseDigest(),
                 QStringLiteral("sha256:f857f6c5da48a7c39721e40de51d9c0fef446a25e0360be4fb8964cbdf48f537"));
        QCOMPARE(facts.baseVersion(), QStringLiteral("45.20261112.0"));
        QCOMPARE(facts.systemName(), QStringLiteral("TUR 0.1.0-dev (based on Fedora Linux 45.20261112.0 (Kinoite))"));
        QCOMPARE(facts.systemId(), QStringLiteral("fedora"));
        QCOMPARE(facts.releasePath(), fixtures + QStringLiteral("/default/tur-release"));
        QCOMPARE(facts.osReleasePath(), fixtures + QStringLiteral("/default/os-release"));

        // The same fixture again changes nothing and says nothing.
        facts.setFixture(fixtures + QStringLiteral("/default"));
        QCOMPARE(changed.count(), 1);
    }

    // The fixture has the keys image/Containerfile writes, and no others.
    void the_fixture_follows_the_image_build()
    {
        const QByteArray containerfile = fileContent(repository + QStringLiteral("/image/Containerfile"));
        const QHash<QString, QString> release =
            SystemFacts::parse(fileContent(fixtures + QStringLiteral("/default/tur-release")));
        const QStringList keys = { QStringLiteral("TUR_VERSION"),      QStringLiteral("TUR_BUILD_ID"),
                                   QStringLiteral("TUR_SOURCE_REVISION"), QStringLiteral("TUR_BASE_IMAGE"),
                                   QStringLiteral("TUR_BASE_DIGEST"),  QStringLiteral("TUR_BASE_VERSION") };
        QCOMPARE(release.size(), keys.size());
        for (const QString &key : keys) {
            QVERIFY2(release.contains(key), qPrintable(key));
            // The image build writes a line "KEY=${...}" for each.
            QVERIFY2(containerfile.contains('"' + key.toLatin1() + "=${"), qPrintable(key));
        }
        // The values are not held to image/base.lock or VERSION: a change of
        // the base pin does not start this workflow, and would break it for
        // whoever came next.
    }

    void once_the_marks_are_replaced()
    {
        SystemFacts facts;
        facts.setFixture(fixtures + QStringLiteral("/rebranded"));
        QVERIFY(facts.failure().isEmpty());
        QCOMPARE(facts.version(), QStringLiteral("0.4.0"));
        QCOMPARE(facts.systemName(), QStringLiteral("TUR 0.4.0"));
        QCOMPARE(facts.systemId(), QStringLiteral("tur"));
    }

    void where_there_is_no_tur()
    {
        SystemFacts facts;
        facts.setFixture(fixtures + QStringLiteral("/not-tur"));
        // Nothing is put in the place of what could not be read.
        QVERIFY(facts.version().isEmpty());
        QVERIFY(facts.build().isEmpty());
        QVERIFY(facts.baseVersion().isEmpty());
        // What the system does call itself is still read.
        QCOMPARE(facts.systemName(), QStringLiteral("Fedora Linux 45.20261112.0 (Kinoite)"));
        QCOMPARE(facts.systemId(), QStringLiteral("fedora"));

        const QVariantMap failure = facts.failure();
        QCOMPARE(failure.value(QStringLiteral("ok")).toBool(), false);
        QCOMPARE(failure.value(QStringLiteral("code")).toString(), QStringLiteral("TUR-DESKTOP-SETTINGS-010"));
        QCOMPARE(failure.value(QStringLiteral("title")).toString(), QStringLiteral("TUR could not read what this system is."));
        QVERIFY(!failure.value(QStringLiteral("reason")).toString().isEmpty());
        const QString detail = failure.value(QStringLiteral("detail")).toString();
        QVERIFY2(detail.contains(fixtures + QStringLiteral("/not-tur/tur-release: ")), qPrintable(detail));
        QVERIFY2(detail.contains(QStringLiteral("/not-tur/os-release: ID=fedora")), qPrintable(detail));
    }

    void where_there_is_nothing_at_all()
    {
        QTemporaryDir empty;
        QVERIFY(empty.isValid());
        SystemFacts facts;
        facts.setFixture(empty.path());
        QVERIFY(facts.version().isEmpty());
        QVERIFY(facts.systemName().isEmpty());
        QVERIFY(facts.systemId().isEmpty());
        const QString detail = facts.failure().value(QStringLiteral("detail")).toString();
        // Both files are named, each with why it could not be read.
        QVERIFY2(detail.contains(empty.path() + QStringLiteral("/tur-release: ")), qPrintable(detail));
        QVERIFY2(detail.contains(empty.path() + QStringLiteral("/os-release: ")), qPrintable(detail));
        QVERIFY2(!detail.contains(QStringLiteral("ID=")), qPrintable(detail));
    }

    // Without a fixture the system's own files are read. The builder is not a
    // TUR system, and says so; on one, this reads the real thing.
    void without_a_fixture_the_system_is_asked()
    {
        const SystemFacts facts;
        QCOMPARE(facts.releasePath(), QStringLiteral("/usr/lib/tur/release"));
        QVERIFY(facts.osReleasePath() == QLatin1String("/etc/os-release")
                || facts.osReleasePath() == QLatin1String("/usr/lib/os-release"));
        if (QFile::exists(QStringLiteral("/usr/lib/tur/release"))) {
            QVERIFY(facts.failure().isEmpty());
            QVERIFY(!facts.version().isEmpty());
        } else {
            QCOMPARE(facts.failure().value(QStringLiteral("code")).toString(), QStringLiteral("TUR-DESKTOP-SETTINGS-010"));
            QVERIFY(facts.version().isEmpty());
        }
        QVERIFY(facts.qtVersion().startsWith(QLatin1String("6.")));
    }

    // The texts are the files at the top of the repository, as they are.
    void the_texts_are_the_repository_s_data()
    {
        QTest::addColumn<QString>("name");
        QTest::addColumn<QString>("file");

        QTest::newRow("the licence") << "licence" << "LICENSE.md";
        QTest::newRow("the required notice") << "notice" << "NOTICE";
        QTest::newRow("the third-party notices") << "third-party" << "THIRD_PARTY_NOTICES.md";
    }

    void the_texts_are_the_repository_s()
    {
        QFETCH(QString, name);
        QFETCH(QString, file);
        const Documents documents;
        const QString shown = documents.text(name);
        QVERIFY(!shown.isEmpty());
        QCOMPARE(shown, QString::fromUtf8(fileContent(repository + QLatin1Char('/') + file)).trimmed());
    }

    void a_text_that_is_not_carried_is_empty()
    {
        const Documents documents;
        QVERIFY(documents.text(QStringLiteral("something-else")).isEmpty());
        QVERIFY(documents.text(QStringLiteral("../../etc/passwd")).isEmpty());
        QVERIFY(documents.text(QString()).isEmpty());
    }

    void the_codes()
    {
        using Settings::Code;
        QCOMPARE(Settings::codeText(Code::UnknownAddress), QStringLiteral("TUR-DESKTOP-SETTINGS-001"));
        QCOMPARE(Settings::codeText(Code::RemovedAddress), QStringLiteral("TUR-DESKTOP-SETTINGS-002"));
        QCOMPARE(Settings::codeText(Code::PageNotAvailable), QStringLiteral("TUR-DESKTOP-SETTINGS-003"));
        QCOMPARE(Settings::codeText(Code::NotAnAddress), QStringLiteral("TUR-DESKTOP-SETTINGS-004"));
        QCOMPARE(Settings::codeText(Code::OwnerUnreachable), QStringLiteral("TUR-DESKTOP-SETTINGS-005"));
        QCOMPARE(Settings::codeText(Code::RowNotShown), QStringLiteral("TUR-DESKTOP-SETTINGS-006"));
        QCOMPARE(Settings::codeText(Code::RegistryUnreadable), QStringLiteral("TUR-DESKTOP-SETTINGS-007"));
        QCOMPARE(Settings::codeText(Code::SystemDescriptionUnreadable), QStringLiteral("TUR-DESKTOP-SETTINGS-010"));

        QCOMPARE(Settings::errorName(Code::UnknownAddress), QStringLiteral("org.tur.Settings1.Error.UnknownAddress"));
        QCOMPARE(Settings::errorMessage(Code::RemovedAddress, QStringLiteral("it went")),
                 QStringLiteral("TUR-DESKTOP-SETTINGS-002: it went"));
        for (const Code code : { Code::UnknownAddress, Code::RemovedAddress, Code::PageNotAvailable, Code::NotAnAddress,
                                 Code::OwnerUnreachable, Code::RowNotShown, Code::RegistryUnreadable,
                                 Code::SystemDescriptionUnreadable }) {
            QVERIFY(!Settings::titleOf(code).isEmpty());
            QVERIFY(Settings::errorName(code).startsWith(QLatin1String("org.tur.Settings1.Error.")));
            const QVariantMap failure = Settings::failure(code, QStringLiteral("why"), QStringLiteral("detail"));
            QCOMPARE(failure.value(QStringLiteral("code")).toString(), Settings::codeText(code));
            QCOMPARE(failure.value(QStringLiteral("ok")).toBool(), false);
        }
    }
};

QTEST_GUILESS_MAIN(TestFacts)

#include "tst_facts.moc"
