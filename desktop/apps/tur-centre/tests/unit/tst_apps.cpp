// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
//
// How the application centre runs tur-apps: the argument vectors it uses
// (held against the recorded runs of the real tur-apps), what AppsProcess
// does with a program that streams, waits to be cancelled, sleeps, floods,
// crashes or is missing, and how recorded runs are played back.

#include "apps.h"
#include "appsfixture.h"
#include "appsprocess.h"

#include <QDir>
#include <QFile>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QSignalSpy>
#include <QTest>

#include <functional>

namespace {

const QString fixtures = QStringLiteral(TUR_CENTRE_FIXTURES);
const QString helper = QStringLiteral(TUR_CENTRE_APPS_HELPER);

AppsRequest request(AppsRequest::Command command, const QString &subject = QString())
{
    AppsRequest out;
    out.command = command;
    out.subject = subject;
    return out;
}

struct Run
{
    bool answered = false;
    QList<QByteArray> lines;
    AppsReply reply;
};

Run run(AppsSource &source, const AppsRequest &asked, int patience = 30000,
        const std::function<void(quint64, const QByteArray &)> &onLine = nullptr)
{
    Run out;
    QSignalSpy finished(&source, &AppsSource::finished);
    const quint64 id = source.submit(asked);
    QObject::connect(&source, &AppsSource::line, &source, [&out, id, onLine](quint64 from, const QByteArray &line) {
        if (from != id)
            return;
        out.lines.append(line);
        if (onLine)
            onLine(from, line);
    });
    if (finished.isEmpty())
        finished.wait(patience);
    out.answered = !finished.isEmpty();
    if (out.answered)
        out.reply = finished.at(0).at(1).value<AppsReply>();
    source.disconnect(&source);
    return out;
}

AppsProcess::Settings helperSettings()
{
    AppsProcess::Settings settings;
    settings.program = helper;
    settings.timeoutMs = 3000;
    return settings;
}

} // namespace

class TestApps : public QObject
{
    Q_OBJECT

private Q_SLOTS:
    void the_arguments_are_tur_apps_command_line();
    void every_recorded_run_was_asked_with_the_centre_s_arguments();
    void the_global_options_come_first();
    void a_streaming_run_hands_on_each_line_as_it_comes();
    void an_installation_is_cancelled_by_writing_cancel();
    void a_run_that_is_not_an_installation_gets_no_input();
    void a_program_that_is_missing_is_reported();
    void a_list_that_does_not_answer_is_stopped();
    void a_program_that_prints_too_much_is_stopped();
    void a_program_that_crashes_is_reported();
    void a_subject_that_cannot_be_passed_is_not_asked();
    void failures_carry_codes_and_plain_words();
    void recorded_runs_are_played_back_in_order_and_hold_where_cancelled();
};

