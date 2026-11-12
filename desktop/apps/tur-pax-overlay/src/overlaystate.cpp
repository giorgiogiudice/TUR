// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#include "overlaystate.h"

#include <QEasingCurve>
#include <QLineF>
#include <QtGlobal>

#include <algorithm>
#include <cmath>
#include <utility>

namespace {

// The pointer's journey (design, 9.4): between 250 ms and 700 ms depending
// on distance, decelerating into the target; "slow down" doubles it.
constexpr int shortestJourney = 250;
constexpr int longestJourney = 700;
constexpr double millisecondsPerPixel = 0.45;
// With reduced motion it fades out and in at the target instead; a fade is
// never longer than 100 ms then (desktop-kit, Motion.fade).
constexpr int reducedFade = 100;
// Its first appearance fades in while it travels.
constexpr int appearFade = 150;
// Where it enters a screen the first time: under the pill, which is 36 px
// high and 12 px from the top (OverlaySurface.qml).
constexpr double entryBelowTop = 64;

} // namespace

OverlayState::OverlayState(QObject *parent)
    : QObject(parent)
{
    bool ok = false;
    const int size = qEnvironmentVariableIntValue("XCURSOR_SIZE", &ok);
    if (ok && size >= 16 && size <= 256)
        m_cursorSize = size;

    m_travel.setStartValue(0.0);
    m_travel.setEndValue(1.0);
    m_travel.setEasingCurve(QEasingCurve::OutCubic);
    connect(&m_travel, &QVariantAnimation::valueChanged, this, [this](const QVariant &value) {
        // A gentle curve: a quadratic Bezier through a control point beside
        // the straight line.
        const double t = value.toDouble();
        const double u = 1 - t;
        setPointer(u * u * m_from + 2 * u * t * m_bend + t * t * m_target);
    });
    connect(&m_travel, &QVariantAnimation::finished, this, [this] {
        setPointer(m_target);
        arrive();
    });

    connect(&m_fade, &QVariantAnimation::valueChanged, this,
            [this](const QVariant &value) { setPointerOpacity(value.toDouble()); });
    connect(&m_fade, &QVariantAnimation::finished, this, [this] {
        if (m_afterFade) {
            auto then = std::move(m_afterFade);
            m_afterFade = nullptr;
            then();
        }
    });
}

const QStringList &OverlayState::activities()
{
    static const QStringList names { QStringLiteral("idle"),      QStringLiteral("paused"),
                                     QStringLiteral("listening"), QStringLiteral("working"),
                                     QStringLiteral("observing"), QStringLiteral("waiting"),
                                     QStringLiteral("acting") };
    return names;
}

bool OverlayState::isActivity(const QString &name)
{
    return activities().contains(name);
}

int OverlayState::loudness(const QString &activity)
{
    return int(activities().indexOf(activity));
}

QVariantList OverlayState::highlights() const
{
    QVariantList out;
    for (const Highlight &highlight : m_highlights) {
        out.append(QVariantMap {
            { QStringLiteral("id"), highlight.id },
            { QStringLiteral("x"), highlight.rect.x() },
            { QStringLiteral("y"), highlight.rect.y() },
            { QStringLiteral("width"), highlight.rect.width() },
            { QStringLiteral("height"), highlight.rect.height() },
            { QStringLiteral("dim"), highlight.dim },
            { QStringLiteral("dimX"), highlight.dimRect.x() },
            { QStringLiteral("dimY"), highlight.dimRect.y() },
            { QStringLiteral("dimWidth"), highlight.dimRect.width() },
            { QStringLiteral("dimHeight"), highlight.dimRect.height() },
            { QStringLiteral("caption"), highlight.caption },
            { QStringLiteral("step"), highlight.step },
            { QStringLiteral("shown"), highlight.shown },
        });
    }
    return out;
}

bool OverlayState::active() const
{
    return m_active;
}

void OverlayState::updateActive()
{
    const bool active = m_on || m_activity != QLatin1String("idle") || m_pointerVisible || !m_highlights.isEmpty();
    if (active != m_active) {
        m_active = active;
        Q_EMIT activeChanged();
    }
}

void OverlayState::setOn(bool on)
{
    if (on != m_on) {
        m_on = on;
        Q_EMIT onChanged();
        updateActive();
    }
}

