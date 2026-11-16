// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#pragma once

#include "companionsettings.h"

#include <QList>
#include <QObject>
#include <QPointer>
#include <QString>
#include <QStringList>
#include <QTimer>
#include <QVariantList>
#include <QVariantMap>
#include <QtQml/qqmlregistration.h>

// P@x's character on the bar, as rules (docs/architecture/
// tur-pax-companion.md, sections 3 and 5). It draws nothing and talks to
// nothing: the sources (AgentSource, HealSource, the overlay's and the
// window's properties) tell it what is so, and it decides the one state the
// character shows and whether it may speak.
//
// The rules it holds:
//  - One state at a time, the one that tells the person most. While P@x's
//    indicator (tur-pax-overlay) says P@x is acting, observing or waiting,
//    the character says the same, whatever else is true.
//  - It speaks first (shows a bubble) only for a notice of a kind the person
//    has left switched on, that it has not said before, while it is not
//    quiet, not hidden, not already speaking, and has not spoken more than
//    the set number of times in the last hour. Anything else waits as a
//    small mark until the person asks for it; nothing pops up later by
//    itself.
//  - A notice has one sentence and at most two actions.
//  - Suggestions are refused unless the person switched them on, and then
//    at most a set number an hour; one that cannot be shown now is dropped,
//    not kept.
class Companion : public QObject
{
    Q_OBJECT
    QML_ELEMENT

    Q_PROPERTY(CompanionSettings *settings READ settings WRITE setSettings NOTIFY settingsChanged FINAL)

    // What the sources say.
    // P@x's service is running and switched on.
    Q_PROPERTY(bool paxOn READ paxOn WRITE setPaxOn NOTIFY inputsChanged FINAL)
    // org.tur.Agent1's State: off, idle, working or paused.
    Q_PROPERTY(QString serviceState READ serviceState WRITE setServiceState NOTIFY inputsChanged FINAL)
    // Why P@x cannot take a task although it is on: the service's own code
    // and sentence. Empty when it can.
    Q_PROPERTY(QVariantMap problem READ problem WRITE setProblem NOTIFY inputsChanged FINAL)
    // What P@x's indicator shows (org.tur.PaxOverlay1.Activity); empty when
    // the indicator is not running.
    Q_PROPERTY(QString overlayActivity READ overlayActivity WRITE setOverlayActivity NOTIFY inputsChanged FINAL)
    // The ask bar is open: the person is talking to P@x.
    Q_PROPERTY(bool askOpen READ askOpen WRITE setAskOpen NOTIFY inputsChanged FINAL)
    // The person has the character's menu or one of its pages open: nothing
    // new is said over it.
    Q_PROPERTY(bool attending READ attending WRITE setAttending NOTIFY inputsChanged FINAL)
    // How many things of P@x's wait for the person's yes or no.
    Q_PROPERTY(int waiting READ waiting WRITE setWaiting NOTIFY inputsChanged FINAL)
    // The computer runs on its battery.
    Q_PROPERTY(bool onBattery READ onBattery WRITE setOnBattery NOTIFY inputsChanged FINAL)
    // The share of the day's spending limit already used, 0 to 1; negative
    // when it is not known.
    Q_PROPERTY(double spent READ spent WRITE setSpent NOTIFY inputsChanged FINAL)
    // The step P@x is on, as the service described it; empty between steps.
    Q_PROPERTY(QVariantMap step READ step WRITE setStep NOTIFY inputsChanged FINAL)

    // What the character shows.
    Q_PROPERTY(QString state READ state NOTIFY stateChanged FINAL)
    // On the bar at all: not hidden, and P@x on or the person wants it there
    // asleep.
    Q_PROPERTY(bool shown READ shown NOTIFY shownChanged FINAL)
    // The notice being said; empty when none is.
    Q_PROPERTY(QVariantMap bubble READ bubble NOTIFY bubbleChanged FINAL)
    Q_PROPERTY(bool speaking READ speaking NOTIFY bubbleChanged FINAL)
    // Notices that wait for the person to ask.
    Q_PROPERTY(int heldCount READ heldCount NOTIFY heldChanged FINAL)
    Q_PROPERTY(bool quiet READ quiet NOTIFY quietChanged FINAL)
    // How much of the day's limit is left, 0 to 1: how high the inner flame
    // burns. 1 when the limit is not known.
    Q_PROPERTY(double fuel READ fuel NOTIFY inputsChanged FINAL)
    // On battery the character saves energy: nothing loops and it dims.
    Q_PROPERTY(bool saving READ saving NOTIFY inputsChanged FINAL)
    // While true the character belongs over a full-screen window too.
    Q_PROPERTY(bool raised READ raised NOTIFY raisedChanged FINAL)

    // For the tests: the time, in milliseconds since the epoch; 0 for the
    // computer's clock.
    Q_PROPERTY(double clock READ clock WRITE setClock NOTIFY clockChanged FINAL)
    // How long a bubble stays before it folds into a mark; 0: until the
    // person answers it.
    Q_PROPERTY(int bubbleSeconds READ bubbleSeconds WRITE setBubbleSeconds NOTIFY timingChanged FINAL)
    // How long "refused" and "error" are shown after a task ended that way.
    Q_PROPERTY(int moodSeconds READ moodSeconds WRITE setMoodSeconds NOTIFY timingChanged FINAL)

public:
    // Every state the character has.
    static QStringList states();
    // The longest sentence a notice may have.
    static int sentenceLimit() { return 280; }

    explicit Companion(QObject *parent = nullptr);

