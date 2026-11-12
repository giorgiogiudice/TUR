// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#pragma once

#include "bustypes.h"

#include <QDBusConnection>
#include <QHash>
#include <QList>
#include <QMap>
#include <QObject>
#include <QPointF>
#include <QRectF>
#include <QSet>
#include <QSizeF>
#include <QString>
#include <QStringList>
#include <QVariant>
#include <QVariantMap>

#include <optional>

class OverlayState;
class QDBusServiceWatcher;
class WindowLocator;
struct WindowPlace;

// org.tur.PaxOverlay1: who may draw, and what. It holds the leases, checks
// every call against the connection that made it, turns a window and a
// rectangle in it into a place on the screen with the WindowLocator, keeps
// that place up to date while the window moves, and tells OverlayState what
// to draw. The adaptor (busadaptor.h) passes the bus calls to it.
//
// What it does not do: decide whether a caller may take a lease. In this
// version anyone on the session bus can (docs/architecture/tur-pax-overlay.md,
// section 6).
class OverlayService : public QObject
{
    Q_OBJECT

public:
    OverlayService(OverlayState *state, WindowLocator *locator, QObject *parent = nullptr);
    ~OverlayService() override;

    // The answer to one call: a value, or an error with its D-Bus name and a
    // message that begins with the error's code.
    struct Outcome
    {
        bool ok = true;
        QString errorName;
        QString errorMessage;
        QVariant value;
    };

    // Puts the object, then the name, on `bus`. On failure, says why.
    bool registerOn(const QDBusConnection &bus, QString *problem);
    QDBusConnection bus() const { return m_bus; }

    // Shown and Screens, the interface's two properties.
    bool shown() const { return m_shown; }
    QStringList screens() const { return m_drawn; }

    // From the surfaces: every output by name with its geometry (global,
    // logical), and the ones on which the overlay's surface has been drawn.
    void setOutputs(const QMap<QString, QRectF> &outputs);
    void setDrawnScreens(const QStringList &drawn);

    Outcome acquire(const QString &caller, const QString &purpose);
    Outcome release(const QString &caller, const QString &lease);
    Outcome setActivity(const QString &caller, const QString &lease, const QString &state, const QVariantMap &detail);
    Outcome movePointer(const QString &caller, const QString &lease, const QString &screen, double x, double y,
                        const QVariantMap &options);
    Outcome pointAt(const QString &caller, const QString &lease, const QString &screen, const PaxOverlay::RectD &rect,
                    const QVariantMap &options);
    Outcome pointInWindow(const QString &caller, const QString &lease, uint pid, const QString &appId,
                          const QString &title, const PaxOverlay::RectD &rect, const QVariantMap &options);
    Outcome pulsePointer(const QString &caller, const QString &lease, const QString &kind);
    Outcome hidePointer(const QString &caller, const QString &lease);
    Outcome highlightRect(const QString &caller, const QString &lease, const QString &screen,
                          const PaxOverlay::RectD &rect, const QVariantMap &options);
    Outcome highlightInWindow(const QString &caller, const QString &lease, uint pid, const QString &appId,
                              const QString &title, const PaxOverlay::RectD &rect, const QVariantMap &options);
    Outcome setCaption(const QString &caller, const QString &lease, uint id, const QString &text,
                       const QStringList &controls);
    Outcome clear(const QString &caller, const QString &lease, uint id);
    // Hide(): every lease ends, nothing is left on screen.
    void hide();

    // Limits, so that one caller cannot fill the screen or the memory.
    static constexpr int maxLeases = 8;
    static constexpr int maxHighlightsPerLease = 16;
    static constexpr int maxTextLength = 300;

Q_SIGNALS:
    void shownChanged(bool shown);
    void screensChanged();
    // For the log: a lease began or ended.
    void leaseBegan(const QString &owner, const QString &purpose);
    void leaseEnded(const QString &owner, const QString &reason);

private:
    struct Lease
    {
        QString token;
        QString owner;
        QString purpose;
        QString activity = QStringLiteral("idle");
        QString scope = QStringLiteral("window");
        // The window the frame goes round, when there is one.
        QString frameWindow;
        QSet<uint> highlights;
    };

    struct TrackedHighlight
    {
        QString lease;
        QString window;
        QRectF inWindow;
        QSizeF clientSize;
        bool dim = false;
    };

    Outcome refuse(int code, const QString &message) const;
    // The lease named, if `caller` holds it.
    Lease *leaseOf(const QString &caller, const QString &token);
    std::optional<QRectF> globalRect(const QString &screen, const PaxOverlay::RectD &rect, Outcome *error) const;
    std::optional<QPointF> globalPoint(const QString &screen, double x, double y, Outcome *error) const;
    std::optional<Outcome> checkText(const QVariantMap &options) const;
    QRectF screenAt(const QRectF &rect) const;

    void endLease(const QString &token, const QString &reason);
    void showLoudest();
    void sendPointer(Lease &lease, const QPointF &target, const QVariantMap &options);
    uint addHighlight(Lease &lease, const QRectF &global, const QRectF &dimRect, const QVariantMap &options);
    void dropHighlight(uint id, const QString &reason);
    void windowChanged(const QString &uuid);
    void windowRemoved(const QString &uuid);
    void ownerLeft(const QString &owner);
    void updateShown();
    void sendTo(const QString &owner, const QString &signal, const QVariantList &arguments);

    OverlayState *m_state;
    WindowLocator *m_locator;
    QDBusConnection m_bus;
    QDBusServiceWatcher *m_watcher = nullptr;
    QHash<QString, Lease> m_leases;
    QHash<uint, TrackedHighlight> m_tracked;
    // The lease whose pointer journey is under way.
    QString m_pointerLease;
    QMap<QString, QRectF> m_outputs;
    QStringList m_drawn;
    bool m_shown = false;
};
