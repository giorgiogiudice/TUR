// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
//
// Reading what the catalogue program answers: the three documents, every way
// the reading can fail, and the vocabulary.

#include "answers.h"
#include "fixturesource.h"

#include <QFile>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QRegularExpression>
#include <QTest>

namespace {

const QString fixtures = QStringLiteral(TUR_CENTRE_FIXTURES);

CatalogueReply reply(const QByteArray &output, int exitStatus = 0, const QByteArray &errorOutput = QByteArray())
{
    CatalogueReply out;
    out.output = output;
    out.exitStatus = exitStatus;
    out.errorOutput = errorOutput;
    out.commandLine = QStringLiteral("tur-catalogue route --json -- org.example.app");
    return out;
}

CatalogueReply failed(CatalogueReply::Failure failure)
{
    CatalogueReply out;
    out.failure = failure;
    out.detail = QStringLiteral("what the system said");
    out.commandLine = QStringLiteral("tur-catalogue search --json -- office");
    return out;
}

// A recorded answer of the fixture set `set`.
CatalogueReply recorded(const QString &set, CatalogueRequest::Command command, const QString &subject)
{
    const FixtureSource source(fixtures + QLatin1Char('/') + set);
    const QList<FixtureSource::Recorded> all = source.invocations();
    for (const FixtureSource::Recorded &one : all) {
        if (one.request.command == command && one.request.subject == subject)
            return reply(one.output, one.exitStatus, one.errorOutput);
    }
    qFatal("the fixtures hold no answer for %s", qPrintable(subject));
    return CatalogueReply();
}

QJsonObject vocabulary()
{
    QFile file(fixtures + QStringLiteral("/vocabulary.json"));
    if (!file.open(QIODevice::ReadOnly))
        qFatal("fixtures/vocabulary.json cannot be read");
    return QJsonDocument::fromJson(file.readAll()).object();
}

QStringList list(const QJsonObject &object, const char *key)
{
    QStringList out;
    const QJsonArray values = object.value(QLatin1String(key)).toArray();
    for (const QJsonValue &value : values)
        out.append(value.toString());
    return out;
}

// The router's answer for Microsoft 365, to be damaged by a test.
QJsonObject officeRoutes()
{
    return QJsonDocument::fromJson(
               recorded(QStringLiteral("default"), CatalogueRequest::Route, QStringLiteral("com.microsoft.office")).output)
        .object();
}

QByteArray text(const QJsonObject &object)
{
    return QJsonDocument(object).toJson(QJsonDocument::Compact);
}

// Replaces one field of options[index].
QJsonObject withOption(QJsonObject report, int index, const QString &key, const QJsonValue &value)
{
    QJsonArray options = report.value(QLatin1String("options")).toArray();
    QJsonObject option = options.at(index).toObject();
    option.insert(key, value);
    options.replace(index, option);
    report.insert(QLatin1String("options"), options);
    return report;
}

} // namespace

class TestAnswers : public QObject
{
    Q_OBJECT

private Q_SLOTS:
    void a_search_is_read();
    void a_search_that_finds_nothing_is_an_answer_not_a_failure();
    void an_entry_is_read();
    void a_route_is_read_as_the_router_wrote_it();
    void no_route_is_an_answer_not_a_failure();
    void every_recorded_answer_is_read();

    void the_catalogue_s_own_error_keeps_its_code_and_words();
    void an_error_on_standard_error_keeps_its_code();
    void output_that_is_not_json_is_refused();
    void output_that_is_not_json_is_refused_data();
    void a_document_of_the_wrong_shape_is_refused();
    void a_document_of_the_wrong_shape_is_refused_data();
    void a_value_with_no_words_is_refused_not_shown();
    void a_value_with_no_words_is_refused_not_shown_data();
    void every_way_the_asking_can_fail_has_its_code();
    void every_way_the_asking_can_fail_has_its_code_data();
    void the_technical_layer_names_the_command_and_how_it_ended();

    void the_vocabulary_is_the_router_s();
    void the_codes_are_well_formed();
};

