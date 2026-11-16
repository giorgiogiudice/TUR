// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#include "surface.h"

#include <LayerShellQt/window.h>

#include <QGuiApplication>
#include <QMargins>
#include <QQuickWindow>
#include <QRect>
#include <QScreen>
#include <QVariant>

#include <algorithm>

namespace {

// How far above the bottom edge the ask bar sits: above the panel of the
// desktop (desktop-experience.md, 7.1: 48 px, floating) with room to spare.
constexpr int barMargin = 96;
// The ask bar's width (Main.qml), and what is kept clear at the screen's
// sides when the bar opens beside P@x's character.
constexpr int barWidth = 640;
constexpr int sideMargin = 8;

QScreen *screenNamed(const QString &name)
{
    const auto screens = QGuiApplication::screens();
    for (QScreen *screen : screens) {
        if (screen->name() == name)
            return screen;
    }
    return nullptr;
}

} // namespace

Surface::Surface(QObject *root, const QVariantMap &anchor, QObject *parent)
    : QObject(parent)
    , m_root(root)
    , m_layerShell(QGuiApplication::platformName() == QLatin1String("wayland"))
    , m_anchor(anchor)
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

void Surface::prepareBar(const QVariantMap &anchor)
{
    m_anchor = anchor;
    place(QStringLiteral("ask"));
}

void Surface::apply()
{
    const QString mode = m_root->property("mode").toString();
    place(mode);
    Q_EMIT modeChanged(mode);
}

void Surface::place(const QString &mode)
{
    if (!m_window || !m_layerShell)
        return;
    QString shape;
    if (mode == QLatin1String("ask"))
        shape = QStringLiteral("bar");
    else if (mode == QLatin1String("panel") || mode == QLatin1String("activity"))
        shape = QStringLiteral("panel");
    if (shape.isEmpty())
        return;

    // Beside P@x's character, when it is on the bar.
    const bool beside = shape == QLatin1String("bar") && m_anchor.value(QStringLiteral("shown")).toBool()
                        && m_anchor.contains(QStringLiteral("x"));
    QScreen *screen = beside ? screenNamed(m_anchor.value(QStringLiteral("screen")).toString()) : nullptr;
    if (!screen)
        screen = m_window->screen();
    int left = 0;
    int lift = barMargin;
    QString placed = shape;
    if (beside && screen) {
        const int width = std::min(barWidth, screen->geometry().width() - 2 * sideMargin);
        const int most = std::max(sideMargin, screen->geometry().width() - width - sideMargin);
        left = std::clamp(int(m_anchor.value(QStringLiteral("x")).toDouble()) - width / 2, sideMargin, most);
        lift = std::max(0, int(m_anchor.value(QStringLiteral("lift")).toDouble()));
        placed = QStringLiteral("bar@%1:%2,%3").arg(screen->name()).arg(left).arg(lift);
    }
    if (placed == m_applied)
        return;

    // A layer-shell surface is placed when it is mapped: hidden for the
    // change, shown again after it.
    const bool shown = m_window->isVisible();
    if (shown)
        m_window->hide();
    LayerShellQt::Window *layer = LayerShellQt::Window::get(m_window);
    if (shape == QLatin1String("bar") && beside && screen) {
        layer->setWantsToBeOnActiveScreen(false);
        m_window->setScreen(screen);
        layer->setAnchors(LayerShellQt::Window::Anchors(LayerShellQt::Window::AnchorBottom)
                          | LayerShellQt::Window::AnchorLeft);
        // The surface does not ask to ignore the bar's reserved strip, so
        // the bottom margin counts from the bar's top edge.
        layer->setMargins(QMargins(left, 0, 0, lift));
    } else if (shape == QLatin1String("bar")) {
        layer->setWantsToBeOnActiveScreen(true);
        layer->setAnchors(LayerShellQt::Window::Anchors(LayerShellQt::Window::AnchorBottom));
        layer->setMargins(QMargins(0, 0, 0, barMargin));
    } else {
        layer->setWantsToBeOnActiveScreen(true);
        layer->setAnchors(LayerShellQt::Window::Anchors(LayerShellQt::Window::AnchorTop)
                          | LayerShellQt::Window::AnchorBottom | LayerShellQt::Window::AnchorRight);
        layer->setMargins(QMargins(0, 0, 0, 0));
        if (QScreen *on = m_window->screen())
            m_window->setHeight(on->availableGeometry().height());
    }
    m_applied = placed;
    if (shown)
        m_window->show();
}
