// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#pragma once

#include <QHash>
#include <QObject>
#include <QPointer>
#include <QStringList>

#include <memory>

class OverlayService;
class OverlayState;
class QQmlEngine;
class QQuickView;
class QScreen;

// One surface per screen (design, 9.3). Under Wayland each is a layer-shell
// surface on the overlay layer, KWin's topmost, anchored to every edge of
// its output, with an empty input region (Qt sends one for
// Qt::WindowTransparentForInput) and no keyboard interactivity, so that
// every pixel of it lets input through and it never takes the keyboard.
//
// A surface is mapped only while there is something to draw, and is unmapped
// the moment there is not. Screens that come and go are followed.
class SurfaceHost : public QObject
{
    Q_OBJECT

public:
    SurfaceHost(QQmlEngine *engine, OverlayState *state, OverlayService *service, QObject *parent = nullptr);
    ~SurfaceHost() override;

    // Whether the surfaces are layer-shell surfaces (under Wayland) or plain
    // windows (any other platform: the tests' offscreen platform).
    bool layerShell() const { return m_layerShell; }
    // Problems loading the surface's QML, for --check.
    QStringList problems() const { return m_problems; }

private:
    struct Surface
    {
        QPointer<QScreen> screen;
        std::unique_ptr<QQuickView> view;
        bool drawn = false;
    };

    void addScreen(QScreen *screen);
    void removeScreen(QScreen *screen);
    void sync();
    void report();

    QQmlEngine *m_engine;
    OverlayState *m_state;
    OverlayService *m_service;
    bool m_layerShell = false;
    QHash<QScreen *, Surface *> m_surfaces;
    QStringList m_problems;
};
