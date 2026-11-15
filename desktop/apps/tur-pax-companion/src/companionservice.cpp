// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#include "companionservice.h"

#include <QDBusError>
#include <QDBusMessage>

QString CompanionService::serviceName()
{
    return QStringLiteral("org.tur.PaxCompanion1");
}

QString CompanionService::objectPath()
{
    return QStringLiteral("/org/tur/PaxCompanion1");
}

QString CompanionService::interfaceName()
{
    return QStringLiteral("org.tur.PaxCompanion1");
}

QStringList CompanionService::quietModes()
{
    return { QStringLiteral("hour"), QStringLiteral("tomorrow"), QStringLiteral("always"), QStringLiteral("off") };
}

CompanionService::CompanionService(QObject *parent)
    : QObject(parent)
    , m_bus(QString())
{
}

QString CompanionService::state() const
{
    return m_companion ? m_companion->state() : QStringLiteral("asleep");
}

bool CompanionService::shown() const
{
    return m_companion && m_companion->shown();
}

bool CompanionService::quiet() const
{
    return m_companion && m_companion->quiet();
}

void CompanionService::setCompanion(Companion *companion)
{
    if (companion == m_companion)
        return;
    if (m_companion)
        m_companion->disconnect(this);
    m_companion = companion;
    if (!m_companion)
        return;
    connect(m_companion, &Companion::stateChanged, this,
            [this] { announce(QStringLiteral("State"), state()); });
    connect(m_companion, &Companion::shownChanged, this,
            [this] { announce(QStringLiteral("Shown"), shown()); });
    connect(m_companion, &Companion::quietChanged, this,
            [this] { announce(QStringLiteral("Quiet"), quiet()); });
}

void CompanionService::setAnchor(const QVariantMap &anchor)
{
    if (anchor == m_anchor)
        return;
    m_anchor = anchor;
    announce(QStringLiteral("Anchor"), m_anchor);
}

void CompanionService::announce(const QString &property, const QVariant &value)
{
    if (!m_registered || !m_bus.isConnected())
        return;
    QDBusMessage changed = QDBusMessage::createSignal(objectPath(), QStringLiteral("org.freedesktop.DBus.Properties"),
                                                      QStringLiteral("PropertiesChanged"));
    changed << interfaceName() << QVariantMap { { property, value } } << QStringList();
    m_bus.send(changed);
}

bool CompanionService::registerOn(const QDBusConnection &bus, QString *problem)
{
    if (!bus.isConnected()) {
        *problem = QStringLiteral("there is no session bus");
        return false;
    }
    m_bus = bus;
    if (!m_bus.registerObject(objectPath(), interfaceName(), this,
                              QDBusConnection::ExportAllSlots | QDBusConnection::ExportAllProperties)) {
        *problem = QStringLiteral("%1 could not be exported: %2").arg(objectPath(), m_bus.lastError().message());
        return false;
    }
    if (!m_bus.registerService(serviceName())) {
        *problem = QStringLiteral("the name %1 is taken: %2").arg(serviceName(), m_bus.lastError().message());
        m_bus.unregisterObject(objectPath());
        return false;
    }
    m_registered = true;
    return true;
}

bool CompanionService::forward(const QDBusConnection &bus, const QString &method, const QVariantList &arguments,
                               QString *problem)
{
    QDBusMessage call = QDBusMessage::createMethodCall(serviceName(), objectPath(), interfaceName(), method);
    call.setArguments(arguments);
    call.setAutoStartService(false);
    const QDBusMessage reply = bus.call(call, QDBus::Block, 10000);
    if (reply.type() == QDBusMessage::ErrorMessage) {
        *problem = reply.errorMessage();
        return false;
    }
    return true;
}

void CompanionService::Show()
{
    if (m_companion && m_companion->settings())
        m_companion->settings()->setHidden(false);
}

void CompanionService::Hide()
{
    if (m_companion && m_companion->settings())
        m_companion->settings()->setHidden(true);
}

void CompanionService::Attend()
{
    if (m_companion && m_companion->settings())
        m_companion->settings()->setHidden(false);
    Q_EMIT attendRequested();
}

void CompanionService::SetQuiet(const QString &mode)
{
    if (!quietModes().contains(mode)) {
        if (calledFromDBus())
            sendErrorReply(QDBusError::InvalidArgs,
                           QStringLiteral("SetQuiet takes one of: %1").arg(quietModes().join(QStringLiteral(", "))));
        return;
    }
    if (m_companion)
        m_companion->setQuiet(mode);
}
