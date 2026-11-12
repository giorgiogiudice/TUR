// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#pragma once

#include "bustypes.h"

#include <QDBusAbstractAdaptor>
#include <QDBusMessage>
#include <QString>
#include <QStringList>
#include <QVariantMap>

class OverlayService;

// org.tur.PaxOverlay1, as dbus/org.tur.PaxOverlay1.xml states it. Written by
// hand and held to the XML by tests; no code is generated from the XML
// (desktop/README.md). Every call is passed to OverlayService with the
// caller's unique bus name, which is what a lease is checked against.
//
// The signals are declared here so that introspection shows them. They are
// not emitted through Qt: OverlayService sends each one to the connection
// that holds the lease, and to nobody else.
class PaxOverlay1Adaptor : public QDBusAbstractAdaptor
{
    Q_OBJECT
    Q_CLASSINFO("D-Bus Interface", "org.tur.PaxOverlay1")
    Q_PROPERTY(bool Shown READ shown)
    Q_PROPERTY(QStringList Screens READ screens)
    Q_PROPERTY(bool CommandsShown READ commandsShown)

public:
    explicit PaxOverlay1Adaptor(OverlayService *service);

    bool shown() const;
    QStringList screens() const;
    bool commandsShown() const;

public Q_SLOTS:
    QString Acquire(const QString &purpose, const QDBusMessage &message);
    void Release(const QString &lease, const QDBusMessage &message);
    void SetActivity(const QString &lease, const QString &state, const QVariantMap &detail,
                     const QDBusMessage &message);
    void MovePointer(const QString &lease, const QString &screen, double x, double y, const QVariantMap &options,
                     const QDBusMessage &message);
    void PointAt(const QString &lease, const QString &screen, const PaxOverlay::RectD &rect,
                 const QVariantMap &options, const QDBusMessage &message);
    void PointInWindow(const QString &lease, uint pid, const QString &app_id, const QString &title,
                       const PaxOverlay::RectD &rect, const QVariantMap &options, const QDBusMessage &message);
    void PulsePointer(const QString &lease, const QString &kind, const QDBusMessage &message);
    void HidePointer(const QString &lease, const QDBusMessage &message);
    uint HighlightRect(const QString &lease, const QString &screen, const PaxOverlay::RectD &rect,
                       const QVariantMap &options, const QDBusMessage &message);
    uint HighlightInWindow(const QString &lease, uint pid, const QString &app_id, const QString &title,
                           const PaxOverlay::RectD &rect, const QVariantMap &options, const QDBusMessage &message);
    void SetCaption(const QString &lease, uint id, const QString &text, const QStringList &controls,
                    const QDBusMessage &message);
    void Clear(const QString &lease, uint id, const QDBusMessage &message);
    void Hide(const QDBusMessage &message);
    void ShowCommands(bool shown);

Q_SIGNALS:
    void PointerArrived(const QString &lease);
    void HighlightLost(uint id, const QString &reason);
    void LeaseEnded(const QString &lease, const QString &reason);
    void StopRequested();
    void PauseRequested();
    void ResumeRequested();
    void GuideControl(uint id, const QString &control);

private:
    OverlayService *m_service;
};
