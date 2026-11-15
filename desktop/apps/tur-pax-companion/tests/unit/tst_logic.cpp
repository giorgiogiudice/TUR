// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
//
// The rules of P@x's character on the bar (docs/architecture/
// tur-pax-companion.md, sections 3, 5 and 6), each as a test: the one state
// it shows; that it agrees with P@x's indicator; when it may speak first and
// when it must keep a notice as a mark; one sentence and two actions; quiet;
// suggestions off unless switched on, and limited; the person's choices
// kept; the healing service's notices, in its own words; and where the
// character and what opens above it go.

#include "agentsource.h"
#include "companion.h"
#include "companionsettings.h"
#include "healsource.h"
#include "placement.h"

#include <QDateTime>
#include <QFile>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QSignalSpy>
#include <QTemporaryDir>
#include <QTest>

namespace {

// Noon on 14 November 2026, local time.
double noon()
{
    return double(QDateTime(QDate(2026, 11, 14), QTime(12, 0)).toMSecsSinceEpoch());
}

constexpr double minute = 60.0 * 1000.0;

QVariantMap notice(const QString &id, const QString &kind = QStringLiteral("repair"))
{
    return { { QStringLiteral("id"), id },
             { QStringLiteral("kind"), kind },
             { QStringLiteral("sender"), QStringLiteral("tur") },
             { QStringLiteral("sentence"), QStringLiteral("Something happened that you should hear of.") },
             { QStringLiteral("actions"), QStringList { QStringLiteral("show"), QStringLiteral("dismiss") } } };
}

QJsonArray printedNotices()
{
    QFile file(QStringLiteral(TUR_COMPANION_FIXTURES "/heal/notices.json"));
    if (!file.open(QIODevice::ReadOnly))
        return {};
    return QJsonDocument::fromJson(file.readAll()).object().value(QLatin1String("printed")).toArray();
}

// A character on the bar with P@x on and nothing happening.
struct Bench {
    CompanionSettings settings;
    Companion companion;

    Bench()
    {
        companion.setSettings(&settings);
        companion.setClock(noon());
        companion.setBubbleSeconds(0);
        companion.setMoodSeconds(0);
        companion.setPaxOn(true);
        companion.setServiceState(QStringLiteral("idle"));
    }
};

} // namespace

class LogicTest : public QObject
{
    Q_OBJECT

private Q_SLOTS:
    // --- The one state -----------------------------------------------------

    void asleep_while_pax_is_off_and_absent_unless_the_person_keeps_it()
    {
        CompanionSettings settings;
        Companion companion;
        companion.setSettings(&settings);
        QCOMPARE(companion.state(), QStringLiteral("asleep"));
        // P@x off: by default the character is not on the bar at all.
        QVERIFY(!companion.shown());
        settings.setPresence(QStringLiteral("always"));
        QVERIFY(companion.shown());
        QCOMPARE(companion.state(), QStringLiteral("asleep"));
        // P@x on: on the bar, awake.
        settings.setPresence(QStringLiteral("with-pax"));
        companion.setPaxOn(true);
        QVERIFY(companion.shown());
        QCOMPARE(companion.state(), QStringLiteral("idle"));
        // Hidden by the person: not on the bar, whatever P@x does.
        settings.setHidden(true);
        QVERIFY(!companion.shown());
        settings.setHidden(false);
        QVERIFY(companion.shown());
    }

    void every_state_is_reached_and_named()
    {
        Bench b;
        QStringList seen;
        const auto see = [&] { seen.append(b.companion.state()); };
        see(); // idle
        b.companion.setServiceState(QStringLiteral("working"));
        see(); // thinking
        b.companion.setServiceState(QStringLiteral("paused"));
        see(); // paused
        b.companion.setServiceState(QStringLiteral("idle"));
        b.companion.setAskOpen(true);
        see(); // listening
        b.companion.setAskOpen(false);
        b.companion.setWaiting(1);
        see(); // waiting
        b.companion.setWaiting(0);
        b.companion.setOverlayActivity(QStringLiteral("observing"));
        see(); // observing
        b.companion.setOverlayActivity(QStringLiteral("acting"));
        see(); // acting
        b.companion.setOverlayActivity(QString());
        QCOMPARE(b.companion.offer(notice(QStringLiteral("a"))), QStringLiteral("shown"));
        see(); // telling
        b.companion.fold();
        see(); // noticing
        QVERIFY(b.companion.unfold());
        b.companion.dismiss();
        b.companion.flag(QStringLiteral("refused"));
        see(); // refused
        b.companion.flag(QStringLiteral("error"));
        see(); // error
        b.companion.flag(QString());
        b.companion.setPaxOn(false);
        see(); // asleep
        QStringList wanted = Companion::states();
        wanted.sort();
        seen.sort();
        QCOMPARE(seen, wanted);
        QCOMPARE(Companion::states().size(), 12);
    }

