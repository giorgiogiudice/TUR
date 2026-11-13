// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#include "conversation.h"

#include "agentclient.h"

#include <QHash>

namespace {

QVariant nothing()
{
    return QVariant::fromValue(nullptr);
}

qint64 tokensOf(const QVariantMap &use)
{
    return use.value(QStringLiteral("input_tokens")).toLongLong() + use.value(QStringLiteral("output_tokens")).toLongLong();
}

bool isNumber(const QVariant &value)
{
    if (!value.isValid() || value.isNull())
        return false;
    bool ok = false;
    value.toDouble(&ok);
    return ok;
}

} // namespace

Conversation::Conversation(QObject *parent)
    : QAbstractListModel(parent)
{
}

Conversation::~Conversation() = default;

int Conversation::rowCount(const QModelIndex &parent) const
{
    return parent.isValid() ? 0 : int(m_entries.size());
}

QVariant Conversation::data(const QModelIndex &index, int role) const
{
    if (!index.isValid() || index.row() < 0 || index.row() >= m_entries.size())
        return QVariant();
    const Entry &entry = m_entries.at(index.row());
    switch (role) {
    case KindRole:
        return entry.kind;
    case Qt::DisplayRole:
    case TextRole:
        return entry.text;
    case TaskRole:
        return entry.task;
    case StepRole:
        return entry.step;
    case StateRole:
        return entry.state;
    case CodeRole:
        return entry.code;
    case TimeRole:
        return entry.time;
    case DetailRole:
        return entry.detail;
    default:
        return QVariant();
    }
}

QHash<int, QByteArray> Conversation::roleNames() const
{
    return {
        { KindRole, "kind" },     { TextRole, "text" },   { TaskRole, "task" }, { StepRole, "step" },
        { StateRole, "phase" },   { CodeRole, "code" },   { TimeRole, "time" }, { DetailRole, "detail" },
    };
}

void Conversation::setClient(AgentClient *client)
{
    if (client == m_client)
        return;
    if (m_client)
        disconnect(m_client, nullptr, this, nullptr);
    m_client = client;
    if (m_client) {
        connect(m_client, &AgentClient::taskStarted, this, &Conversation::taskStarted);
        connect(m_client, &AgentClient::textAdded, this, &Conversation::textAdded);
        connect(m_client, &AgentClient::actionProposed, this, &Conversation::actionProposed);
        connect(m_client, &AgentClient::actionCompleted, this, &Conversation::actionCompleted);
        connect(m_client, &AgentClient::taskFinished, this, &Conversation::taskFinished);
        connect(m_client, &AgentClient::statusChanged, this, [this] {
            Q_EMIT spendChanged();
            Q_EMIT taskChanged();
        });
    }
    Q_EMIT clientChanged();
    Q_EMIT spendChanged();
}

bool Conversation::paused() const
{
    return running() && m_client && m_client->serviceState() == QLatin1String("paused");
}

QVariantMap Conversation::today() const
{
    if (!m_client)
        return {};
    return m_client->status().value(QStringLiteral("spending")).toMap().value(QStringLiteral("today")).toMap();
}

