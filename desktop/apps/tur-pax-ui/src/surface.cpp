// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#include "surface.h"

#include <LayerShellQt/window.h>

#include <QGuiApplication>
#include <QMargins>
#include <QQuickWindow>
#include <QRect>
#include <QScreen>
#include <QVariant>

namespace {

// How far above the bottom edge the ask bar sits: above the panel of the
// desktop (desktop-experience.md, 7.1: 48 px, floating) with room to spare.
constexpr int barMargin = 96;

} // namespace

Surface::Surface(QObject *root, QObject *parent)
    : QObject(parent)
    , m_root(root)
    , m_layerShell(QGuiApplication::platformName() == QLatin1String("wayland"))
{
    m_window = qobject_cast<QQuickWindow *>(root->property("surface").value<QObject *>());
    if (m_window && m_layerShell) {
        LayerShellQt::Window *layer = LayerShellQt::Window::get(m_window);
        layer->setLayer(LayerShellQt::Window::LayerTop);
        // The bar takes the keyboard when it is shown, and gives it back
        // when the person clicks elsewhere.
        layer->setKeyboardInteractivity(LayerShellQt::Window::KeyboardInteractivityOnDemand);
        // Pushes no window aside (9.2: "unless the user pins it").
        layer->setExclusiveZone(0);
        layer->setScope(QStringLiteral("tur-pax-ui"));
        layer->setWantsToBeOnActiveScreen(true);
        layer->setActivateOnShow(true);
    }
    connect(root, SIGNAL(modeChanged()), this, SLOT(apply()));
    apply();
    root->setProperty("ready", true);
}

void Surface::apply()
{
    if (!m_window || !m_layerShell)
        return;
    const QString mode = m_root->property("mode").toString();
    QString shape;
    if (mode == QLatin1String("ask"))
        shape = QStringLiteral("bar");
    else if (mode == QLatin1String("panel") || mode == QLatin1String("activity"))
        shape = QStringLiteral("panel");
    if (shape.isEmpty() || shape == m_applied)
        return;

    // A layer-shell surface is placed when it is mapped: hidden for the
    // change, shown again after it.
    const bool shown = m_window->isVisible();
    if (shown)
        m_window->hide();
    LayerShellQt::Window *layer = LayerShellQt::Window::get(m_window);
    if (shape == QLatin1String("bar")) {
        layer->setAnchors(LayerShellQt::Window::Anchors(LayerShellQt::Window::AnchorBottom));
        layer->setMargins(QMargins(0, 0, 0, barMargin));
    } else {
        layer->setAnchors(LayerShellQt::Window::Anchors(LayerShellQt::Window::AnchorTop)
                          | LayerShellQt::Window::AnchorBottom | LayerShellQt::Window::AnchorRight);
        layer->setMargins(QMargins(0, 0, 0, 0));
        if (QScreen *screen = m_window->screen())
            m_window->setHeight(screen->availableGeometry().height());
    }
    m_applied = shape;
    if (shown)
        m_window->show();
}
