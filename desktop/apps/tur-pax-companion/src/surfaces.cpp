// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#include "surfaces.h"

#include "companionservice.h"

#include <LayerShellQt/window.h>

#include <QGuiApplication>
#include <QJSValue>
#include <QMouseEvent>
#include <QTextStream>
#include <QQuickWindow>
#include <QRectF>
#include <QRegion>
#include <QVariant>
#include <QVariantList>
#include <QVariantMap>

namespace {

// A `var` property of a QML object holds a JavaScript value; this is what
// it holds, as Qt's own types.
QVariant plain(const QVariant &value)
{
    if (value.metaType() == QMetaType::fromType<QJSValue>())
        return value.value<QJSValue>().toVariant();
    return value;
}

} // namespace

Surfaces::Surfaces(QObject *root, CompanionService *service, QObject *parent)
    : QObject(parent)
    , m_root(root)
    , m_service(service)
    , m_layerShell(QGuiApplication::platformName() == QLatin1String("wayland"))
    , m_trace(qEnvironmentVariableIsSet("TUR_COMPANION_TRACE"))
{
    m_strip = qobject_cast<QQuickWindow *>(root->property("strip").value<QObject *>());
    m_probe = qobject_cast<QQuickWindow *>(root->property("probe").value<QObject *>());

    if (m_strip)
        m_strip->setFlags(Qt::FramelessWindowHint | Qt::WindowStaysOnTopHint);
    if (m_probe) {
        m_probe->setFlags(Qt::FramelessWindowHint | Qt::WindowTransparentForInput | Qt::WindowDoesNotAcceptFocus);
    }

    if (m_layerShell && m_strip) {
        LayerShellQt::Window *layer = LayerShellQt::Window::get(m_strip);
        layer->setAnchors(LayerShellQt::Window::Anchors(LayerShellQt::Window::AnchorBottom)
                          | LayerShellQt::Window::AnchorLeft | LayerShellQt::Window::AnchorRight);
        // To the screen's own edge, through the bar's reserved strip, and
        // pushing nothing aside.
        layer->setExclusiveZone(-1);
        layer->setKeyboardInteractivity(LayerShellQt::Window::KeyboardInteractivityNone);
        layer->setActivateOnShow(false);
        layer->setWantsToBeOnActiveScreen(false);
        layer->setScope(QStringLiteral("tur-pax-companion"));
    }
    if (m_layerShell && m_probe) {
        LayerShellQt::Window *layer = LayerShellQt::Window::get(m_probe);
        layer->setLayer(LayerShellQt::Window::LayerBackground);
        layer->setAnchors(LayerShellQt::Window::Anchors(LayerShellQt::Window::AnchorTop)
                          | LayerShellQt::Window::AnchorBottom | LayerShellQt::Window::AnchorLeft);
        // Zero: placed beside the surfaces that reserve room, the bar among
        // them, and reserving none itself.
        layer->setExclusiveZone(0);
        layer->setKeyboardInteractivity(LayerShellQt::Window::KeyboardInteractivityNone);
        layer->setActivateOnShow(false);
        layer->setWantsToBeOnActiveScreen(false);
        layer->setScope(QStringLiteral("tur-pax-companion-probe"));
    }

    if (m_trace && m_strip)
        m_strip->installEventFilter(this);
    connect(root, SIGNAL(hitRectsChanged()), this, SLOT(applyMask()));
    connect(root, SIGNAL(attendedChanged()), this, SLOT(applyKeyboard()));
    connect(root, SIGNAL(raisedChanged()), this, SLOT(applyLayer()));
    connect(root, SIGNAL(anchorChanged()), this, SLOT(pushAnchor()));
    applyLayer();
    applyMask();
    pushAnchor();

    root->setProperty("layered", m_layerShell);
    root->setProperty("ready", true);
}

void Surfaces::applyMask()
{
    if (!m_strip)
        return;
    QRegion region;
    const QVariantList rects = plain(m_root->property("hitRects")).toList();
    for (const QVariant &rect : rects)
        region += rect.toRectF().toAlignedRect();
    // An empty mask would mean "all of it". There is always the character;
    // if there ever were nothing, a point outside the strip takes nothing.
    if (region.isEmpty())
        region = QRegion(QRect(-2, -2, 1, 1));
    m_strip->setMask(region);
    if (m_trace) {
        QTextStream out(stderr);
        out << "tur-pax-companion: trace: strip " << m_strip->width() << " x " << m_strip->height()
            << ", input region";
        for (const QRect &rect : region)
            out << ' ' << rect.x() << ',' << rect.y() << ' ' << rect.width() << 'x' << rect.height();
        out << '\n';
    }
}

bool Surfaces::eventFilter(QObject *watched, QEvent *event)
{
    const QEvent::Type type = event->type();
    if (type == QEvent::MouseButtonPress || type == QEvent::MouseButtonRelease
        || type == QEvent::MouseButtonDblClick) {
        const auto *mouse = static_cast<QMouseEvent *>(event);
        QTextStream(stderr) << "tur-pax-companion: trace: "
                            << (type == QEvent::MouseButtonPress     ? "press"
                                : type == QEvent::MouseButtonRelease ? "release"
                                                                     : "double click")
                            << " at " << mouse->position().x() << ',' << mouse->position().y() << " in a strip "
                            << m_strip->width() << " x " << m_strip->height() << '\n';
    }
    return QObject::eventFilter(watched, event);
}

void Surfaces::applyKeyboard()
{
    if (!m_strip)
        return;
    const bool attended = m_root->property("attended").toBool();
    if (m_layerShell) {
        LayerShellQt::Window *layer = LayerShellQt::Window::get(m_strip);
        layer->setKeyboardInteractivity(attended ? LayerShellQt::Window::KeyboardInteractivityExclusive
                                                 : LayerShellQt::Window::KeyboardInteractivityNone);
    }
    if (attended)
        m_strip->requestActivate();
}

void Surfaces::applyLayer()
{
    if (!m_strip || !m_layerShell)
        return;
    const bool raised = m_root->property("raised").toBool();
    LayerShellQt::Window::get(m_strip)->setLayer(raised ? LayerShellQt::Window::LayerOverlay
                                                        : LayerShellQt::Window::LayerTop);
}

void Surfaces::pushAnchor()
{
    if (m_service)
        m_service->setAnchor(plain(m_root->property("anchor")).toMap());
}
