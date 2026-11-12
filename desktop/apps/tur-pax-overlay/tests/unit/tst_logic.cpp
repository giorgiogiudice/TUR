// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
//
// The rules of org.tur.PaxOverlay1 without a bus or a compositor: leases are
// answered only for the connection that took them; a rectangle in a window
// lands at the window's content plus the rectangle; a highlight follows its
// window when it moves and is dropped when the window changes size, is
// minimised, is covered where the highlight is, or closes; the loudest state
// is shown; and a frame for a window that cannot be found goes round the
// screen and says so.

#include "overlayservice.h"
#include "overlaystate.h"
#include "windowlocator.h"

#include <QElapsedTimer>
#include <QHash>
#include <QList>
#include <QSignalSpy>
#include <QStringList>
#include <QTest>

namespace {

// A compositor in a test: windows are whatever the test says they are.
class FakeLocator : public WindowLocator
{
public:
    QHash<QString, WindowPlace> windows;
    QStringList order;

    bool available() const override { return true; }
    QString mechanism() const override { return QStringLiteral("a fake locator"); }
    std::optional<WindowPlace> find(uint pid, const QString &appId, const QString &title) const override
    {
        std::optional<WindowPlace> best;
        for (const QString &uuid : order) {
            const WindowPlace &place = windows[uuid];
            if ((pid == 0 || place.pid == pid) && (appId.isEmpty() || place.appId == appId)
                && (title.isEmpty() || place.title == title))
                best = place;
        }
        return best;
    }
    std::optional<WindowPlace> place(const QString &uuid) const override
    {
        if (!windows.contains(uuid))
            return std::nullopt;
        return windows.value(uuid);
    }
    bool covered(const QString &uuid, const QRectF &rect) const override
    {
        const qsizetype at = order.indexOf(uuid);
        for (qsizetype i = at + 1; at >= 0 && i < order.size(); ++i) {
            const WindowPlace &above = windows[order.at(i)];
            if (!above.minimized && above.frame.intersects(rect))
                return true;
        }
        return false;
    }

    void add(const QString &uuid, uint pid, const QString &appId, const QString &title, const QRectF &client)
    {
        WindowPlace place;
        place.uuid = uuid;
        place.pid = pid;
        place.appId = appId;
        place.title = title;
        place.client = client;
        // A title bar of 30 px above the content.
        place.frame = client.adjusted(0, -30, 0, 0);
        windows.insert(uuid, place);
        order.append(uuid);
    }
    void move(const QString &uuid, const QPointF &to)
    {
        WindowPlace &place = windows[uuid];
        place.client.moveTopLeft(to);
        place.frame = place.client.adjusted(0, -30, 0, 0);
        Q_EMIT changed(uuid);
    }
};

PaxOverlay::RectD rectD(double x, double y, double width, double height)
{
    return PaxOverlay::RectD { x, y, width, height };
}

QRectF highlightRect(const OverlayState &state, uint id)
{
    for (const QVariant &entry : state.highlights()) {
        const QVariantMap map = entry.toMap();
        if (map.value(QStringLiteral("id")).toUInt() == id) {
            return QRectF(map.value(QStringLiteral("x")).toDouble(), map.value(QStringLiteral("y")).toDouble(),
                          map.value(QStringLiteral("width")).toDouble(),
                          map.value(QStringLiteral("height")).toDouble());
        }
    }
    return QRectF();
}

} // namespace

class LogicTest : public QObject
{
    Q_OBJECT

private:
    const QString me = QStringLiteral(":1.10");
    const QString other = QStringLiteral(":1.11");

    struct Rig
    {
        OverlayState state;
        FakeLocator locator;
        OverlayService service { &state, &locator };

        Rig()
        {
            state.setFrozen(true);
            service.setOutputs({ { QStringLiteral("Virtual-1"), QRectF(0, 0, 1280, 800) },
                                 { QStringLiteral("Virtual-2"), QRectF(1280, 0, 1280, 800) } });
            locator.add(QStringLiteral("{settings}"), 4242, QStringLiteral("org.tur.Settings"),
                        QStringLiteral("Settings"), QRectF(200, 130, 900, 600));
        }

