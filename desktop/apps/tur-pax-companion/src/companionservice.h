// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#pragma once

#include "companion.h"
#include "healsource.h"

#include <QDBusConnection>
#include <QDBusContext>
#include <QDBusMessage>
#include <QList>
#include <QObject>
#include <QPair>
#include <QPointer>
#include <QString>
#include <QStringList>
#include <QVariantMap>

class QQuickWindow;

// org.tur.PaxCompanion1 (dbus/org.tur.PaxCompanion1.xml): how the person's
// other programs reach P@x's character on the bar. It brings the character
// back, hides it, gives it the keyboard, makes it quiet, and says where it
// is so that P@x's ask bar can open beside it.
//
// It carries no words: nothing can make the character say anything through
// it. What the character says, it has read itself from the system's own
// records (tur-pax-companion.md, section 6). Announce carries an identifier
// of the healing service's: the words are the ones the character reads
// from that service's own program.
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
    // Where the healing service's notices are read, for Announce.
    void setHeal(HealSource *heal);
    // The surface the bubble is drawn on: Announce answers true once it
    // has drawn the bubble.
    void setWindow(QQuickWindow *window);
    // How long a drawn bubble may take before Announce gives up and says
    // false, in milliseconds.
    void setFrameTimeout(int milliseconds) { m_frameTimeout = milliseconds; }

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
    // The healing service asks the character to say its notice `id`
    // (tur-pax-companion.md, section 7). The answer is true only once the
    // bubble is on the screen; false, at once, when the character cannot
    // say it now. "heal" is the one source.
    bool Announce(const QString &source, qulonglong id);

Q_SIGNALS:
    void attendRequested();

private:
    void announce(const QString &property, const QVariant &value);
    void announced(qint64 id, const QString &result);
    void whenOnScreen(const QDBusMessage &call, qint64 id);
    void reply(const QDBusMessage &call, bool taken);

    QPointer<Companion> m_companion;
    QPointer<HealSource> m_heal;
    QPointer<QQuickWindow> m_window;
    QList<QPair<qint64, QDBusMessage>> m_pending;
    int m_frameTimeout = 3000;
    QVariantMap m_anchor;
    QDBusConnection m_bus;
    bool m_registered = false;
};
