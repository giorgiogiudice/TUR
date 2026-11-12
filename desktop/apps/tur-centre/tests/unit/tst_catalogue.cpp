// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
//
// What the views read: the search and an application's page, over recorded
// answers of the router, over a program that fails, and over a source that
// answers out of order.

#include "catalogue.h"
#include "fixturesource.h"

#include <QSignalSpy>
#include <QTest>

namespace {

const QString fixtures = QStringLiteral(TUR_CENTRE_FIXTURES);
const QString helper = QStringLiteral(TUR_CENTRE_HELPER);

// A source the test answers by hand, in whatever order it likes, and that
// counts what it is asked.
class ManualSource : public CatalogueSource
{
public:
    struct Asked
    {
        quint64 id = 0;
        CatalogueRequest request;
    };

    quint64 submit(const CatalogueRequest &request) override
    {
        Asked asked;
        asked.id = nextId();
        asked.request = request;
        waiting.append(asked);
        ++submitted;
        return asked.id;
    }

    void cancel(quint64 id) override
    {
        for (qsizetype i = 0; i < waiting.size(); ++i) {
            if (waiting.at(i).id == id) {
                waiting.removeAt(i);
                ++cancelled;
                return;
            }
        }
    }

    // Answers the oldest waiting question about `subject` from `recorded`.
    bool answer(const QString &subject, FixtureSource &recorded)
    {
        for (qsizetype i = 0; i < waiting.size(); ++i) {
            if (waiting.at(i).request.subject != subject)
                continue;
            const Asked asked = waiting.takeAt(i);
            const QList<FixtureSource::Recorded> all = recorded.invocations();
            for (const FixtureSource::Recorded &one : all) {
                if (one.request.command == asked.request.command && one.request.subject == subject) {
                    CatalogueReply reply;
                    reply.exitStatus = one.exitStatus;
                    reply.output = one.output;
                    Q_EMIT finished(asked.id, reply);
                    return true;
                }
            }
            return false;
        }
        return false;
    }

    QList<Asked> waiting;
    int submitted = 0;
    int cancelled = 0;
};

QVariantMap row(const HitsModel *hits, int index)
{
    QVariantMap out;
    const QHash<int, QByteArray> names = hits->roleNames();
    for (auto role = names.cbegin(); role != names.cend(); ++role)
        out.insert(QString::fromLatin1(role.value()), hits->data(hits->index(index), role.key()));
    return out;
}

} // namespace

class TestCatalogue : public QObject
{
    Q_OBJECT

private Q_SLOTS:
    void a_search_shows_its_rows_then_fills_them_in();
    void the_row_carries_what_the_router_recommends();
    void a_search_that_finds_nothing_says_so();
    void an_empty_query_asks_nothing();
    void only_the_last_search_is_shown();
    void a_search_fails_with_the_failure_s_code();
    void a_search_can_be_tried_again();

    void an_application_is_its_entry_and_the_router_s_options();
    void no_route_is_stated_with_its_blocked_routes();
    void an_assumed_machine_is_said_to_be_assumed();
    void an_application_the_catalogue_does_not_know_fails_with_the_catalogue_s_code();
    void all_reports_are_the_catalogue_s_records_for_that_way();
    void only_the_last_application_asked_for_is_shown();

    void an_application_is_asked_about_once();
    void a_failure_is_not_remembered();
    void a_change_of_configuration_forgets_what_was_answered();
    void the_vocabulary_is_offered_to_the_views();
};