QVariantMap Conversation::spend() const
{
    const QVariantMap status = m_client ? m_client->status() : QVariantMap();
    const QVariantMap spending = status.value(QStringLiteral("spending")).toMap();
    const QVariantMap now = spending.value(QStringLiteral("today")).toMap();
    const QVariantMap dayLimit = spending.value(QStringLiteral("day_limit")).toMap();
    const QVariantMap taskLimit = spending.value(QStringLiteral("task_limit")).toMap();
    const bool priceKnown = status.value(QStringLiteral("price_known")).toBool();

    QVariant taskCost = nothing();
    qint64 taskTokens = -1;
    if (running() && !now.isEmpty() && !m_todayAtStart.isEmpty()) {
        taskTokens = qMax<qint64>(0, tokensOf(now) - tokensOf(m_todayAtStart));
        if (priceKnown && isNumber(now.value(QStringLiteral("cost"))))
            taskCost = qMax(0.0, now.value(QStringLiteral("cost")).toDouble()
                                     - m_todayAtStart.value(QStringLiteral("cost")).toDouble());
    } else if (!running() && !m_lastTask.isEmpty()) {
        taskTokens = m_lastTokens;
        if (isNumber(m_lastCost))
            taskCost = m_lastCost.toDouble();
    }

    const auto orNothing = [](const QVariant &value) { return isNumber(value) ? value : nothing(); };
    QString currency = spending.value(QStringLiteral("currency")).toString();
    if (currency.isEmpty() && !running())
        currency = m_lastCurrency;
    return {
        { QStringLiteral("known"), !status.isEmpty() },
        { QStringLiteral("priceKnown"), priceKnown },
        { QStringLiteral("currency"), currency },
        { QStringLiteral("taskCost"), taskCost },
        { QStringLiteral("taskTokens"), taskTokens },
        { QStringLiteral("todayCost"), orNothing(now.value(QStringLiteral("cost"))) },
        { QStringLiteral("todayTokens"), now.isEmpty() ? qint64(-1) : tokensOf(now) },
        { QStringLiteral("dayLimitCost"), orNothing(dayLimit.value(QStringLiteral("cost"))) },
        { QStringLiteral("dayLimitTokens"), orNothing(dayLimit.value(QStringLiteral("tokens"))) },
        { QStringLiteral("taskLimitCost"), orNothing(taskLimit.value(QStringLiteral("cost"))) },
        { QStringLiteral("taskLimitTokens"), orNothing(taskLimit.value(QStringLiteral("tokens"))) },
        { QStringLiteral("dayLimitReached"), spending.value(QStringLiteral("day_limit_reached")).toBool() },
    };
}

void Conversation::append(const Entry &entry)
{
    beginInsertRows(QModelIndex(), int(m_entries.size()), int(m_entries.size()));
    m_entries.append(entry);
    endInsertRows();
    Q_EMIT countChanged();
}

void Conversation::changed(int row)
{
    const QModelIndex at = index(row);
    Q_EMIT dataChanged(at, at);
}

int Conversation::findStep(const QString &task, int stepIndex) const
{
    for (int row = int(m_entries.size()) - 1; row >= 0; --row) {
        const Entry &entry = m_entries.at(row);
        if (entry.kind == QLatin1String("step") && entry.task == task
            && entry.step.value(QStringLiteral("index")).toInt() == stepIndex)
            return row;
    }
    return -1;
}

int Conversation::findPending(const QString &request) const
{
    for (int row = int(m_entries.size()) - 1; row >= 0; --row) {
        const Entry &entry = m_entries.at(row);
        if (entry.kind == QLatin1String("you") && entry.state == QLatin1String("pending") && entry.text == request)
            return row;
    }
    return -1;
}

void Conversation::setTask(const QString &task)
{
    if (task == m_task && !m_starting)
        return;
    m_task = task;
    m_starting = false;
    Q_EMIT taskChanged();
}

void Conversation::ask(const QString &request)
{
    const QString text = request.trimmed();
    if (text.isEmpty())
        return;
    requested(text);
    if (!m_client) {
        startRefused(QStringLiteral("org.freedesktop.DBus.Error.ServiceUnknown"),
                     tr("P@x's service is not on this computer."));
        return;
    }
    const QPointer<Conversation> self(this);
    m_client->callWith(QStringLiteral("StartTask"), { text, QString() }, [self](const QVariantMap &reply) {
        if (!self)
            return;
        if (!reply.value(QStringLiteral("ok")).toBool()) {
            self->startRefused(reply.value(QStringLiteral("code")).toString(),
                               reply.value(QStringLiteral("message")).toString());
            return;
        }
        // TaskStarted may have come first; if it has not, this is the task.
        const QString task = reply.value(QStringLiteral("value")).toString();
        if (self->m_starting && self->m_task.isEmpty())
            self->setTask(task);
    });
}

void Conversation::stop()
{
    if (!m_client)
        return;
    m_client->callWith(QStringLiteral("CancelTask"), { m_task }, {});
}

void Conversation::pause()
{
    if (m_client)
        m_client->callWith(QStringLiteral("Pause"), {}, {});
}