    // The character never contradicts P@x's indicator: while the indicator
    // says P@x controls, sees or waits, the character says the same,
    // whatever the service, the ask bar, a notice or a mood would make it.
    void it_says_what_the_indicator_says_data()
    {
        QTest::addColumn<QString>("indicator");
        QTest::addColumn<QString>("state");
        QTest::newRow("acting") << QStringLiteral("acting") << QStringLiteral("acting");
        QTest::newRow("observing") << QStringLiteral("observing") << QStringLiteral("observing");
        QTest::newRow("waiting") << QStringLiteral("waiting") << QStringLiteral("waiting");
    }

    void it_says_what_the_indicator_says()
    {
        QFETCH(QString, indicator);
        QFETCH(QString, state);
        const QStringList services { QStringLiteral("idle"), QStringLiteral("working"), QStringLiteral("paused") };
        const QStringList moods { QString(), QStringLiteral("refused") };
        for (const QString &service : services) {
            for (const QString &mood : moods) {
                for (const bool asking : { false, true }) {
                    Bench b;
                    b.companion.setServiceState(service);
                    b.companion.flag(mood);
                    b.companion.setAskOpen(asking);
                    b.companion.offer(notice(QStringLiteral("n")));
                    b.companion.setOverlayActivity(indicator);
                    QCOMPARE(b.companion.state(), state);
                }
            }
        }
        // And it shows "controlling" only while the indicator does.
        Bench b;
        for (const QString &service : services) {
            b.companion.setServiceState(service);
            QVERIFY(b.companion.state() != QLatin1String("acting"));
        }
        b.companion.setServiceState(QStringLiteral("idle"));
        b.companion.setOverlayActivity(QStringLiteral("working"));
        QCOMPARE(b.companion.state(), QStringLiteral("thinking"));
        b.companion.setOverlayActivity(QStringLiteral("acting"));
        QCOMPARE(b.companion.state(), QStringLiteral("acting"));
        b.companion.setOverlayActivity(QStringLiteral("idle"));
        QVERIFY(b.companion.state() != QLatin1String("acting"));
    }

    void on_but_unable_is_an_error_with_the_services_words()
    {
        Bench b;
        b.companion.setProblem({ { QStringLiteral("code"), QStringLiteral("TUR-AI-KEY-002") },
                                 { QStringLiteral("message"), QStringLiteral("There is no key.") } });
        QCOMPARE(b.companion.state(), QStringLiteral("error"));
        b.companion.setProblem({});
        QCOMPARE(b.companion.state(), QStringLiteral("idle"));
    }

    // --- Speaking first ------------------------------------------------------

    void it_speaks_once_about_what_is_switched_on()
    {
        Bench b;
        QSignalSpy spoke(&b.companion, &Companion::bubbleChanged);
        QCOMPARE(b.companion.offer(notice(QStringLiteral("heal:3"))), QStringLiteral("shown"));
        QCOMPARE(spoke.count(), 1);
        QCOMPARE(b.companion.bubble().value(QStringLiteral("id")).toString(), QStringLiteral("heal:3"));
        QCOMPARE(b.companion.state(), QStringLiteral("telling"));
        // Offered again while it is being said: nothing new.
        QCOMPARE(b.companion.offer(notice(QStringLiteral("heal:3"))), QStringLiteral("held"));
        QCOMPARE(b.companion.heldCount(), 0);
        b.companion.dismiss();
        QVERIFY(!b.companion.speaking());
        // Offered again after it was said: never twice.
        QCOMPARE(b.companion.offer(notice(QStringLiteral("heal:3"))), QStringLiteral("told"));
        QVERIFY(!b.companion.speaking());
        QCOMPARE(b.companion.state(), QStringLiteral("idle"));
    }

    void a_kind_switched_off_is_never_said()
    {
        Bench b;
        const QStringList kinds = CompanionSettings::kindNames();
        QCOMPARE(kinds.size(), 5);
        for (const QString &kind : kinds) {
            // Every kind is on unless the person switches it off.
            QVERIFY2(b.settings.kindOn(kind), qPrintable(kind));
            b.settings.setKind(kind, false);
            QCOMPARE(b.companion.offer(notice(QStringLiteral("off:") + kind, kind)), QStringLiteral("off"));
            QVERIFY(!b.companion.speaking());
            QCOMPARE(b.companion.heldCount(), 0);
            // Switched on again, the old notice does not come back.
            b.settings.setKind(kind, true);
            QCOMPARE(b.companion.offer(notice(QStringLiteral("off:") + kind, kind)), QStringLiteral("told"));
            QCOMPARE(b.companion.offer(notice(QStringLiteral("on:") + kind, kind)), QStringLiteral("shown"));
            b.companion.dismiss();
        }
        QCOMPARE(b.companion.offer(notice(QStringLiteral("x"), QStringLiteral("advert"))), QStringLiteral("invalid"));
    }