void TestCatalogue::a_search_shows_its_rows_then_fills_them_in()
{
    FixtureSource recorded(fixtures + QStringLiteral("/default"));
    Catalogue catalogue;
    auto *source = new ManualSource;
    catalogue.setSource(source);
    SearchSession *search = catalogue.search();
    QCOMPARE(search->status(), SearchSession::Idle);
    QVERIFY(search->settled());
    QCOMPARE(search->hits()->count(), 0);

    QSignalSpy status(search, &SearchSession::statusChanged);
    search->setQuery(QStringLiteral("  office "));
    // What is asked is what was typed, without the space around it.
    QCOMPARE(search->query(), QStringLiteral("office"));
    QCOMPARE(source->waiting.first().request.subject, QStringLiteral("office"));
    // Asking does not wait: the answer is not there yet.
    QCOMPARE(search->status(), SearchSession::Searching);
    QVERIFY(!search->settled());
    QCOMPARE(search->hits()->count(), 0);

    // The rows arrive with the search's answer, before anything is known
    // about how each application runs.
    QVERIFY(source->answer(QStringLiteral("office"), recorded));
    QCOMPARE(search->status(), SearchSession::Results);
    QCOMPARE(search->hits()->count(), 2);
    QCOMPARE(search->answeredQuery(), QStringLiteral("office"));
    QVariantMap first = row(search->hits(), 0);
    QCOMPARE(first.value(QStringLiteral("applicationId")).toString(), QStringLiteral("com.microsoft.office"));
    QCOMPARE(first.value(QStringLiteral("name")).toString(), QStringLiteral("Microsoft 365"));
    QCOMPARE(first.value(QStringLiteral("publisher")).toString(), QStringLiteral("Microsoft"));
    QCOMPARE(first.value(QStringLiteral("detailed")).toBool(), false);
    QVERIFY(first.value(QStringLiteral("recommended")).toMap().isEmpty());
    QVERIFY(!search->settled());

    // Then each row is filled in, as its answers arrive: the second first.
    QSignalSpy changed(search->hits(), &QAbstractItemModel::dataChanged);
    while (source->answer(QStringLiteral("org.libreoffice.libreoffice"), recorded)) { }
    QCOMPARE(changed.count(), 1);
    QCOMPARE(row(search->hits(), 0).value(QStringLiteral("detailed")).toBool(), false);
    QCOMPARE(row(search->hits(), 1).value(QStringLiteral("detailed")).toBool(), true);
    QVERIFY(!search->settled());

    while (source->answer(QStringLiteral("com.microsoft.office"), recorded)) { }
    QCOMPARE(row(search->hits(), 0).value(QStringLiteral("detailed")).toBool(), true);
    QVERIFY(search->settled());
    QCOMPARE(search->status(), SearchSession::Results);
    QVERIFY(search->failure().isEmpty());
    // The order is the catalogue's, whatever order the answers came in.
    QCOMPARE(search->hits()->ids(),
             (QStringList { QStringLiteral("com.microsoft.office"), QStringLiteral("org.libreoffice.libreoffice") }));
    QCOMPARE(status.count(), 2);
}

void TestCatalogue::the_row_carries_what_the_router_recommends()
{
    Catalogue catalogue;
    catalogue.setFixture(fixtures + QStringLiteral("/default"));
    SearchSession *search = catalogue.search();
    search->setQuery(QStringLiteral("office"));
    QTRY_VERIFY(search->settled());

    // Microsoft 365: the router recommends the Windows guest, which nobody
    // has reported on, and which needs two things first.
    const QVariantMap office = row(search->hits(), 0);
    const QVariantMap recommended = office.value(QStringLiteral("recommended")).toMap();
    QCOMPARE(recommended.value(QStringLiteral("route")).toString(), QStringLiteral("windows-guest"));
    QCOMPARE(recommended.value(QStringLiteral("rating")).toString(), QStringLiteral("unknown"));
    QCOMPARE(recommended.value(QStringLiteral("prerequisites")).toList().size(), 2);
    QCOMPARE(office.value(QStringLiteral("entry")).toMap().value(QStringLiteral("summary")).toString(),
             QStringLiteral("Office suite: Word, Excel, PowerPoint and others."));

    const QVariantMap libre = row(search->hits(), 1);
    QCOMPARE(libre.value(QStringLiteral("recommended")).toMap().value(QStringLiteral("route")).toString(),
             QStringLiteral("flatpak"));

    // An application that cannot run is listed all the same, and says so.
    search->setQuery(QStringLiteral("valorant"));
    QTRY_VERIFY(search->settled());
    QCOMPARE(search->hits()->count(), 1);
    QCOMPARE(row(search->hits(), 0).value(QStringLiteral("recommended")).toMap().value(QStringLiteral("route")).toString(),
             QStringLiteral("no-route"));

    // Another name, and a typing mistake: the catalogue's search finds it.
    search->setQuery(QStringLiteral("ms office"));
    QTRY_VERIFY(search->settled());
    QCOMPARE(search->hits()->ids(), QStringList { QStringLiteral("com.microsoft.office") });
    search->setQuery(QStringLiteral("ofice"));
    QTRY_VERIFY(search->settled());
    QCOMPARE(search->hits()->ids(), QStringList { QStringLiteral("com.microsoft.office") });

    // A class of applications that share an answer.
    search->setQuery(QStringLiteral("banking app"));
    QTRY_VERIFY(search->settled());
    QCOMPARE(search->hits()->ids(), QStringList { QStringLiteral("tur.class.android-banking") });
}

