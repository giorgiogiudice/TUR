// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#pragma once

#include "address.h"
#include "codes.h"
#include "registry.h"

#include <QDBusConnection>
#include <QDBusMessage>
#include <QObject>
#include <QPointer>
#include <QQuickItem>
#include <QString>
#include <QStringList>
#include <QVariantList>
#include <QVariantMap>
#include <QtQml/qqmlregistration.h>

#include <functional>

namespace Settings {
class Settings1Adaptor;
class Addressable1Adaptor;
} // namespace Settings

// What stands between an address and the window (desktop-experience.md,
// section 10.3). It takes an address from wherever one comes from (the
// command line, a link, a search result, org.tur.Settings1 on the session
// bus), finds out from the registries whose it is, and
//
//  - for a page of this program, asks the window to show it (showRequested)
//    and reports where the row is;
//  - for a page of KDE System Settings, starts System Settings at the module;
//  - for a page of another TUR program, passes the call on to that program's
//    org.tur.Addressable1.
//
// It changes no setting. Opening a page and pointing at a row is all it does.
class SettingsHost : public QObject
{
    Q_OBJECT
    QML_ELEMENT

    Q_PROPERTY(Registry *registry READ registry WRITE setRegistry NOTIFY registryChanged FINAL)

    // The program that is KDE System Settings. A name is looked up on the
    // PATH; nothing is given to a shell.
    Q_PROPERTY(QString systemSettingsProgram READ systemSettingsProgram WRITE setSystemSettingsProgram
                   NOTIFY systemSettingsProgramChanged FINAL)

    // Where the window is: the page it shows, or the row that is highlighted.
    // The window keeps it up to date; org.tur.Settings1 offers it as Address.
    Q_PROPERTY(QString currentAddress READ currentAddress WRITE setCurrentAddress NOTIFY currentAddressChanged FINAL)

    // Whether the program is on the session bus as org.tur.Settings1, and if
    // not, why.
    Q_PROPERTY(bool onBus READ onBus NOTIFY busChanged FINAL)
    Q_PROPERTY(QString busProblem READ busProblem NOTIFY busChanged FINAL)

public:
    explicit SettingsHost(QObject *parent = nullptr);
    ~SettingsHost() override;

    Registry *registry() const { return m_registry; }
    void setRegistry(Registry *registry);
    QString systemSettingsProgram() const { return m_systemSettings; }
    void setSystemSettingsProgram(const QString &program);
    QString currentAddress() const { return m_currentAddress; }
    void setCurrentAddress(const QString &address);
    bool onBus() const { return m_onBus; }
    QString busProblem() const { return m_busProblem; }
    QDBusConnection bus() const { return m_bus; }

    // What an address given on the command line or as a link does: a row is
    // shown and highlighted, a page is opened. Returns `ok`, and when it is
    // false the content of a message: code, title, reason, detail.
    Q_INVOKABLE QVariantMap go(const QString &address);

    // Opens the page; no highlight. An empty address only brings the window
    // forward.
    Q_INVOKABLE QVariantMap open(const QString &address);

    // Shows the page, brings the row into view, gives it the keyboard and
    // highlights it. Options: highlight (default true), duration in seconds
    // (0, the default, keeps the highlight until clearHighlight), dim (dim
    // the rest of the window), pax (draw the highlight in P@x's gold: the
    // caller is the assistant).
    Q_INVOKABLE QVariantMap reveal(const QString &address, const QVariantMap &options);

    Q_INVOKABLE void clearHighlight();

    // Starts KDE System Settings where it last was.
    Q_INVOKABLE QVariantMap openSystemSettings();

    // The window's answer to showRequested: found, and for a row x, y,
    // width, height (in the window), highlighted, focused, windowTitle.
    Q_INVOKABLE void answer(const QVariantMap &shown);

    // A row told the window that its value or state changed.
    Q_INVOKABLE void rowChanged(const QString &address);

    // The row with this address under `root`, or null. And all of them.
    Q_INVOKABLE QQuickItem *findRow(QQuickItem *root, const QString &address) const;
    Q_INVOKABLE QVariantList rows(QQuickItem *root) const;

    // Makes every layout under `root` do now what it would do before the
    // next frame, so that an item's position can be read at once. A caller
    // on the bus gets the rectangle of a row whether or not the window has
    // been drawn since the page changed.
    Q_INVOKABLE void settle(QQuickItem *root) const;

    // Puts the program on the session bus: org.tur.Settings1 at
    // /org/tur/Settings1 and org.tur.Addressable1 at /org/tur/Addressable1,
    // under the names org.tur.Settings1 and org.tur.Settings. False, with
    // busProblem, if there is no bus or another tur-settings has the name.
    Q_INVOKABLE bool startBus();
    void stopBus();

    // What the adaptors call. A request that this program answers itself is
    // answered before the call returns; one for another program is answered
    // when that program has.
    struct Outcome
    {
        bool ok = false;
        Settings::Code code = Settings::Code::UnknownAddress;
        QString reason;
        QString detail;
        QVariantMap result;     // what Reveal returns
        QString forwardService; // not empty: pass the call on to this name
        Settings::Address address;
    };
    enum class Verb { Open, Reveal };
    Outcome perform(const QString &address, const QVariantMap &options, Verb verb);

    // Passes a Reveal on to another program's org.tur.Addressable1 and calls
    // `done` with its reply. False, and `done` is not called, if there is no
    // session bus to reach the program on.
    bool forward(const QString &service, const QString &address, const QVariantMap &options,
                 const std::function<void(const QDBusMessage &)> &done);

    // The registry entry of an address as JSON, with the owning program.
    Outcome resolve(const QString &address) const;

    static QVariantMap optionsFrom(const QVariantMap &given, bool highlightByDefault);

Q_SIGNALS:
    void registryChanged();
    void systemSettingsProgramChanged();
    void currentAddressChanged();
    void busChanged();

    // To the window: show this page, and this row of it if `row` is not
    // empty. The window answers with answer() before it returns.
    void showRequested(const QString &page, const QString &row, const QVariantMap &options);
    void clearRequested();

    // To the window: come forward.
    void presentRequested();

private:
    Outcome failed(Settings::Code code, const QString &reason, const QString &detail = QString()) const;
    Outcome passedOn(const Outcome &outcome, const QVariantMap &options);
    Outcome startSystemSettings(const QString &module, const QStringList &arguments);
    QVariantMap toView(const Outcome &outcome) const;

    QPointer<Registry> m_registry;
    QString m_systemSettings = QStringLiteral("systemsettings");
    QString m_currentAddress;
    QDBusConnection m_bus;
    bool m_onBus = false;
    QString m_busProblem;
    QObject *m_settingsObject = nullptr;
    QObject *m_addressableObject = nullptr;
    Settings::Settings1Adaptor *m_settingsAdaptor = nullptr;
    Settings::Addressable1Adaptor *m_addressableAdaptor = nullptr;
    QVariantMap m_shown;
    bool m_answered = false;
};