void OverlayState::setCommandsShown(bool shown)
{
    if (shown != m_commandsShown) {
        m_commandsShown = shown;
        Q_EMIT commandsShownChanged();
    }
}

void OverlayState::setCursorSize(int size)
{
    size = std::clamp(size, 16, 256);
    if (size != m_cursorSize) {
        m_cursorSize = size;
        Q_EMIT cursorSizeChanged();
    }
}

void OverlayState::setMotionFactor(qreal factor)
{
    if (!std::isfinite(factor) || factor < 0)
        factor = 1;
    if (!qFuzzyCompare(factor + 1, m_motionFactor + 1)) {
        m_motionFactor = factor;
        Q_EMIT motionChanged();
    }
}

void OverlayState::setFrozen(bool frozen)
{
    if (frozen != m_frozen) {
        m_frozen = frozen;
        Q_EMIT motionChanged();
    }
}

void OverlayState::setScreens(const QList<QRectF> &screens)
{
    m_screens = screens;
}

void OverlayState::setScreenRects(const QVariantList &rects)
{
    QList<QRectF> screens;
    for (const QVariant &rect : rects)
        screens.append(rect.toRectF());
    setScreens(screens);
}

void OverlayState::setActivity(const QString &activity, const QString &scope, const QRectF &frame)
{
    const QString name = isActivity(activity) ? activity : QStringLiteral("idle");
    const QString where = scope == QLatin1String("screen") ? scope : QStringLiteral("window");
    if (name == m_activity && where == m_scope && frame == m_frameRect)
        return;
    m_activity = name;
    m_scope = where;
    m_frameRect = frame;
    Q_EMIT activityChanged();
    updateActive();
}

void OverlayState::setFrameRect(const QRectF &frame)
{
    if (frame != m_frameRect) {
        m_frameRect = frame;
        Q_EMIT activityChanged();
    }
}

void OverlayState::setPointer(const QPointF &point)
{
    if (point != m_pointer) {
        m_pointer = point;
        Q_EMIT pointerMoved();
    }
}

void OverlayState::setPointerOpacity(qreal opacity)
{
    if (!qFuzzyCompare(opacity + 1, m_pointerOpacity + 1)) {
        m_pointerOpacity = opacity;
        Q_EMIT pointerChanged();
    }
}

void OverlayState::setMoving(bool moving)
{
    if (moving != m_moving) {
        m_moving = moving;
        Q_EMIT pointerChanged();
    }
}

void OverlayState::fade(qreal to, int duration, std::function<void()> then)
{
    m_fade.stop();
    m_afterFade = std::move(then);
    if (duration <= 0) {
        setPointerOpacity(to);
        if (m_afterFade) {
            auto after = std::move(m_afterFade);
            m_afterFade = nullptr;
            after();
        }
        return;
    }
    m_fade.setStartValue(m_pointerOpacity);
    m_fade.setEndValue(to);
    m_fade.setDuration(duration);
    m_fade.start();
}

QPointF OverlayState::entryPoint(const QPointF &target) const
{
    for (const QRectF &screen : m_screens) {
        if (screen.contains(target))
            return QPointF(screen.center().x(), screen.top() + entryBelowTop);
    }
    // Not on a known screen: it appears where it is going.
    return target;
}

void OverlayState::movePointer(const QPointF &target, bool slow)
{
    m_travel.stop();
    m_fade.stop();
    m_afterFade = nullptr;
    m_target = target;

    const bool first = !m_pointerVisible;
    if (first) {
        m_pointerVisible = true;
        setPointerOpacity(0);
        setPointer(entryPoint(target));
        Q_EMIT pointerChanged();
        updateActive();
    }
    m_from = m_pointer;
    setMoving(true);

    if (m_frozen) {
        // Tests and pictures: no time passes.
        setPointerOpacity(1);
        setPointer(target);
        arrive();
        return;
    }
    if (m_motionFactor <= 0) {
        // Reduced motion: nothing travels. It fades out, reappears at the
        // target and fades in, and the target gets a ring.
        fade(0, first ? 0 : reducedFade, [this] {
            setPointer(m_target);
            fade(1, reducedFade, [this] {
                pulse();
                arrive();
            });
        });
        return;
    }

    const double distance = QLineF(m_from, m_target).length();
    double duration = std::clamp(shortestJourney + distance * millisecondsPerPixel, double(shortestJourney),
                                 double(longestJourney));
    duration *= m_motionFactor * (slow ? 2 : 1);

    // The curve bends to one side of the straight line by a fifth of the
    // distance, at most 120 px.
    const QPointF middle = (m_from + m_target) / 2;
    const QPointF along = m_target - m_from;
    const double bend = std::min(distance * 0.2, 120.0);
    m_bend = distance > 0.5 ? middle + QPointF(along.y(), -along.x()) / distance * bend : middle;

    if (first)
        fade(1, std::min(appearFade, int(duration)), nullptr);
    else
        setPointerOpacity(1);
    m_travel.setDuration(std::max(1, int(std::lround(duration))));
    m_travel.start();
}