void TestCatalogue::a_search_that_finds_nothing_says_so()
{
    Catalogue catalogue;
    catalogue.setFixture(fixtures + QStringLiteral("/default"));
    SearchSession *search = catalogue.search();
    search->setQuery(QStringLiteral("photothing"));
    QTRY_COMPARE(search->status(), SearchSession::NothingFound);
    QVERIFY(search->settled());
    QCOMPARE(search->hits()->count(), 0);
    QCOMPARE(search->answeredQuery(), QStringLiteral("photothing"));
    // Nothing found is an answer, not a failure.
    QVERIFY(search->failure().isEmpty());
}

void TestCatalogue::an_empty_query_asks_nothing()
{
    Catalogue catalogue;
    auto *source = new ManualSource;
    catalogue.setSource(source);
    SearchSession *search = catalogue.search();

    search->setQuery(QStringLiteral("   "));
    QCOMPARE(search->status(), SearchSession::Idle);
    QCOMPARE(source->submitted, 0);

    search->setQuery(QStringLiteral("office"));
    QCOMPARE(source->submitted, 1);
    // Clearing the field forgets the question that was on its way.
    search->setQuery(QString());
    QCOMPARE(search->status(), SearchSession::Idle);
    QCOMPARE(source->cancelled, 1);
    QVERIFY(search->settled());
    QCOMPARE(search->hits()->count(), 0);
    QCOMPARE(search->answeredQuery(), QString());
}

void TestCatalogue::only_the_last_search_is_shown()
{
    FixtureSource recorded(fixtures + QStringLiteral("/default"));
    Catalogue catalogue;
    auto *source = new ManualSource;
    catalogue.setSource(source);
    SearchSession *search = catalogue.search();

    // Typing: three searches, each sent before the last was answered.
    search->setQuery(QStringLiteral("krita"));
    search->setQuery(QStringLiteral("valorant"));
    search->setQuery(QStringLiteral("office"));
    // The first two were taken back when they were replaced.
    QCOMPARE(source->submitted, 3);
    QCOMPARE(source->cancelled, 2);
    QCOMPARE(source->waiting.size(), 1);
    QCOMPARE(source->waiting.first().request.subject, QStringLiteral("office"));
    QCOMPARE(search->status(), SearchSession::Searching);

    QVERIFY(source->answer(QStringLiteral("office"), recorded));
    QCOMPARE(search->status(), SearchSession::Results);
    QCOMPARE(search->hits()->count(), 2);

    // A new search while the rows are being filled in: what arrives for the
    // old rows is not put into the new ones.
    search->setQuery(QStringLiteral("krita"));
    QVERIFY(source->answer(QStringLiteral("krita"), recorded));
    QCOMPARE(search->hits()->ids(), QStringList { QStringLiteral("org.kde.krita") });
    while (source->answer(QStringLiteral("com.microsoft.office"), recorded)) { }
    while (source->answer(QStringLiteral("org.libreoffice.libreoffice"), recorded)) { }
    QCOMPARE(search->hits()->ids(), QStringList { QStringLiteral("org.kde.krita") });
    QVERIFY(!search->settled());
    QCOMPARE(row(search->hits(), 0).value(QStringLiteral("detailed")).toBool(), false);

    while (source->answer(QStringLiteral("org.kde.krita"), recorded)) { }
    QVERIFY(search->settled());
    QCOMPARE(row(search->hits(), 0).value(QStringLiteral("detailed")).toBool(), true);
    QCOMPARE(search->answeredQuery(), QStringLiteral("krita"));
}

