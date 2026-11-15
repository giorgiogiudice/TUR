// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#pragma once

#include "companion.h"

#include <QJsonArray>
#include <QJsonObject>
#include <QList>
#include <QObject>
#include <QPointer>
#include <QProcess>
#include <QString>
#include <QStringList>
#include <QVariantMap>
#include <QtQml/qqmlregistration.h>

// What the character learns from the self-healing service
// (docs/architecture/self-healing.md, section 4; tur-pax-companion.md,
// section 7): the notices this person has not seen, as
// `tur-diag heal notices --json` prints them (an array of `id`, `at_utc`,
// `code`, `kind`, `title`, `body`, `actions`). The program is run as a
// separate program, which is the command-line side of the licence boundary
// and the interface the healing design names; nothing of it is linked.
//
// The character never speaks of a healing notice by itself. The healing
// service's own program, `tur-diag heal notify`, asks it
// (org.tur.PaxCompanion1.Announce, the owner's decision of 15 November
// 2026): the character then runs `tur-diag heal notices --json`, finds the
// notice by its identifier, and says it if it can say it now. If it cannot
// (hidden, asleep, quiet, busy, switched off, its number for the hour
// spoken), it says no at once and keeps nothing, and `notify` shows the
// desktop's notification instead. It never runs the program with
// --mark-shown: `notify` is the only writer of what was seen.
//
// A notice's words are the healing service's, unchanged. The character's
// sentence is its `body`, or its `title` when the body is too long for one
// bubble; its commands are shown as text for the person to run, never run
// by the character.
class HealSource : public QObject
{
    Q_OBJECT
    QML_ELEMENT

    Q_PROPERTY(Companion *companion READ companion WRITE setCompanion NOTIFY companionChanged FINAL)
    // The program and its arguments; empty: tur-diag heal notices --json.
    Q_PROPERTY(QStringList command READ command WRITE setCommand NOTIFY commandChanged FINAL)
    // Why the notices could not be read; empty when they could.
    Q_PROPERTY(QString problem READ problem NOTIFY problemChanged FINAL)
    // How many notices the last reading held.
    Q_PROPERTY(int count READ count NOTIFY countChanged FINAL)

public:
    static QStringList defaultCommand();

    // The character's notice for one of the healing service's; empty when
    // the object is not one, or is of a kind the person is not told of.
    static QVariantMap noticeFrom(const QJsonObject &heal);
    // The character's kind for an event of the healing service: "repair"
    // for what was put right, "fault" for what waits for the person.
    static QString kindOf(const QString &event);

    explicit HealSource(QObject *parent = nullptr);

    Companion *companion() const { return m_companion; }
    void setCompanion(Companion *companion);
    QStringList command() const { return m_command; }
    void setCommand(const QStringList &command);
    QString problem() const { return m_problem; }
    int count() const { return m_count; }

    // The healing service asks the character to say the notice `id`. The
    // program is run, the notice found, and Companion::announce decides;
    // `announced` says what became of it: "shown", one of
    // Companion::announce's refusals, "unknown" (no such notice is waiting)
    // or "unread" (the notices could not be read). Asks made while the
    // program runs are answered from the next reading.
    Q_INVOKABLE void announce(qint64 id);
    // Offers every notice of a JSON array as the program prints it, as if
    // the healing service had asked for each: for the tests and the
    // pictures. Returns how many notices it held, or -1 if it is not such
    // an array.
    Q_INVOKABLE int ingest(const QString &json);

Q_SIGNALS:
    void companionChanged();
    void commandChanged();
    void problemChanged();
    void countChanged();
    void announced(qint64 id, const QString &result);

private:
    void run();
    void setProblem(const QString &problem);
    void finished(int exitCode, QProcess::ExitStatus status);
    void answer(const QList<qint64> &ids, const QJsonArray *notices);

    QPointer<Companion> m_companion;
    QStringList m_command;
    QString m_problem;
    int m_count = 0;
    // Asked for, waiting for the program that runs now (m_reading) or for
    // the next run (m_asked).
    QList<qint64> m_reading;
    QList<qint64> m_asked;
    QProcess m_process;
};