        QString lease(const QString &caller)
        {
            const auto outcome = service.acquire(caller, QStringLiteral("test"));
            return outcome.value.toString();
        }
    };

private Q_SLOTS:
    void a_lease_is_answered_only_for_its_holder()
    {
        Rig rig;
        const QString lease = rig.lease(me);
        QCOMPARE(lease.size(), 32);
        QVERIFY(rig.service.setActivity(me, lease, QStringLiteral("working"), {}).ok);

        const auto stolen = rig.service.setActivity(other, lease, QStringLiteral("acting"), {});
        QVERIFY(!stolen.ok);
        QCOMPARE(stolen.errorName, QStringLiteral("org.tur.PaxOverlay1.Error.NoLease"));
        QVERIFY(stolen.errorMessage.startsWith(QStringLiteral("TUR-DESKTOP-PAXOVERLAY-001: ")));
        QVERIFY(!rig.service.release(other, lease).ok);
        QVERIFY(!rig.service.movePointer(other, lease, QString(), 10, 10, {}).ok);
        QVERIFY(!rig.service.highlightRect(other, lease, QString(), rectD(1, 1, 5, 5), {}).ok);
        QVERIFY(!rig.service.setActivity(me, QStringLiteral("made-up"), QStringLiteral("acting"), {}).ok);
        QCOMPARE(rig.state.activity(), QStringLiteral("working"));

        QVERIFY(rig.service.release(me, lease).ok);
        QVERIFY(!rig.service.release(me, lease).ok);
        QCOMPARE(rig.state.activity(), QStringLiteral("idle"));
        QVERIFY(!rig.state.active());
    }

    void two_leases_are_different()
    {
        Rig rig;
        QVERIFY(rig.lease(me) != rig.lease(me));
    }

    void arguments_are_checked()
    {
        Rig rig;
        const QString lease = rig.lease(me);
        const auto bad = [](const OverlayService::Outcome &outcome) {
            return !outcome.ok && outcome.errorName == QLatin1String("org.tur.PaxOverlay1.Error.InvalidArgument");
        };
        QVERIFY(bad(rig.service.setActivity(me, lease, QStringLiteral("dancing"), {})));
        QVERIFY(bad(rig.service.setActivity(me, lease, QStringLiteral("observing"),
                                            { { QStringLiteral("scope"), QStringLiteral("everything") } })));
        QVERIFY(bad(rig.service.highlightRect(me, lease, QString(), rectD(0, 0, 0, 10), {})));
        QVERIFY(bad(rig.service.highlightRect(me, lease, QString(), rectD(0, 0, -5, 10), {})));
        QVERIFY(bad(rig.service.highlightRect(me, lease, QString(), rectD(qQNaN(), 0, 5, 10), {})));
        QVERIFY(bad(rig.service.highlightRect(me, lease, QString(), rectD(1e9, 0, 5, 10), {})));
        QVERIFY(bad(rig.service.highlightRect(me, lease, QStringLiteral("HDMI-9"), rectD(0, 0, 5, 10), {})));
        QVERIFY(bad(rig.service.highlightRect(me, lease, QString(), rectD(0, 0, 5, 10),
                                              { { QStringLiteral("caption"), QString(400, QLatin1Char('x')) } })));
        QVERIFY(bad(rig.service.pointAt(me, lease, QString(), rectD(0, 0, 5, 10),
                                        { { QStringLiteral("anchor"), QStringLiteral("middle") } })));
        QVERIFY(bad(rig.service.pulsePointer(me, lease, QStringLiteral("double-click"))));
        QVERIFY(rig.state.highlights().isEmpty());

        const auto unknown = rig.service.clear(me, lease, 99);
        QCOMPARE(unknown.errorName, QStringLiteral("org.tur.PaxOverlay1.Error.UnknownHighlight"));
    }

    void limits()
    {
        Rig rig;
        QStringList leases;
        for (int i = 0; i < OverlayService::maxLeases; ++i)
            leases.append(rig.lease(me));
        const auto ninth = rig.service.acquire(me, QStringLiteral("one too many"));
        QCOMPARE(ninth.errorName, QStringLiteral("org.tur.PaxOverlay1.Error.LimitReached"));
        for (int i = 0; i < OverlayService::maxHighlightsPerLease; ++i)
            QVERIFY(rig.service.highlightRect(me, leases.first(), QString(), rectD(i, i, 5, 5), {}).ok);
        const auto seventeenth = rig.service.highlightRect(me, leases.first(), QString(), rectD(1, 1, 5, 5), {});
        QCOMPARE(seventeenth.errorName, QStringLiteral("org.tur.PaxOverlay1.Error.LimitReached"));
    }