void TestCatalogue::a_search_fails_with_the_failure_s_code()
{
    Catalogue catalogue;
    catalogue.setProgram(QStringLiteral("/nonexistent/tur-catalogue"));
    SearchSession *search = catalogue.search();
    search->setQuery(QStringLiteral("office"));
    QTRY_COMPARE(search->status(), SearchSession::Failed);
    QVERIFY(search->settled());
    QCOMPARE(search->hits()->count(), 0);
    const QVariantMap failure = search->failure();
    QCOMPARE(failure.value(QStringLiteral("code")).toString(), Answers::NotStarted);
    QVERIFY(!failure.value(QStringLiteral("title")).toString().isEmpty());
    QVERIFY(!failure.value(QStringLiteral("reason")).toString().isEmpty());
    QVERIFY(failure.value(QStringLiteral("detail")).toString().contains(
        QStringLiteral("Command: /nonexistent/tur-catalogue search --json -- office")));
    QCOMPARE(failure.value(QStringLiteral("retryable")).toBool(), true);

    // The catalogue's own failure keeps the catalogue's code: the stand-in
    // reports a catalogue it cannot load, as tur-catalogue does.
    catalogue.setProgram(helper);
    search->setQuery(QStringLiteral("refused"));
    QTRY_COMPARE(search->status(), SearchSession::Failed);
    QCOMPARE(search->failure().value(QStringLiteral("code")).toString(), QStringLiteral("TUR-DESKTOP-CATALOGUE-001"));

    // A program that prints nonsense, one that prints too much to be an
    // answer of this kind, and one that ends abnormally.
    search->setQuery(QStringLiteral("garbage"));
    QTRY_COMPARE(search->status(), SearchSession::Failed);
    QCOMPARE(search->failure().value(QStringLiteral("code")).toString(), Answers::Unreadable);
    search->setQuery(QStringLiteral("crash"));
    QTRY_COMPARE(search->status(), SearchSession::Failed);
    QCOMPARE(search->failure().value(QStringLiteral("code")).toString(), Answers::Crashed);

    // One that never answers is stopped at the time limit.
    catalogue.setTimeout(300);
    search->setQuery(QStringLiteral("sleep"));
    QCOMPARE(search->status(), SearchSession::Searching);
    QTRY_COMPARE_WITH_TIMEOUT(search->status(), SearchSession::Failed, 20000);
    QCOMPARE(search->failure().value(QStringLiteral("code")).toString(), Answers::TimedOut);
}

void TestCatalogue::a_search_can_be_tried_again()
{
    FixtureSource recorded(fixtures + QStringLiteral("/default"));
    Catalogue catalogue;
    auto *source = new ManualSource;
    catalogue.setSource(source);
    SearchSession *search = catalogue.search();

    search->setQuery(QStringLiteral("krita"));
    // The program fails this once.
    CatalogueReply crashed;
    crashed.failure = CatalogueReply::Crashed;
    Q_EMIT source->finished(source->waiting.takeFirst().id, crashed);
    QCOMPARE(search->status(), SearchSession::Failed);
    QCOMPARE(search->failure().value(QStringLiteral("code")).toString(), Answers::Crashed);

    search->retry();
    QCOMPARE(search->status(), SearchSession::Searching);
    QVERIFY(search->failure().isEmpty());
    QVERIFY(source->answer(QStringLiteral("krita"), recorded));
    while (source->answer(QStringLiteral("org.kde.krita"), recorded)) { }
    QCOMPARE(search->status(), SearchSession::Results);
    QVERIFY(search->settled());
}