void OverlayState::arrive()
{
    setMoving(false);
    for (Highlight &highlight : m_highlights) {
        if (!highlight.shown) {
            highlight.shown = true;
            Q_EMIT highlightsChanged();
        }
    }
    Q_EMIT pointerArrived();
}

void OverlayState::pulse()
{
    if (!m_pointerVisible)
        return;
    m_pulsePoint = m_pointer;
    ++m_pulses;
    Q_EMIT pulsed();
}

void OverlayState::hidePointer()
{
    m_travel.stop();
    m_fade.stop();
    m_afterFade = nullptr;
    const bool changed = m_pointerVisible || m_moving;
    m_pointerVisible = false;
    m_moving = false;
    m_pointerOpacity = 0;
    // A highlight that waited for the pointer does not wait any longer.
    for (Highlight &highlight : m_highlights) {
        if (!highlight.shown) {
            highlight.shown = true;
            Q_EMIT highlightsChanged();
        }
    }
    if (changed)
        Q_EMIT pointerChanged();
    updateActive();
}

uint OverlayState::addHighlight(const QRectF &rect, const QVariantMap &options)
{
    Highlight highlight;
    highlight.id = m_nextId++;
    highlight.rect = rect;
    highlight.dim = options.value(QStringLiteral("dim")).toBool();
    highlight.dimRect = options.value(QStringLiteral("dimRect")).toRectF();
    highlight.caption = options.value(QStringLiteral("caption")).toString();
    highlight.step = options.value(QStringLiteral("step")).toString();
    highlight.shown = !(options.value(QStringLiteral("afterPointer")).toBool() && m_moving);
    m_highlights.append(highlight);
    Q_EMIT highlightsChanged();
    updateActive();
    return highlight.id;
}

bool OverlayState::moveHighlight(uint id, const QRectF &rect, const QRectF &dimRect)
{
    for (Highlight &highlight : m_highlights) {
        if (highlight.id == id) {
            if (highlight.rect != rect || highlight.dimRect != dimRect) {
                highlight.rect = rect;
                highlight.dimRect = dimRect;
                Q_EMIT highlightsChanged();
            }
            return true;
        }
    }
    return false;
}

bool OverlayState::setCaption(uint id, const QString &caption)
{
    for (Highlight &highlight : m_highlights) {
        if (highlight.id == id) {
            if (highlight.caption != caption) {
                highlight.caption = caption;
                Q_EMIT highlightsChanged();
            }
            return true;
        }
    }
    return false;
}

bool OverlayState::removeHighlight(uint id)
{
    for (qsizetype i = 0; i < m_highlights.size(); ++i) {
        if (m_highlights.at(i).id == id) {
            m_highlights.removeAt(i);
            Q_EMIT highlightsChanged();
            updateActive();
            return true;
        }
    }
    return false;
}

bool OverlayState::hasHighlight(uint id) const
{
    return std::any_of(m_highlights.cbegin(), m_highlights.cend(),
                       [id](const Highlight &highlight) { return highlight.id == id; });
}

void OverlayState::clear()
{
    hidePointer();
    if (!m_highlights.isEmpty()) {
        m_highlights.clear();
        Q_EMIT highlightsChanged();
    }
    setActivity(QStringLiteral("idle"), QStringLiteral("window"), QRectF());
    updateActive();
}

QPointF OverlayState::anchorOf(const QRectF &rect, const QString &anchor)
{
    const double inset = std::min(rect.height() / 4, rect.width() / 2);
    if (anchor == QLatin1String("start"))
        return QPointF(rect.left() + inset, rect.center().y());
    if (anchor == QLatin1String("end"))
        return QPointF(rect.right() - inset, rect.center().y());
    return rect.center();
}
