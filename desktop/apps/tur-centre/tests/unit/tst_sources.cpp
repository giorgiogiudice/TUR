// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
//
// The two places the application centre's answers come from: a program that
// is run, and a directory of recorded answers. What each does when things go
// wrong, and that neither ever blocks the caller.

#include "fixturesource.h"
#include "processsource.h"

#include <QElapsedTimer>
#include <QFile>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QSignalSpy>
#include <QTemporaryDir>
#include <QTest>

#include <algorithm>

namespace {

const QString fixtures = QStringLiteral(TUR_CENTRE_FIXTURES);
const QString helper = QStringLiteral(TUR_CENTRE_HELPER);

ProcessSource::Settings helperSettings()
{
    ProcessSource::Settings settings;
    settings.program = helper;
    settings.timeoutMs = 20000;
    return settings;
}

struct Outcome
{
    bool answered = false;
    quint64 id = 0;
    quint64 answeredId = 0;
    CatalogueReply reply;
};

// Asks one question and waits for its answer.
Outcome ask(CatalogueSource &source, CatalogueRequest::Command command, const QString &subject, int patience = 30000)
{
    QSignalSpy spy(&source, &CatalogueSource::finished);
    Outcome out;
    CatalogueRequest request;
    request.command = command;
    request.subject = subject;
    out.id = source.submit(request);
    if (spy.isEmpty())
        spy.wait(patience);
    out.answered = !spy.isEmpty();
    if (out.answered) {
        out.answeredId = spy.at(0).at(0).toULongLong();
        out.reply = spy.at(0).at(1).value<CatalogueReply>();
    }
    return out;
}

QStringList echoedArguments(const CatalogueReply &reply)
{
    QStringList out;
    const QJsonArray arguments =
        QJsonDocument::fromJson(reply.output).object().value(QLatin1String("arguments")).toArray();
    for (const QJsonValue &argument : arguments)
        out.append(argument.toString());
    return out;
}

void write(const QString &path, const QByteArray &content)
{
    QFile file(path);
    if (!file.open(QIODevice::WriteOnly) || file.write(content) != content.size())
        qFatal("cannot write %s", qPrintable(path));
}

} // namespace

class TestSources : public QObject
{
    Q_OBJECT

private Q_SLOTS:
    void initTestCase();

    void the_arguments_are_a_vector_with_the_subject_after_two_dashes();
    void the_arguments_are_those_the_fixtures_were_recorded_with();
    void the_program_is_run_with_those_arguments_and_no_shell();
    void asking_returns_at_once_and_the_answer_comes_later();
    void the_exit_status_and_the_output_are_handed_on_as_they_are();
    void a_program_that_is_missing_is_reported();
    void a_program_that_does_not_answer_is_stopped();
    void a_program_that_prints_too_much_is_stopped();
    void a_program_that_crashes_is_reported();
    void standard_error_is_kept_up_to_a_limit();
    void the_program_is_given_no_input();
    void no_more_run_at_once_than_allowed();
    void a_question_that_was_cancelled_is_not_answered();
    void a_subject_that_cannot_be_passed_is_not_asked();
    void a_command_line_is_readable();

    void a_fixture_answers_what_was_recorded();
    void a_fixture_answers_later_too();
    void a_fixture_does_not_guess();
    void a_fixture_directory_that_is_missing_or_broken_says_so();
};

void TestSources::initTestCase()
{
    QVERIFY2(QFile::exists(helper), qPrintable(helper));
}

