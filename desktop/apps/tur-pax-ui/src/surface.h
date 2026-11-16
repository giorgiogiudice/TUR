// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#pragma once

#include <QObject>
#include <QPointer>
#include <QString>
#include <QVariantMap>

class QQuickWindow;

// Makes P@x's surface a layer-shell surface under Wayland
// (desktop-experience.md, 9.2): the top layer, keyboard "on demand", pushing
// no window aside. The ask bar is anchored to the bottom edge of the screen
// the person is working on, above the panel; the conversation panel to the
// right edge, from top to bottom. Under any other platform (the tests'
// offscreen one) the surface stays an ordinary window and this does nothing.
//
// While P@x's character is on the bar (tur-pax-companion; its
// org.tur.PaxCompanion1.Anchor says where), the ask bar opens beside the
// character instead: on the character's screen, centred on it as far as the
// screen allows, just above the bar, where the character's bubble is.
//
// Configured before the surface is first shown, and again each time it
// changes size or place, with the surface hidden for the change.
class Surface : public QObject
{
    Q_OBJECT

public:
    // `root` is Main.qml's object: its `surface` window, `mode` and `ready`.
    // `anchor` is where P@x's character is, if it is on the bar.
    explicit Surface(QObject *root, const QVariantMap &anchor = {}, QObject *parent = nullptr);

    bool layerShell() const { return m_layerShell; }

    // Places the ask bar for `anchor` (empty: the middle of the bottom
    // edge) before it is shown.
    void prepareBar(const QVariantMap &anchor);

Q_SIGNALS:
    // What the window shows changed.
    void modeChanged(const QString &mode);

private Q_SLOTS:
    void apply();

private:
    void place(const QString &mode);

    QObject *m_root;
    QPointer<QQuickWindow> m_window;
    bool m_layerShell = false;
    QString m_applied;
    QVariantMap m_anchor;
};