void TestCatalogue::an_application_is_its_entry_and_the_router_s_options()
{
    Catalogue catalogue;
    catalogue.setFixture(fixtures + QStringLiteral("/default"));
    ApplicationSession *application = catalogue.application();
    QCOMPARE(application->status(), ApplicationSession::Idle);

    application->setApplicationId(QStringLiteral("com.microsoft.office"));
    QCOMPARE(application->status(), ApplicationSession::Loading);
    QTRY_COMPARE(application->status(), ApplicationSession::Ready);

    QCOMPARE(application->entry().value(QStringLiteral("name")).toString(), QStringLiteral("Microsoft 365"));
    QVERIFY(!application->noRoute());
    QVERIFY(!application->hostAssumed());
    QVERIFY(application->failure().isEmpty());

    // Every option, in the router's order: the layer before Windows itself.
    const QVariantList options = application->options();
    QCOMPARE(options.size(), 3);
    QCOMPARE(options.at(0).toMap().value(QStringLiteral("route")).toString(), QStringLiteral("wine"));
    QCOMPARE(options.at(1).toMap().value(QStringLiteral("route")).toString(), QStringLiteral("windows-guest"));
    QCOMPARE(options.at(2).toMap().value(QStringLiteral("route")).toString(), QStringLiteral("web"));

    // The one the router marks recommended, and the others in its order.
    QCOMPARE(application->recommended().value(QStringLiteral("route")).toString(), QStringLiteral("windows-guest"));
    const QVariantList others = application->others();
    QCOMPARE(others.size(), 2);
    QCOMPARE(others.at(0).toMap().value(QStringLiteral("route")).toString(), QStringLiteral("wine"));
    QCOMPARE(others.at(1).toMap().value(QStringLiteral("route")).toString(), QStringLiteral("web"));

    // Nothing is asked again for the page that is already there.
    QSignalSpy loaded(application, &ApplicationSession::loaded);
    application->setApplicationId(QStringLiteral("com.microsoft.office"));
    QCOMPARE(application->status(), ApplicationSession::Ready);
    QCOMPARE(loaded.count(), 0);

    // No application: nothing to show.
    application->setApplicationId(QString());
    QCOMPARE(application->status(), ApplicationSession::Idle);
    QTRY_VERIFY(application->entry().isEmpty());
    QVERIFY(application->options().isEmpty());
}

void TestCatalogue::no_route_is_stated_with_its_blocked_routes()
{
    Catalogue catalogue;
    catalogue.setFixture(fixtures + QStringLiteral("/default"));
    ApplicationSession *application = catalogue.application();

    application->setApplicationId(QStringLiteral("com.riotgames.valorant"));
    QTRY_COMPARE(application->status(), ApplicationSession::Ready);
    QVERIFY(application->noRoute());
    const QVariantMap statement = application->recommended();
    QCOMPARE(statement.value(QStringLiteral("route")).toString(), QStringLiteral("no-route"));
    QVERIFY(statement.value(QStringLiteral("reason")).toString().startsWith(
        QStringLiteral("No route on TUR runs Valorant on this machine.")));
    QCOMPARE(statement.value(QStringLiteral("alternative")).toMap().value(QStringLiteral("kind")).toString(),
             QStringLiteral("dual-boot"));
    // The routes that were considered follow, each blocked.
    QCOMPARE(application->others().size(), 1);
    const QVariantMap proton = application->others().at(0).toMap();
    QCOMPARE(proton.value(QStringLiteral("route")).toString(), QStringLiteral("proton"));
    QCOMPARE(proton.value(QStringLiteral("outlook")).toString(), QStringLiteral("blocked"));
    QCOMPARE(proton.value(QStringLiteral("rating")).toString(), QStringLiteral("broken"));

    application->setApplicationId(QStringLiteral("tur.class.android-banking"));
    QTRY_COMPARE(application->status(), ApplicationSession::Ready);
    QVERIFY(application->noRoute());
    QCOMPARE(application->recommended().value(QStringLiteral("alternative")).toMap().value(QStringLiteral("kind")).toString(),
             QStringLiteral("certified-android-device"));
    QCOMPARE(application->entry().value(QStringLiteral("entry_kind")).toString(), QStringLiteral("class"));
}

