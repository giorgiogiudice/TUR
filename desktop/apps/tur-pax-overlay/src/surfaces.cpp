// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#include "surfaces.h"

#include "overlayservice.h"
#include "overlaystate.h"

#include <LayerShellQt/window.h>

#include <QGuiApplication>
#include <QMap>
#include <QQmlEngine>
#include <QQmlError>
#include <QQuickItem>
#include <QQuickView>
#include <QRectF>
#include <QScreen>

SurfaceHost::SurfaceHost(QQmlEngine *engine, OverlayState *state, OverlayService *service, QObject *parent)
    : QObject(parent)
    , m_engine(engine)
    , m_state(state)
    , m_service(service)
    , m_layerShell(QGuiApplication::platformName() == QLatin1String("wayland"))
{
    const auto screens = QGuiApplication::screens();
    for (QScreen *screen : screens)
        addScreen(screen);
    connect(qGuiApp, &QGuiApplication::screenAdded, this, &SurfaceHost::addScreen);
    connect(qGuiApp, &QGuiApplication::screenRemoved, this, &SurfaceHost::removeScreen);
    connect(m_state, &OverlayState::activeChanged, this, &SurfaceHost::sync);
    sync();
}

SurfaceHost::~SurfaceHost()
{
    qDeleteAll(m_surfaces);
}

void SurfaceHost::addScreen(QScreen *screen)
{
    if (!screen || m_surfaces.contains(screen))
        return;
    auto *surface = new Surface;
    surface->screen = screen;
    surface->view = std::make_unique<QQuickView>(m_engine, nullptr);
    QQuickView *view = surface->view.get();

    view->setTitle(QStringLiteral("P@x"));
    view->setColor(Qt::transparent);
    // Every pixel lets input through (Qt sends the compositor an empty input
    // region for WindowTransparentForInput), and the window never asks for
    // the keyboard.
    view->setFlags(Qt::FramelessWindowHint | Qt::WindowTransparentForInput | Qt::WindowDoesNotAcceptFocus
                   | Qt::WindowStaysOnTopHint);
    view->setScreen(screen);
    view->setGeometry(screen->geometry());
    view->setResizeMode(QQuickView::SizeRootObjectToView);
    view->setInitialProperties({
        { QStringLiteral("overlay"), QVariant::fromValue(m_state) },
        { QStringLiteral("screenName"), screen->name() },
        { QStringLiteral("screenGeometry"), QRectF(screen->geometry()) },
    });
    view->loadFromModule("Tur.PaxOverlay", "OverlaySurface");
    const auto errors = view->errors();
    for (const QQmlError &error : errors)
        m_problems.append(error.toString());

    if (m_layerShell) {
        LayerShellQt::Window *layer = LayerShellQt::Window::get(view);
        layer->setLayer(LayerShellQt::Window::LayerOverlay);
        layer->setAnchors(LayerShellQt::Window::Anchors(LayerShellQt::Window::AnchorTop)
                          | LayerShellQt::Window::AnchorBottom | LayerShellQt::Window::AnchorLeft
                          | LayerShellQt::Window::AnchorRight);
        // Not pushed aside by panels, and pushes nothing aside.
        layer->setExclusiveZone(-1);
        layer->setKeyboardInteractivity(LayerShellQt::Window::KeyboardInteractivityNone);
        layer->setActivateOnShow(false);
        layer->setWantsToBeOnActiveScreen(false);
        layer->setScreen(screen);
        layer->setScope(QStringLiteral("tur-pax-overlay"));
    }

    // A surface counts as shown once a frame of it has reached the
    // compositor.
    connect(view, &QQuickWindow::frameSwapped, this, [this, surface] {
        if (!surface->drawn && surface->view->isVisible()) {
            surface->drawn = true;
            report();
        }
    });
    connect(screen, &QScreen::geometryChanged, this, [this, surface](const QRect &geometry) {
        surface->view->setGeometry(geometry);
        if (QQuickItem *root = surface->view->rootObject())
            root->setProperty("screenGeometry", QRectF(geometry));
        report();
    });

    m_surfaces.insert(screen, surface);
    sync();
}

void SurfaceHost::removeScreen(QScreen *screen)
{
    Surface *surface = m_surfaces.take(screen);
    delete surface;
    report();
}

void SurfaceHost::sync()
{
    const bool wanted = m_state->active();
    for (Surface *surface : std::as_const(m_surfaces)) {
        QQuickView *view = surface->view.get();
        if (wanted && !view->isVisible()) {
            surface->drawn = false;
            view->show();
        } else if (!wanted && view->isVisible()) {
            view->hide();
            surface->drawn = false;
        }
    }
    report();
}

void SurfaceHost::report()
{
    QMap<QString, QRectF> outputs;
    QStringList drawn;
    for (const Surface *surface : std::as_const(m_surfaces)) {
        if (!surface->screen)
            continue;
        const QString name = surface->screen->name();
        outputs.insert(name, QRectF(surface->screen->geometry()));
        if (surface->drawn && surface->view->isVisible())
            drawn.append(name);
    }
    m_service->setOutputs(outputs);
    m_service->setDrawnScreens(drawn);
}
