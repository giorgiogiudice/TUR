// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#pragma once

#include <QList>
#include <QObject>
#include <QPointF>
#include <QRectF>
#include <QString>
#include <QVariantAnimation>
#include <QVariantList>
#include <QVariantMap>
#include <QtQml/qqmlregistration.h>

#include <functional>

// What the overlay shows, in global logical pixels, for every screen's
// surface to draw its part of: the activity, the frame, P@x's pointer and the
// highlights. It decides nothing about who may change it (that is
// OverlayService) and knows nothing of windows (the service turns a window
// and a rectangle in it into a place on the screen before it gets here).
//
// P@x's pointer moves here, not in QML: there is one pointer, and every
// screen's surface draws it from the same position, so that it crosses from
// one screen to the next at the same moment on both (design, 9.4).
class OverlayState : public QObject
{
    Q_OBJECT
    QML_ELEMENT

    Q_PROPERTY(QString activity READ activity NOTIFY activityChanged FINAL)
    Q_PROPERTY(QString scope READ scope NOTIFY activityChanged FINAL)
    Q_PROPERTY(QRectF frameRect READ frameRect NOTIFY activityChanged FINAL)
    Q_PROPERTY(bool pointerVisible READ pointerVisible NOTIFY pointerChanged FINAL)
    Q_PROPERTY(QPointF pointer READ pointer NOTIFY pointerMoved FINAL)
    Q_PROPERTY(qreal pointerOpacity READ pointerOpacity NOTIFY pointerChanged FINAL)
    Q_PROPERTY(bool pointerMoving READ pointerMoving NOTIFY pointerChanged FINAL)
    Q_PROPERTY(int pulses READ pulses NOTIFY pulsed FINAL)
    Q_PROPERTY(QPointF pulsePoint READ pulsePoint NOTIFY pulsed FINAL)
    Q_PROPERTY(QVariantList highlights READ highlights NOTIFY highlightsChanged FINAL)
    Q_PROPERTY(bool active READ active NOTIFY activeChanged FINAL)
    Q_PROPERTY(bool on READ on WRITE setOn NOTIFY onChanged FINAL)
    Q_PROPERTY(bool commandsShown READ commandsShown WRITE setCommandsShown NOTIFY commandsShownChanged FINAL)
    Q_PROPERTY(int cursorSize READ cursorSize WRITE setCursorSize NOTIFY cursorSizeChanged FINAL)
    Q_PROPERTY(qreal motionFactor READ motionFactor WRITE setMotionFactor NOTIFY motionChanged FINAL)
    Q_PROPERTY(bool frozen READ frozen WRITE setFrozen NOTIFY motionChanged FINAL)

public:
    explicit OverlayState(QObject *parent = nullptr);

    // The states of the activity indicator (design, 9.3), from the one that
    // tells the person least to the one that tells most.
    static const QStringList &activities();
    static bool isActivity(const QString &name);
    static int loudness(const QString &activity);

    QString activity() const { return m_activity; }
    QString scope() const { return m_scope; }
    QRectF frameRect() const { return m_frameRect; }
    bool pointerVisible() const { return m_pointerVisible; }
    QPointF pointer() const { return m_pointer; }
    qreal pointerOpacity() const { return m_pointerOpacity; }
    bool pointerMoving() const { return m_moving; }
    int pulses() const { return m_pulses; }
    QPointF pulsePoint() const { return m_pulsePoint; }
    QVariantList highlights() const;
    bool active() const;
    // P@x is switched on: somebody holds a lease. While it is, the pill is
    // drawn even when P@x is idle, with the stop combination (the owner's
    // answer of 12 November 2026, desktop-experience.md 17.1, answer 2).
    bool on() const { return m_on; }
    void setOn(bool on);
    // The full list of P@x's keyboard commands is shown under the pill.
    bool commandsShown() const { return m_commandsShown; }
    void setCommandsShown(bool shown);
    int cursorSize() const { return m_cursorSize; }
    void setCursorSize(int size);
    qreal motionFactor() const { return m_motionFactor; }
    void setMotionFactor(qreal factor);
    bool frozen() const { return m_frozen; }
    void setFrozen(bool frozen);

    // Where the screens are, in global logical pixels. The pointer's first
    // appearance starts below the pill of the screen it is going to.
    void setScreens(const QList<QRectF> &screens);
    Q_INVOKABLE void setScreenRects(const QVariantList &rects);

    // The activity, the scope ("window" or "screen") and the rectangle the
    // frame goes round (empty: the screens' edges).
    Q_INVOKABLE void setActivity(const QString &activity, const QString &scope, const QRectF &frame);
    Q_INVOKABLE void setFrameRect(const QRectF &frame);

    // Sends the pointer to `target`: it travels there (design, 9.4).
    Q_INVOKABLE void movePointer(const QPointF &target, bool slow);
    Q_INVOKABLE void pulse();
    Q_INVOKABLE void hidePointer();

    // A highlight. `rect` is the target, without the clearance. Keys of
    // `options`: dim (b), dimRect (rect: what is dimmed; empty: the screen),
    // caption (s), step (s), afterPointer (b: shown when the pointer's
    // current journey ends). Returns its number.
    Q_INVOKABLE uint addHighlight(const QRectF &rect, const QVariantMap &options);
    Q_INVOKABLE bool moveHighlight(uint id, const QRectF &rect, const QRectF &dimRect);
    Q_INVOKABLE bool setCaption(uint id, const QString &caption);
    Q_INVOKABLE bool removeHighlight(uint id);
    Q_INVOKABLE bool hasHighlight(uint id) const;

    // Everything off the screen at once.
    Q_INVOKABLE void clear();

    // Where the tip goes for a rectangle: its centre, or the middle of its
    // left or right edge, inset by a quarter of its height.
    static QPointF anchorOf(const QRectF &rect, const QString &anchor);

Q_SIGNALS:
    void activityChanged();
    void pointerChanged();
    void pointerMoved();
    void pulsed();
    void highlightsChanged();
    void activeChanged();
    void onChanged();
    void commandsShownChanged();
    void cursorSizeChanged();
    void motionChanged();
    // The pointer finished a journey.
    void pointerArrived();

private:
    struct Highlight
    {
        uint id = 0;
        QRectF rect;
        QRectF dimRect;
        bool dim = false;
        QString caption;
        QString step;
        bool shown = true;
    };

    void setPointer(const QPointF &point);
    void setPointerOpacity(qreal opacity);
    void setMoving(bool moving);
    void arrive();
    void fade(qreal to, int duration, std::function<void()> then);
    QPointF entryPoint(const QPointF &target) const;
    void updateActive();

    QString m_activity = QStringLiteral("idle");
    QString m_scope = QStringLiteral("window");
    QRectF m_frameRect;

    bool m_pointerVisible = false;
    QPointF m_pointer;
    QPointF m_from;
    QPointF m_target;
    QPointF m_bend;
    qreal m_pointerOpacity = 0;
    bool m_moving = false;
    int m_pulses = 0;
    QPointF m_pulsePoint;
    QVariantAnimation m_travel;
    QVariantAnimation m_fade;
    std::function<void()> m_afterFade;

    QList<Highlight> m_highlights;
    uint m_nextId = 1;
    bool m_active = false;
    bool m_on = false;
    bool m_commandsShown = false;

    QList<QRectF> m_screens;
    int m_cursorSize = 24;
    qreal m_motionFactor = 1;
    bool m_frozen = false;
};