    void quiet_is_quiet_and_nothing_pops_up_afterwards()
    {
        Bench b;
        b.companion.setQuiet(QStringLiteral("hour"));
        QVERIFY(b.companion.quiet());
        QCOMPARE(b.companion.offer(notice(QStringLiteral("a"))), QStringLiteral("held"));
        QCOMPARE(b.companion.offer(notice(QStringLiteral("b"), QStringLiteral("fault"))), QStringLiteral("held"));
        QVERIFY(!b.companion.speaking());
        QCOMPARE(b.companion.heldCount(), 2);
        // A mark, not a bubble.
        QCOMPARE(b.companion.state(), QStringLiteral("noticing"));

        // An hour later it is no longer quiet, and still says nothing by
        // itself: what waited is said when the person asks.
        QSignalSpy spoke(&b.companion, &Companion::bubbleChanged);
        b.companion.setClock(noon() + 61 * minute);
        QVERIFY(!b.companion.quiet());
        QCOMPARE(spoke.count(), 0);
        QVERIFY(!b.companion.speaking());
        QCOMPARE(b.companion.heldCount(), 2);
        QVERIFY(b.companion.unfold());
        QCOMPARE(b.companion.bubble().value(QStringLiteral("id")).toString(), QStringLiteral("a"));
        b.companion.dismiss();
        // The next one does not follow by itself either.
        QVERIFY(!b.companion.speaking());
        QCOMPARE(b.companion.heldCount(), 1);
        QVERIFY(b.companion.unfold());
        QCOMPARE(b.companion.bubble().value(QStringLiteral("id")).toString(), QStringLiteral("b"));
        b.companion.dismiss();
        QVERIFY(!b.companion.unfold());
        QCOMPARE(b.companion.state(), QStringLiteral("idle"));
    }

    void going_quiet_takes_back_what_was_being_said()
    {
        Bench b;
        QCOMPARE(b.companion.offer(notice(QStringLiteral("a"))), QStringLiteral("shown"));
        b.companion.setQuiet(QStringLiteral("always"));
        QVERIFY(b.companion.quiet());
        QVERIFY(!b.companion.speaking());
        QCOMPARE(b.companion.heldCount(), 1);
        b.companion.setQuiet(QStringLiteral("off"));
        QVERIFY(!b.companion.quiet());
        QVERIFY(!b.companion.speaking());
    }

    void quiet_for_an_hour_until_tomorrow_and_until_i_say()
    {
        Bench b;
        b.companion.setQuiet(QStringLiteral("hour"));
        QCOMPARE(b.settings.quietUntil(), noon() + 60 * minute);
        QVERIFY(b.settings.quietAt(noon() + 59 * minute));
        QVERIFY(!b.settings.quietAt(noon() + 61 * minute));

        b.companion.setQuiet(QStringLiteral("tomorrow"));
        const QDateTime until = QDateTime::fromMSecsSinceEpoch(qint64(b.settings.quietUntil()));
        QCOMPARE(until.date(), QDate(2026, 11, 15));
        QCOMPARE(until.time(), QTime(7, 0));
        QVERIFY(b.companion.quiet());

        b.companion.setQuiet(QStringLiteral("always"));
        QCOMPARE(b.settings.quietUntil(), -1.0);
        b.companion.setClock(noon() + 400 * 24 * 60 * minute);
        QVERIFY(b.companion.quiet());

        b.companion.setQuiet(QStringLiteral("off"));
        QVERIFY(!b.companion.quiet());
        QVERIFY(!b.settings.setQuiet(QStringLiteral("forever and ever"), noon()));
    }

    void it_does_not_speak_over_itself_the_ask_bar_or_its_menu()
    {
        Bench b;
        QCOMPARE(b.companion.offer(notice(QStringLiteral("a"))), QStringLiteral("shown"));
        QCOMPARE(b.companion.offer(notice(QStringLiteral("b"))), QStringLiteral("held"));
        QCOMPARE(b.companion.bubble().value(QStringLiteral("id")).toString(), QStringLiteral("a"));
        b.companion.dismiss();
        // Answering one does not bring the next.
        QVERIFY(!b.companion.speaking());

        // The ask bar opens: what was being said makes room.
        QVERIFY(b.companion.unfold());
        b.companion.setAskOpen(true);
        QVERIFY(!b.companion.speaking());
        QCOMPARE(b.companion.state(), QStringLiteral("listening"));
        QCOMPARE(b.companion.offer(notice(QStringLiteral("c"))), QStringLiteral("held"));
        b.companion.setAskOpen(false);
        QVERIFY(!b.companion.speaking());

        b.companion.setAttending(true);
        QCOMPARE(b.companion.offer(notice(QStringLiteral("d"))), QStringLiteral("held"));
        b.companion.setAttending(false);
        QCOMPARE(b.companion.heldCount(), 3);
    }