void TestSources::the_arguments_are_a_vector_with_the_subject_after_two_dashes()
{
    ProcessSource::Settings settings;
    CatalogueRequest request;
    request.command = CatalogueRequest::Search;
    request.subject = QStringLiteral("ms office");
    // One argument, spaces and all: nothing splits it.
    QCOMPARE(ProcessSource::arguments(request, settings),
             (QStringList { QStringLiteral("search"), QStringLiteral("--json"), QStringLiteral("--"),
                            QStringLiteral("ms office") }));

    request.command = CatalogueRequest::Show;
    request.subject = QStringLiteral("org.kde.krita");
    QCOMPARE(ProcessSource::arguments(request, settings),
             (QStringList { QStringLiteral("show"), QStringLiteral("--json"), QStringLiteral("--"),
                            QStringLiteral("org.kde.krita") }));

    settings.catalogueDirectory = QStringLiteral("/usr/share/x/catalogue");
    settings.hostFacts = QStringLiteral("/run/x/host.json");
    settings.preferences = QStringLiteral("/home/u/prefs.json");
    // The host facts and the preferences are the router's inputs only.
    QCOMPARE(ProcessSource::arguments(request, settings),
             (QStringList { QStringLiteral("--catalogue"), QStringLiteral("/usr/share/x/catalogue"),
                            QStringLiteral("show"), QStringLiteral("--json"), QStringLiteral("--"),
                            QStringLiteral("org.kde.krita") }));
    request.command = CatalogueRequest::Route;
    QCOMPARE(ProcessSource::arguments(request, settings),
             (QStringList { QStringLiteral("--catalogue"), QStringLiteral("/usr/share/x/catalogue"),
                            QStringLiteral("route"), QStringLiteral("--json"), QStringLiteral("--host"),
                            QStringLiteral("/run/x/host.json"), QStringLiteral("--prefs"),
                            QStringLiteral("/home/u/prefs.json"), QStringLiteral("--"),
                            QStringLiteral("org.kde.krita") }));

    // Whatever the subject begins with, it comes last, after "--".
    request.subject = QStringLiteral("--catalogue=/etc");
    const QStringList arguments = ProcessSource::arguments(request, settings);
    QCOMPARE(arguments.last(), QStringLiteral("--catalogue=/etc"));
    QCOMPARE(arguments.at(arguments.size() - 2), QStringLiteral("--"));
}

void TestSources::the_arguments_are_those_the_fixtures_were_recorded_with()
{
    // The Rust test that writes the fixtures records the arguments it gave
    // tur-catalogue. ProcessSource must give the same, or the program would
    // be tested against answers to questions it does not ask.
    int checked = 0;
    for (const QString &set : { QStringLiteral("default"), QStringLiteral("assumed-host") }) {
        const FixtureSource source(fixtures + QLatin1Char('/') + set);
        QVERIFY2(source.problem().isEmpty(), qPrintable(source.problem()));
        const QList<FixtureSource::Recorded> all = source.invocations();
        for (const FixtureSource::Recorded &one : all) {
            ProcessSource::Settings settings;
            if (one.host == QLatin1String("stated"))
                settings.hostFacts = QStringLiteral("<host-facts>");
            QCOMPARE(ProcessSource::arguments(one.request, settings), one.arguments);
            ++checked;
        }
    }
    QVERIFY(checked >= 20);
}

void TestSources::the_program_is_run_with_those_arguments_and_no_shell()
{
    ProcessSource::Settings settings = helperSettings();
    settings.catalogueDirectory = QStringLiteral("/some/where");
    ProcessSource source(settings);

    // Everything a shell would act on arrives as one argument, untouched.
    const QString subject = QStringLiteral("$(echo injected) ; echo x | cat > /tmp/y `id` \"quoted\" 'single' *");
    const Outcome outcome = ask(source, CatalogueRequest::Search, subject);
    QVERIFY(outcome.answered);
    QCOMPARE(outcome.answeredId, outcome.id);
    QCOMPARE(outcome.reply.failure, CatalogueReply::None);
    QCOMPARE(outcome.reply.exitStatus, 0);
    QCOMPARE(echoedArguments(outcome.reply),
             (QStringList { QStringLiteral("--catalogue"), QStringLiteral("/some/where"), QStringLiteral("search"),
                            QStringLiteral("--json"), QStringLiteral("--"), subject }));

    // And something that looks like an option is a value.
    const Outcome option = ask(source, CatalogueRequest::Route, QStringLiteral("--help"));
    QVERIFY(option.answered);
    const QStringList echoed = echoedArguments(option.reply);
    QCOMPARE(echoed.last(), QStringLiteral("--help"));
    QCOMPARE(echoed.at(echoed.size() - 2), QStringLiteral("--"));
}

void TestSources::asking_returns_at_once_and_the_answer_comes_later()
{
    ProcessSource source(helperSettings());
    QSignalSpy spy(&source, &CatalogueSource::finished);
    CatalogueRequest request;
    request.subject = QStringLiteral("sleep");

    QElapsedTimer timer;
    timer.start();
    const quint64 first = source.submit(request);
    request.subject = QStringLiteral("office");
    const quint64 second = source.submit(request);
    // submit() neither waits for the program nor answers from inside itself
    // (specification 30.1: nothing blocks the thread that draws).
    QVERIFY2(timer.elapsed() < 1000, "submit() returned at once");
    QCOMPARE(spy.count(), 0);
    QVERIFY(first != 0);
    QVERIFY(second != first);

    // The quick one answers while the slow one is still running.
    QVERIFY(spy.wait(30000));
    QCOMPARE(spy.at(0).at(0).toULongLong(), second);
    QCOMPARE(source.running(), 1);
    source.cancel(first);
    QCOMPARE(source.running(), 0);
}