void TestApps::the_arguments_are_tur_apps_command_line()
{
    QCOMPARE(request(AppsRequest::Plan, QStringLiteral("org.example.app")).arguments(),
             QStringList({ "plan", "--json", "--", "org.example.app" }));
    AppsRequest withFile = request(AppsRequest::Install, QStringLiteral("org.example.app"));
    withFile.installerFile = QStringLiteral("/home/someone/My Downloads/setup.exe");
    QCOMPARE(withFile.arguments(), QStringList({ "install", "--yes", "--events", "--installer",
                                                 "/home/someone/My Downloads/setup.exe", "--", "org.example.app" }));
    QCOMPARE(request(AppsRequest::Update, QStringLiteral("x.y.z")).arguments(),
             QStringList({ "update", "--events", "--", "x.y.z" }));
    // Removal never relies on a default: the person's answer is always
    // passed.
    AppsRequest keep = request(AppsRequest::Remove, QStringLiteral("x.y.z"));
    QCOMPARE(keep.arguments(), QStringList({ "remove", "--keep-data", "--events", "--", "x.y.z" }));
    keep.deleteData = true;
    QCOMPARE(keep.arguments(), QStringList({ "remove", "--delete-data", "--events", "--", "x.y.z" }));
    QCOMPARE(request(AppsRequest::List).arguments(), QStringList({ "list", "--json" }));
    QCOMPARE(request(AppsRequest::ListWithUpdates).arguments(), QStringList({ "list", "--check-updates", "--json" }));
    QCOMPARE(request(AppsRequest::Sources).arguments(), QStringList({ "sources", "--json" }));
    QCOMPARE(request(AppsRequest::EnableSource, QStringLiteral("flathub")).arguments(),
             QStringList({ "sources", "enable", "--json", "--", "flathub" }));
    QCOMPARE(request(AppsRequest::DisableSource, QStringLiteral("fedora")).arguments(),
             QStringList({ "sources", "disable", "--json", "--", "fedora" }));
    // A subject that begins with dashes is still a value.
    QCOMPARE(request(AppsRequest::Plan, QStringLiteral("--yes")).arguments().last(), QStringLiteral("--yes"));
    QVERIFY(request(AppsRequest::Install).streams());
    QVERIFY(!request(AppsRequest::Plan).streams());
}

void TestApps::every_recorded_run_was_asked_with_the_centre_s_arguments()
{
    // desktop/ci/centre-e2e/record.py ran the real tur-apps with these
    // vectors; each must be one the centre makes, so that the centre is
    // tested against answers to the questions it really asks.
    const QDir root(fixtures + QStringLiteral("/apps"));
    const QStringList scenarios = root.entryList(QDir::Dirs | QDir::NoDotAndDotDot, QDir::Name);
    QVERIFY2(scenarios.size() >= 6, qPrintable(scenarios.join(QLatin1Char(' '))));
    int checked = 0;
    for (const QString &scenario : scenarios) {
        AppsFixture source(root.filePath(scenario));
        QVERIFY2(source.problem().isEmpty(), qPrintable(source.problem()));
        for (const AppsFixture::Recorded &one : source.invocations()) {
            const QStringList &recorded = one.arguments;
            QVERIFY(recorded.size() >= 2);
            const QString subject = recorded.contains(QStringLiteral("--")) ? recorded.last() : QString();
            QList<AppsRequest> candidates;
            for (const AppsRequest::Command command :
                 { AppsRequest::Plan, AppsRequest::Install, AppsRequest::Update, AppsRequest::Remove,
                   AppsRequest::List, AppsRequest::ListWithUpdates, AppsRequest::Sources,
                   AppsRequest::EnableSource, AppsRequest::DisableSource }) {
                AppsRequest asked = request(command, subject);
                candidates << asked;
                asked.deleteData = true;
                candidates << asked;
                asked.deleteData = false;
                asked.installerFile = QStringLiteral("/somewhere/installer.exe");
                candidates << asked;
            }
            bool found = false;
            for (const AppsRequest &candidate : std::as_const(candidates))
                found = found || AppsFixture::matches(recorded, candidate.arguments());
            QVERIFY2(found, qPrintable(scenario + QStringLiteral(": ") + recorded.join(QLatin1Char(' '))));
            ++checked;
        }
    }
    QVERIFY(checked >= 20);
}

void TestApps::the_global_options_come_first()
{
    AppsProcess::Settings settings;
    settings.catalogueDirectory = QStringLiteral("/usr/share/tur/catalogue");
    settings.hostFacts = QStringLiteral("/run/host.json");
    QCOMPARE(AppsProcess::arguments(request(AppsRequest::List), settings),
             QStringList({ "--catalogue", "/usr/share/tur/catalogue", "--host", "/run/host.json", "list", "--json" }));
}