    // A screen's coordinates are relative to the screen.
    void a_screen_rectangle_is_moved_to_its_screen()
    {
        Rig rig;
        const QString lease = rig.lease(me);
        const auto outcome = rig.service.highlightRect(me, lease, QStringLiteral("Virtual-2"), rectD(10, 20, 30, 40), {});
        QVERIFY(outcome.ok);
        QCOMPARE(highlightRect(rig.state, outcome.value.toUInt()), QRectF(1290, 20, 30, 40));

        QVERIFY(rig.service.movePointer(me, lease, QStringLiteral("Virtual-2"), 5, 6, {}).ok);
        QCOMPARE(rig.state.pointer(), QPointF(1285, 6));
        QVERIFY(rig.service.pointAt(me, lease, QString(), rectD(100, 100, 200, 40), {}).ok);
        QCOMPARE(rig.state.pointer(), QPointF(200, 120));
        QVERIFY(rig.service.pointAt(me, lease, QString(), rectD(100, 100, 200, 40),
                                    { { QStringLiteral("anchor"), QStringLiteral("start") } })
                    .ok);
        QCOMPARE(rig.state.pointer(), QPointF(110, 120));
        QVERIFY(rig.service.pointAt(me, lease, QString(), rectD(100, 100, 200, 40),
                                    { { QStringLiteral("anchor"), QStringLiteral("end") } })
                    .ok);
        QCOMPARE(rig.state.pointer(), QPointF(290, 120));
    }

    // The heart of the positioning: what org.tur.Settings1.Reveal returns is
    // relative to the window's content; the compositor says where that is.
    void a_window_rectangle_lands_on_the_window_content()
    {
        Rig rig;
        const QString lease = rig.lease(me);
        const auto outcome = rig.service.highlightInWindow(me, lease, 4242, QStringLiteral("org.tur.Settings"),
                                                           QStringLiteral("Settings"), rectD(24, 180, 600, 56),
                                                           { { QStringLiteral("dim"), true } });
        QVERIFY2(outcome.ok, qPrintable(outcome.errorMessage));
        const uint id = outcome.value.toUInt();
        QCOMPARE(highlightRect(rig.state, id), QRectF(224, 310, 600, 56));
        const QVariantMap entry = rig.state.highlights().constFirst().toMap();
        // What is dimmed is the window's content.
        QCOMPARE(entry.value(QStringLiteral("dimX")).toDouble(), 200.0);
        QCOMPARE(entry.value(QStringLiteral("dimWidth")).toDouble(), 900.0);

        QVERIFY(rig.service.pointInWindow(me, lease, 4242, QString(), QString(), rectD(24, 180, 600, 56), {}).ok);
        QCOMPARE(rig.state.pointer(), QPointF(524, 338));

        // Any one of the three may be left out.
        QVERIFY(rig.service.highlightInWindow(me, lease, 0, QStringLiteral("org.tur.Settings"), QString(),
                                              rectD(1, 1, 5, 5), {})
                    .ok);
        const auto missing = rig.service.highlightInWindow(me, lease, 4243, QString(), QString(), rectD(1, 1, 5, 5), {});
        QCOMPARE(missing.errorName, QStringLiteral("org.tur.PaxOverlay1.Error.WindowNotFound"));
        QVERIFY(missing.errorMessage.contains(QStringLiteral("pid 4243")));
    }