void TestAnswers::a_search_is_read()
{
    Answers::SearchAnswer answer;
    Answers::Failure failure;
    QVERIFY(Answers::readSearch(recorded(QStringLiteral("default"), CatalogueRequest::Search, QStringLiteral("office")),
                                &answer, &failure));
    QCOMPARE(answer.query, QStringLiteral("office"));
    QCOMPARE(answer.hits.size(), 2);
    // The order is the catalogue's.
    QCOMPARE(answer.hits.at(0).id, QStringLiteral("com.microsoft.office"));
    QCOMPARE(answer.hits.at(0).name, QStringLiteral("Microsoft 365"));
    QCOMPARE(answer.hits.at(0).publisher, QStringLiteral("Microsoft"));
    QCOMPARE(answer.hits.at(0).category, QStringLiteral("productivity"));
    QCOMPARE(answer.hits.at(1).id, QStringLiteral("org.libreoffice.libreoffice"));
    QVERIFY(answer.hits.at(0).score > answer.hits.at(1).score);
}

void TestAnswers::a_search_that_finds_nothing_is_an_answer_not_a_failure()
{
    const CatalogueReply nothing =
        recorded(QStringLiteral("default"), CatalogueRequest::Search, QStringLiteral("photothing"));
    // The program ends with status 6 and still prints its document.
    QCOMPARE(nothing.exitStatus, 6);
    Answers::SearchAnswer answer;
    Answers::Failure failure;
    QVERIFY(Answers::readSearch(nothing, &answer, &failure));
    QCOMPARE(answer.query, QStringLiteral("photothing"));
    QVERIFY(answer.hits.isEmpty());
}

void TestAnswers::an_entry_is_read()
{
    QVariantMap entry;
    Answers::Failure failure;
    QVERIFY(Answers::readEntry(
        recorded(QStringLiteral("default"), CatalogueRequest::Show, QStringLiteral("com.microsoft.office")), &entry,
        &failure));
    QCOMPARE(entry.value(QStringLiteral("name")).toString(), QStringLiteral("Microsoft 365"));
    QCOMPARE(entry.value(QStringLiteral("summary")).toString(),
             QStringLiteral("Office suite: Word, Excel, PowerPoint and others."));
    const QVariantList editions = entry.value(QStringLiteral("editions")).toList();
    QCOMPARE(editions.size(), 2);
    const QVariantMap windows = editions.at(0).toMap();
    QCOMPARE(windows.value(QStringLiteral("obtain")).toString(), QStringLiteral("publisher-page"));
    QCOMPARE(windows.value(QStringLiteral("records")).toList().size(), 1);
    QCOMPARE(windows.value(QStringLiteral("references")).toList().size(), 2);
}

void TestAnswers::a_route_is_read_as_the_router_wrote_it()
{
    const CatalogueReply office =
        recorded(QStringLiteral("default"), CatalogueRequest::Route, QStringLiteral("com.microsoft.office"));
    // "The recommended route needs something first" is status 4, and an
    // answer.
    QCOMPARE(office.exitStatus, 4);
    QVariantMap report;
    Answers::Failure failure;
    QVERIFY(Answers::readRoutes(office, &report, &failure));
    QCOMPARE(report.value(QStringLiteral("recommended")).toString(), QStringLiteral("windows-guest"));
    QCOMPARE(report.value(QStringLiteral("host_assumed")).toBool(), false);

    const QVariantList options = report.value(QStringLiteral("options")).toList();
    QCOMPARE(options.size(), 3);
    // The compatibility layer is listed before Windows itself, although it
    // is rated as not working: the router's order is kept.
    const QVariantMap layer = options.at(0).toMap();
    QCOMPARE(layer.value(QStringLiteral("route")).toString(), QStringLiteral("wine"));
    QCOMPARE(layer.value(QStringLiteral("rating")).toString(), QStringLiteral("broken"));
    QCOMPARE(layer.value(QStringLiteral("outlook")).toString(), QStringLiteral("doubtful"));
    const QVariantMap source = layer.value(QStringLiteral("rating_source")).toMap();
    QCOMPARE(source.value(QStringLiteral("kind")).toString(), QStringLiteral("external"));
    QCOMPARE(source.value(QStringLiteral("name")).toString(), QStringLiteral("WineHQ AppDB"));
    QCOMPARE(source.value(QStringLiteral("source_rating")).toString(), QStringLiteral("Garbage"));
    QCOMPARE(source.value(QStringLiteral("version_tested")).toString(), QStringLiteral("365 Business"));
    QCOMPARE(source.value(QStringLiteral("date")).toString(), QStringLiteral("2026-11-12"));

    const QVariantMap guest = options.at(1).toMap();
    QCOMPARE(guest.value(QStringLiteral("route")).toString(), QStringLiteral("windows-guest"));
    QCOMPARE(guest.value(QStringLiteral("recommended")).toBool(), true);
    // Unknown stays unknown.
    QCOMPARE(guest.value(QStringLiteral("rating")).toString(), QStringLiteral("unknown"));
    QCOMPARE(guest.value(QStringLiteral("rating_source")).toMap().value(QStringLiteral("kind")).toString(),
             QStringLiteral("unrated"));
    const QVariantList prerequisites = guest.value(QStringLiteral("prerequisites")).toList();
    QCOMPARE(prerequisites.size(), 2);
    // The router's sentences, as they are.
    QCOMPARE(prerequisites.at(0).toMap().value(QStringLiteral("text")).toString(),
             QStringLiteral("The Windows guest must be set up."));
    QCOMPARE(prerequisites.at(1).toMap().value(QStringLiteral("text")).toString(),
             QStringLiteral("You must supply a Windows licence: TUR does not include one."));
    // Null stays null: an option that is not "no route" has no alternative.
    QVERIFY(guest.value(QStringLiteral("alternative")).isNull());
}

