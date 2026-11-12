// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#pragma once

#include "windowlocator.h"

#include <QHash>
#include <QPointer>

namespace KWayland::Client {
class ConnectionThread;
class PlasmaWindow;
class PlasmaWindowManagement;
class Registry;
} // namespace KWayland::Client

// Finds windows through the Plasma window-management protocol
// (org_kde_plasma_window_management), which KWin 6.8 offers to every client
// that is not sandboxed, read with KWayland's client library (LGPL). It is
// the one place in TUR that reads another program's window geometry, so that
// it can be replaced when a supported interface exists
// (docs/architecture/tur-pax-overlay.md, section 4).
//
// What it is given by KWin, per window: an identifier, the process
// identifier, the application identifier, the title, the frame geometry and
// the client-area geometry in global logical pixels, whether it is minimised
// or active, and the stacking order of all windows.
class KWinWindowLocator : public WindowLocator
{
    Q_OBJECT

public:
    explicit KWinWindowLocator(QObject *parent = nullptr);

    bool available() const override;
    QString mechanism() const override;
    std::optional<WindowPlace> find(uint pid, const QString &appId, const QString &title) const override;
    std::optional<WindowPlace> place(const QString &uuid) const override;
    bool covered(const QString &uuid, const QRectF &rect) const override;

    // Every window KWin reported, for the log.
    QList<WindowPlace> windows() const;

private:
    void watch(KWayland::Client::PlasmaWindow *window);
    static WindowPlace placeOf(const KWayland::Client::PlasmaWindow *window);
    QStringList stackingOrder() const;

    KWayland::Client::ConnectionThread *m_connection = nullptr;
    KWayland::Client::Registry *m_registry = nullptr;
    KWayland::Client::PlasmaWindowManagement *m_management = nullptr;
    QHash<QString, QPointer<KWayland::Client::PlasmaWindow>> m_windows;
    QString m_problem;
};
