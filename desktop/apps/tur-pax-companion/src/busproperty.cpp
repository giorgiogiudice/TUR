// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#include "busproperty.h"

#include <QDBusConnectionInterface>
#include <QDBusMessage>
#include <QDBusPendingCall>
#include <QDBusPendingCallWatcher>
#include <QDBusReply>
#include <QDBusServiceWatcher>
#include <QDBusVariant>

namespace {

const QString propertiesInterface = QStringLiteral("org.freedesktop.DBus.Properties");

} // namespace

BusProperty::BusProperty(QObject *parent)
    : QObject(parent)
{
}

BusProperty::~BusProperty()
{
    stop();
}

void BusProperty::setBusAddress(const QString &address)
{
    if (address == m_busAddress)
        return;
    m_busAddress = address;
    Q_EMIT targetChanged();
}

void BusProperty::setSystem(bool system)
{
    if (system == m_system)
        return;
    m_system = system;
    Q_EMIT targetChanged();
}

void BusProperty::setService(const QString &service)
{
    if (service == m_service)
        return;
    m_service = service;
    Q_EMIT targetChanged();
}

void BusProperty::setPath(const QString &path)
{
    if (path == m_path)
        return;
    m_path = path;
    Q_EMIT targetChanged();
}

void BusProperty::setIface(const QString &iface)
{
    if (iface == m_interface)
        return;
    m_interface = iface;
    Q_EMIT targetChanged();
}

void BusProperty::setName(const QString &name)
{
    if (name == m_name)
        return;
    m_name = name;
    Q_EMIT targetChanged();
}

void BusProperty::stop()
{
    ++m_generation;
    if (m_connection && m_connection->isConnected()) {
        m_connection->disconnect(m_service, m_path, propertiesInterface, QStringLiteral("PropertiesChanged"), this,
                                 SLOT(onPropertiesChanged(QString,QVariantMap,QStringList)));
    }
    delete m_watcher;
    m_watcher = nullptr;
    m_connection.reset();
    if (!m_connectionName.isEmpty())
        QDBusConnection::disconnectFromBus(m_connectionName);
    m_connectionName.clear();
}

void BusProperty::start()
{
    stop();
    if (m_service.isEmpty() || m_path.isEmpty() || m_interface.isEmpty() || m_name.isEmpty()) {
        take(QVariant());
        return;
    }
    if (!m_busAddress.isEmpty()) {
        m_connectionName = QStringLiteral("tur-pax-companion-%1").arg(quintptr(this), 0, 16);
        m_connection.emplace(QDBusConnection::connectToBus(m_busAddress, m_connectionName));
    } else if (m_system) {
        m_connection.emplace(QDBusConnection::systemBus());
    } else {
        m_connection.emplace(QDBusConnection::sessionBus());
    }
    if (!m_connection->isConnected()) {
        take(QVariant());
        return;
    }
    m_connection->connect(m_service, m_path, propertiesInterface, QStringLiteral("PropertiesChanged"), this,
                          SLOT(onPropertiesChanged(QString,QVariantMap,QStringList)));
    m_watcher = new QDBusServiceWatcher(m_service, *m_connection,
                                        QDBusServiceWatcher::WatchForRegistration
                                            | QDBusServiceWatcher::WatchForUnregistration,
                                        this);
    connect(m_watcher, &QDBusServiceWatcher::serviceRegistered, this, &BusProperty::fetch);
    connect(m_watcher, &QDBusServiceWatcher::serviceUnregistered, this, [this] {
        ++m_generation;
        take(QVariant());
    });

    bool owned = false;
    if (QDBusConnectionInterface *bus = m_connection->interface()) {
        const QDBusReply<bool> registered = bus->isServiceRegistered(m_service);
        owned = registered.isValid() && registered.value();
    }
    if (owned)
        fetch();
    else
        take(QVariant());
}

void BusProperty::fetch()
{
    if (!m_connection || !m_connection->isConnected())
        return;
    QDBusMessage get = QDBusMessage::createMethodCall(m_service, m_path, propertiesInterface, QStringLiteral("Get"));
    get << m_interface << m_name;
    // Reading a property is never a reason to start a program.
    get.setAutoStartService(false);
    const quint64 generation = ++m_generation;
    auto *watcher = new QDBusPendingCallWatcher(m_connection->asyncCall(get, 10000), this);
    connect(watcher, &QDBusPendingCallWatcher::finished, this, [this, generation](QDBusPendingCallWatcher *done) {
        done->deleteLater();
        if (generation != m_generation)
            return;
        const QDBusMessage reply = done->reply();
        if (reply.type() != QDBusMessage::ReplyMessage) {
            take(QVariant());
            return;
        }
        take(reply.arguments().value(0).value<QDBusVariant>().variant());
    });
}

void BusProperty::onPropertiesChanged(const QString &iface, const QVariantMap &changed,
                                      const QStringList &invalidated)
{
    if (iface != m_interface)
        return;
    const auto found = changed.constFind(m_name);
    if (found != changed.cend()) {
        // A newer value than any answer still on its way.
        ++m_generation;
        take(found.value());
    } else if (invalidated.contains(m_name)) {
        fetch();
    }
}

void BusProperty::take(const QVariant &value)
{
    if (value == m_value && value.isValid() == m_value.isValid())
        return;
    m_value = value;
    Q_EMIT valueChanged();
}