void TestAnswers::no_route_is_an_answer_not_a_failure()
{
    const CatalogueReply valorant =
        recorded(QStringLiteral("default"), CatalogueRequest::Route, QStringLiteral("com.riotgames.valorant"));
    QCOMPARE(valorant.exitStatus, 3);
    QVariantMap report;
    Answers::Failure failure;
    QVERIFY(Answers::readRoutes(valorant, &report, &failure));
    QCOMPARE(report.value(QStringLiteral("recommended")).toString(), QStringLiteral("no-route"));
    const QVariantList options = report.value(QStringLiteral("options")).toList();
    const QVariantMap statement = options.at(0).toMap();
    QCOMPARE(statement.value(QStringLiteral("route")).toString(), QStringLiteral("no-route"));
    QCOMPARE(statement.value(QStringLiteral("alternative")).toMap().value(QStringLiteral("kind")).toString(),
             QStringLiteral("dual-boot"));
    // The blocked route follows the statement.
    QCOMPARE(options.at(1).toMap().value(QStringLiteral("outlook")).toString(), QStringLiteral("blocked"));
}

void TestAnswers::every_recorded_answer_is_read()
{
    int answers = 0;
    int failures = 0;
    for (const QString &set : { QStringLiteral("default"), QStringLiteral("assumed-host") }) {
        const FixtureSource source(fixtures + QLatin1Char('/') + set);
        QVERIFY2(source.problem().isEmpty(), qPrintable(source.problem()));
        const QList<FixtureSource::Recorded> all = source.invocations();
        QVERIFY(!all.isEmpty());
        for (const FixtureSource::Recorded &one : all) {
            const CatalogueReply answer = reply(one.output, one.exitStatus, one.errorOutput);
            Answers::Failure failure;
            bool read = false;
            switch (one.request.command) {
            case CatalogueRequest::Search: {
                Answers::SearchAnswer search;
                read = Answers::readSearch(answer, &search, &failure);
                break;
            }
            case CatalogueRequest::Show: {
                QVariantMap entry;
                read = Answers::readEntry(answer, &entry, &failure);
                break;
            }
            case CatalogueRequest::Route: {
                QVariantMap report;
                read = Answers::readRoutes(answer, &report, &failure);
                break;
            }
            }
            if (read) {
                ++answers;
                continue;
            }
            // What is not an answer is the catalogue's own report of an
            // error, never a document this program could not read.
            ++failures;
            QVERIFY2(failure.code.startsWith(QLatin1String("TUR-DESKTOP-CATALOGUE-")),
                     qPrintable(one.file + QStringLiteral(": ") + failure.code + QLatin1Char('\n') + failure.detail));
        }
    }
    QVERIFY(answers >= 20);
    // `show` and `route` of the identifier no application has.
    QCOMPARE(failures, 2);
}

