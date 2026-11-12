// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#pragma once

#include <QList>
#include <QPointF>
#include <QRasterWindow>
#include <QString>

// A plain window of the test's own, that writes down every key and every
// click it is given: the witness that input passed through the overlay to
// the window under it, and that the keyboard never left it.
class ProbeWindow : public QRasterWindow
{
    Q_OBJECT

public:
    ProbeWindow();

    QString typed;
    QList<QPointF> clicks;
    // How many times the window lost the keyboard.
    int deactivations = 0;

protected:
    void paintEvent(QPaintEvent *event) override;
    void keyPressEvent(QKeyEvent *event) override;
    void mousePressEvent(QMouseEvent *event) override;
    bool event(QEvent *event) override;
};
