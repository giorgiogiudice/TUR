// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#include "fixturebus.h"

#include <QCoreApplication>
#include <QDBusConnectionInterface>
#include <QDBusError>
#include <QDBusMessage>
#include <QDBusReply>
#include <QDir>
#include <QElapsedTimer>
#include <QJsonDocument>
#include <QProcessEnvironment>
#include <QThread>

#include <csignal>

#include <sys/types.h>

namespace {

const QString agentName = QStringLiteral("org.tur.Agent1");

} // namespace

FixtureBus::FixtureBus(const QString &agentProgram, const QString &fixture, QObject *parent)
    : QObject(parent)
    , m_program(agentProgram)
    , m_fixture(fixture)
{
}

FixtureBus::~FixtureBus()
{
    stopAgent();
    if (m_connection) {
        m_connection.reset();
        QDBusConnection::disconnectFromBus(QStringLiteral("tur-pax-ui-fixture-control"));
    }
    if (m_daemon.state() != QProcess::NotRunning) {
        m_daemon.terminate();
        if (!m_daemon.waitForFinished(5000)) {
            m_daemon.kill();
            m_daemon.waitForFinished(5000);
        }
    }
}

QString FixtureBus::servicesDirectory() const
{
    return m_dir.path() + QStringLiteral("/data/dbus-1/services");
}

void FixtureBus::wait(int milliseconds)
{
    QElapsedTimer timer;
    timer.start();
    while (timer.elapsed() < milliseconds) {
        QCoreApplication::processEvents(QEventLoop::AllEvents, 20);
        QThread::msleep(10);
    }
}

bool FixtureBus::start(QString *problem)
{
    if (!m_dir.isValid()) {
        *problem = QStringLiteral("no temporary directory");
        return false;
    }
    QDir().mkpath(servicesDirectory());
    QDir().mkpath(m_dir.path() + QStringLiteral("/runtime"));
    QDir().mkpath(m_dir.path() + QStringLiteral("/no-data"));
    const QString chosen = QStringLiteral("unix:path=") + m_dir.path() + QStringLiteral("/bus");
    QProcessEnvironment environment = QProcessEnvironment::systemEnvironment();
    environment.insert(QStringLiteral("DBUS_SESSION_BUS_ADDRESS"), chosen);
    // The bus reads activation files from XDG_DATA_HOME/dbus-1/services, and
    // nothing of the machine's.
    environment.insert(QStringLiteral("XDG_DATA_HOME"), m_dir.path() + QStringLiteral("/data"));
    environment.insert(QStringLiteral("XDG_DATA_DIRS"), m_dir.path() + QStringLiteral("/no-data"));
    environment.insert(QStringLiteral("XDG_RUNTIME_DIR"), m_dir.path() + QStringLiteral("/runtime"));
    m_daemon.setProcessEnvironment(environment);
    m_daemon.setProcessChannelMode(QProcess::ForwardedErrorChannel);
    m_daemon.start(QStringLiteral("dbus-daemon"),
                   { QStringLiteral("--session"), QStringLiteral("--nofork"), QStringLiteral("--nopidfile"),
                     QStringLiteral("--print-address=1"), QStringLiteral("--address=") + chosen });
    if (!m_daemon.waitForStarted(15000)) {
        *problem = QStringLiteral("dbus-daemon could not be started");
        return false;
    }
    if (!m_daemon.waitForReadyRead(15000)) {
        *problem = QStringLiteral("dbus-daemon did not print its address");
        return false;
    }
    m_address = QString::fromUtf8(m_daemon.readLine()).trimmed();
    m_agent.setProcessEnvironment(environment);
    m_agent.setProcessChannelMode(QProcess::ForwardedChannels);
    m_connection.emplace(QDBusConnection::connectToBus(m_address, QStringLiteral("tur-pax-ui-fixture-control")));
    if (!m_connection->isConnected()) {
        *problem = QStringLiteral("the test cannot connect to its bus: ") + m_connection->lastError().message();
        return false;
    }
    if (!startAgent()) {
        *problem = QStringLiteral("the stand-in for org.tur.Agent1 did not take its name");
        return false;
    }
    return true;
}

