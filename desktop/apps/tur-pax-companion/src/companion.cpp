// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#include "companion.h"

#include <QDateTime>

#include <algorithm>
#include <limits>

namespace {

constexpr qint64 hour = 60 * 60 * 1000;
// How many notices may wait as marks. More than this in one session means
// something is wrong with a source; the oldest goes.
constexpr int heldLimit = 50;

bool isAny(const QString &value, std::initializer_list<const char *> names)
{
    for (const char *name : names) {
        if (value == QLatin1String(name))
            return true;
    }
    return false;
}

} // namespace

QStringList Companion::states()
{
    return { QStringLiteral("asleep"),   QStringLiteral("idle"),      QStringLiteral("noticing"),
             QStringLiteral("listening"), QStringLiteral("thinking"),  QStringLiteral("observing"),
             QStringLiteral("acting"),   QStringLiteral("waiting"),   QStringLiteral("paused"),
             QStringLiteral("telling"),  QStringLiteral("refused"),   QStringLiteral("error") };
}

Companion::Companion(QObject *parent)
    : QObject(parent)
{
    m_bubbleTimer.setSingleShot(true);
    m_moodTimer.setSingleShot(true);
    m_quietTimer.setSingleShot(true);
    connect(&m_bubbleTimer, &QTimer::timeout, this, &Companion::fold);
    connect(&m_moodTimer, &QTimer::timeout, this, [this] { flag(QString()); });
    connect(&m_quietTimer, &QTimer::timeout, this, &Companion::updateQuiet);
}

void Companion::setSettings(CompanionSettings *settings)
{
    if (settings == m_settings)
        return;
    if (m_settings)
        m_settings->disconnect(this);
    m_settings = settings;
    if (m_settings) {
        connect(m_settings, &CompanionSettings::hiddenChanged, this, &Companion::update);
        connect(m_settings, &CompanionSettings::presenceChanged, this, &Companion::update);
        connect(m_settings, &CompanionSettings::overFullScreenChanged, this, &Companion::update);
        connect(m_settings, &CompanionSettings::quietUntilChanged, this, &Companion::updateQuiet);
    }
    Q_EMIT settingsChanged();
    updateQuiet();
    update();
}

void Companion::setPaxOn(bool on)
{
    if (on == m_paxOn)
        return;
    m_paxOn = on;
    Q_EMIT inputsChanged();
    update();
}

void Companion::setServiceState(const QString &state)
{
    if (state == m_serviceState)
        return;
    m_serviceState = state;
    Q_EMIT inputsChanged();
    update();
}

void Companion::setProblem(const QVariantMap &problem)
{
    if (problem == m_problem)
        return;
    m_problem = problem;
    Q_EMIT inputsChanged();
    update();
}

void Companion::setOverlayActivity(const QString &activity)
{
    if (activity == m_overlayActivity)
        return;
    m_overlayActivity = activity;
    Q_EMIT inputsChanged();
    update();
}

void Companion::setAskOpen(bool open)
{
    if (open == m_askOpen)
        return;
    m_askOpen = open;
    // The person is talking to P@x: what the character was saying makes
    // room, and waits as a mark.
    if (open && speaking())
        fold();
    Q_EMIT inputsChanged();
    update();
}

void Companion::setAttending(bool attending)
{
    if (attending == m_attending)
        return;
    m_attending = attending;
    Q_EMIT inputsChanged();
}

void Companion::setWaiting(int count)
{
    count = std::max(count, 0);
    if (count == m_waiting)
        return;
    m_waiting = count;
    Q_EMIT inputsChanged();
    update();
}

void Companion::setOnBattery(bool onBattery)
{
    if (onBattery == m_onBattery)
        return;
    m_onBattery = onBattery;
    Q_EMIT inputsChanged();
}

void Companion::setSpent(double spent)
{
    if (spent > 1)
        spent = 1;
    if (qFuzzyCompare(spent + 2.0, m_spent + 2.0))
        return;
    m_spent = spent;
    Q_EMIT inputsChanged();
}

void Companion::setStep(const QVariantMap &step)
{
    if (step == m_step)
        return;
    m_step = step;
    Q_EMIT inputsChanged();
}

double Companion::fuel() const
{
    if (m_spent < 0)
        return 1.0;
    return std::clamp(1.0 - m_spent, 0.0, 1.0);
}

void Companion::setClock(double clock)
{
    if (qint64(clock) == m_clock)
        return;
    m_clock = qint64(clock);
    Q_EMIT clockChanged();
    updateQuiet();
}