    void it_speaks_only_so_often_in_an_hour()
    {
        Bench b;
        b.settings.setSpeakPerHour(2);
        QCOMPARE(b.companion.offer(notice(QStringLiteral("a"))), QStringLiteral("shown"));
        b.companion.dismiss();
        b.companion.setClock(noon() + 10 * minute);
        QCOMPARE(b.companion.offer(notice(QStringLiteral("b"))), QStringLiteral("shown"));
        b.companion.dismiss();
        b.companion.setClock(noon() + 20 * minute);
        // The third within the hour waits as a mark.
        QCOMPARE(b.companion.offer(notice(QStringLiteral("c"))), QStringLiteral("held"));
        QVERIFY(!b.companion.speaking());
        QCOMPARE(b.companion.state(), QStringLiteral("noticing"));
        // An hour after the first, it may speak again.
        b.companion.setClock(noon() + 61 * minute);
        QCOMPARE(b.companion.offer(notice(QStringLiteral("d"))), QStringLiteral("shown"));
        // What the person asks for is not counted.
        b.companion.dismiss();
        QVERIFY(b.companion.unfold());
        QCOMPARE(b.companion.bubble().value(QStringLiteral("id")).toString(), QStringLiteral("c"));
    }

    void off_the_bar_or_asleep_it_consumes_nothing()
    {
        Bench b;
        b.settings.setHidden(true);
        QCOMPARE(b.companion.offer(notice(QStringLiteral("a"))), QStringLiteral("hidden"));
        b.settings.setHidden(false);
        b.companion.setPaxOn(false);
        b.settings.setPresence(QStringLiteral("always"));
        QVERIFY(b.companion.shown());
        QCOMPARE(b.companion.offer(notice(QStringLiteral("a"))), QStringLiteral("hidden"));
        // Back and awake, the notice is still new.
        b.companion.setPaxOn(true);
        QCOMPARE(b.companion.offer(notice(QStringLiteral("a"))), QStringLiteral("shown"));
        // Hidden while speaking: the bubble goes with it and waits.
        b.settings.setHidden(true);
        QVERIFY(!b.companion.speaking());
        QCOMPARE(b.companion.heldCount(), 1);
    }

    void one_sentence_and_at_most_two_actions()
    {
        Bench b;
        QVariantMap wordy = notice(QStringLiteral("a"));
        wordy.insert(QStringLiteral("actions"),
                     QStringList { QStringLiteral("show"), QStringLiteral("dismiss"), QStringLiteral("ask"),
                                   QStringLiteral("more") });
        QCOMPARE(b.companion.offer(wordy), QStringLiteral("shown"));
        QCOMPARE(b.companion.bubble().value(QStringLiteral("actions")).toStringList(),
                 (QStringList { QStringLiteral("show"), QStringLiteral("dismiss") }));
        b.companion.dismiss();

        QVariantMap nameless = notice(QString());
        QCOMPARE(b.companion.offer(nameless), QStringLiteral("invalid"));
        QVariantMap silent = notice(QStringLiteral("b"));
        silent.insert(QStringLiteral("sentence"), QStringLiteral("   "));
        QCOMPARE(b.companion.offer(silent), QStringLiteral("invalid"));
        QVariantMap endless = notice(QStringLiteral("c"));
        endless.insert(QStringLiteral("sentence"), QString(Companion::sentenceLimit() + 1, QLatin1Char('x')));
        QCOMPARE(b.companion.offer(endless), QStringLiteral("invalid"));
        // Only TUR and P@x speak through the character.
        QVariantMap stranger = notice(QStringLiteral("d"));
        stranger.insert(QStringLiteral("sender"), QStringLiteral("someone"));
        QCOMPARE(b.companion.offer(stranger), QStringLiteral("invalid"));
        QVERIFY(!b.companion.speaking());
    }

    void a_bubble_left_alone_folds_into_a_mark()
    {
        Bench b;
        b.companion.setBubbleSeconds(1);
        QCOMPARE(b.companion.offer(notice(QStringLiteral("a"))), QStringLiteral("shown"));
        QTRY_VERIFY_WITH_TIMEOUT(!b.companion.speaking(), 5000);
        QCOMPARE(b.companion.heldCount(), 1);
        QCOMPARE(b.companion.state(), QStringLiteral("noticing"));
        // Asked for, it stays until answered.
        QVERIFY(b.companion.unfold());
        QTest::qWait(1500);
        QVERIFY(b.companion.speaking());
        b.companion.dismiss();
        QCOMPARE(b.companion.heldCount(), 0);
    }

