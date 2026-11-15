// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#pragma once

#include <QObject>
#include <QPointer>

class CompanionService;
class QQuickWindow;

// Makes the character's two windows layer-shell surfaces under Wayland
// (docs/architecture/tur-pax-companion.md, section 2), and keeps four things
// true of the strip the character lives in:
//
//  - Only the character and what is open above it take a click: the
//    surface's input region is those rectangles and nothing else, so a
//    click anywhere else on the strip goes to what is under it.
//  - It has no keyboard, except between the person asking for it
//    (org.tur.PaxCompanion1.Attend) and Esc.
//  - It is on the overlay layer, above a full-screen window, while the
//    person wants it to stay there or P@x acts, sees or waits; otherwise on
//    the top layer, which a full-screen window covers as it covers the bar.
//  - org.tur.PaxCompanion1.Anchor says where the character is.
//
// The probe is one transparent column on the background layer that takes no
// input and respects the bar's exclusive zone: the compositor's answer to
// its height is the height left beside the bar.
//
// Under any other platform (the tests' offscreen one) the windows stay
// ordinary windows and only the input region and the anchor are kept.
class Surfaces : public QObject
{
    Q_OBJECT

public:
    // `root` is Main.qml's object.
    Surfaces(QObject *root, CompanionService *service, QObject *parent = nullptr);

    bool layerShell() const { return m_layerShell; }

private Q_SLOTS:
    void applyMask();
    void applyKeyboard();
    void applyLayer();
    void pushAnchor();

private:
    QObject *m_root;
    CompanionService *m_service;
    QPointer<QQuickWindow> m_strip;
    QPointer<QQuickWindow> m_probe;
    bool m_layerShell = false;
};