void Companion::setBubbleSeconds(int seconds)
{
    seconds = std::max(seconds, 0);
    if (seconds == m_bubbleSeconds)
        return;
    m_bubbleSeconds = seconds;
    if (seconds == 0)
        m_bubbleTimer.stop();
    Q_EMIT timingChanged();
}

void Companion::setMoodSeconds(int seconds)
{
    seconds = std::max(seconds, 0);
    if (seconds == m_moodSeconds)
        return;
    m_moodSeconds = seconds;
    Q_EMIT timingChanged();
}

double Companion::now() const
{
    return m_clock != 0 ? double(m_clock) : double(QDateTime::currentMSecsSinceEpoch());
}

void Companion::updateQuiet()
{
    const double at = now();
    const bool quiet = m_settings && m_settings->quietAt(at);
    m_quietTimer.stop();
    if (quiet && m_settings->quietUntil() > 0 && m_clock == 0) {
        const double left = m_settings->quietUntil() - at + 50;
        m_quietTimer.start(int(std::min(left, double(std::numeric_limits<int>::max()))));
    }
    if (quiet == m_quiet)
        return;
    m_quiet = quiet;
    // Going quiet takes back what was being said; it waits as a mark.
    if (quiet && speaking())
        fold();
    Q_EMIT quietChanged();
}

void Companion::setQuiet(const QString &mode)
{
    if (m_settings)
        m_settings->setQuiet(mode, now());
}

QString Companion::resolve() const
{
    if (!m_paxOn)
        return QStringLiteral("asleep");
    // P@x's indicator first: while it says P@x acts, sees or waits, the
    // character says the same.
    if (m_overlayActivity == QLatin1String("acting"))
        return QStringLiteral("acting");
    if (m_overlayActivity == QLatin1String("observing"))
        return QStringLiteral("observing");
    if (m_mood == QLatin1String("error"))
        return QStringLiteral("error");
    if (m_overlayActivity == QLatin1String("waiting") || m_waiting > 0)
        return QStringLiteral("waiting");
    if (m_overlayActivity == QLatin1String("paused") || m_serviceState == QLatin1String("paused"))
        return QStringLiteral("paused");
    if (m_mood == QLatin1String("refused"))
        return QStringLiteral("refused");
    if (m_serviceState == QLatin1String("working") || m_overlayActivity == QLatin1String("working"))
        return QStringLiteral("thinking");
    if (!m_problem.isEmpty())
        return QStringLiteral("error");
    if (m_askOpen || m_overlayActivity == QLatin1String("listening"))
        return QStringLiteral("listening");
    if (speaking())
        return QStringLiteral("telling");
    if (!m_held.isEmpty())
        return QStringLiteral("noticing");
    return QStringLiteral("idle");
}

void Companion::update()
{
    const bool hidden = m_settings && m_settings->hidden();
    const bool always = m_settings && m_settings->presence() == QLatin1String("always");
    const bool shown = !hidden && (m_paxOn || always);
    if (shown != m_shown) {
        m_shown = shown;
        // Off the bar, nothing is being said.
        if (!shown && speaking())
            fold();
        Q_EMIT shownChanged();
    }
    const bool stays = !m_settings || m_settings->overFullScreen();
    const bool raised = stays || isAny(m_overlayActivity, { "acting", "observing", "waiting" });
    if (raised != m_raised) {
        m_raised = raised;
        Q_EMIT raisedChanged();
    }
    const QString state = resolve();
    if (state != m_state) {
        m_state = state;
        Q_EMIT stateChanged();
    }
}

QVariantMap Companion::checked(const QVariantMap &notice, const QString &kind) const
{
    const QString id = notice.value(QStringLiteral("id")).toString();
    const QString sentence = notice.value(QStringLiteral("sentence")).toString().trimmed();
    const QString sender = notice.value(QStringLiteral("sender")).toString();
    if (id.isEmpty() || sentence.isEmpty() || sentence.size() > sentenceLimit())
        return {};
    if (sender != QLatin1String("tur") && sender != QLatin1String("pax"))
        return {};
    QVariantMap out = notice;
    out.insert(QStringLiteral("kind"), kind);
    out.insert(QStringLiteral("sentence"), sentence);
    // At most two actions (section 4 of the design).
    QStringList actions = notice.value(QStringLiteral("actions")).toStringList();
    while (actions.size() > 2)
        actions.removeLast();
    out.insert(QStringLiteral("actions"), actions);
    return out;
}

bool Companion::waitingInQueue(const QString &id) const
{
    if (m_bubble.value(QStringLiteral("id")).toString() == id)
        return true;
    return std::any_of(m_held.cbegin(), m_held.cend(),
                       [&id](const QVariantMap &held) { return held.value(QStringLiteral("id")).toString() == id; });
}