void TestApps::a_streaming_run_hands_on_each_line_as_it_comes()
{
    AppsProcess source(helperSettings());
    const Run out = run(source, request(AppsRequest::Install, QStringLiteral("lines")));
    QVERIFY(out.answered);
    QCOMPARE(out.reply.failure, AppsReply::None);
    QCOMPARE(out.reply.exitStatus, 0);
    // The last line has no line end, and is handed on all the same.
    QCOMPARE(out.lines.size(), 4);
    QVERIFY(out.lines.first().contains("\"plan\""));
    QVERIFY(out.lines.last().contains("\"result\""));
    QVERIFY(out.reply.commandLine.endsWith(QStringLiteral("install --yes --events -- lines")));
}

void TestApps::an_installation_is_cancelled_by_writing_cancel()
{
    AppsProcess source(helperSettings());
    quint64 started = 0;
    const Run out = run(source, request(AppsRequest::Install, QStringLiteral("wait-cancel")), 30000,
                        [&source, &started](quint64 id, const QByteArray &line) {
                            if (line.contains("step-started")) {
                                started = id;
                                source.cancel(id);
                            }
                        });
    QVERIFY(out.answered);
    QVERIFY(started != 0);
    QCOMPARE(out.reply.exitStatus, 3);
    QVERIFY(out.lines.last().contains("TUR-DESKTOP-APPS-013"));
}

void TestApps::a_run_that_is_not_an_installation_gets_no_input()
{
    AppsProcess source(helperSettings());
    const Run out = run(source, request(AppsRequest::Plan, QStringLiteral("no-input")));
    QVERIFY(out.answered);
    QCOMPARE(QJsonDocument::fromJson(out.reply.output).object().value(QLatin1String("input")).toString(),
             QStringLiteral("none"));
}

void TestApps::a_program_that_is_missing_is_reported()
{
    AppsProcess::Settings settings;
    settings.program = QStringLiteral("/nonexistent/tur-apps");
    AppsProcess source(settings);
    const Run out = run(source, request(AppsRequest::List));
    QVERIFY(out.answered);
    QCOMPARE(out.reply.failure, AppsReply::NotStarted);
    const QVariantMap failure = Apps::failureOf(out.reply);
    QCOMPARE(failure.value(QStringLiteral("code")).toString(), Apps::NotStarted);
    QVERIFY(failure.value(QStringLiteral("detail")).toString().contains(QStringLiteral("/nonexistent/tur-apps")));
}

void TestApps::a_list_that_does_not_answer_is_stopped()
{
    AppsProcess source(helperSettings());
    AppsRequest asked = request(AppsRequest::Plan, QStringLiteral("sleep"));
    const Run out = run(source, asked);
    QVERIFY(out.answered);
    QCOMPARE(out.reply.failure, AppsReply::TimedOut);
    QCOMPARE(Apps::failureOf(out.reply).value(QStringLiteral("code")).toString(), Apps::TimedOut);
}

void TestApps::a_program_that_prints_too_much_is_stopped()
{
    AppsProcess::Settings settings = helperSettings();
    settings.maximumOutputBytes = 64 * 1024;
    AppsProcess source(settings);
    const Run out = run(source, request(AppsRequest::Install, QStringLiteral("flood")));
    QVERIFY(out.answered);
    QCOMPARE(out.reply.failure, AppsReply::TooLarge);
}

void TestApps::a_program_that_crashes_is_reported()
{
    AppsProcess source(helperSettings());
    const Run out = run(source, request(AppsRequest::Update, QStringLiteral("crash")));
    QVERIFY(out.answered);
    QCOMPARE(out.reply.failure, AppsReply::Crashed);
}

void TestApps::a_subject_that_cannot_be_passed_is_not_asked()
{
    AppsProcess source(helperSettings());
    for (const QString &subject : { QString(), QStringLiteral("a\nb"), QString(300, QLatin1Char('x')) }) {
        const Run out = run(source, request(AppsRequest::Plan, subject));
        QVERIFY(out.answered);
        QCOMPARE(out.reply.failure, AppsReply::Refused);
    }
}

