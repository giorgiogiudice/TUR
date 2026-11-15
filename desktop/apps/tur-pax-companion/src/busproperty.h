// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#pragma once

#include <QDBusConnection>
#include <QObject>
#include <QString>
#include <QStringList>
#include <QVariant>
#include <QVariantMap>
#include <QtQml/qqmlregistration.h>

#include <optional>

class QDBusServiceWatcher;

// One property of another program's D-Bus object, kept up to date: read
// when the program is there, followed through PropertiesChanged, and empty
// when the program is not there. It never starts the program it reads.
//
// The character reads three this way (docs/architecture/
// tur-pax-companion.md, section 6): what P@x's indicator shows
// (org.tur.PaxOverlay1.Activity), what P@x's window shows
// (org.tur.PaxUi1.Mode), and whether the computer runs on its battery
// (org.freedesktop.UPower.OnBattery, on the system bus).
class BusProperty : public QObject
{
    Q_OBJECT
    QML_ELEMENT

    // A D-Bus address to use instead of the session or system bus (tests).
    Q_PROPERTY(QString busAddress READ busAddress WRITE setBusAddress NOTIFY targetChanged FINAL)
    // The system bus instead of the session bus.
    Q_PROPERTY(bool system READ system WRITE setSystem NOTIFY targetChanged FINAL)
    Q_PROPERTY(QString service READ service WRITE setService NOTIFY targetChanged FINAL)
    Q_PROPERTY(QString path READ path WRITE setPath NOTIFY targetChanged FINAL)
    Q_PROPERTY(QString iface READ iface WRITE setIface NOTIFY targetChanged FINAL)
    Q_PROPERTY(QString name READ name WRITE setName NOTIFY targetChanged FINAL)
    // The property's value; invalid while the program is not there.
    Q_PROPERTY(QVariant value READ value NOTIFY valueChanged FINAL)
    Q_PROPERTY(bool present READ present NOTIFY valueChanged FINAL)

public:
    explicit BusProperty(QObject *parent = nullptr);
    ~BusProperty() override;

    QString busAddress() const { return m_busAddress; }
    void setBusAddress(const QString &address);
    bool system() const { return m_system; }
    void setSystem(bool system);
    QString service() const { return m_service; }
    void setService(const QString &service);
    QString path() const { return m_path; }
    void setPath(const QString &path);
    QString iface() const { return m_interface; }
    void setIface(const QString &iface);
    QString name() const { return m_name; }
    void setName(const QString &name);
    QVariant value() const { return m_value; }
    bool present() const { return m_value.isValid(); }

    // Connects, reads the property if its program is there, and follows it.
    Q_INVOKABLE void start();

Q_SIGNALS:
    void targetChanged();
    void valueChanged();

private Q_SLOTS:
    void onPropertiesChanged(const QString &iface, const QVariantMap &changed, const QStringList &invalidated);

private:
    void stop();
    void fetch();
    void take(const QVariant &value);

    QString m_busAddress;
    bool m_system = false;
    QString m_service;
    QString m_path;
    QString m_interface;
    QString m_name;
    QVariant m_value;

    std::optional<QDBusConnection> m_connection;
    QString m_connectionName;
    QDBusServiceWatcher *m_watcher = nullptr;
    quint64 m_generation = 0;
};