    void refused_and_error_pass()
    {
        Bench b;
        b.companion.setMoodSeconds(1);
        b.companion.flag(QStringLiteral("refused"));
        QCOMPARE(b.companion.state(), QStringLiteral("refused"));
        QTRY_COMPARE_WITH_TIMEOUT(b.companion.state(), QStringLiteral("idle"), 5000);
        b.companion.flag(QStringLiteral("error"));
        QCOMPARE(b.companion.state(), QStringLiteral("error"));
        // Answering the bubble about it acknowledges it.
        b.companion.dismiss();
        QCOMPARE(b.companion.state(), QStringLiteral("idle"));
        b.companion.flag(QStringLiteral("delighted"));
        QCOMPARE(b.companion.state(), QStringLiteral("idle"));
    }

    // --- Suggestions ---------------------------------------------------------

    void suggestions_are_off_unless_switched_on_and_then_limited()
    {
        Bench b;
        QVariantMap hint = notice(QStringLiteral("suggest:1"));
        // Off by default: refused, and nothing is kept.
        QVERIFY(!b.settings.suggestions());
        QCOMPARE(b.companion.suggest(hint), QStringLiteral("off"));
        QVERIFY(!b.companion.speaking());
        QCOMPARE(b.companion.heldCount(), 0);

        b.settings.setSuggestions(true);
        QCOMPARE(b.settings.suggestionsPerHour(), 2);
        QCOMPARE(b.companion.suggest(hint), QStringLiteral("shown"));
        QCOMPARE(b.companion.bubble().value(QStringLiteral("kind")).toString(), QStringLiteral("suggestion"));
        // Left alone, a suggestion is dropped, not kept as a mark.
        b.companion.fold();
        QCOMPARE(b.companion.heldCount(), 0);
        // The same one is not made twice.
        QCOMPARE(b.companion.suggest(hint), QStringLiteral("told"));

        b.companion.setClock(noon() + 5 * minute);
        QCOMPARE(b.companion.suggest(notice(QStringLiteral("suggest:2"))), QStringLiteral("shown"));
        b.companion.dismiss();
        b.companion.setClock(noon() + 10 * minute);
        // The set number an hour, and no more.
        QCOMPARE(b.companion.suggest(notice(QStringLiteral("suggest:3"))), QStringLiteral("limited"));
        QVERIFY(!b.companion.speaking());
        b.companion.setClock(noon() + 62 * minute);
        QCOMPARE(b.companion.suggest(notice(QStringLiteral("suggest:3"))), QStringLiteral("shown"));
        b.companion.dismiss();

        // Never while quiet, and never while P@x is busy or speaking.
        b.companion.setClock(noon() + 200 * minute);
        b.companion.setQuiet(QStringLiteral("hour"));
        QCOMPARE(b.companion.suggest(notice(QStringLiteral("suggest:4"))), QStringLiteral("quiet"));
        b.companion.setQuiet(QStringLiteral("off"));
        b.companion.setServiceState(QStringLiteral("working"));
        QCOMPARE(b.companion.suggest(notice(QStringLiteral("suggest:4"))), QStringLiteral("busy"));
        b.companion.setServiceState(QStringLiteral("idle"));
        b.companion.offer(notice(QStringLiteral("real")));
        QCOMPARE(b.companion.suggest(notice(QStringLiteral("suggest:4"))), QStringLiteral("busy"));
        b.companion.dismiss();
        // And never with P@x off.
        b.companion.setPaxOn(false);
        QCOMPARE(b.companion.suggest(notice(QStringLiteral("suggest:4"))), QStringLiteral("hidden"));
    }

    // --- Honest signals ----------------------------------------------------

    void the_flame_burns_as_high_as_the_days_limit_allows()
    {
        Bench b;
        // Not known: it does not pretend.
        QCOMPARE(b.companion.fuel(), 1.0);
        b.companion.setSpent(0.25);
        QCOMPARE(b.companion.fuel(), 0.75);
        b.companion.setSpent(1.4);
        QCOMPARE(b.companion.fuel(), 0.0);
        QVERIFY(!b.companion.saving());
        b.companion.setOnBattery(true);
        QVERIFY(b.companion.saving());
    }

    void what_a_status_says_of_the_days_limit()
    {
        const auto status = [](const QVariant &cost, const QVariant &costLimit, int in, int out, int tokens) {
            return QVariantMap {
                { QStringLiteral("spending"),
                  QVariantMap { { QStringLiteral("today"),
                                  QVariantMap { { QStringLiteral("cost"), cost },
                                                { QStringLiteral("input_tokens"), in },
                                                { QStringLiteral("output_tokens"), out } } },
                                { QStringLiteral("day_limit"),
                                  QVariantMap { { QStringLiteral("cost"), costLimit },
                                                { QStringLiteral("tokens"), tokens } } } } } };
        };
        // In tokens when the limit is in tokens.
        QCOMPARE(AgentSource::spentOf(status(0.0, QVariant::fromValue(nullptr), 300000, 200000, 2000000)), 0.25);
        // In money when the limit is in money.
        QCOMPARE(AgentSource::spentOf(status(1.25, 5.0, 10, 10, 2000000)), 0.25);
        // Not known when the status does not say.
        QVERIFY(AgentSource::spentOf({}) < 0);
    }