void TestSources::the_exit_status_and_the_output_are_handed_on_as_they_are()
{
    ProcessSource source(helperSettings());
    // "noisy" ends with status 6, like a search that finds nothing.
    const Outcome outcome = ask(source, CatalogueRequest::Search, QStringLiteral("noisy"));
    QVERIFY(outcome.answered);
    QCOMPARE(outcome.reply.failure, CatalogueReply::None);
    QCOMPARE(outcome.reply.exitStatus, 6);
    QCOMPARE(outcome.reply.output.trimmed(), QByteArray("{\"query\": \"noisy\", \"hits\": []}"));
    QVERIFY(outcome.reply.commandLine.endsWith(QStringLiteral(" search --json -- noisy")));

    const Outcome silent = ask(source, CatalogueRequest::Search, QStringLiteral("silent"));
    QVERIFY(silent.answered);
    QCOMPARE(silent.reply.failure, CatalogueReply::None);
    QCOMPARE(silent.reply.exitStatus, 1);
    QVERIFY(silent.reply.output.isEmpty());

    const Outcome refused = ask(source, CatalogueRequest::Search, QStringLiteral("refused"));
    QVERIFY(refused.answered);
    QCOMPARE(refused.reply.exitStatus, 1);
    QVERIFY(refused.reply.errorOutput.startsWith("error[TUR-DESKTOP-CATALOGUE-001]: "));
}

void TestSources::a_program_that_is_missing_is_reported()
{
    ProcessSource::Settings settings;
    settings.program = QStringLiteral("/nonexistent/tur-catalogue");
    ProcessSource source(settings);
    const Outcome outcome = ask(source, CatalogueRequest::Search, QStringLiteral("office"));
    QVERIFY(outcome.answered);
    QCOMPARE(outcome.reply.failure, CatalogueReply::NotStarted);
    QVERIFY(!outcome.reply.detail.isEmpty());
    QCOMPARE(outcome.reply.commandLine, QStringLiteral("/nonexistent/tur-catalogue search --json -- office"));
    QCOMPARE(source.running(), 0);

    // A name that is not on the PATH is the same failure.
    settings.program = QStringLiteral("tur-catalogue-that-is-not-installed");
    ProcessSource named(settings);
    const Outcome second = ask(named, CatalogueRequest::Route, QStringLiteral("org.kde.krita"));
    QVERIFY(second.answered);
    QCOMPARE(second.reply.failure, CatalogueReply::NotStarted);
}

void TestSources::a_program_that_does_not_answer_is_stopped()
{
    ProcessSource::Settings settings = helperSettings();
    settings.timeoutMs = 400;
    ProcessSource source(settings);

    QElapsedTimer timer;
    timer.start();
    const Outcome outcome = ask(source, CatalogueRequest::Route, QStringLiteral("sleep"));
    QVERIFY(outcome.answered);
    QCOMPARE(outcome.reply.failure, CatalogueReply::TimedOut);
    QVERIFY(outcome.reply.detail.contains(QStringLiteral("400 ms")));
    // After the limit, and long before the program would have ended.
    QVERIFY2(timer.elapsed() >= 350, "not before the time limit");
    QVERIFY2(timer.elapsed() < 20000, "the program was not waited for");
    QCOMPARE(source.running(), 0);

    // The source goes on working.
    const Outcome next = ask(source, CatalogueRequest::Search, QStringLiteral("office"));
    QVERIFY(next.answered);
    QCOMPARE(next.reply.failure, CatalogueReply::None);
}

void TestSources::a_program_that_prints_too_much_is_stopped()
{
    ProcessSource::Settings settings = helperSettings();
    settings.maximumOutputBytes = 256 * 1024;
    ProcessSource source(settings);

    QElapsedTimer timer;
    timer.start();
    const Outcome outcome = ask(source, CatalogueRequest::Search, QStringLiteral("flood"));
    QVERIFY(outcome.answered);
    QCOMPARE(outcome.reply.failure, CatalogueReply::TooLarge);
    // None of it is handed on.
    QVERIFY(outcome.reply.output.isEmpty());
    QVERIFY2(timer.elapsed() < 15000, "stopped as soon as the limit was passed, not at the time limit");
    QCOMPARE(source.running(), 0);

    // Just under the limit is an answer.
    settings.maximumOutputBytes = 4 * 1024 * 1024;
    ProcessSource generous(settings);
    const Outcome small = ask(generous, CatalogueRequest::Search, QStringLiteral("office"));
    QVERIFY(small.answered);
    QCOMPARE(small.reply.failure, CatalogueReply::None);
}