void TestCatalogue::an_assumed_machine_is_said_to_be_assumed()
{
    Catalogue catalogue;
    catalogue.setFixture(fixtures + QStringLiteral("/assumed-host"));
    ApplicationSession *application = catalogue.application();
    application->setApplicationId(QStringLiteral("com.netflix.netflix"));
    QTRY_COMPARE(application->status(), ApplicationSession::Ready);
    // No host facts were given to the router: it assumed a machine, and the
    // page must say so.
    QVERIFY(application->hostAssumed());
    QCOMPARE(application->recommended().value(QStringLiteral("route")).toString(), QStringLiteral("web"));
    QCOMPARE(application->others().size(), 1);
    QCOMPARE(application->others().at(0).toMap().value(QStringLiteral("outlook")).toString(),
             QStringLiteral("doubtful"));
}

void TestCatalogue::an_application_the_catalogue_does_not_know_fails_with_the_catalogue_s_code()
{
    Catalogue catalogue;
    catalogue.setFixture(fixtures + QStringLiteral("/default"));
    ApplicationSession *application = catalogue.application();
    application->setApplicationId(QStringLiteral("org.example.missing"));
    QTRY_COMPARE(application->status(), ApplicationSession::Failed);
    const QVariantMap failure = application->failure();
    QCOMPARE(failure.value(QStringLiteral("code")).toString(), QStringLiteral("TUR-DESKTOP-CATALOGUE-009"));
    // The catalogue's own words.
    QCOMPARE(failure.value(QStringLiteral("title")).toString(), QStringLiteral("No application has this ID"));
    QVERIFY(failure.value(QStringLiteral("reason")).toString().contains(QStringLiteral("org.example.missing")));
    QCOMPARE(failure.value(QStringLiteral("retryable")).toBool(), false);
    QVERIFY(application->entry().isEmpty());
    QVERIFY(application->options().isEmpty());
    QVERIFY(!application->noRoute());

    // An identifier that cannot even be asked about.
    application->setApplicationId(QStringLiteral("two\nlines"));
    QTRY_COMPARE(application->status(), ApplicationSession::Failed);
    QCOMPARE(application->failure().value(QStringLiteral("code")).toString(), Answers::Refused);

    // And one a fixture has no answer for is not guessed at.
    application->setApplicationId(QStringLiteral("org.mozilla.firefox"));
    QTRY_COMPARE(application->status(), ApplicationSession::Failed);
    QCOMPARE(application->failure().value(QStringLiteral("code")).toString(), Answers::NotRecorded);

    // A page that loads afterwards carries no trace of the failure.
    application->setApplicationId(QStringLiteral("org.kde.krita"));
    QTRY_COMPARE(application->status(), ApplicationSession::Ready);
    QVERIFY(application->failure().isEmpty());
}

