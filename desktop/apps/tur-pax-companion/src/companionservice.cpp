// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#include "companionservice.h"

#include <QDBusError>
#include <QDBusMessage>
#include <QQuickWindow>
#include <QTimer>

#include <memory>

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

void CompanionService::setHeal(HealSource *heal)
{
    if (heal == m_heal)
        return;
    if (m_heal)
        m_heal->disconnect(this);
    m_heal = heal;
    if (m_heal)
        connect(m_heal, &HealSource::announced, this, &CompanionService::announced);
}

void CompanionService::setWindow(QQuickWindow *window)
{
    m_window = window;
}

bool CompanionService::Announce(const QString &source, qulonglong id)
{
    if (source != QLatin1String("heal")) {
        if (calledFromDBus())
            sendErrorReply(QDBusError::InvalidArgs, QStringLiteral("Announce knows one source: heal"));
        return false;
    }
    // What can be known without reading anything is answered at once: not
    // on the bar, asleep, quiet, already speaking, or the person is busy
    // with the ask bar or the character's menu.
    if (!calledFromDBus() || !m_companion || !m_heal || !m_companion->shown() || !m_companion->paxOn()
        || m_companion->quiet() || m_companion->speaking() || m_companion->askOpen() || m_companion->attending())
        return false;
    setDelayedReply(true);
    m_pending.append({ qint64(id), message() });
    m_heal->announce(qint64(id));
    return false;
}

void CompanionService::announced(qint64 id, const QString &result)
{
    for (qsizetype i = 0; i < m_pending.size();) {
        if (m_pending.at(i).first != id) {
            ++i;
            continue;
        }
        const QDBusMessage call = m_pending.takeAt(i).second;
        if (result == QLatin1String("shown"))
            whenOnScreen(call, id);
        else
            reply(call, false);
    }
}

void CompanionService::whenOnScreen(const QDBusMessage &call, qint64 id)
{
    const QString notice = QStringLiteral("heal:%1").arg(id);
    QQuickWindow *window = m_window;
    if (!window || !window->isVisible()) {
        if (m_companion)
            m_companion->withdraw(notice);
        reply(call, false);
        return;
    }
    // The first frame the window shows after the bubble was set may have
    // been prepared before it; the second one has it. One more frame is
    // asked for, so that a still scene does not wait for ever.
    auto frames = std::make_shared<int>(0);
    auto *guard = new QObject(this);
    connect(window, &QQuickWindow::frameSwapped, guard, [this, call, frames, guard, window] {
        if (++*frames < 2) {
            QMetaObject::invokeMethod(window, &QQuickWindow::update, Qt::QueuedConnection);
            return;
        }
        reply(call, true);
        guard->deleteLater();
    }, Qt::QueuedConnection);
    QTimer::singleShot(m_frameTimeout, guard, [this, call, notice, guard] {
        if (m_companion)
            m_companion->withdraw(notice);
        reply(call, false);
        guard->deleteLater();
    });
    window->update();
}

void CompanionService::reply(const QDBusMessage &call, bool taken)
{
    m_bus.send(call.createReply(QVariant(taken)));
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