void TestSources::a_program_that_crashes_is_reported()
{
    ProcessSource source(helperSettings());
    const Outcome outcome = ask(source, CatalogueRequest::Show, QStringLiteral("crash"));
    QVERIFY(outcome.answered);
    QCOMPARE(outcome.reply.failure, CatalogueReply::Crashed);
    QVERIFY(outcome.reply.output.isEmpty());
    QCOMPARE(source.running(), 0);
}

void TestSources::standard_error_is_kept_up_to_a_limit()
{
    ProcessSource source(helperSettings());
    // 256 KiB on standard error: read to the end, kept up to the limit.
    const Outcome outcome = ask(source, CatalogueRequest::Search, QStringLiteral("noisy"));
    QVERIFY(outcome.answered);
    QCOMPARE(outcome.reply.failure, CatalogueReply::None);
    QCOMPARE(outcome.reply.errorOutput.size(), ProcessSource::MaximumErrorBytes);
}

void TestSources::the_program_is_given_no_input()
{
    ProcessSource source(helperSettings());
    // The helper reads its standard input to the end. It ends at once,
    // because there is nothing there and nothing ever will be.
    const Outcome outcome = ask(source, CatalogueRequest::Search, QStringLiteral("input"));
    QVERIFY(outcome.answered);
    QCOMPARE(outcome.reply.failure, CatalogueReply::None);
    QCOMPARE(outcome.reply.output.trimmed(), QByteArray("{\"input_bytes\": 0}"));
}

void TestSources::no_more_run_at_once_than_allowed()
{
    ProcessSource::Settings settings = helperSettings();
    settings.concurrency = 2;
    settings.timeoutMs = 300;
    ProcessSource source(settings);

    int answers = 0;
    int most = 0;
    connect(&source, &CatalogueSource::finished, this, [&](quint64, const CatalogueReply &reply) {
        QCOMPARE(reply.failure, CatalogueReply::TimedOut);
        ++answers;
    });

    CatalogueRequest request;
    request.subject = QStringLiteral("sleep");
    for (int i = 0; i < 5; ++i)
        source.submit(request);
    QCOMPARE(source.queued(), 5);

    QElapsedTimer timer;
    timer.start();
    while (answers < 5 && timer.elapsed() < 30000) {
        most = std::max(most, source.running());
        QVERIFY(source.running() <= 2);
        QTest::qWait(20);
    }
    QCOMPARE(answers, 5);
    QCOMPARE(most, 2);
    // Three rounds of the time limit: two, two, one.
    QVERIFY2(timer.elapsed() >= 800, "the questions waited their turn");
    QCOMPARE(source.running(), 0);
    QCOMPARE(source.queued(), 0);
}

void TestSources::a_question_that_was_cancelled_is_not_answered()
{
    ProcessSource::Settings settings = helperSettings();
    settings.concurrency = 1;
    ProcessSource source(settings);
    QSignalSpy spy(&source, &CatalogueSource::finished);

    CatalogueRequest request;
    request.subject = QStringLiteral("sleep");
    const quint64 running = source.submit(request);
    const quint64 waiting = source.submit(request);
    QTRY_COMPARE(source.running(), 1);
    QCOMPARE(source.queued(), 1);

    // One that waits its turn, and one whose program is running.
    source.cancel(waiting);
    QCOMPARE(source.queued(), 0);
    source.cancel(running);
    QCOMPARE(source.running(), 0);
    QTest::qWait(300);
    QCOMPARE(spy.count(), 0);

    // Cancelling what is not there is harmless.
    source.cancel(running);
    source.cancel(12345);
}