void TestCatalogue::all_reports_are_the_catalogue_s_records_for_that_way()
{
    Catalogue catalogue;
    catalogue.setFixture(fixtures + QStringLiteral("/default"));
    ApplicationSession *application = catalogue.application();

    // Elden Ring through Proton has two records: ProtonDB's and Valve's.
    application->setApplicationId(QStringLiteral("com.bandainamco.eldenring"));
    QTRY_COMPARE(application->status(), ApplicationSession::Ready);
    const QVariantMap proton = application->recommended();
    QCOMPARE(proton.value(QStringLiteral("route")).toString(), QStringLiteral("proton"));
    const QVariantList reports = application->reports(proton);
    QCOMPARE(reports.size(), 2);
    QCOMPARE(reports.at(0).toMap().value(QStringLiteral("source_name")).toString(), QStringLiteral("ProtonDB"));
    QCOMPARE(reports.at(1).toMap().value(QStringLiteral("source_name")).toString(),
             QStringLiteral("Valve Steam Deck compatibility"));
    // The rating on the page is the one the router returns for the option.
    QCOMPARE(proton.value(QStringLiteral("rating")).toString(), QStringLiteral("usable"));

    // Microsoft 365: one record for the layer, none for Windows or the web.
    application->setApplicationId(QStringLiteral("com.microsoft.office"));
    QTRY_COMPARE(application->status(), ApplicationSession::Ready);
    QCOMPARE(application->reports(application->others().at(0).toMap()).size(), 1);
    QCOMPARE(application->reports(application->recommended()).size(), 0);
    QCOMPARE(application->reports(application->others().at(1).toMap()).size(), 0);
    // An option that runs no edition has no records.
    QCOMPARE(application->reports(QVariantMap()).size(), 0);
}

void TestCatalogue::only_the_last_application_asked_for_is_shown()
{
    FixtureSource recorded(fixtures + QStringLiteral("/default"));
    Catalogue catalogue;
    auto *source = new ManualSource;
    catalogue.setSource(source);
    ApplicationSession *application = catalogue.application();

    application->setApplicationId(QStringLiteral("org.kde.krita"));
    application->setApplicationId(QStringLiteral("com.microsoft.office"));
    QCOMPARE(application->status(), ApplicationSession::Loading);

    // The second answers first; the first arrives late and is not shown.
    while (source->answer(QStringLiteral("com.microsoft.office"), recorded)) { }
    QCOMPARE(application->status(), ApplicationSession::Ready);
    QCOMPARE(application->entry().value(QStringLiteral("name")).toString(), QStringLiteral("Microsoft 365"));
    while (source->answer(QStringLiteral("org.kde.krita"), recorded)) { }
    QCOMPARE(application->status(), ApplicationSession::Ready);
    QCOMPARE(application->entry().value(QStringLiteral("name")).toString(), QStringLiteral("Microsoft 365"));
    QCOMPARE(application->applicationId(), QStringLiteral("com.microsoft.office"));
}

void TestCatalogue::an_application_is_asked_about_once()
{
    FixtureSource recorded(fixtures + QStringLiteral("/default"));
    Catalogue catalogue;
    auto *source = new ManualSource;
    catalogue.setSource(source);

    // The search asks about Krita for its row: `show` and `route`.
    catalogue.search()->setQuery(QStringLiteral("krita"));
    QVERIFY(source->answer(QStringLiteral("krita"), recorded));
    QCOMPARE(source->submitted, 3);
    // The page asks for the same application while that is under way: no
    // second pair of questions.
    catalogue.application()->setApplicationId(QStringLiteral("org.kde.krita"));
    QCOMPARE(source->submitted, 3);
    while (source->answer(QStringLiteral("org.kde.krita"), recorded)) { }
    QVERIFY(catalogue.search()->settled());
    QCOMPARE(catalogue.application()->status(), ApplicationSession::Ready);

    // And afterwards it is remembered: opening the page again asks nothing.
    catalogue.application()->setApplicationId(QString());
    catalogue.application()->setApplicationId(QStringLiteral("org.kde.krita"));
    QTRY_COMPARE(catalogue.application()->status(), ApplicationSession::Ready);
    QCOMPARE(source->submitted, 3);
}

