// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#pragma once

#include <QObject>
#include <QtQml/qqmlregistration.h>

// Where the character and what opens above it go, on the screen the bar is
// on (docs/architecture/tur-pax-companion.md, section 2). Pure arithmetic,
// in the screen's logical pixels, with distances upwards counted from the
// screen's bottom edge.
//
// The bar's thickness is not asked of Plasma: it is what the compositor
// leaves a layer-shell surface that does not ask to ignore other surfaces'
// exclusive zones (`usableHeight`), taken from the screen's height. The
// character sits inside that strip; with no strip (no bar, or one that
// hides) it rests just above the screen's edge.
class Placement : public QObject
{
    Q_OBJECT
    QML_ELEMENT

    Q_PROPERTY(double screenWidth READ screenWidth WRITE setScreenWidth NOTIFY changed FINAL)
    Q_PROPERTY(double screenHeight READ screenHeight WRITE setScreenHeight NOTIFY changed FINAL)
    // The height the compositor leaves beside the bar; 0 or less: not known.
    Q_PROPERTY(double usableHeight READ usableHeight WRITE setUsableHeight NOTIFY changed FINAL)
    // The side of the character's box when it has room.
    Q_PROPERTY(double box READ box WRITE setBox NOTIFY changed FINAL)
    // Along the bar, 0 at its start and 1 at its end.
    Q_PROPERTY(double along READ along WRITE setAlong NOTIFY changed FINAL)
    Q_PROPERTY(bool rightToLeft READ rightToLeft WRITE setRightToLeft NOTIFY changed FINAL)

    // The bar's thickness.
    Q_PROPERTY(double inset READ inset NOTIFY changed FINAL)
    // The side of the character's box: `box`, or less in a thin bar.
    Q_PROPERTY(double size READ size NOTIFY changed FINAL)
    // The left edge of the character's box.
    Q_PROPERTY(double characterX READ characterX NOTIFY changed FINAL)
    // From the screen's bottom edge to the bottom of the character's box.
    Q_PROPERTY(double characterLift READ characterLift NOTIFY changed FINAL)
    // From the screen's bottom edge to the bottom of what opens above the
    // character: clear of the bar and of the character.
    Q_PROPERTY(double popupLift READ popupLift NOTIFY changed FINAL)

public:
    // Kept clear at each end of the bar, and between the bar and what opens
    // above it.
    static constexpr double margin = 8;
    // A bar thinner than this is not one the character can sit in.
    static constexpr double thinnest = 24;
    // Within this distance of an end, a dragged character goes to the end.
    static constexpr double snap = 24;

    explicit Placement(QObject *parent = nullptr);

    double screenWidth() const { return m_screenWidth; }
    void setScreenWidth(double width);
    double screenHeight() const { return m_screenHeight; }
    void setScreenHeight(double height);
    double usableHeight() const { return m_usableHeight; }
    void setUsableHeight(double height);
    double box() const { return m_box; }
    void setBox(double box);
    double along() const { return m_along; }
    void setAlong(double along);
    bool rightToLeft() const { return m_rightToLeft; }
    void setRightToLeft(bool rightToLeft);

    double inset() const;
    double size() const;
    double characterX() const;
    double characterLift() const;
    double popupLift() const;

    // The left edge of something `width` wide that opens above the
    // character: centred on it, and kept on the screen.
    Q_INVOKABLE double popupX(double width) const;
    // The place along the bar of a character whose box's left edge is at
    // `x`: what a drag ends with.
    Q_INVOKABLE double alongAt(double x) const;
    // The left edge of the box at that place along the bar.
    Q_INVOKABLE double xAt(double along) const;

Q_SIGNALS:
    void changed();

private:
    double travel() const;

    double m_screenWidth = 0;
    double m_screenHeight = 0;
    double m_usableHeight = 0;
    double m_box = 40;
    double m_along = 1;
    bool m_rightToLeft = false;
};
