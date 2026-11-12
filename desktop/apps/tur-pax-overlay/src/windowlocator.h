// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#pragma once

#include <QObject>
#include <QRectF>
#include <QString>

#include <optional>

// Where another program's window is on the screen.
//
// Under Wayland a client knows positions only inside its own surfaces: a
// program cannot learn where another program's window is, and cannot even
// learn where its own window is. Only the compositor knows. The overlay asks
// it through the Plasma window-management protocol (kwinlocator.cpp, in the
// program); see docs/architecture/tur-pax-overlay.md, section 4. The tests
// give the overlay a locator of their own.
struct WindowPlace
{
    // The compositor's identifier for the window.
    QString uuid;
    // The window's content (its client area), in global logical pixels. A
    // rectangle "in the window" is relative to this one's top left corner.
    QRectF client;
    // The window with its decoration, in global logical pixels.
    QRectF frame;
    bool minimized = false;
    bool active = false;
    uint pid = 0;
    QString appId;
    QString title;
};

class WindowLocator : public QObject
{
    Q_OBJECT

public:
    using QObject::QObject;

    // Whether this locator can find windows at all. A locator that cannot
    // fails every lookup, and says why in mechanism().
    virtual bool available() const = 0;
    // How windows are found, in a few words, for the log and the docs.
    virtual QString mechanism() const = 0;
    // The window that matches. A pid of 0, an empty application identifier
    // or an empty title match anything. When several windows match, the one
    // stacked highest. Nothing if none matches.
    virtual std::optional<WindowPlace> find(uint pid, const QString &appId, const QString &title) const = 0;
    // The window with this identifier, if it is still there.
    virtual std::optional<WindowPlace> place(const QString &uuid) const = 0;
    // Whether a window stacked above `uuid` overlaps `rect` (global).
    virtual bool covered(const QString &uuid, const QRectF &rect) const = 0;

Q_SIGNALS:
    // The window moved, changed size, was minimised or restored, or the
    // stacking order changed (then uuid is empty: any window may be covered).
    void changed(const QString &uuid);
    // The window is gone.
    void removed(const QString &uuid);
};

// A locator that finds nothing: where there is no compositor interface, as
// under the offscreen platform of the tests.
class NoWindowLocator : public WindowLocator
{
    Q_OBJECT

public:
    explicit NoWindowLocator(const QString &why, QObject *parent = nullptr);

    bool available() const override { return false; }
    QString mechanism() const override { return m_why; }
    std::optional<WindowPlace> find(uint, const QString &, const QString &) const override { return std::nullopt; }
    std::optional<WindowPlace> place(const QString &) const override { return std::nullopt; }
    bool covered(const QString &, const QRectF &) const override { return false; }

private:
    QString m_why;
};