void TestAnswers::the_catalogue_s_own_error_keeps_its_code_and_words()
{
    const CatalogueReply missing =
        recorded(QStringLiteral("default"), CatalogueRequest::Route, QStringLiteral("org.example.missing"));
    QCOMPARE(missing.exitStatus, 6);
    QVariantMap report;
    Answers::Failure failure;
    QVERIFY(!Answers::readRoutes(missing, &report, &failure));
    QCOMPARE(failure.code, QStringLiteral("TUR-DESKTOP-CATALOGUE-009"));
    QCOMPARE(failure.code, Answers::UnknownApplication);
    QCOMPARE(failure.title, QStringLiteral("No application has this ID"));
    QVERIFY(failure.reason.contains(QStringLiteral("org.example.missing")));
    QVERIFY(failure.detail.contains(QStringLiteral("Exit status: 6")));
    // Asking again would give the same answer.
    QVERIFY(!failure.retryable);
    QVERIFY(report.isEmpty());

    QVariantMap entry;
    QVERIFY(!Answers::readEntry(
        recorded(QStringLiteral("default"), CatalogueRequest::Show, QStringLiteral("org.example.missing")), &entry,
        &failure));
    QCOMPARE(failure.code, Answers::UnknownApplication);

    // A catalogue error with another code can be tried again.
    QVERIFY(!Answers::readRoutes(
        reply(R"({"error":{"code":"TUR-DESKTOP-CATALOGUE-012","title":"A host-facts or preferences file is not valid","message":"the host-facts file /etc/x is not valid: expected value at line 1 column 1"}})",
              1),
        &report, &failure));
    QCOMPARE(failure.code, QStringLiteral("TUR-DESKTOP-CATALOGUE-012"));
    QVERIFY(failure.retryable);

    // An error without a proper code is not passed on as one.
    QVERIFY(!Answers::readRoutes(reply(R"({"error":{"code":"oops","title":"x","message":"y"}})", 1), &report, &failure));
    QCOMPARE(failure.code, Answers::Unreadable);
}

void TestAnswers::an_error_on_standard_error_keeps_its_code()
{
    // With --json, tur-catalogue still reports a catalogue it cannot load on
    // standard error, as text, and prints nothing on standard output.
    const CatalogueReply unreadable = reply(
        QByteArray(), 1,
        "error[TUR-DESKTOP-CATALOGUE-001]: cannot read the catalogue directory /nowhere: it does not exist\n");
    Answers::SearchAnswer answer;
    Answers::Failure failure;
    QVERIFY(!Answers::readSearch(unreadable, &answer, &failure));
    QCOMPARE(failure.code, QStringLiteral("TUR-DESKTOP-CATALOGUE-001"));
    QCOMPARE(failure.reason, QStringLiteral("cannot read the catalogue directory /nowhere: it does not exist"));
    QVERIFY(!failure.title.isEmpty());
    QVERIFY(failure.detail.contains(QStringLiteral("Exit status: 1")));
    QVERIFY(failure.detail.contains(QStringLiteral("Standard error: error[TUR-DESKTOP-CATALOGUE-001]")));

    // The first of several problems names the code; all are in the detail.
    const CatalogueReply invalid = reply(QByteArray(), 5,
                                         "error[TUR-DESKTOP-CATALOGUE-003]: a.toml: bad\n"
                                         "error[TUR-DESKTOP-CATALOGUE-006]: b.toml: unsourced\n2 problems found\n");
    QVERIFY(!Answers::readSearch(invalid, &answer, &failure));
    QCOMPARE(failure.code, QStringLiteral("TUR-DESKTOP-CATALOGUE-003"));
    QVERIFY(failure.detail.contains(QStringLiteral("TUR-DESKTOP-CATALOGUE-006")));

    // Nothing at all, anywhere: this program's own code.
    QVERIFY(!Answers::readSearch(reply(QByteArray(), 1), &answer, &failure));
    QCOMPARE(failure.code, Answers::Unreadable);
    QVERIFY(failure.detail.contains(QStringLiteral("printed nothing")));
}

