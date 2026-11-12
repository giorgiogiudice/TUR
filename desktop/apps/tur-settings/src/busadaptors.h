// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#pragma once

#include "bustypes.h"

#include <QDBusAbstractAdaptor>
#include <QDBusMessage>
#include <QList>
#include <QString>
#include <QVariantMap>

class SettingsHost;

namespace Settings {

// org.tur.Addressable1, as desktop/apps/tur-settings/dbus/org.tur.Addressable1.xml
// states it: what every TUR program with addressable content offers. The
// adaptor is written by hand and held to the XML by a test; no code is
// generated from the XML (desktop/README.md, "Qt's tools are run, not
// linked").
class Addressable1Adaptor : public QDBusAbstractAdaptor
{
    Q_OBJECT
    Q_CLASSINFO("D-Bus Interface", "org.tur.Addressable1")

public:
    Addressable1Adaptor(QObject *object, SettingsHost *host);

public Q_SLOTS:
    QVariantMap Reveal(const QString &address, const QVariantMap &options, const QDBusMessage &message);
    void ClearHighlight();
    QString List();

Q_SIGNALS:
    void RowChanged(const QString &address);

private:
    SettingsHost *m_host;
};

// org.tur.Settings1, as desktop/apps/tur-settings/dbus/org.tur.Settings1.xml
// states it: the one entry point for every address, TUR's or KDE's.
class Settings1Adaptor : public QDBusAbstractAdaptor
{
    Q_OBJECT
    Q_CLASSINFO("D-Bus Interface", "org.tur.Settings1")
    Q_PROPERTY(QString Address READ address)

public:
    Settings1Adaptor(QObject *object, SettingsHost *host);

    QString address() const;

public Q_SLOTS:
    void Open(const QString &address, const QDBusMessage &message);
    QVariantMap Reveal(const QString &address, const QVariantMap &options, const QDBusMessage &message);
    void ClearHighlight();
    QString Resolve(const QString &address, const QDBusMessage &message);
    QList<Settings::SearchHit> Search(const QString &text, uint limit);

private:
    SettingsHost *m_host;
};

} // namespace Settings