    void the_one_suggestion_is_a_plain_rule_over_the_status()
    {
        const auto status = [](int used) {
            return QVariantMap {
                { QStringLiteral("spending"),
                  QVariantMap { { QStringLiteral("day"), QStringLiteral("2026-11-14") },
                                { QStringLiteral("today"),
                                  QVariantMap { { QStringLiteral("cost"), 0.0 },
                                                { QStringLiteral("input_tokens"), used },
                                                { QStringLiteral("output_tokens"), 0 } } },
                                { QStringLiteral("day_limit"),
                                  QVariantMap { { QStringLiteral("cost"), QVariant::fromValue(nullptr) },
                                                { QStringLiteral("tokens"), 1000 } } } } } };
        };
        // Half used, or all of it: nothing to suggest.
        QVERIFY(AgentSource::limitSuggestion(status(500)).isEmpty());
        QVERIFY(AgentSource::limitSuggestion(status(1000)).isEmpty());
        QVERIFY(AgentSource::limitSuggestion({}).isEmpty());
        // Most of it used: one suggestion a day.
        const QVariantMap hint = AgentSource::limitSuggestion(status(950));
        QCOMPARE(hint.value(QStringLiteral("id")).toString(), QStringLiteral("suggest:limit:2026-11-14"));
        QCOMPARE(hint.value(QStringLiteral("opens")).toString(), QStringLiteral("setup"));
        QVERIFY(hint.value(QStringLiteral("actions")).toStringList().size() <= 2);

        // The rules drop it unless the person switched suggestions on.
        Bench b;
        QCOMPARE(b.companion.suggest(hint), QStringLiteral("off"));
        b.settings.setSuggestions(true);
        QCOMPARE(b.companion.suggest(hint), QStringLiteral("shown"));
        b.companion.dismiss();
        QCOMPARE(b.companion.suggest(hint), QStringLiteral("told"));
    }

    void over_a_full_screen_window_only_when_it_matters_unless_asked_to_stay()
    {
        Bench b;
        // The person's choice to stay is the default.
        QVERIFY(b.settings.overFullScreen());
        QVERIFY(b.companion.raised());
        b.settings.setOverFullScreen(false);
        QVERIFY(!b.companion.raised());
        b.companion.setServiceState(QStringLiteral("working"));
        b.companion.offer(notice(QStringLiteral("a")));
        QVERIFY(!b.companion.raised());
        for (const char *matters : { "acting", "observing", "waiting" }) {
            b.companion.setOverlayActivity(QLatin1String(matters));
            QVERIFY2(b.companion.raised(), matters);
            b.companion.setOverlayActivity(QStringLiteral("working"));
            QVERIFY(!b.companion.raised());
        }
    }

    // --- The person's choices ----------------------------------------------

    void what_the_person_chose_is_kept()
    {
        QTemporaryDir dir;
        QVERIFY(dir.isValid());
        const QString path = dir.path() + QStringLiteral("/tur/pax-companion.json");
        {
            CompanionSettings first;
            first.setPath(path);
            // What a new person gets.
            for (const QString &kind : CompanionSettings::kindNames())
                QVERIFY(first.kindOn(kind));
            QVERIFY(!first.suggestions());
            QVERIFY(!first.hidden());
            QCOMPARE(first.presence(), QStringLiteral("with-pax"));
            QCOMPARE(first.along(), 1.0);
            QVERIFY(first.overFullScreen());
            QCOMPARE(first.quietUntil(), 0.0);

            first.setKind(QStringLiteral("task"), false);
            first.setSuggestions(true);
            first.setQuiet(QStringLiteral("always"), noon());
            first.setHidden(true);
            first.setPresence(QStringLiteral("always"));
            first.setAlong(0.25);
            first.setOverFullScreen(false);
            first.markTold(QStringLiteral("heal:3"));
        }
        CompanionSettings second;
        second.setPath(path);
        QVERIFY(!second.kindOn(QStringLiteral("task")));
        QVERIFY(second.kindOn(QStringLiteral("repair")));
        QVERIFY(second.suggestions());
        QCOMPARE(second.quietUntil(), -1.0);
        QVERIFY(second.hidden());
        QCOMPARE(second.presence(), QStringLiteral("always"));
        QCOMPARE(second.along(), 0.25);
        QVERIFY(!second.overFullScreen());
        QVERIFY(second.told(QStringLiteral("heal:3")));
        QVERIFY(!second.told(QStringLiteral("heal:4")));

        // A notice said in one session is not said again in the next.
        Companion companion;
        second.setHidden(false);
        second.setQuiet(QStringLiteral("off"), noon());
        companion.setSettings(&second);
        companion.setClock(noon());
        companion.setPaxOn(true);
        QCOMPARE(companion.offer(notice(QStringLiteral("heal:3"))), QStringLiteral("told"));
    }