void Conversation::resume()
{
    if (m_client)
        m_client->callWith(QStringLiteral("Resume"), {}, {});
}

void Conversation::clear()
{
    beginResetModel();
    m_entries.clear();
    endResetModel();
    m_answered.clear();
    if (!running()) {
        m_lastTask.clear();
        m_lastOutcome.clear();
        m_lastCost = QVariant();
        m_lastTokens = -1;
    }
    Q_EMIT countChanged();
    Q_EMIT taskChanged();
    Q_EMIT spendChanged();
}

QVariantMap Conversation::entry(int row) const
{
    QVariantMap out;
    if (row < 0 || row >= m_entries.size())
        return out;
    const QHash<int, QByteArray> names = roleNames();
    for (auto it = names.constBegin(); it != names.constEnd(); ++it)
        out.insert(QString::fromLatin1(it.value()), data(index(row), it.key()));
    return out;
}

QVariantList Conversation::entriesOf(const QString &kind) const
{
    QVariantList out;
    for (int row = 0; row < m_entries.size(); ++row) {
        if (m_entries.at(row).kind == kind)
            out.append(entry(row));
    }
    return out;
}

void Conversation::requested(const QString &request)
{
    Entry entry;
    entry.kind = QStringLiteral("you");
    entry.text = request;
    entry.state = QStringLiteral("pending");
    append(entry);
    m_starting = true;
    Q_EMIT taskChanged();
}

void Conversation::startRefused(const QString &code, const QString &message)
{
    for (int row = int(m_entries.size()) - 1; row >= 0; --row) {
        Entry &entry = m_entries[row];
        if (entry.kind == QLatin1String("you") && entry.state == QLatin1String("pending")) {
            entry.state = QStringLiteral("refused");
            changed(row);
            break;
        }
    }
    Entry card;
    card.kind = QStringLiteral("system");
    card.state = QStringLiteral("refused");
    card.code = code;
    card.text = message;
    append(card);
    m_starting = false;
    Q_EMIT taskChanged();
    Q_EMIT attentionWanted();
}

void Conversation::taskStarted(const QString &task, const QVariant &details)
{
    const QVariantMap started = details.toMap();
    const QString request = started.value(QStringLiteral("request")).toString();
    const qint64 at = started.value(QStringLiteral("started_at")).toLongLong();
    const int row = findPending(request);
    if (row >= 0) {
        Entry &entry = m_entries[row];
        entry.task = task;
        entry.state = QStringLiteral("started");
        entry.time = at;
        changed(row);
    } else {
        // A task this window did not ask for: one a schedule started, or one
        // another program started for the person.
        Entry entry;
        const bool attended = started.value(QStringLiteral("attended"), true).toBool();
        entry.kind = attended ? QStringLiteral("you") : QStringLiteral("system");
        entry.state = attended ? QStringLiteral("elsewhere") : QStringLiteral("scheduled");
        entry.text = request;
        entry.task = task;
        entry.time = at;
        append(entry);
    }
    m_todayAtStart = today();
    m_lastCost = QVariant();
    m_lastTokens = -1;
    setTask(task);
    Q_EMIT spendChanged();
    Q_EMIT attentionWanted();
}

void Conversation::textAdded(const QString &task, const QString &kind, const QString &text)
{
    Entry entry;
    entry.task = task;
    entry.text = text;
    if (kind == QLatin1String("notice")) {
        entry.kind = QStringLiteral("system");
        entry.state = QStringLiteral("notice");
    } else {
        entry.kind = QStringLiteral("pax");
        entry.state = kind == QLatin1String("answer") ? QStringLiteral("answer") : QStringLiteral("interim");
        if (kind == QLatin1String("answer"))
            m_answered.append(task);
    }
    append(entry);
}

