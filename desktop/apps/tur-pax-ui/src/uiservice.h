// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#pragma once

#include <QDBusConnection>
#include <QDBusContext>
#include <QObject>
#include <QString>
#include <QStringList>

// org.tur.PaxUi1 (dbus/org.tur.PaxUi1.xml): how other programs, and a second
// start of tur-pax-ui, ask the running window to show itself. The tray item
// of P@x (tur-notifier), the launcher's "Ask P@x" result and P@x's key all
// end here. It shows and hides; it carries no words to P@x, so nothing can
// ask P@x anything in the person's name through it.
class UiService : public QObject, protected QDBusContext
{
    Q_OBJECT
    Q_CLASSINFO("D-Bus Interface", "org.tur.PaxUi1")
    Q_PROPERTY(uint InterfaceVersion READ interfaceVersion CONSTANT FINAL)
    // What the window shows now: "" (nothing), ask, panel, activity or setup.
    // P@x's character on the bar follows it: while the ask bar is open the
    // character listens and says nothing over it.
    Q_PROPERTY(QString Mode READ mode FINAL)

public:
    // The program's well-known name on the session bus, its object and its
    // interface.
    static QString serviceName();
    static QString objectPath();
    static QString interfaceName();
    // What Show takes.
    static QStringList modes();

    explicit UiService(QObject *parent = nullptr);

    uint interfaceVersion() const { return 1; }
    QString mode() const { return m_mode; }
    // What the window shows, from the program; sends PropertiesChanged.
    void setMode(const QString &mode);

    // Exports the object and takes the name. False, with the reason, if the
    // name is someone else's.
    bool registerOn(QDBusConnection bus, QString *problem);

    // Asks the window that owns the name on `bus` to show `mode`. For a
    // second start of the program.
    static bool forward(QDBusConnection bus, const QString &mode, QString *problem);

public Q_SLOTS:
    // ask, panel, activity or setup.
    void Show(const QString &mode);
    void Hide();

Q_SIGNALS:
    void showRequested(const QString &mode);
    void hideRequested();

private:
    QString m_mode;
    QDBusConnection m_bus = QDBusConnection(QString());
    bool m_registered = false;
};
