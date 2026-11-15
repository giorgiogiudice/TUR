// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#pragma once

#include "companion.h"

#include <QDBusConnection>
#include <QDBusContext>
#include <QObject>
#include <QPointer>
#include <QString>
#include <QStringList>
#include <QVariantMap>

// org.tur.PaxCompanion1 (dbus/org.tur.PaxCompanion1.xml): how the person's
// other programs reach P@x's character on the bar. It brings the character
// back, hides it, gives it the keyboard, makes it quiet, and says where it
// is so that P@x's ask bar can open beside it.
//
// It carries no words: nothing can make the character say anything through
// it. What the character says, it has read itself from the system's own
// records (tur-pax-companion.md, section 6).
class CompanionService : public QObject, protected QDBusContext
{
    Q_OBJECT
    Q_CLASSINFO("D-Bus Interface", "org.tur.PaxCompanion1")
    Q_PROPERTY(uint InterfaceVersion READ interfaceVersion CONSTANT FINAL)
    // One of Companion::states().
    Q_PROPERTY(QString State READ state FINAL)
    // On the bar now.
    Q_PROPERTY(bool Shown READ shown FINAL)
    Q_PROPERTY(bool Quiet READ quiet FINAL)
    // Where the character is: "screen" (s, the output's name), "x" (d, the
    // middle of the character from the screen's left edge, logical pixels),
    // "lift" (d, how far above the bar's top edge what opens beside the
    // character should start) and "shown" (b).
    Q_PROPERTY(QVariantMap Anchor READ anchor FINAL)

public:
    static QString serviceName();
    static QString objectPath();
    static QString interfaceName();
    // What SetQuiet takes.
    static QStringList quietModes();

    explicit CompanionService(QObject *parent = nullptr);

    uint interfaceVersion() const { return 1; }
    QString state() const;
    bool shown() const;
    bool quiet() const;
    QVariantMap anchor() const { return m_anchor; }

    // The character these properties are about.
    void setCompanion(Companion *companion);
    // Where it is, from the program.
    void setAnchor(const QVariantMap &anchor);

    // Exports the object and takes the name. False, with the reason, if the
    // name is someone else's.
    bool registerOn(const QDBusConnection &bus, QString *problem);
    // Calls `method` on the character that owns the name on `bus`; for a
    // second start of the program.
    static bool forward(const QDBusConnection &bus, const QString &method, const QVariantList &arguments,
                        QString *problem);

public Q_SLOTS:
    // Puts the character back on the bar.
    void Show();
    // Takes it off the bar until Show.
    void Hide();
    // Gives the character the keyboard: what it is saying, or its menu.
    void Attend();
    // "hour", "tomorrow", "always" or "off".
    void SetQuiet(const QString &mode);

Q_SIGNALS:
    void attendRequested();

private:
    void announce(const QString &property, const QVariant &value);

    QPointer<Companion> m_companion;
    QVariantMap m_anchor;
    QDBusConnection m_bus;
    bool m_registered = false;
};