    CompanionSettings *settings() const { return m_settings; }
    void setSettings(CompanionSettings *settings);

    bool paxOn() const { return m_paxOn; }
    void setPaxOn(bool on);
    QString serviceState() const { return m_serviceState; }
    void setServiceState(const QString &state);
    QVariantMap problem() const { return m_problem; }
    void setProblem(const QVariantMap &problem);
    QString overlayActivity() const { return m_overlayActivity; }
    void setOverlayActivity(const QString &activity);
    bool askOpen() const { return m_askOpen; }
    void setAskOpen(bool open);
    bool attending() const { return m_attending; }
    void setAttending(bool attending);
    int waiting() const { return m_waiting; }
    void setWaiting(int count);
    bool onBattery() const { return m_onBattery; }
    void setOnBattery(bool onBattery);
    double spent() const { return m_spent; }
    void setSpent(double spent);
    QVariantMap step() const { return m_step; }
    void setStep(const QVariantMap &step);

    QString state() const { return m_state; }
    bool shown() const { return m_shown; }
    QVariantMap bubble() const { return m_bubble; }
    bool speaking() const { return !m_bubble.isEmpty(); }
    int heldCount() const { return int(m_held.size()); }
    bool quiet() const { return m_quiet; }
    double fuel() const;
    bool saving() const { return m_onBattery; }
    bool raised() const { return m_raised; }

    double clock() const { return double(m_clock); }
    void setClock(double clock);
    int bubbleSeconds() const { return m_bubbleSeconds; }
    void setBubbleSeconds(int seconds);
    int moodSeconds() const { return m_moodSeconds; }
    void setMoodSeconds(int seconds);

    // A source offers a notice: `id` (unique), `kind` (one of
    // CompanionSettings::kindNames()), `sender` ("tur" or "pax"),
    // `sentence`, and optionally `title`, `detail`, `code`, `commands`
    // (lines a person can run) and `actions` (identifiers; the first two
    // are kept). Returns what became of it: "shown", "held" (waits as a
    // mark: quiet, already speaking, or spoken too often this hour), "off"
    // (the person switched that kind off), "told" (said before), "hidden"
    // (the character is not on the bar: nothing is consumed) or "invalid".
    Q_INVOKABLE QString offer(const QVariantMap &notice);
    // The same for a suggestion (`kind` is set to "suggestion"). Returns
    // "shown", or why not: "off", "quiet", "busy", "limited", "hidden",
    // "told", "invalid". A suggestion is never held.
    Q_INVOKABLE QString suggest(const QVariantMap &notice);
    // The healing service asks the character to say one of its notices now
    // (org.tur.PaxCompanion1.Announce; design, section 7). Unlike offer,
    // nothing is kept: it is said now, or refused at once so that the
    // healing service shows the desktop's notification instead. Returns
    // "shown", or why not: "invalid", "hidden" (not on the bar, or asleep),
    // "told" (said before), "off" (the person switched that kind off),
    // "quiet", "busy" (already speaking; the ask bar or the menu is open)
    // or "limited" (it has spoken its number for the hour).
    Q_INVOKABLE QString announce(const QVariantMap &notice);
    // Takes back the bubble of the notice `id`, if it is the one being said,
    // without keeping it as a mark: an announcement that did not reach the
    // screen in time is the desktop notification's to make.
    Q_INVOKABLE void withdraw(const QString &id);

    // The person answered the bubble ("Not now", or one of its actions): it
    // is gone.
    Q_INVOKABLE void dismiss();
    // The bubble closes without an answer: it waits as a mark.
    Q_INVOKABLE void fold();
    // The person asked what waits: the oldest notice is shown, whatever the
    // hour's count. False when nothing waits.
    Q_INVOKABLE bool unfold();
    // A task ended refused or in an error: shown on the character for
    // moodSeconds, or until the person acknowledges it. "" clears it.
    Q_INVOKABLE void flag(const QString &mood);
    // "hour", "tomorrow", "always" or "off".
    Q_INVOKABLE void setQuiet(const QString &mode);
    // The time the rules go by.
    Q_INVOKABLE double now() const;

Q_SIGNALS:
    void settingsChanged();
    void inputsChanged();
    void stateChanged();
    void shownChanged();
    void bubbleChanged();
    void heldChanged();
    void quietChanged();
    void raisedChanged();
    void clockChanged();
    void timingChanged();

private:
    void update();
    void updateQuiet();
    QString resolve() const;
    QVariantMap checked(const QVariantMap &notice, const QString &kind) const;
    bool waitingInQueue(const QString &id) const;
    int spokenLastHour(const QList<qint64> &times) const;
    void show(const QVariantMap &notice);

    QPointer<CompanionSettings> m_settings;
    bool m_paxOn = false;
    QString m_serviceState;
    QVariantMap m_problem;
    QString m_overlayActivity;
    bool m_askOpen = false;
    bool m_attending = false;
    int m_waiting = 0;
    bool m_onBattery = false;
    double m_spent = -1;
    QVariantMap m_step;

    QString m_state = QStringLiteral("asleep");
    bool m_shown = false;
    bool m_quiet = false;
    bool m_raised = false;
    QVariantMap m_bubble;
    QList<QVariantMap> m_held;
    QString m_mood;
    QList<qint64> m_spoken;
    QList<qint64> m_suggested;

    qint64 m_clock = 0;
    int m_bubbleSeconds = 12;
    int m_moodSeconds = 30;
    QTimer m_bubbleTimer;
    QTimer m_moodTimer;
    QTimer m_quietTimer;
};