    // --- The healing service's notices -------------------------------------

    void the_healing_services_notices_are_said_in_its_words()
    {
        const QJsonArray printed = printedNotices();
        QCOMPARE(printed.size(), 5);

        const QVariantMap repaired = HealSource::noticeFrom(printed.at(0).toObject());
        QCOMPARE(repaired.value(QStringLiteral("id")).toString(), QStringLiteral("heal:3"));
        QCOMPARE(repaired.value(QStringLiteral("kind")).toString(), QStringLiteral("repair"));
        QCOMPARE(repaired.value(QStringLiteral("sender")).toString(), QStringLiteral("tur"));
        QCOMPARE(repaired.value(QStringLiteral("sentence")).toString(),
                 printed.at(0).toObject().value(QLatin1String("body")).toString());
        QCOMPARE(repaired.value(QStringLiteral("code")).toString(), QStringLiteral("TUR-DESKTOP-SERVICE-001"));
        QVERIFY(repaired.value(QStringLiteral("commands")).toStringList().isEmpty());

        const QVariantMap waiting = HealSource::noticeFrom(printed.at(1).toObject());
        QCOMPARE(waiting.value(QStringLiteral("kind")).toString(), QStringLiteral("fault"));
        // The command is text for the person; the character has no action
        // that runs it.
        QCOMPARE(waiting.value(QStringLiteral("commands")).toStringList(),
                 QStringList { QStringLiteral("tur-diag heal apply TUR-NETWORK-RESOLVER-001 restore-resolver-link") });
        QCOMPARE(waiting.value(QStringLiteral("actions")).toStringList(),
                 (QStringList { QStringLiteral("show"), QStringLiteral("dismiss") }));

        QCOMPARE(HealSource::noticeFrom(printed.at(2).toObject()).value(QStringLiteral("kind")).toString(),
                 QStringLiteral("fault"));

        // Too long for one bubble: the title is said, the whole text kept.
        const QVariantMap wordy = HealSource::noticeFrom(printed.at(3).toObject());
        QVERIFY(printed.at(3).toObject().value(QLatin1String("body")).toString().size() > 180);
        QCOMPARE(wordy.value(QStringLiteral("sentence")).toString(), QStringLiteral("A system service stopped working"));
        QCOMPARE(wordy.value(QStringLiteral("detail")).toString(),
                 printed.at(3).toObject().value(QLatin1String("body")).toString());

        // An event the person is not told of is not a notice.
        QVERIFY(HealSource::noticeFrom(printed.at(4).toObject()).isEmpty());
        QVERIFY(HealSource::noticeFrom(QJsonObject()).isEmpty());

        for (const QJsonValue &entry : printed) {
            const QVariantMap made = HealSource::noticeFrom(entry.toObject());
            if (!made.isEmpty())
                QVERIFY(made.value(QStringLiteral("sentence")).toString().size() <= Companion::sentenceLimit());
        }
    }

    void the_healing_services_program_is_run_and_its_file_watched()
    {
        Bench b;
        QTemporaryDir dir;
        QVERIFY(dir.isValid());
        const QString printedFile = dir.path() + QStringLiteral("/printed.json");
        const QString stateFile = dir.path() + QStringLiteral("/state.json");
        const auto write = [](const QString &path, const QByteArray &content) {
            QFile file(path);
            QVERIFY(file.open(QIODevice::WriteOnly | QIODevice::Truncate));
            file.write(content);
        };
        const QJsonArray printed = printedNotices();
        write(printedFile, QJsonDocument(QJsonArray { printed.at(0) }).toJson());
        write(stateFile, "{}");

        HealSource heal;
        heal.setCompanion(&b.companion);
        // A stand-in for `tur-diag heal notices --json`: it prints the file.
        heal.setCommand({ QStringLiteral("cat"), printedFile });
        heal.setStateFile(stateFile);
        heal.start();
        QTRY_COMPARE_WITH_TIMEOUT(heal.count(), 1, 10000);
        QVERIFY(heal.problem().isEmpty());
        QCOMPARE(b.companion.bubble().value(QStringLiteral("id")).toString(), QStringLiteral("heal:3"));

        // The healing service wrote its state: the notices are read again,
        // the one already said is not said again, the new one waits.
        write(printedFile, QJsonDocument(QJsonArray { printed.at(0), printed.at(1) }).toJson());
        write(stateFile, "{\"changed\": true}");
        QTRY_COMPARE_WITH_TIMEOUT(heal.count(), 2, 10000);
        QCOMPARE(b.companion.bubble().value(QStringLiteral("id")).toString(), QStringLiteral("heal:3"));
        QCOMPARE(b.companion.heldCount(), 1);

        // A program that is not there, or prints something else, is a
        // problem that is said nowhere on the bar.
        HealSource missing;
        missing.setCompanion(&b.companion);
        missing.setCommand({ QStringLiteral("tur-no-such-program-anywhere") });
        missing.setStateFile(QString());
        missing.refresh();
        QTRY_VERIFY_WITH_TIMEOUT(!missing.problem().isEmpty(), 10000);
        QCOMPARE(missing.ingest(QStringLiteral("{\"not\": \"an array\"}")), -1);
        QCOMPARE(HealSource::defaultCommand(),
                 (QStringList { QStringLiteral("tur-diag"), QStringLiteral("heal"), QStringLiteral("notices"),
                                QStringLiteral("--json") }));
    }

