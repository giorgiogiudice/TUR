// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#include "companionsettings.h"

#include <QDateTime>
#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QSaveFile>
#include <QStandardPaths>
#include <QTime>

#include <algorithm>

namespace {

// How many identifiers of notices already said are remembered.
constexpr int toldLimit = 400;

} // namespace

QStringList CompanionSettings::kindNames()
{
    return { QStringLiteral("repair"), QStringLiteral("fault"), QStringLiteral("approval"), QStringLiteral("task"),
             QStringLiteral("update") };
}

QString CompanionSettings::defaultPath()
{
    const QString base = QStandardPaths::writableLocation(QStandardPaths::GenericConfigLocation);
    if (base.isEmpty())
        return QString();
    return base + QStringLiteral("/tur/pax-companion.json");
}

CompanionSettings::CompanionSettings(QObject *parent)
    : QObject(parent)
{
}

void CompanionSettings::setPath(const QString &path)
{
    if (path == m_path)
        return;
    m_path = path;
    Q_EMIT pathChanged();
    load();
}

QVariantMap CompanionSettings::kinds() const
{
    QVariantMap out;
    const QStringList names = kindNames();
    for (const QString &name : names)
        out.insert(name, !m_off.contains(name));
    return out;
}

bool CompanionSettings::kindOn(const QString &kind) const
{
    return kindNames().contains(kind) && !m_off.contains(kind);
}

void CompanionSettings::setKind(const QString &kind, bool on)
{
    if (!kindNames().contains(kind) || on == !m_off.contains(kind))
        return;
    if (on)
        m_off.removeAll(kind);
    else
        m_off.append(kind);
    Q_EMIT kindsChanged();
    save();
}

void CompanionSettings::setSuggestions(bool on)
{
    if (on == m_suggestions)
        return;
    m_suggestions = on;
    Q_EMIT suggestionsChanged();
    save();
}

void CompanionSettings::setSuggestionsPerHour(int count)
{
    count = std::clamp(count, 1, 12);
    if (count == m_suggestionsPerHour)
        return;
    m_suggestionsPerHour = count;
    Q_EMIT suggestionsChanged();
    save();
}

void CompanionSettings::setSpeakPerHour(int count)
{
    count = std::clamp(count, 1, 60);
    if (count == m_speakPerHour)
        return;
    m_speakPerHour = count;
    Q_EMIT speakPerHourChanged();
    save();
}

void CompanionSettings::setHidden(bool hidden)
{
    if (hidden == m_hidden)
        return;
    m_hidden = hidden;
    Q_EMIT hiddenChanged();
    save();
}

void CompanionSettings::setPresence(const QString &presence)
{
    if (presence == m_presence || (presence != QLatin1String("with-pax") && presence != QLatin1String("always")))
        return;
    m_presence = presence;
    Q_EMIT presenceChanged();
    save();
}

void CompanionSettings::setAlong(double along)
{
    along = std::clamp(along, 0.0, 1.0);
    if (qFuzzyCompare(along + 1.0, m_along + 1.0))
        return;
    m_along = along;
    Q_EMIT alongChanged();
    save();
}

void CompanionSettings::setOverFullScreen(bool stay)
{
    if (stay == m_overFullScreen)
        return;
    m_overFullScreen = stay;
    Q_EMIT overFullScreenChanged();
    save();
}

bool CompanionSettings::setQuiet(const QString &mode, double now)
{
    qint64 until = 0;
    if (mode == QLatin1String("hour")) {
        until = qint64(now) + 60 * 60 * 1000;
    } else if (mode == QLatin1String("tomorrow")) {
        const QDateTime at = QDateTime::fromMSecsSinceEpoch(qint64(now));
        until = QDateTime(at.date().addDays(1), QTime(7, 0)).toMSecsSinceEpoch();
    } else if (mode == QLatin1String("always")) {
        until = -1;
    } else if (mode != QLatin1String("off")) {
        return false;
    }
    if (until != m_quietUntil) {
        m_quietUntil = until;
        Q_EMIT quietUntilChanged();
        save();
    }
    return true;
}

bool CompanionSettings::quietAt(double now) const
{
    return m_quietUntil < 0 || double(m_quietUntil) > now;
}

bool CompanionSettings::told(const QString &id) const
{
    return m_told.contains(id);
}

void CompanionSettings::markTold(const QString &id)
{
    if (id.isEmpty() || m_told.contains(id))
        return;
    m_told.append(id);
    while (m_told.size() > toldLimit)
        m_told.removeFirst();
    save();
}

void CompanionSettings::load()
{
    if (m_path.isEmpty())
        return;
    QFile file(m_path);
    if (!file.open(QIODevice::ReadOnly))
        return;
    const QJsonObject stored = QJsonDocument::fromJson(file.readAll()).object();
    if (stored.isEmpty())
        return;

    m_off.clear();
    const QJsonArray off = stored.value(QLatin1String("off")).toArray();
    for (const QJsonValue &kind : off) {
        if (kindNames().contains(kind.toString()))
            m_off.append(kind.toString());
    }
    m_suggestions = stored.value(QLatin1String("suggestions")).toBool(false);
    m_suggestionsPerHour = std::clamp(stored.value(QLatin1String("suggestions_per_hour")).toInt(2), 1, 12);
    m_speakPerHour = std::clamp(stored.value(QLatin1String("speak_per_hour")).toInt(6), 1, 60);
    m_quietUntil = qint64(stored.value(QLatin1String("quiet_until")).toDouble(0));
    m_hidden = stored.value(QLatin1String("hidden")).toBool(false);
    const QString presence = stored.value(QLatin1String("presence")).toString();
    m_presence = presence == QLatin1String("always") ? presence : QStringLiteral("with-pax");
    m_along = std::clamp(stored.value(QLatin1String("along")).toDouble(1.0), 0.0, 1.0);
    m_overFullScreen = stored.value(QLatin1String("over_full_screen")).toBool(true);
    m_told.clear();
    const QJsonArray told = stored.value(QLatin1String("told")).toArray();
    for (const QJsonValue &id : told)
        m_told.append(id.toString());

    Q_EMIT kindsChanged();
    Q_EMIT suggestionsChanged();
    Q_EMIT speakPerHourChanged();
    Q_EMIT quietUntilChanged();
    Q_EMIT hiddenChanged();
    Q_EMIT presenceChanged();
    Q_EMIT alongChanged();
    Q_EMIT overFullScreenChanged();
}

void CompanionSettings::save() const
{
    if (m_path.isEmpty())
        return;
    QDir().mkpath(QFileInfo(m_path).absolutePath());
    QJsonObject stored;
    stored.insert(QLatin1String("version"), 1);
    stored.insert(QLatin1String("off"), QJsonArray::fromStringList(m_off));
    stored.insert(QLatin1String("suggestions"), m_suggestions);
    stored.insert(QLatin1String("suggestions_per_hour"), m_suggestionsPerHour);
    stored.insert(QLatin1String("speak_per_hour"), m_speakPerHour);
    stored.insert(QLatin1String("quiet_until"), double(m_quietUntil));
    stored.insert(QLatin1String("hidden"), m_hidden);
    stored.insert(QLatin1String("presence"), m_presence);
    stored.insert(QLatin1String("along"), m_along);
    stored.insert(QLatin1String("over_full_screen"), m_overFullScreen);
    stored.insert(QLatin1String("told"), QJsonArray::fromStringList(m_told));
    QSaveFile file(m_path);
    if (!file.open(QIODevice::WriteOnly))
        return;
    file.write(QJsonDocument(stored).toJson(QJsonDocument::Indented));
    file.commit();
}
