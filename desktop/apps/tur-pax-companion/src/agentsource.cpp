// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#include "agentsource.h"

#include <QDBusConnection>
#include <QDBusMessage>
#include <QDBusPendingCall>
#include <QDBusPendingCallWatcher>
#include <QStringList>

namespace {

const QString windowService = QStringLiteral("org.tur.PaxUi1");
const QString windowPath = QStringLiteral("/org/tur/PaxUi1");

QStringList showAndNotNow()
{
    return { QStringLiteral("show"), QStringLiteral("dismiss") };
}

double numberOf(const QVariant &value)
{
    bool ok = false;
    const double number = value.toDouble(&ok);
    return ok && value.isValid() && !value.isNull() ? number : -1;
}

} // namespace

AgentSource::AgentSource(QObject *parent)
    : QObject(parent)
{
    // A character on the bar never starts P@x.
    m_client.setAutoStart(false);
    connect(&m_client, &AgentClient::presenceChanged, this, &AgentSource::onStatus);
    connect(&m_client, &AgentClient::statusChanged, this, &AgentSource::onStatus);
    connect(&m_client, &AgentClient::taskStarted, this, &AgentSource::onTaskStarted);
    connect(&m_client, &AgentClient::actionProposed, this, &AgentSource::onStep);
    connect(&m_client, &AgentClient::actionCompleted, this, &AgentSource::onStep);
    connect(&m_client, &AgentClient::taskFinished, this, &AgentSource::onTaskFinished);
    connect(&m_client, &AgentClient::scheduleProposed, this, &AgentSource::onScheduleProposed);
    connect(&m_client, &AgentClient::busAddressChanged, this, &AgentSource::busAddressChanged);
}

void AgentSource::setBusAddress(const QString &address)
{
    m_client.setBusAddress(address);
}

void AgentSource::setCompanion(Companion *companion)
{
    if (companion == m_companion)
        return;
    m_companion = companion;
    Q_EMIT companionChanged();
    onStatus();
}

void AgentSource::setLongTaskSeconds(int seconds)
{
    if (seconds == m_longTaskSeconds)
        return;
    m_longTaskSeconds = seconds;
    Q_EMIT longTaskSecondsChanged();
}

void AgentSource::setProblem(const QString &problem)
{
    if (problem == m_problem)
        return;
    m_problem = problem;
    Q_EMIT problemChanged();
}

void AgentSource::start()
{
    m_client.start();
    onStatus();
}

double AgentSource::spentOf(const QVariantMap &status)
{
    const QVariantMap spending = status.value(QStringLiteral("spending")).toMap();
    const QVariantMap today = spending.value(QStringLiteral("today")).toMap();
    const QVariantMap limit = spending.value(QStringLiteral("day_limit")).toMap();
    // In money when the limit is in money; in tokens otherwise.
    const double costLimit = numberOf(limit.value(QStringLiteral("cost")));
    const double cost = numberOf(today.value(QStringLiteral("cost")));
    if (costLimit > 0 && cost >= 0)
        return cost / costLimit;
    const double tokenLimit = numberOf(limit.value(QStringLiteral("tokens")));
    const double in = numberOf(today.value(QStringLiteral("input_tokens")));
    const double out = numberOf(today.value(QStringLiteral("output_tokens")));
    if (tokenLimit > 0 && in >= 0 && out >= 0)
        return (in + out) / tokenLimit;
    return -1;
}

QVariantMap AgentSource::limitSuggestion(const QVariantMap &status)
{
    const double spent = spentOf(status);
    if (spent < 0.9 || spent >= 1.0)
        return {};
    const QString day = status.value(QStringLiteral("spending")).toMap().value(QStringLiteral("day")).toString();
    QVariantMap hint;
    hint.insert(QStringLiteral("id"), QStringLiteral("suggest:limit:") + day);
    hint.insert(QStringLiteral("sender"), QStringLiteral("tur"));
    hint.insert(QStringLiteral("sentence"),
                tr("It looks like P@x has used most of today's spending limit. You can change the limit in P@x's "
                   "setup."));
    hint.insert(QStringLiteral("actions"), showAndNotNow());
    hint.insert(QStringLiteral("opens"), QStringLiteral("setup"));
    return hint;
}