void TestSources::a_subject_that_cannot_be_passed_is_not_asked()
{
    QVERIFY(CatalogueSource::isUsableSubject(QStringLiteral("office")));
    QVERIFY(CatalogueSource::isUsableSubject(QStringLiteral("Dvořák's café 東京 🙂")));
    QVERIFY(CatalogueSource::isUsableSubject(QString(CatalogueSource::MaximumSubjectLength, QLatin1Char('a'))));
    QVERIFY(!CatalogueSource::isUsableSubject(QString()));
    QVERIFY(!CatalogueSource::isUsableSubject(QString(CatalogueSource::MaximumSubjectLength + 1, QLatin1Char('a'))));
    QVERIFY(!CatalogueSource::isUsableSubject(QStringLiteral("two\nlines")));
    QVERIFY(!CatalogueSource::isUsableSubject(QStringLiteral("a\tb")));
    QVERIFY(!CatalogueSource::isUsableSubject(QString(1, QChar(0))));

    // No program is started for one: the program here does not exist, and
    // the failure is not "could not be started".
    ProcessSource::Settings settings;
    settings.program = QStringLiteral("/nonexistent/tur-catalogue");
    ProcessSource source(settings);
    const Outcome outcome = ask(source, CatalogueRequest::Route, QStringLiteral("two\nlines"));
    QVERIFY(outcome.answered);
    QCOMPARE(outcome.reply.failure, CatalogueReply::Refused);

    const FixtureSource fixture(fixtures + QStringLiteral("/default"));
    FixtureSource recorded(fixtures + QStringLiteral("/default"));
    const Outcome second = ask(recorded, CatalogueRequest::Search, QString(500, QLatin1Char('x')));
    QVERIFY(second.answered);
    QCOMPARE(second.reply.failure, CatalogueReply::Refused);
    QVERIFY(fixture.problem().isEmpty());
}

void TestSources::a_command_line_is_readable()
{
    QCOMPARE(ProcessSource::commandLine(QStringLiteral("tur-catalogue"),
                                        { QStringLiteral("route"), QStringLiteral("--json"), QStringLiteral("--"),
                                          QStringLiteral("org.kde.krita") }),
             QStringLiteral("tur-catalogue route --json -- org.kde.krita"));
    QCOMPARE(ProcessSource::commandLine(QStringLiteral("tur-catalogue"),
                                        { QStringLiteral("search"), QStringLiteral("--"),
                                          QStringLiteral("ms office") }),
             QStringLiteral("tur-catalogue search -- 'ms office'"));
    QCOMPARE(ProcessSource::commandLine(QStringLiteral("x"), { QStringLiteral("it's") }),
             QStringLiteral("x 'it'\\''s'"));
}

void TestSources::a_fixture_answers_what_was_recorded()
{
    FixtureSource source(fixtures + QStringLiteral("/default"));
    QVERIFY2(source.problem().isEmpty(), qPrintable(source.problem()));
    QVERIFY(source.invocations().size() >= 20);

    const Outcome search = ask(source, CatalogueRequest::Search, QStringLiteral("office"));
    QVERIFY(search.answered);
    QCOMPARE(search.answeredId, search.id);
    QCOMPARE(search.reply.failure, CatalogueReply::None);
    QCOMPARE(search.reply.exitStatus, 0);
    const QJsonObject found = QJsonDocument::fromJson(search.reply.output).object();
    QCOMPARE(found.value(QLatin1String("query")).toString(), QStringLiteral("office"));
    QCOMPARE(found.value(QLatin1String("hits")).toArray().size(), 2);
    QCOMPARE(search.reply.commandLine, QStringLiteral("tur-catalogue search --json -- office"));

    // The exit status is the recorded one: 4, "needs something first".
    const Outcome route = ask(source, CatalogueRequest::Route, QStringLiteral("com.microsoft.office"));
    QVERIFY(route.answered);
    QCOMPARE(route.reply.exitStatus, 4);
    QCOMPARE(QJsonDocument::fromJson(route.reply.output).object().value(QLatin1String("recommended")).toString(),
             QStringLiteral("windows-guest"));

    // The same identifier under another command is another answer.
    const Outcome show = ask(source, CatalogueRequest::Show, QStringLiteral("com.microsoft.office"));
    QVERIFY(show.answered);
    QCOMPARE(show.reply.exitStatus, 0);
    QCOMPARE(QJsonDocument::fromJson(show.reply.output).object().value(QLatin1String("name")).toString(),
             QStringLiteral("Microsoft 365"));

    // The other machine: nothing was said about it, and the router says so.
    FixtureSource assumed(fixtures + QStringLiteral("/assumed-host"));
    const Outcome netflix = ask(assumed, CatalogueRequest::Route, QStringLiteral("com.netflix.netflix"));
    QVERIFY(netflix.answered);
    QCOMPARE(QJsonDocument::fromJson(netflix.reply.output).object().value(QLatin1String("host_assumed")).toBool(),
             true);
}