void Conversation::updateStep(const QString &task, const QVariantMap &step)
{
    const int stepIndex = step.value(QStringLiteral("index")).toInt();
    const int row = findStep(task, stepIndex);
    const QString code = step.value(QStringLiteral("error_code")).toString();
    if (row < 0) {
        Entry entry;
        entry.kind = QStringLiteral("step");
        entry.task = task;
        entry.step = step;
        entry.state = step.value(QStringLiteral("state")).toString();
        entry.text = step.value(QStringLiteral("title")).toString();
        entry.code = code;
        append(entry);
        return;
    }
    Entry &entry = m_entries[row];
    entry.step = step;
    entry.state = step.value(QStringLiteral("state")).toString();
    entry.text = step.value(QStringLiteral("title")).toString();
    entry.code = code;
    changed(row);
}

void Conversation::actionProposed(const QString &task, const QVariant &step)
{
    updateStep(task, step.toMap());
}

void Conversation::actionCompleted(const QString &task, const QVariant &step)
{
    updateStep(task, step.toMap());
}

void Conversation::taskFinished(const QString &task, const QVariant &result)
{
    const QVariantMap finished = result.toMap();
    const QString outcome = finished.value(QStringLiteral("outcome")).toString();
    const QString code = finished.value(QStringLiteral("error_code")).toString();
    const QString message = finished.value(QStringLiteral("message")).toString();

    // What the steps came to.
    int done = 0;
    int refused = 0;
    int failed = 0;
    int stopped = 0;
    for (int row = 0; row < m_entries.size(); ++row) {
        Entry &entry = m_entries[row];
        if (entry.kind != QLatin1String("step") || entry.task != task)
            continue;
        if (entry.state == QLatin1String("running")) {
            entry.state = QStringLiteral("stopped");
            changed(row);
        }
        if (entry.state == QLatin1String("done"))
            ++done;
        else if (entry.state == QLatin1String("refused") || entry.state == QLatin1String("rejected"))
            ++refused;
        else if (entry.state == QLatin1String("failed"))
            ++failed;
        else
            ++stopped;
    }

    const QString answer = finished.value(QStringLiteral("answer")).toString();
    if (!answer.isEmpty() && !m_answered.contains(task)) {
        Entry words;
        words.kind = QStringLiteral("pax");
        words.state = QStringLiteral("answer");
        words.task = task;
        words.text = answer;
        append(words);
        m_answered.append(task);
    }

    const QVariantMap usage = finished.value(QStringLiteral("usage")).toMap();
    m_lastCost = finished.value(QStringLiteral("cost"));
    m_lastCurrency = finished.value(QStringLiteral("currency")).toString();
    m_lastTokens = tokensOf(usage);

    Entry card;
    card.task = task;
    card.code = code;
    card.text = message;
    card.time = finished.value(QStringLiteral("finished_at")).toLongLong();
    card.detail = {
        { QStringLiteral("outcome"), outcome },
        { QStringLiteral("done"), done },
        { QStringLiteral("refused"), refused },
        { QStringLiteral("failed"), failed },
        { QStringLiteral("stopped"), stopped },
        { QStringLiteral("tokens"), m_lastTokens },
        { QStringLiteral("cost"), isNumber(m_lastCost) ? m_lastCost : nothing() },
        { QStringLiteral("currency"), m_lastCurrency },
    };
    if (outcome == QLatin1String("completed")) {
        card.kind = QStringLiteral("summary");
        card.state = outcome;
        append(card);
    } else {
        card.kind = QStringLiteral("system");
        card.state = outcome.isEmpty() ? QStringLiteral("failed") : outcome;
        // The service has already said it as a notice (a limit that was
        // reached): that card becomes this one, so that it is said once.
        int notice = -1;
        for (int row = int(m_entries.size()) - 1; row >= 0; --row) {
            const Entry &entry = m_entries.at(row);
            if (entry.task != task)
                break;
            if (entry.kind == QLatin1String("system") && entry.state == QLatin1String("notice")
                && entry.text == message) {
                notice = row;
                break;
            }
        }
        if (notice >= 0) {
            m_entries[notice] = card;
            changed(notice);
        } else {
            append(card);
        }
    }

    m_lastTask = task;
    m_lastOutcome = outcome;
    if (m_task == task)
        m_task.clear();
    m_starting = false;
    m_todayAtStart.clear();
    Q_EMIT taskChanged();
    Q_EMIT spendChanged();
    Q_EMIT attentionWanted();
}