void TestApps::failures_carry_codes_and_plain_words()
{
    AppsReply reply;
    reply.exitStatus = 3;
    reply.commandLine = QStringLiteral("tur-apps plan --json -- x.y.z");
    const QJsonObject error {
        { QStringLiteral("code"), QStringLiteral("TUR-DESKTOP-APPS-008") },
        { QStringLiteral("title"), QStringLiteral("whatever tur-apps says") },
        { QStringLiteral("message"), QStringLiteral("Flathub is not switched on for you.") },
    };
    const QVariantMap failure = Apps::failureOf(reply, error);
    QCOMPARE(failure.value(QStringLiteral("code")).toString(), QStringLiteral("TUR-DESKTOP-APPS-008"));
    // The title is the application centre's, for the code: translatable.
    QCOMPARE(failure.value(QStringLiteral("title")).toString(), QStringLiteral("The source is not switched on"));
    QCOMPARE(failure.value(QStringLiteral("reason")).toString(), QStringLiteral("Flathub is not switched on for you."));
    QVERIFY(failure.value(QStringLiteral("detail")).toString().contains(QStringLiteral("Exit status: 3")));
    QVERIFY(failure.value(QStringLiteral("retryable")).toBool());
    // A code this version does not know keeps tur-apps' own title.
    QCOMPARE(Apps::titleOf(QStringLiteral("TUR-DESKTOP-APPS-099"), QStringLiteral("Theirs")), QStringLiteral("Theirs"));
    // No route: trying again cannot help.
    QVERIFY(!Apps::failureOf(reply, QJsonObject { { QStringLiteral("code"), QStringLiteral("TUR-DESKTOP-APPS-003") } })
                 .value(QStringLiteral("retryable"))
                 .toBool());
}

void TestApps::recorded_runs_are_played_back_in_order_and_hold_where_cancelled()
{
    AppsFixture source(fixtures + QStringLiteral("/apps/install"));
    QVERIFY2(source.problem().isEmpty(), qPrintable(source.problem()));
    // The list before the installation, then the list after it, then the
    // last one again.
    const Run before = run(source, request(AppsRequest::List));
    const Run after = run(source, request(AppsRequest::List));
    const Run again = run(source, request(AppsRequest::List));
    const auto count = [](const Run &one) {
        return QJsonDocument::fromJson(one.reply.output).object().value(QLatin1String("applications")).toArray().size();
    };
    QCOMPARE(count(before), 0);
    QCOMPARE(count(after), 1);
    QCOMPARE(count(again), 1);
    // A request that was not recorded is not guessed at.
    const Run missing = run(source, request(AppsRequest::Plan, QStringLiteral("org.example.nothing")));
    QCOMPARE(missing.reply.failure, AppsReply::NotRecorded);

    // The cancelled run stops where the recorder cancelled it, and goes on
    // once the centre cancels.
    AppsFixture cancelled(fixtures + QStringLiteral("/apps/cancel"));
    int cancelAfter = -1;
    for (const AppsFixture::Recorded &one : cancelled.invocations()) {
        if (one.cancelAfter >= 0)
            cancelAfter = one.cancelAfter;
    }
    QVERIFY(cancelAfter > 0);
    QSignalSpy finished(&cancelled, &AppsSource::finished);
    QSignalSpy lines(&cancelled, &AppsSource::line);
    const quint64 id = cancelled.submit(request(AppsRequest::Install, QStringLiteral("net.sourceforge.chromium-bsu")));
    QTRY_COMPARE(lines.size(), cancelAfter);
    QTest::qWait(200);
    QCOMPARE(lines.size(), cancelAfter);
    QVERIFY(finished.isEmpty());
    cancelled.cancel(id);
    QTRY_VERIFY(!finished.isEmpty());
    QVERIFY(lines.last().at(1).toByteArray().contains("TUR-DESKTOP-APPS-013"));
}

QTEST_GUILESS_MAIN(TestApps)

#include "tst_apps.moc"