void TestSources::a_fixture_answers_later_too()
{
    FixtureSource source(fixtures + QStringLiteral("/default"));
    QSignalSpy spy(&source, &CatalogueSource::finished);
    CatalogueRequest request;
    request.subject = QStringLiteral("office");
    const quint64 kept = source.submit(request);
    const quint64 dropped = source.submit(request);
    // Not from inside submit(): the same path as a real program.
    QCOMPARE(spy.count(), 0);
    source.cancel(dropped);
    QVERIFY(spy.wait(5000));
    QTest::qWait(50);
    QCOMPARE(spy.count(), 1);
    QCOMPARE(spy.at(0).at(0).toULongLong(), kept);
}

void TestSources::a_fixture_does_not_guess()
{
    FixtureSource source(fixtures + QStringLiteral("/default"));

    // "offic" was not recorded. It is not answered from "office", and not
    // with "nothing found".
    const Outcome near = ask(source, CatalogueRequest::Search, QStringLiteral("offic"));
    QVERIFY(near.answered);
    QCOMPARE(near.reply.failure, CatalogueReply::NotRecorded);
    QVERIFY(near.reply.output.isEmpty());
    QVERIFY(!near.reply.detail.isEmpty());

    const Outcome upper = ask(source, CatalogueRequest::Search, QStringLiteral("Office"));
    QVERIFY(upper.answered);
    QCOMPARE(upper.reply.failure, CatalogueReply::NotRecorded);

    const Outcome unknown = ask(source, CatalogueRequest::Route, QStringLiteral("org.mozilla.firefox"));
    QVERIFY(unknown.answered);
    QCOMPARE(unknown.reply.failure, CatalogueReply::NotRecorded);
}

void TestSources::a_fixture_directory_that_is_missing_or_broken_says_so()
{
    FixtureSource missing(QStringLiteral("/nonexistent/fixtures"));
    QVERIFY(missing.problem().contains(QStringLiteral("does not exist")));
    const Outcome outcome = ask(missing, CatalogueRequest::Search, QStringLiteral("office"));
    QVERIFY(outcome.answered);
    QCOMPARE(outcome.reply.failure, CatalogueReply::NotRecorded);
    QCOMPARE(outcome.reply.detail, missing.problem());

    QTemporaryDir empty;
    QVERIFY(empty.isValid());
    const FixtureSource nothing(empty.path());
    QVERIFY(nothing.problem().contains(QStringLiteral("holds no .json file")));

    const QByteArray good = R"({"invocations": [{"command": "search", "subject": "a", "exit_status": 0,
        "stderr": "", "stdout": {"query": "a", "hits": []}}]})";

    const auto problemOf = [&good](const QByteArray &second) {
        QTemporaryDir directory;
        write(directory.filePath(QStringLiteral("a.json")), good);
        write(directory.filePath(QStringLiteral("b.json")), second);
        const FixtureSource source(directory.path());
        // Nothing of a directory with a bad file is used.
        if (!source.problem().isEmpty() && !source.invocations().isEmpty())
            return QStringLiteral("a bad directory still answers");
        return source.problem();
    };

    QVERIFY(problemOf("{").contains(QStringLiteral("b.json")));
    QVERIFY(problemOf("{}").contains(QStringLiteral("invocations")));
    QVERIFY(problemOf(R"({"invocations": [{"command": "install", "subject": "a", "exit_status": 0, "stdout": null}]})")
                .contains(QStringLiteral("install")));
    QVERIFY(problemOf(R"({"invocations": [{"command": "show", "exit_status": 0, "stdout": null}]})")
                .contains(QStringLiteral("lacks")));
    QVERIFY(problemOf(R"({"invocations": [{"command": "show", "subject": "a", "exit_status": 0, "stdout": "text"}]})")
                .contains(QStringLiteral("lacks")));
    // The same question twice: which answer would be given?
    QVERIFY(problemOf(good).contains(QStringLiteral("recorded twice")));
    // A second file that is in order.
    QVERIFY(problemOf(R"({"invocations": [{"command": "show", "subject": "a", "exit_status": 1, "stderr": "e",
        "stdout": null}]})")
                .isEmpty());
}

QTEST_GUILESS_MAIN(TestSources)

#include "tst_sources.moc"