void AgentSource::onStatus()
{
    if (!m_companion)
        return;
    const QVariantMap status = m_client.status();
    const bool there = m_client.presence() == AgentClient::Present && m_client.statusKnown();
    const bool on = there && status.value(QStringLiteral("enabled")).toBool();
    m_companion->setPaxOn(on);
    m_companion->setServiceState(on ? m_client.serviceState() : QString());
    m_companion->setWaiting(on ? status.value(QStringLiteral("proposals")).toInt() : 0);
    m_companion->setSpent(on ? spentOf(status) : -1);
    QVariantMap problem;
    if (on && !status.value(QStringLiteral("ready")).toBool()) {
        // The service's own code and sentence, when it gives them.
        const QVariantMap said = status.value(QStringLiteral("problem")).toMap();
        problem.insert(QStringLiteral("ready"), false);
        problem.insert(QStringLiteral("code"), said.value(QStringLiteral("error_code")).toString());
        problem.insert(QStringLiteral("message"), said.value(QStringLiteral("message")).toString());
    }
    m_companion->setProblem(problem);
    if (on) {
        const QVariantMap hint = limitSuggestion(status);
        if (!hint.isEmpty())
            m_companion->suggest(hint);
    }
    if (!on) {
        m_companion->setStep({});
        m_tasks.clear();
    }
}

void AgentSource::onTaskStarted(const QString &task, const QVariant &details)
{
    Running running;
    running.startedAt = m_companion ? m_companion->now() : 0;
    const QVariantMap said = details.toMap();
    running.attended = !said.contains(QStringLiteral("attended")) || said.value(QStringLiteral("attended")).toBool();
    m_tasks.insert(task, running);
    if (m_companion) {
        m_companion->setStep({});
        // A new task is the person moving on from the last one's end.
        m_companion->flag(QString());
    }
}

void AgentSource::onStep(const QString &task, const QVariant &step)
{
    const QVariantMap said = step.toMap();
    const QString state = said.value(QStringLiteral("state")).toString();
    if (state == QLatin1String("refused")) {
        Running &running = m_tasks[task];
        ++running.refused;
        running.lastRefusal = said.value(QStringLiteral("message")).toString();
    }
    if (!m_companion)
        return;
    if (state == QLatin1String("running")) {
        QVariantMap shown;
        shown.insert(QStringLiteral("title"), said.value(QStringLiteral("title")));
        shown.insert(QStringLiteral("reason"), said.value(QStringLiteral("reason")));
        shown.insert(QStringLiteral("class"), said.value(QStringLiteral("class")));
        m_companion->setStep(shown);
    } else {
        m_companion->setStep({});
    }
}