bool FixtureBus::agentOwned()
{
    if (!m_connection || !m_connection->isConnected())
        return false;
    const QDBusReply<bool> owned = m_connection->interface()->isServiceRegistered(agentName);
    return owned.isValid() && owned.value();
}

bool FixtureBus::startAgent()
{
    if (m_agent.state() == QProcess::NotRunning) {
        m_agent.start(m_program, { QStringLiteral("--fixture"), m_fixture, QStringLiteral("--bus"), m_address });
        if (!m_agent.waitForStarted(15000))
            return false;
    }
    QElapsedTimer timer;
    timer.start();
    while (!agentOwned() && timer.elapsed() < 15000)
        wait(20);
    return agentOwned();
}

void FixtureBus::stopAgent()
{
    if (m_agent.state() == QProcess::NotRunning)
        return;
    m_agent.terminate();
    if (!m_agent.waitForFinished(5000)) {
        m_agent.kill();
        m_agent.waitForFinished(5000);
    }
    QElapsedTimer timer;
    timer.start();
    while (agentOwned() && timer.elapsed() < 5000)
        wait(20);
}

QVariant FixtureBus::control(const QString &method, const QVariantList &arguments)
{
    if (!m_connection)
        return QVariant();
    QDBusMessage call = QDBusMessage::createMethodCall(agentName, QStringLiteral("/org/tur/Agent1Fixture"),
                                                       QStringLiteral("org.tur.Agent1Fixture"), method);
    call.setArguments(arguments);
    call.setAutoStartService(false);
    const QDBusMessage reply = m_connection->call(call, QDBus::Block, 10000);
    if (reply.type() == QDBusMessage::ErrorMessage) {
        qWarning("fixture: %s failed: %s", qPrintable(method), qPrintable(reply.errorMessage()));
        return QVariant();
    }
    return reply.arguments().value(0);
}

bool FixtureBus::use(const QVariantMap &scenario)
{
    const QString text = QString::fromUtf8(QJsonDocument::fromVariant(scenario).toJson(QJsonDocument::Compact));
    QDBusMessage call = QDBusMessage::createMethodCall(agentName, QStringLiteral("/org/tur/Agent1Fixture"),
                                                       QStringLiteral("org.tur.Agent1Fixture"), QStringLiteral("Use"));
    call << text;
    call.setAutoStartService(false);
    return m_connection && m_connection->call(call, QDBus::Block, 10000).type() == QDBusMessage::ReplyMessage;
}

QVariantList FixtureBus::calls()
{
    return QJsonDocument::fromJson(control(QStringLiteral("Calls"), {}).toString().toUtf8()).toVariant().toList();
}

QStringList FixtureBus::callNames()
{
    QStringList out;
    const QVariantList all = calls();
    for (const QVariant &call : all)
        out.append(call.toMap().value(QStringLiteral("method")).toString());
    return out;
}

bool FixtureBus::keyMatches(const QString &key)
{
    return control(QStringLiteral("KeyMatches"), { key }).toBool();
}

void FixtureBus::release()
{
    control(QStringLiteral("Release"), {});
}

void FixtureBus::reloadServices()
{
    if (!m_connection)
        return;
    const QDBusMessage call = QDBusMessage::createMethodCall(QStringLiteral("org.freedesktop.DBus"),
                                                             QStringLiteral("/org/freedesktop/DBus"),
                                                             QStringLiteral("org.freedesktop.DBus"),
                                                             QStringLiteral("ReloadConfig"));
    m_connection->call(call, QDBus::Block, 10000);
}

void FixtureBus::killOwner()
{
    if (!m_connection || !agentOwned())
        return;
    QDBusMessage call = QDBusMessage::createMethodCall(QStringLiteral("org.freedesktop.DBus"),
                                                       QStringLiteral("/org/freedesktop/DBus"),
                                                       QStringLiteral("org.freedesktop.DBus"),
                                                       QStringLiteral("GetConnectionUnixProcessID"));
    call << agentName;
    const QDBusMessage reply = m_connection->call(call, QDBus::Block, 10000);
    const uint pid = reply.arguments().value(0).toUInt();
    if (reply.type() == QDBusMessage::ReplyMessage && pid > 0)
        ::kill(pid_t(pid), SIGTERM);
    QElapsedTimer timer;
    timer.start();
    while (agentOwned() && timer.elapsed() < 10000)
        wait(20);
}