    void a_highlight_follows_its_window_and_is_dropped_when_it_cannot()
    {
        Rig rig;
        const QString lease = rig.lease(me);
        const auto first = rig.service.highlightInWindow(me, lease, 4242, QString(), QString(), rectD(24, 180, 600, 56), {});
        const uint id = first.value.toUInt();

        // Moved: it goes with the window.
        rig.locator.move(QStringLiteral("{settings}"), QPointF(300, 100));
        QCOMPARE(highlightRect(rig.state, id), QRectF(324, 280, 600, 56));

        // Covered where the highlight is: dropped.
        rig.locator.add(QStringLiteral("{other}"), 9, QStringLiteral("org.example"), QStringLiteral("Other"),
                        QRectF(250, 300, 200, 100));
        Q_EMIT rig.locator.changed(QString());
        QVERIFY(!rig.state.hasHighlight(id));

        // A window above that does not overlap it does not drop it.
        rig.locator.windows[QStringLiteral("{other}")].client = QRectF(2000, 500, 100, 100);
        rig.locator.windows[QStringLiteral("{other}")].frame = QRectF(2000, 470, 100, 130);
        const uint second =
            rig.service.highlightInWindow(me, lease, 4242, QString(), QString(), rectD(24, 180, 600, 56), {}).value.toUInt();
        Q_EMIT rig.locator.changed(QString());
        QVERIFY(rig.state.hasHighlight(second));

        // Resized: dropped, because the content may have moved inside it.
        rig.locator.windows[QStringLiteral("{settings}")].client.setWidth(800);
        Q_EMIT rig.locator.changed(QStringLiteral("{settings}"));
        QVERIFY(!rig.state.hasHighlight(second));

        // Minimised: dropped.
        const uint third =
            rig.service.highlightInWindow(me, lease, 4242, QString(), QString(), rectD(1, 1, 5, 5), {}).value.toUInt();
        rig.locator.windows[QStringLiteral("{settings}")].minimized = true;
        Q_EMIT rig.locator.changed(QStringLiteral("{settings}"));
        QVERIFY(!rig.state.hasHighlight(third));

        // Closed: dropped.
        rig.locator.windows[QStringLiteral("{settings}")].minimized = false;
        const uint fourth =
            rig.service.highlightInWindow(me, lease, 4242, QString(), QString(), rectD(1, 1, 5, 5), {}).value.toUInt();
        rig.locator.windows.remove(QStringLiteral("{settings}"));
        Q_EMIT rig.locator.removed(QStringLiteral("{settings}"));
        QVERIFY(!rig.state.hasHighlight(fourth));
    }

    void the_loudest_state_is_shown()
    {
        Rig rig;
        const QString quiet = rig.lease(me);
        const QString loud = rig.lease(other);
        QVERIFY(rig.service.setActivity(me, quiet, QStringLiteral("listening"), {}).ok);
        QVERIFY(rig.service.setActivity(other, loud, QStringLiteral("acting"),
                                        { { QStringLiteral("scope"), QStringLiteral("screen") } })
                    .ok);
        QCOMPARE(rig.state.activity(), QStringLiteral("acting"));
        QVERIFY(rig.service.release(other, loud).ok);
        QCOMPARE(rig.state.activity(), QStringLiteral("listening"));
    }

    void the_frame_goes_round_the_window_or_says_screen()
    {
        Rig rig;
        const QString lease = rig.lease(me);
        QVERIFY(rig.service.setActivity(me, lease, QStringLiteral("observing"),
                                        { { QStringLiteral("scope"), QStringLiteral("window") },
                                          { QStringLiteral("pid"), 4242u } })
                    .ok);
        QCOMPARE(rig.state.scope(), QStringLiteral("window"));
        QCOMPARE(rig.state.frameRect(), QRectF(200, 100, 900, 630));
        rig.locator.move(QStringLiteral("{settings}"), QPointF(0, 50));
        QCOMPARE(rig.state.frameRect(), QRectF(0, 20, 900, 630));

        // A window that cannot be found: the screen, and the words say so.
        QVERIFY(rig.service.setActivity(me, lease, QStringLiteral("observing"),
                                        { { QStringLiteral("scope"), QStringLiteral("window") },
                                          { QStringLiteral("pid"), 1u } })
                    .ok);
        QCOMPARE(rig.state.scope(), QStringLiteral("screen"));
        QVERIFY(rig.state.frameRect().isEmpty());
    }