void TestCatalogue::a_failure_is_not_remembered()
{
    FixtureSource recorded(fixtures + QStringLiteral("/default"));
    Catalogue catalogue;
    auto *source = new ManualSource;
    catalogue.setSource(source);
    ApplicationSession *application = catalogue.application();

    application->setApplicationId(QStringLiteral("org.kde.krita"));
    QCOMPARE(source->waiting.size(), 2);
    // `show` answers; `route` times out.
    CatalogueReply timedOut;
    timedOut.failure = CatalogueReply::TimedOut;
    for (qsizetype i = 0; i < source->waiting.size(); ++i) {
        if (source->waiting.at(i).request.command == CatalogueRequest::Route) {
            Q_EMIT source->finished(source->waiting.takeAt(i).id, timedOut);
            break;
        }
    }
    QCOMPARE(application->status(), ApplicationSession::Loading);
    QVERIFY(source->answer(QStringLiteral("org.kde.krita"), recorded));
    QCOMPARE(application->status(), ApplicationSession::Failed);
    QCOMPARE(application->failure().value(QStringLiteral("code")).toString(), Answers::TimedOut);
    // Half an answer is not shown as a page.
    QVERIFY(application->entry().isEmpty());

    // "Try again" asks again, and this time it works.
    application->retry();
    QCOMPARE(application->status(), ApplicationSession::Loading);
    QCOMPARE(source->submitted, 4);
    while (source->answer(QStringLiteral("org.kde.krita"), recorded)) { }
    QCOMPARE(application->status(), ApplicationSession::Ready);
    QVERIFY(application->failure().isEmpty());
    QCOMPARE(application->recommended().value(QStringLiteral("route")).toString(), QStringLiteral("flatpak"));
}

void TestCatalogue::a_change_of_configuration_forgets_what_was_answered()
{
    Catalogue catalogue;
    QSignalSpy changed(&catalogue, &Catalogue::configurationChanged);
    catalogue.setFixture(fixtures + QStringLiteral("/default"));
    QCOMPARE(changed.count(), 1);
    catalogue.setFixture(fixtures + QStringLiteral("/default"));
    QCOMPARE(changed.count(), 1);

    ApplicationSession *application = catalogue.application();
    application->setApplicationId(QStringLiteral("com.netflix.netflix"));
    QTRY_COMPARE(application->status(), ApplicationSession::Failed);
    QCOMPARE(application->failure().value(QStringLiteral("code")).toString(), Answers::NotRecorded);

    // Another machine's answers: what the first set said is not kept.
    catalogue.setFixture(fixtures + QStringLiteral("/assumed-host"));
    QCOMPARE(changed.count(), 2);
    application->retry();
    QTRY_COMPARE(application->status(), ApplicationSession::Ready);
    QVERIFY(application->hostAssumed());

    QCOMPARE(catalogue.program(), QStringLiteral("tur-catalogue"));
    catalogue.setProgram(QStringLiteral("other"));
    catalogue.setCatalogueDirectory(QStringLiteral("/c"));
    catalogue.setHostFacts(QStringLiteral("/h"));
    catalogue.setPreferences(QStringLiteral("/p"));
    catalogue.setTimeout(0);
    QCOMPARE(catalogue.program(), QStringLiteral("other"));
    QCOMPARE(catalogue.catalogueDirectory(), QStringLiteral("/c"));
    QCOMPARE(catalogue.hostFacts(), QStringLiteral("/h"));
    QCOMPARE(catalogue.preferences(), QStringLiteral("/p"));
    // A time limit is at least a millisecond: never "no limit".
    QCOMPARE(catalogue.timeout(), 1);
    QCOMPARE(changed.count(), 7);
}

void TestCatalogue::the_vocabulary_is_offered_to_the_views()
{
    const Catalogue catalogue;
    const QVariantMap known = catalogue.knownVocabulary();
    QCOMPARE(known.value(QStringLiteral("routes")).toStringList(), Answers::routes());
    QCOMPARE(known.value(QStringLiteral("ratings")).toStringList(), Answers::ratings());
    QCOMPARE(known.value(QStringLiteral("outlooks")).toStringList(), Answers::outlooks());
    QCOMPARE(known.value(QStringLiteral("severities")).toStringList(), Answers::severities());
    QCOMPARE(known.value(QStringLiteral("rating_source_kinds")).toStringList(), Answers::ratingSourceKinds());
}

QTEST_GUILESS_MAIN(TestCatalogue)

#include "tst_catalogue.moc"