void TestAnswers::output_that_is_not_json_is_refused_data()
{
    QTest::addColumn<QByteArray>("output");
    QTest::newRow("plain text") << QByteArray("this is not JSON\n");
    QTest::newRow("cut short") << QByteArray("{\"query\": \"office\", \"hits\": [");
    QTest::newRow("a list") << QByteArray("[1, 2, 3]");
    QTest::newRow("a number") << QByteArray("42");
    QTest::newRow("two documents") << QByteArray("{\"query\": \"a\", \"hits\": []}\n{\"query\": \"b\", \"hits\": []}");
    QTest::newRow("not UTF-8") << QByteArray("{\"query\": \"\xff\xfe\", \"hits\": []}");
}

void TestAnswers::output_that_is_not_json_is_refused()
{
    QFETCH(QByteArray, output);
    Answers::SearchAnswer answer;
    QVariantMap map;
    Answers::Failure failure;

    QVERIFY(!Answers::readSearch(reply(output), &answer, &failure));
    QCOMPARE(failure.code, Answers::Unreadable);
    QVERIFY(!failure.title.isEmpty());
    QVERIFY(!failure.reason.isEmpty());
    QVERIFY(failure.detail.contains(QStringLiteral("Command: tur-catalogue")));
    QVERIFY(answer.hits.isEmpty());

    QVERIFY(!Answers::readEntry(reply(output), &map, &failure));
    QCOMPARE(failure.code, Answers::Unreadable);
    QVERIFY(!Answers::readRoutes(reply(output), &map, &failure));
    QCOMPARE(failure.code, Answers::Unreadable);
    QVERIFY(map.isEmpty());
}

void TestAnswers::a_document_of_the_wrong_shape_is_refused_data()
{
    QTest::addColumn<QByteArray>("output");
    QTest::addColumn<QString>("named");

    const QJsonObject office = officeRoutes();
    QVERIFY(!office.isEmpty());

    QTest::newRow("an empty object") << QByteArray("{}") << QStringLiteral("application");

    QJsonObject damaged = office;
    damaged.remove(QLatin1String("options"));
    QTest::newRow("no options") << text(damaged) << QStringLiteral("options");

    damaged = office;
    damaged.insert(QLatin1String("options"), QJsonArray());
    QTest::newRow("an empty list of options") << text(damaged) << QStringLiteral("never returns an empty answer");

    damaged = office;
    damaged.insert(QLatin1String("host_assumed"), QStringLiteral("no"));
    QTest::newRow("host_assumed is text") << text(damaged) << QStringLiteral("host_assumed");

    QTest::newRow("a reason that is a number")
        << text(withOption(office, 0, QStringLiteral("reason"), 7)) << QStringLiteral("options[0].reason");
    QTest::newRow("caveats that are not a list")
        << text(withOption(office, 1, QStringLiteral("caveats"), QStringLiteral("none")))
        << QStringLiteral("options[1].caveats");
    QTest::newRow("a rating source without its kind")
        << text(withOption(office, 0, QStringLiteral("rating_source"), QJsonObject()))
        << QStringLiteral("options[0].rating_source.kind");
    QTest::newRow("an external source without its name")
        << text(withOption(office, 0, QStringLiteral("rating_source"),
                           QJsonObject { { QLatin1String("kind"), QStringLiteral("external") },
                                         { QLatin1String("url"), QStringLiteral("https://example.org/") },
                                         { QLatin1String("date"), QStringLiteral("2026-11-12") } }))
        << QStringLiteral("options[0].rating_source.name");
    QTest::newRow("no option is recommended")
        << text(withOption(office, 1, QStringLiteral("recommended"), false)) << QStringLiteral("0 options are marked");
    QTest::newRow("two options are recommended")
        << text(withOption(office, 0, QStringLiteral("recommended"), true)) << QStringLiteral("2 options are marked");
    QTest::newRow("an alternative that is text")
        << text(withOption(office, 2, QStringLiteral("alternative"), QStringLiteral("a Mac")))
        << QStringLiteral("options[2].alternative");
    QTest::newRow("a caveat without its text")
        << text(withOption(office, 0, QStringLiteral("caveats"),
                           QJsonArray { QJsonObject { { QLatin1String("code"), QStringLiteral("requirement") },
                                                      { QLatin1String("severity"), QStringLiteral("note") } } }))
        << QStringLiteral("options[0].caveats[0].text");
}

