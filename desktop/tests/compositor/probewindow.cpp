// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#include "probewindow.h"

#include <QKeyEvent>
#include <QMouseEvent>
#include <QPainter>

ProbeWindow::ProbeWindow()
{
    setTitle(QStringLiteral("TUR compositor probe"));
    resize(480, 320);
}

void ProbeWindow::paintEvent(QPaintEvent *)
{
    QPainter painter(this);
    // A plain grey that is neither gold nor any colour the overlay draws.
    painter.fillRect(QRect(QPoint(0, 0), size()), QColor(0x5A, 0x63, 0x70));
}

void ProbeWindow::keyPressEvent(QKeyEvent *event)
{
    typed += event->text();
}

void ProbeWindow::mousePressEvent(QMouseEvent *event)
{
    clicks.append(event->position());
}

bool ProbeWindow::event(QEvent *event)
{
    if (event->type() == QEvent::FocusOut)
        ++deactivations;
    return QRasterWindow::event(event);
}