int Companion::spokenLastHour(const QList<qint64> &times) const
{
    const qint64 since = qint64(now()) - hour;
    return int(std::count_if(times.cbegin(), times.cend(), [since](qint64 at) { return at > since; }));
}

void Companion::show(const QVariantMap &notice)
{
    m_bubble = notice;
    if (m_settings)
        m_settings->markTold(notice.value(QStringLiteral("id")).toString());
    if (m_bubbleSeconds > 0)
        m_bubbleTimer.start(m_bubbleSeconds * 1000);
    Q_EMIT bubbleChanged();
    update();
}

QString Companion::offer(const QVariantMap &notice)
{
    const QString kind = notice.value(QStringLiteral("kind")).toString();
    if (!CompanionSettings::kindNames().contains(kind))
        return QStringLiteral("invalid");
    const QVariantMap ready = checked(notice, kind);
    if (ready.isEmpty())
        return QStringLiteral("invalid");
    const QString id = ready.value(QStringLiteral("id")).toString();
    if (waitingInQueue(id))
        return QStringLiteral("held");
    if (m_settings && m_settings->told(id))
        return QStringLiteral("told");
    // Off the bar, or asleep, the character consumes nothing: the desktop's
    // own notice is the one the person gets.
    if (!m_shown || !m_paxOn)
        return QStringLiteral("hidden");
    if (m_settings && !m_settings->kindOn(kind)) {
        m_settings->markTold(id);
        return QStringLiteral("off");
    }
    const int limit = m_settings ? m_settings->speakPerHour() : 6;
    if (m_quiet || speaking() || m_askOpen || m_attending || spokenLastHour(m_spoken) >= limit) {
        m_held.append(ready);
        while (m_held.size() > heldLimit)
            m_held.removeFirst();
        Q_EMIT heldChanged();
        update();
        return QStringLiteral("held");
    }
    m_spoken.append(qint64(now()));
    while (m_spoken.size() > 120)
        m_spoken.removeFirst();
    show(ready);
    return QStringLiteral("shown");
}

QString Companion::suggest(const QVariantMap &notice)
{
    const QVariantMap ready = checked(notice, QStringLiteral("suggestion"));
    if (ready.isEmpty())
        return QStringLiteral("invalid");
    if (!m_settings || !m_settings->suggestions())
        return QStringLiteral("off");
    if (!m_shown || !m_paxOn)
        return QStringLiteral("hidden");
    const QString id = ready.value(QStringLiteral("id")).toString();
    if (m_settings->told(id) || waitingInQueue(id))
        return QStringLiteral("told");
    if (m_quiet)
        return QStringLiteral("quiet");
    if (speaking() || m_askOpen || m_attending || m_state != QLatin1String("idle"))
        return QStringLiteral("busy");
    if (spokenLastHour(m_suggested) >= m_settings->suggestionsPerHour())
        return QStringLiteral("limited");
    m_suggested.append(qint64(now()));
    while (m_suggested.size() > 60)
        m_suggested.removeFirst();
    show(ready);
    return QStringLiteral("shown");
}

void Companion::dismiss()
{
    m_bubbleTimer.stop();
    if (!m_mood.isEmpty())
        flag(QString());
    if (!speaking())
        return;
    m_bubble.clear();
    Q_EMIT bubbleChanged();
    update();
}

void Companion::fold()
{
    m_bubbleTimer.stop();
    if (!speaking())
        return;
    // A suggestion that was not answered is not kept.
    if (m_bubble.value(QStringLiteral("kind")).toString() != QLatin1String("suggestion")) {
        m_held.prepend(m_bubble);
        Q_EMIT heldChanged();
    }
    m_bubble.clear();
    Q_EMIT bubbleChanged();
    update();
}

bool Companion::unfold()
{
    if (speaking())
        return true;
    if (m_held.isEmpty())
        return false;
    const QVariantMap next = m_held.takeFirst();
    Q_EMIT heldChanged();
    show(next);
    // The person asked: the bubble stays until they answer it.
    m_bubbleTimer.stop();
    return true;
}

void Companion::flag(const QString &mood)
{
    const QString wanted = isAny(mood, { "refused", "error" }) ? mood : QString();
    m_moodTimer.stop();
    if (!wanted.isEmpty() && m_moodSeconds > 0)
        m_moodTimer.start(m_moodSeconds * 1000);
    if (wanted == m_mood)
        return;
    m_mood = wanted;
    update();
}