    void ending_a_lease_takes_everything_it_drew()
    {
        Rig rig;
        const QString mine = rig.lease(me);
        const QString theirs = rig.lease(other);
        const uint a = rig.service.highlightRect(me, mine, QString(), rectD(1, 1, 5, 5), {}).value.toUInt();
        const uint b = rig.service.highlightRect(other, theirs, QString(), rectD(1, 1, 5, 5), {}).value.toUInt();
        QVERIFY(rig.service.movePointer(me, mine, QString(), 50, 50, {}).ok);
        QVERIFY(rig.state.pointerVisible());

        QSignalSpy ended(&rig.service, &OverlayService::leaseEnded);
        QVERIFY(rig.service.release(me, mine).ok);
        QVERIFY(!rig.state.hasHighlight(a));
        QVERIFY(rig.state.hasHighlight(b));
        QVERIFY(!rig.state.pointerVisible());
        QCOMPARE(ended.size(), 1);
        QCOMPARE(ended.at(0).at(1).toString(), QStringLiteral("released"));

        // Hide: everything, everybody's.
        QVERIFY(rig.service.setActivity(other, theirs, QStringLiteral("acting"), {}).ok);
        rig.service.hide();
        QVERIFY(!rig.state.hasHighlight(b));
        QCOMPARE(rig.state.activity(), QStringLiteral("idle"));
        QVERIFY(!rig.state.active());
        QVERIFY(!rig.service.setActivity(other, theirs, QStringLiteral("acting"), {}).ok);
        QCOMPARE(ended.size(), 2);
        QCOMPARE(ended.at(1).at(1).toString(), QStringLiteral("hidden"));
    }

    void a_highlight_can_wait_for_the_pointer()
    {
        Rig rig;
        rig.state.setFrozen(false);
        rig.state.setMotionFactor(1);
        const QString lease = rig.lease(me);
        QSignalSpy arrived(&rig.state, &OverlayState::pointerArrived);
        const uint id = rig.service
                            .highlightRect(me, lease, QString(), rectD(600, 500, 200, 40),
                                           { { QStringLiteral("point"), true } })
                            .value.toUInt();
        QVariantMap entry = rig.state.highlights().constFirst().toMap();
        QVERIFY(rig.state.pointerMoving());
        QCOMPARE(entry.value(QStringLiteral("shown")).toBool(), false);
        QVERIFY(arrived.wait(3000));
        entry = rig.state.highlights().constFirst().toMap();
        QCOMPARE(entry.value(QStringLiteral("id")).toUInt(), id);
        QCOMPARE(entry.value(QStringLiteral("shown")).toBool(), true);
        QCOMPARE(rig.state.pointer(), QPointF(700, 520));
    }

    // The journey takes 250 to 700 ms by distance (design, 9.4), twice that
    // when slow, and with reduced motion nothing travels.
    void the_pointer_travels_in_the_designed_time()
    {
        Rig rig;
        rig.state.setFrozen(false);
        rig.state.setMotionFactor(1);
        QSignalSpy arrived(&rig.state, &OverlayState::pointerArrived);

        rig.state.movePointer(QPointF(640, 400), false);
        QVERIFY(arrived.wait(3000));
        QElapsedTimer clock;
        clock.start();
        rig.state.movePointer(QPointF(660, 400), false);
        QVERIFY(arrived.wait(3000));
        const qint64 shortJourney = clock.restart();
        QVERIFY2(shortJourney >= 240 && shortJourney < 600, qPrintable(QString::number(shortJourney)));
        rig.state.movePointer(QPointF(2400, 780), false);
        QVERIFY(arrived.wait(3000));
        const qint64 longJourney = clock.restart();
        QVERIFY2(longJourney >= 690 && longJourney < 1200, qPrintable(QString::number(longJourney)));
        rig.state.movePointer(QPointF(660, 400), true);
        QVERIFY(arrived.wait(5000));
        const qint64 slowJourney = clock.restart();
        QVERIFY2(slowJourney >= 1390 && slowJourney < 2200, qPrintable(QString::number(slowJourney)));

        // Reduced motion: a fade out and in, and a ring at the target.
        rig.state.setMotionFactor(0);
        const int pulses = rig.state.pulses();
        const QPointF start = rig.state.pointer();
        QList<QPointF> visited;
        connect(&rig.state, &OverlayState::pointerMoved, this, [&] { visited.append(rig.state.pointer()); });
        rig.state.movePointer(QPointF(1300, 20), false);
        QVERIFY(arrived.wait(3000));
        for (const QPointF &point : std::as_const(visited)) {
            QVERIFY2(point == start || point == QPointF(1300, 20),
                     "with reduced motion the pointer jumps; it does not pass through points between");
        }
        QCOMPARE(rig.state.pointer(), QPointF(1300, 20));
        QCOMPARE(rig.state.pulses(), pulses + 1);
        QCOMPARE(rig.state.pointerOpacity(), 1.0);
    }
};

QTEST_GUILESS_MAIN(LogicTest)

#include "tst_logic.moc"
