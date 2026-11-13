// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#pragma once

#include <QDBusConnection>
#include <QObject>
#include <QProcess>
#include <QString>
#include <QStringList>
#include <QTemporaryDir>
#include <QVariantList>
#include <QVariantMap>

#include <optional>

// A session bus of the tests' own (a dbus-daemon nothing else is on) with
// the stand-in for org.tur.Agent1 on it (fixture/agent.cpp), and the means
// to steer the stand-in. The QML tests reach it as the context property
// `fixture`; the C++ tests use it directly.
class FixtureBus : public QObject
{
    Q_OBJECT
    Q_PROPERTY(QString address READ address CONSTANT FINAL)

public:
    FixtureBus(const QString &agentProgram, const QString &fixture, QObject *parent = nullptr);
    ~FixtureBus() override;

    // Starts the bus and the stand-in. False, with the reason, if either
    // could not be started.
    bool start(QString *problem);

    QString address() const { return m_address; }
    // Where the bus looks for activation files.
    QString servicesDirectory() const;
    QString fixtureDirectory() const { return m_fixture; }
    QString agentProgram() const { return m_program; }

    // The stand-in: started, stopped (the service is then absent), and
    // whether it owns org.tur.Agent1.
    Q_INVOKABLE bool startAgent();
    Q_INVOKABLE void stopAgent();
    Q_INVOKABLE bool agentOwned();

    // org.tur.Agent1Fixture: which recorded status and task to answer with,
    // what was called, whether a key was given, and letting a held task or
    // download go on.
    Q_INVOKABLE bool use(const QVariantMap &scenario);
    Q_INVOKABLE QVariantList calls();
    Q_INVOKABLE QStringList callNames();
    Q_INVOKABLE bool keyMatches(const QString &key);
    Q_INVOKABLE void release();

    // Asks the bus to read its activation files again.
    void reloadServices();
    // Ends whatever owns org.tur.Agent1, whoever started it (the bus, from
    // an activation file), and waits until the name is free.
    void killOwner();

private:
    QVariant control(const QString &method, const QVariantList &arguments);
    void wait(int milliseconds);

    QString m_program;
    QString m_fixture;
    QTemporaryDir m_dir;
    QProcess m_daemon;
    QProcess m_agent;
    QString m_address;
    std::optional<QDBusConnection> m_connection;
};
