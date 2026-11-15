// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#include "placement.h"

#include <algorithm>

Placement::Placement(QObject *parent)
    : QObject(parent)
{
}

void Placement::setScreenWidth(double width)
{
    if (width == m_screenWidth)
        return;
    m_screenWidth = width;
    Q_EMIT changed();
}

void Placement::setScreenHeight(double height)
{
    if (height == m_screenHeight)
        return;
    m_screenHeight = height;
    Q_EMIT changed();
}

void Placement::setUsableHeight(double height)
{
    if (height == m_usableHeight)
        return;
    m_usableHeight = height;
    Q_EMIT changed();
}

void Placement::setBox(double box)
{
    box = std::max(box, thinnest);
    if (box == m_box)
        return;
    m_box = box;
    Q_EMIT changed();
}

void Placement::setAlong(double along)
{
    along = std::clamp(along, 0.0, 1.0);
    if (along == m_along)
        return;
    m_along = along;
    Q_EMIT changed();
}

void Placement::setRightToLeft(bool rightToLeft)
{
    if (rightToLeft == m_rightToLeft)
        return;
    m_rightToLeft = rightToLeft;
    Q_EMIT changed();
}

double Placement::inset() const
{
    if (m_usableHeight <= 0 || m_usableHeight >= m_screenHeight)
        return 0;
    // More than half the screen is not a bar: something else is reserved.
    const double inset = m_screenHeight - m_usableHeight;
    return inset > m_screenHeight / 2 ? 0 : inset;
}

double Placement::size() const
{
    const double bar = inset();
    if (bar < thinnest)
        return m_box;
    return std::clamp(bar - 4, thinnest, m_box);
}

double Placement::travel() const
{
    return std::max(0.0, m_screenWidth - size() - 2 * margin);
}

double Placement::xAt(double along) const
{
    along = std::clamp(along, 0.0, 1.0);
    const double from = m_rightToLeft ? 1.0 - along : along;
    return margin + from * travel();
}

double Placement::characterX() const
{
    return xAt(m_along);
}

double Placement::alongAt(double x) const
{
    const double room = travel();
    if (room <= 0)
        return 1;
    double from = std::clamp((x - margin) / room, 0.0, 1.0);
    if (from * room <= snap)
        from = 0;
    else if ((1.0 - from) * room <= snap)
        from = 1;
    return m_rightToLeft ? 1.0 - from : from;
}

double Placement::characterLift() const
{
    const double bar = inset();
    if (bar < thinnest)
        return 4;
    return std::max(0.0, (bar - size()) / 2);
}

double Placement::popupLift() const
{
    return std::max(inset(), characterLift() + size()) + margin;
}

double Placement::popupX(double width) const
{
    const double centre = characterX() + size() / 2;
    const double most = std::max(margin, m_screenWidth - width - margin);
    return std::clamp(centre - width / 2, margin, most);
}
