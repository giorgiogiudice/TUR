// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#include "kwinlocator.h"

#include <KWayland/Client/connection_thread.h>
#include <KWayland/Client/plasmawindowmanagement.h>
#include <KWayland/Client/registry.h>

#include <QGuiApplication>
#include <QLoggingCategory>

namespace {
Q_LOGGING_CATEGORY(locatorLog, "tur.pax.overlay.locator", QtInfoMsg)
}

using KWayland::Client::ConnectionThread;
using KWayland::Client::PlasmaWindow;
using KWayland::Client::PlasmaWindowManagement;
using KWayland::Client::Registry;

KWinWindowLocator::KWinWindowLocator(QObject *parent)
    : WindowLocator(parent)
{
    if (QGuiApplication::platformName() != QLatin1String("wayland")) {
        m_problem = QStringLiteral("not a Wayland session (platform %1)").arg(QGuiApplication::platformName());
        return;
    }
    m_connection = ConnectionThread::fromApplication(this);
    if (!m_connection) {
        m_problem = QStringLiteral("no Wayland connection");
        return;
    }
    m_problem = QStringLiteral("the compositor has not offered org_kde_plasma_window_management yet");
    m_registry = new Registry(this);
    m_registry->create(m_connection);
    connect(m_registry, &Registry::plasmaWindowManagementAnnounced, this, [this](quint32 name, quint32 version) {
        if (m_management)
            return;
        m_management = m_registry->createPlasmaWindowManagement(name, version, this);
        m_problem.clear();
        qCInfo(locatorLog) << "org_kde_plasma_window_management version" << version;
        connect(m_management, &PlasmaWindowManagement::windowCreated, this, &KWinWindowLocator::watch);
        connect(m_management, &PlasmaWindowManagement::stackingOrderUuidsChanged, this,
                [this] { Q_EMIT changed(QString()); });
        const auto existing = m_management->windows();
        for (PlasmaWindow *window : existing)
            watch(window);
    });
    connect(m_registry, &Registry::interfacesAnnounced, this, [this] {
        if (!m_management) {
            // KWin 6.8 refuses the interface to sandboxed clients only
            // (src/wayland_server.cpp, KWinDisplay::allowInterface).
            m_problem = QStringLiteral("the compositor does not offer org_kde_plasma_window_management to this "
                                       "program (is it sandboxed, or is the compositor not KWin?)");
            qCWarning(locatorLog) << m_problem;
        }
    });
    m_registry->setup();
}

bool KWinWindowLocator::available() const
{
    return m_management != nullptr;
}

QString KWinWindowLocator::mechanism() const
{
    if (!m_problem.isEmpty())
        return QStringLiteral("windows cannot be found: ") + m_problem;
    return QStringLiteral("windows found with the Plasma window-management protocol");
}

WindowPlace KWinWindowLocator::placeOf(const PlasmaWindow *window)
{
    WindowPlace place;
    place.uuid = QString::fromUtf8(window->uuid());
    place.frame = QRectF(window->geometry());
    const QRect client = window->clientGeometry();
    place.client = client.isValid() ? QRectF(client) : place.frame;
    place.minimized = window->isMinimized();
    place.active = window->isActive();
    place.pid = window->pid();
    place.appId = window->appId();
    place.title = window->title();
    return place;
}

void KWinWindowLocator::watch(PlasmaWindow *window)
{
    if (!window)
        return;
    const QString uuid = QString::fromUtf8(window->uuid());
    if (m_windows.contains(uuid))
        return;
    m_windows.insert(uuid, window);
    const WindowPlace place = placeOf(window);
    qCInfo(locatorLog).noquote() << "window" << uuid << "app id" << place.appId << "pid" << place.pid << "title"
                                 << place.title << "client" << place.client << "frame" << place.frame;
    const auto report = [this, uuid] { Q_EMIT changed(uuid); };
    connect(window, &PlasmaWindow::geometryChanged, this, report);
    connect(window, &PlasmaWindow::clientGeometryChanged, this, report);
    connect(window, &PlasmaWindow::minimizedChanged, this, report);
    connect(window, &PlasmaWindow::activeChanged, this, report);
    connect(window, &PlasmaWindow::unmapped, this, [this, uuid] {
        m_windows.remove(uuid);
        Q_EMIT removed(uuid);
    });
}

QStringList KWinWindowLocator::stackingOrder() const
{
    QStringList order;
    if (!m_management)
        return order;
    const auto uuids = m_management->stackingOrderUuids();
    for (const QByteArray &uuid : uuids)
        order.append(QString::fromUtf8(uuid));
    return order;
}

std::optional<WindowPlace> KWinWindowLocator::find(uint pid, const QString &appId, const QString &title) const
{
    const QStringList order = stackingOrder();
    std::optional<WindowPlace> best;
    qsizetype bestRank = -2;
    for (auto it = m_windows.cbegin(); it != m_windows.cend(); ++it) {
        const PlasmaWindow *window = it.value().data();
        if (!window)
            continue;
        if (pid != 0 && window->pid() != pid)
            continue;
        if (!appId.isEmpty() && window->appId() != appId)
            continue;
        if (!title.isEmpty() && window->title() != title)
            continue;
        // The stacking order runs from the bottom to the top.
        const qsizetype rank = order.indexOf(it.key());
        if (!best || rank > bestRank) {
            best = placeOf(window);
            bestRank = rank;
        }
    }
    return best;
}

std::optional<WindowPlace> KWinWindowLocator::place(const QString &uuid) const
{
    const auto it = m_windows.constFind(uuid);
    if (it == m_windows.cend() || !it.value())
        return std::nullopt;
    return placeOf(it.value().data());
}

bool KWinWindowLocator::covered(const QString &uuid, const QRectF &rect) const
{
    const QStringList order = stackingOrder();
    const qsizetype at = order.indexOf(uuid);
    if (at < 0)
        return false;
    for (qsizetype i = at + 1; i < order.size(); ++i) {
        const auto it = m_windows.constFind(order.at(i));
        if (it == m_windows.cend() || !it.value())
            continue;
        const PlasmaWindow *above = it.value().data();
        if (!above->isMinimized() && QRectF(above->geometry()).intersects(rect))
            return true;
    }
    return false;
}

QList<WindowPlace> KWinWindowLocator::windows() const
{
    QList<WindowPlace> out;
    for (auto it = m_windows.cbegin(); it != m_windows.cend(); ++it) {
        if (it.value())
            out.append(placeOf(it.value().data()));
    }
    return out;
}
