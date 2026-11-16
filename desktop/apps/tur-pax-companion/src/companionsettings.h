// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#pragma once

#include <QObject>
#include <QString>
#include <QStringList>
#include <QVariantMap>
#include <QtQml/qqmlregistration.h>

// What the person chose for P@x's character on the bar
// (docs/architecture/tur-pax-companion.md, section 5): which kinds of notice
// it may speak first about, whether it is quiet and until when, whether it
// is hidden, where along the bar it sits, and whether it may make
// suggestions. Kept in one small JSON file of the person's
// ($XDG_CONFIG_HOME/tur/pax-companion.json); with an empty `path` nothing is
// read or written, which is what the tests and the pictures use.
//
// The file also remembers which notices the character has already said, so
// that a notice is said once.
class CompanionSettings : public QObject
{
    Q_OBJECT
    QML_ELEMENT

    Q_PROPERTY(QString path READ path WRITE setPath NOTIFY pathChanged FINAL)
    // Each kind of notice (kindNames()) and whether the character may speak
    // first about it.
    Q_PROPERTY(QVariantMap kinds READ kinds NOTIFY kindsChanged FINAL)
    // "It looks like you…": off unless the person switches it on.
    Q_PROPERTY(bool suggestions READ suggestions WRITE setSuggestions NOTIFY suggestionsChanged FINAL)
    Q_PROPERTY(int suggestionsPerHour READ suggestionsPerHour WRITE setSuggestionsPerHour NOTIFY suggestionsChanged FINAL)
    // How many times an hour the character may speak first, of any kind.
    Q_PROPERTY(int speakPerHour READ speakPerHour WRITE setSpeakPerHour NOTIFY speakPerHourChanged FINAL)
    // Milliseconds since the epoch until which the character says nothing;
    // 0: not quiet; -1: quiet until the person says otherwise.
    Q_PROPERTY(double quietUntil READ quietUntil NOTIFY quietUntilChanged FINAL)
    // Taken off the bar by the person.
    Q_PROPERTY(bool hidden READ hidden WRITE setHidden NOTIFY hiddenChanged FINAL)
    // "with-pax": on the bar while P@x is switched on, absent while it is
    // off. "always": on the bar, asleep while P@x is off.
    Q_PROPERTY(QString presence READ presence WRITE setPresence NOTIFY presenceChanged FINAL)
    // Where along the bar, from its start (0) to its end (1).
    Q_PROPERTY(double along READ along WRITE setAlong NOTIFY alongChanged FINAL)
    // Whether the character stays over a full-screen window, or steps aside
    // with the bar and comes back only while P@x acts or waits.
    Q_PROPERTY(bool overFullScreen READ overFullScreen WRITE setOverFullScreen NOTIFY overFullScreenChanged FINAL)

public:
    // repair, fault, approval, task, update.
    static QStringList kindNames();
    // The file a person's choices are kept in.
    static QString defaultPath();

    explicit CompanionSettings(QObject *parent = nullptr);

    QString path() const { return m_path; }
    void setPath(const QString &path);
    QVariantMap kinds() const;
    bool suggestions() const { return m_suggestions; }
    void setSuggestions(bool on);
    int suggestionsPerHour() const { return m_suggestionsPerHour; }
    void setSuggestionsPerHour(int count);
    int speakPerHour() const { return m_speakPerHour; }
    void setSpeakPerHour(int count);
    double quietUntil() const { return double(m_quietUntil); }
    bool hidden() const { return m_hidden; }
    void setHidden(bool hidden);
    QString presence() const { return m_presence; }
    void setPresence(const QString &presence);
    double along() const { return m_along; }
    void setAlong(double along);
    bool overFullScreen() const { return m_overFullScreen; }
    void setOverFullScreen(bool stay);

    Q_INVOKABLE bool kindOn(const QString &kind) const;
    Q_INVOKABLE void setKind(const QString &kind, bool on);

    // "hour", "tomorrow", "always" or "off", counted from `now`
    // (milliseconds since the epoch). "tomorrow" is 07:00 of the next day,
    // local time. False for anything else.
    Q_INVOKABLE bool setQuiet(const QString &mode, double now);
    // Whether the character is quiet at `now`.
    Q_INVOKABLE bool quietAt(double now) const;

    // Whether the notice of that identifier was already said.
    bool told(const QString &id) const;
    void markTold(const QString &id);

Q_SIGNALS:
    void pathChanged();
    void kindsChanged();
    void suggestionsChanged();
    void speakPerHourChanged();
    void quietUntilChanged();
    void hiddenChanged();
    void presenceChanged();
    void alongChanged();
    void overFullScreenChanged();

private:
    void load();
    void save() const;

    QString m_path;
    QStringList m_off;
    bool m_suggestions = false;
    int m_suggestionsPerHour = 2;
    int m_speakPerHour = 6;
    qint64 m_quietUntil = 0;
    bool m_hidden = false;
    QString m_presence = QStringLiteral("with-pax");
    double m_along = 1.0;
    // Step aside by default: the owner's decision of 15 November 2026.
    bool m_overFullScreen = false;
    QStringList m_told;
};