void TestAnswers::a_document_of_the_wrong_shape_is_refused()
{
    QFETCH(QByteArray, output);
    QFETCH(QString, named);
    QVariantMap report;
    Answers::Failure failure;
    QVERIFY(!Answers::readRoutes(reply(output, 4), &report, &failure));
    QCOMPARE(failure.code, Answers::Unreadable);
    // The technical layer says which part of the document is wrong.
    QVERIFY2(failure.detail.contains(named), qPrintable(failure.detail));
    QVERIFY(report.isEmpty());
}

void TestAnswers::a_value_with_no_words_is_refused_not_shown_data()
{
    QTest::addColumn<QByteArray>("output");
    QTest::addColumn<QString>("named");

    const QJsonObject office = officeRoutes();
    QTest::newRow("a route")
        << text(withOption(office, 2, QStringLiteral("route"), QStringLiteral("teleport"))) << QStringLiteral("teleport");
    QTest::newRow("a rating")
        << text(withOption(office, 0, QStringLiteral("rating"), QStringLiteral("excellent")))
        << QStringLiteral("excellent");
    QTest::newRow("an outlook")
        << text(withOption(office, 0, QStringLiteral("outlook"), QStringLiteral("hopeful"))) << QStringLiteral("hopeful");
    QTest::newRow("a severity")
        << text(withOption(office, 0, QStringLiteral("caveats"),
                           QJsonArray { QJsonObject { { QLatin1String("code"), QStringLiteral("requirement") },
                                                      { QLatin1String("severity"), QStringLiteral("fatal") },
                                                      { QLatin1String("requirement"), QJsonValue() },
                                                      { QLatin1String("text"), QStringLiteral("x") } } }))
        << QStringLiteral("fatal");
    QTest::newRow("a kind of source")
        << text(withOption(office, 0, QStringLiteral("rating_source"),
                           QJsonObject { { QLatin1String("kind"), QStringLiteral("rumour") } }))
        << QStringLiteral("rumour");

    QJsonObject damaged = office;
    damaged.insert(QLatin1String("recommended"), QStringLiteral("teleport"));
    QTest::newRow("the recommended route") << text(damaged) << QStringLiteral("teleport");
}

void TestAnswers::a_value_with_no_words_is_refused_not_shown()
{
    // A newer router may know a route or a rating this program does not. It
    // is not shown raw, and it is not shown as something else.
    QFETCH(QByteArray, output);
    QFETCH(QString, named);
    QVariantMap report;
    Answers::Failure failure;
    QVERIFY(!Answers::readRoutes(reply(output, 0), &report, &failure));
    QCOMPARE(failure.code, Answers::Unreadable);
    QVERIFY2(failure.detail.contains(named), qPrintable(failure.detail));
    QVERIFY2(failure.detail.contains(QStringLiteral("has no words for")), qPrintable(failure.detail));
    QVERIFY(report.isEmpty());
}

void TestAnswers::every_way_the_asking_can_fail_has_its_code_data()
{
    QTest::addColumn<int>("failure");
    QTest::addColumn<QString>("code");
    QTest::addColumn<bool>("retryable");
    QTest::newRow("the program is missing") << int(CatalogueReply::NotStarted) << Answers::NotStarted << true;
    QTest::newRow("no answer in time") << int(CatalogueReply::TimedOut) << Answers::TimedOut << true;
    QTest::newRow("too much output") << int(CatalogueReply::TooLarge) << Answers::TooLarge << true;
    QTest::newRow("the program crashed") << int(CatalogueReply::Crashed) << Answers::Crashed << true;
    QTest::newRow("not recorded") << int(CatalogueReply::NotRecorded) << Answers::NotRecorded << false;
    QTest::newRow("not a usable subject") << int(CatalogueReply::Refused) << Answers::Refused << false;
}