    // --- Where things go ---------------------------------------------------

    void the_character_sits_in_the_bar()
    {
        Placement place;
        place.setScreenWidth(1280);
        place.setScreenHeight(800);
        // The compositor leaves 756 of 800: a bar 44 thick.
        place.setUsableHeight(756);
        QCOMPARE(place.inset(), 44.0);
        QCOMPARE(place.size(), 40.0);
        // Inside the bar, in the middle of its thickness.
        QCOMPARE(place.characterLift(), 2.0);
        QVERIFY(place.characterLift() + place.size() <= place.inset());
        // At the bar's end by default.
        QCOMPARE(place.along(), 1.0);
        QCOMPARE(place.characterX(), 1280.0 - 40.0 - Placement::margin);
        // What opens above it starts clear of the bar.
        QCOMPARE(place.popupLift(), 44.0 + Placement::margin);
        // Beside the character, and on the screen.
        QCOMPARE(place.popupX(320), 1280.0 - 320.0 - Placement::margin);
        place.setAlong(0.5);
        QCOMPARE(place.popupX(320), place.characterX() + 20.0 - 160.0);
        place.setAlong(0);
        QCOMPARE(place.characterX(), Placement::margin);
        QCOMPARE(place.popupX(320), Placement::margin);

        // The start of the bar is on the right in a right-to-left layout.
        place.setRightToLeft(true);
        QCOMPARE(place.characterX(), 1280.0 - 40.0 - Placement::margin);
        place.setAlong(1);
        QCOMPARE(place.characterX(), Placement::margin);
        place.setRightToLeft(false);
    }

    void dragged_it_stays_where_it_is_left_and_snaps_to_an_end()
    {
        Placement place;
        place.setScreenWidth(1280);
        place.setScreenHeight(800);
        place.setUsableHeight(756);
        const double middle = place.alongAt(600);
        QVERIFY(middle > 0.4 && middle < 0.6);
        place.setAlong(middle);
        QVERIFY(qAbs(place.characterX() - 600) < 0.5);
        // Close to an end: at the end.
        QCOMPARE(place.alongAt(20), 0.0);
        QCOMPARE(place.alongAt(1220), 1.0);
        // Beyond the screen: still on the bar.
        QCOMPARE(place.alongAt(-500), 0.0);
        QCOMPARE(place.alongAt(5000), 1.0);
        place.setRightToLeft(true);
        QCOMPARE(place.alongAt(20), 1.0);
        QCOMPARE(place.alongAt(1220), 0.0);
    }

    void without_a_bar_it_rests_above_the_screens_edge()
    {
        Placement place;
        place.setScreenWidth(1280);
        place.setScreenHeight(800);
        // Not known yet, or no bar, or a bar that hides.
        for (const double usable : { 0.0, 1.0, 800.0, 900.0 }) {
            place.setUsableHeight(usable);
            QCOMPARE(place.inset(), 0.0);
            QCOMPARE(place.size(), 40.0);
            QCOMPARE(place.characterLift(), 4.0);
            QCOMPARE(place.popupLift(), 4.0 + 40.0 + Placement::margin);
        }
        // A thin bar: a smaller character, still inside it.
        place.setUsableHeight(770);
        QCOMPARE(place.inset(), 30.0);
        QCOMPARE(place.size(), 26.0);
        QCOMPARE(place.characterLift(), 2.0);
        // A bar thicker than the character's box.
        place.setUsableHeight(736);
        QCOMPARE(place.size(), 40.0);
        QCOMPARE(place.characterLift(), 12.0);
        QCOMPARE(place.popupLift(), 64.0 + Placement::margin);
    }
};

QTEST_GUILESS_MAIN(LogicTest)

#include "tst_logic.moc"
