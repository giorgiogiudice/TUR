// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#include "uiservice.h"

#include <QDBusConnectionInterface>
#include <QDBusError>
#include <QDBusMessage>

QString UiService::serviceName()
{
    return QStringLiteral("org.tur.PaxUi1");
}

QString UiService::objectPath()
{
    return QStringLiteral("/org/tur/PaxUi1");
}

QString UiService::interfaceName()
{
    return QStringLiteral("org.tur.PaxUi1");
}

QStringList UiService::modes()
{
    return { QStringLiteral("ask"), QStringLiteral("panel"), QStringLiteral("activity"), QStringLiteral("setup") };
}

UiService::UiService(QObject *parent)
    : QObject(parent)
{
}

bool UiService::registerOn(QDBusConnection bus, QString *problem)
{
    if (!bus.isConnected()) {
        *problem = QStringLiteral("there is no session bus");
        return false;
    }
    if (!bus.registerObject(objectPath(), interfaceName(), this,
                            QDBusConnection::ExportAllSlots | QDBusConnection::ExportAllProperties)) {
        *problem = QStringLiteral("%1 could not be exported: %2").arg(objectPath(), bus.lastError().message());
        return false;
    }
    if (!bus.registerService(serviceName())) {
        *problem = QStringLiteral("the name %1 is taken: %2").arg(serviceName(), bus.lastError().message());
        bus.unregisterObject(objectPath());
        return false;
    }
    return true;
}

bool UiService::forward(QDBusConnection bus, const QString &mode, QString *problem)
{
    QDBusMessage call = QDBusMessage::createMethodCall(serviceName(), objectPath(), interfaceName(), QStringLiteral("Show"));
    call << mode;
    // A program that is starting may own the name a moment before it
    // answers.
    const QDBusMessage reply = bus.call(call, QDBus::Block, 10000);
    if (reply.type() == QDBusMessage::ErrorMessage) {
        *problem = reply.errorMessage();
        return false;
    }
    return true;
}

void UiService::Show(const QString &mode)
{
    if (!modes().contains(mode)) {
        if (calledFromDBus())
            sendErrorReply(QDBusError::InvalidArgs,
                           QStringLiteral("Show takes one of: %1").arg(modes().join(QStringLiteral(", "))));
        return;
    }
    Q_EMIT showRequested(mode);
}

void UiService::Hide()
{
    Q_EMIT hideRequested();
}