void TestAnswers::every_way_the_asking_can_fail_has_its_code()
{
    QFETCH(int, failure);
    QFETCH(QString, code);
    QFETCH(bool, retryable);
    const CatalogueReply asked = failed(CatalogueReply::Failure(failure));

    Answers::SearchAnswer answer;
    QVariantMap map;
    Answers::Failure search;
    Answers::Failure entry;
    Answers::Failure routes;
    QVERIFY(!Answers::readSearch(asked, &answer, &search));
    QVERIFY(!Answers::readEntry(asked, &map, &entry));
    QVERIFY(!Answers::readRoutes(asked, &map, &routes));
    for (const Answers::Failure &one : { search, entry, routes }) {
        QCOMPARE(one.code, code);
        QCOMPARE(one.retryable, retryable);
        // What happened, and why, as sentences.
        QVERIFY(one.title.endsWith(QLatin1Char('.')));
        QVERIFY(one.reason.endsWith(QLatin1Char('.')));
        // The technical layer: the command, and what the system said. No
        // exit status, because the program did not end by itself.
        QVERIFY(one.detail.contains(QStringLiteral("Command: tur-catalogue search --json -- office")));
        QVERIFY(one.detail.contains(QStringLiteral("what the system said")));
        QVERIFY(!one.detail.contains(QStringLiteral("Exit status")));
    }

    const QVariantMap shown = search.toMap();
    QCOMPARE(shown.value(QStringLiteral("code")).toString(), code);
    QCOMPARE(shown.value(QStringLiteral("retryable")).toBool(), retryable);
    QCOMPARE(shown.value(QStringLiteral("title")).toString(), search.title);
}

void TestAnswers::the_technical_layer_names_the_command_and_how_it_ended()
{
    CatalogueReply ended = reply("not JSON", 2, QByteArray(5000, 'e'));
    const QString detail = Answers::technicalDetail(ended, QStringLiteral("a note"));
    const QStringList lines = detail.split(QLatin1Char('\n'));
    QCOMPARE(lines.at(0), QStringLiteral("Command: tur-catalogue route --json -- org.example.app"));
    QCOMPARE(lines.at(1), QStringLiteral("Exit status: 2"));
    QCOMPARE(lines.at(2), QStringLiteral("a note"));
    // Standard error is cut short, and says so.
    QVERIFY(lines.at(3).startsWith(QStringLiteral("Standard error: eeee")));
    QVERIFY(lines.at(3).endsWith(QStringLiteral(" ...")));
    QVERIFY(lines.at(3).size() < 2100);
}

void TestAnswers::the_vocabulary_is_the_router_s()
{
    // fixtures/vocabulary.json is written from the router's own types by a
    // test in the Rust workflow. A value the router gains fails here until
    // this program has words for it (the design's risk 17).
    const QJsonObject router = vocabulary();
    QVERIFY(!router.isEmpty());
    QCOMPARE(Answers::routes(), list(router, "routes"));
    QCOMPARE(Answers::ratings(), list(router, "ratings"));
    QCOMPARE(Answers::outlooks(), list(router, "outlooks"));
    QCOMPARE(Answers::severities(), list(router, "severities"));
    QCOMPARE(Answers::ratingSourceKinds(), list(router, "rating_source_kinds"));

    // The catalogue's codes this program names are the catalogue's.
    const QStringList codes = list(router, "error_codes");
    QVERIFY(codes.contains(Answers::UnknownApplication));
    QVERIFY(codes.contains(Answers::NoMatch));
    QVERIFY(codes.contains(Answers::NoRoute));
}

void TestAnswers::the_codes_are_well_formed()
{
    const QRegularExpression pattern(QStringLiteral("^TUR-DESKTOP-CENTRE-[0-9]{3}$"));
    const QStringList own { Answers::NotStarted, Answers::TimedOut, Answers::TooLarge, Answers::Unreadable,
                            Answers::Crashed,    Answers::NotRecorded, Answers::Refused };
    QStringList seen;
    for (const QString &code : own) {
        QVERIFY2(pattern.match(code).hasMatch(), qPrintable(code));
        QVERIFY2(!seen.contains(code), qPrintable(code));
        seen.append(code);
    }
}

QTEST_GUILESS_MAIN(TestAnswers)

#include "tst_answers.moc"
