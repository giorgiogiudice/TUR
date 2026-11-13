// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#pragma once

#include <QObject>
#include <QPointer>
#include <QString>

class QQuickWindow;

// Makes P@x's surface a layer-shell surface under Wayland
// (desktop-experience.md, 9.2): the top layer, keyboard "on demand", pushing
// no window aside. The ask bar is anchored to the bottom edge of the screen
// the person is working on, above the panel; the conversation panel to the
// right edge, from top to bottom. Under any other platform (the tests'
// offscreen one) the surface stays an ordinary window and this does nothing.
//
// Configured before the surface is first shown, and again each time it
// changes size or place, with the surface hidden for the change.
class Surface : public QObject
{
    Q_OBJECT

public:
    // `root` is Main.qml's object: its `surface` window, `mode` and `ready`.
    Surface(QObject *root, QObject *parent = nullptr);

    bool layerShell() const { return m_layerShell; }

private Q_SLOTS:
    void apply();

private:
    QObject *m_root;
    QPointer<QQuickWindow> m_window;
    bool m_layerShell = false;
    QString m_applied;
};