void AgentSource::onTaskFinished(const QString &task, const QVariant &result)
{
    const Running running = m_tasks.take(task);
    if (!m_companion)
        return;
    m_companion->setStep({});
    const QVariantMap said = result.toMap();
    const QString outcome = said.value(QStringLiteral("outcome")).toString();
    const QString message = said.value(QStringLiteral("message")).toString();
    const QString code = said.value(QStringLiteral("error_code")).toString();

    QVariantMap notice;
    notice.insert(QStringLiteral("id"), QStringLiteral("task:") + task);
    notice.insert(QStringLiteral("kind"), QStringLiteral("task"));
    // The system speaks about P@x, in the third person (design, 9.2).
    notice.insert(QStringLiteral("sender"), QStringLiteral("tur"));
    notice.insert(QStringLiteral("actions"), showAndNotNow());
    notice.insert(QStringLiteral("opens"), QStringLiteral("panel"));

    if (outcome == QLatin1String("cancelled")) {
        // The person stopped it; they know.
        return;
    }
    if (outcome == QLatin1String("completed")) {
        if (running.refused > 0) {
            m_companion->flag(QStringLiteral("refused"));
            notice.insert(QStringLiteral("sentence"),
                          running.refused == 1 ? tr("P@x finished, but one step was refused.")
                                               : tr("P@x finished, but %1 steps were refused.").arg(running.refused));
            notice.insert(QStringLiteral("detail"), running.lastRefusal);
        } else {
            const double took = running.startedAt > 0 ? (m_companion->now() - running.startedAt) / 1000.0 : 0;
            if (running.attended && took < m_longTaskSeconds)
                return;
            notice.insert(QStringLiteral("sentence"), tr("P@x finished what you asked."));
        }
    } else if (outcome == QLatin1String("spend_limit")) {
        m_companion->flag(QStringLiteral("refused"));
        notice.insert(QStringLiteral("sentence"),
                      message.isEmpty() ? tr("P@x stopped: a spending limit was reached.") : message);
        notice.insert(QStringLiteral("code"), code);
    } else {
        m_companion->flag(QStringLiteral("error"));
        notice.insert(QStringLiteral("sentence"), message.isEmpty() ? tr("P@x could not finish the task.") : message);
        notice.insert(QStringLiteral("code"), code);
    }
    m_companion->offer(notice);
}

void AgentSource::onScheduleProposed(const QString &proposal, const QVariant &details)
{
    Q_UNUSED(details)
    if (!m_companion)
        return;
    QVariantMap notice;
    notice.insert(QStringLiteral("id"), QStringLiteral("proposal:") + proposal);
    notice.insert(QStringLiteral("kind"), QStringLiteral("approval"));
    notice.insert(QStringLiteral("sender"), QStringLiteral("tur"));
    notice.insert(QStringLiteral("sentence"),
                  tr("P@x proposes a reminder or a scheduled task. Nothing is scheduled until you say yes."));
    notice.insert(QStringLiteral("actions"), showAndNotNow());
    notice.insert(QStringLiteral("opens"), QStringLiteral("panel"));
    m_companion->offer(notice);
    m_client.refresh();
}

void AgentSource::stop()
{
    if (m_client.presence() != AgentClient::Present) {
        setProblem(tr("P@x is not running; there is nothing to stop."));
        return;
    }
    m_client.callWith(QStringLiteral("CancelTask"), { QString() }, [this](const QVariantMap &reply) {
        setProblem(reply.value(QStringLiteral("ok")).toBool() ? QString()
                                                              : reply.value(QStringLiteral("message")).toString());
    });
}

void AgentSource::showWindow(const QString &mode)
{
    Q_EMIT windowRequested(mode);
    const QDBusConnection bus = m_client.connection();
    if (!bus.isConnected()) {
        setProblem(tr("There is no session bus, so P@x's window cannot be opened."));
        return;
    }
    QDBusMessage call = QDBusMessage::createMethodCall(windowService, windowPath, windowService, QStringLiteral("Show"));
    call << mode;
    auto *watcher = new QDBusPendingCallWatcher(bus.asyncCall(call, 15000), this);
    connect(watcher, &QDBusPendingCallWatcher::finished, this, [this](QDBusPendingCallWatcher *done) {
        done->deleteLater();
        const QDBusMessage reply = done->reply();
        setProblem(reply.type() == QDBusMessage::ErrorMessage ? reply.errorMessage() : QString());
    });
}

void AgentSource::ask(const QString &request)
{
    if (request.trimmed().isEmpty())
        return;
    m_client.callWith(QStringLiteral("StartTask"), { request, QString() }, [this](const QVariantMap &reply) {
        if (reply.value(QStringLiteral("ok")).toBool()) {
            setProblem(QString());
            showWindow(QStringLiteral("panel"));
        } else {
            setProblem(reply.value(QStringLiteral("message")).toString());
        }
    });
}
