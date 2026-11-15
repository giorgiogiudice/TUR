// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#pragma once

#include "companion.h"

#include <QFileSystemWatcher>
#include <QJsonObject>
#include <QObject>
#include <QPointer>
#include <QProcess>
#include <QString>
#include <QStringList>
#include <QTimer>
#include <QVariantMap>
#include <QtQml/qqmlregistration.h>

// What the character learns from the self-healing service
// (docs/architecture/self-healing.md, section 4): the notices this person
// has not seen, as `tur-diag heal notices --json` prints them (an array of
// `id`, `at_utc`, `code`, `kind`, `title`, `body`, `actions`). The program
// is run as a separate program, which is the command-line side of the
// licence boundary and the interface the healing design names; nothing of
// it is linked.
//
// It is run when the character starts and whenever the healing service's
// state file changes. It is never run with --mark-shown: who marks a notice
// seen is the healing service's to decide (tur-pax-companion.md, section 7).
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
    // The file whose change means there may be something new.
    Q_PROPERTY(QString stateFile READ stateFile WRITE setStateFile NOTIFY stateFileChanged FINAL)
    // Why the notices could not be read; empty when they could.
    Q_PROPERTY(QString problem READ problem NOTIFY problemChanged FINAL)
    // How many notices the last reading held.
    Q_PROPERTY(int count READ count NOTIFY countChanged FINAL)

public:
    static QStringList defaultCommand();
    static QString defaultStateFile();

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
    QString stateFile() const { return m_stateFile; }
    void setStateFile(const QString &file);
    QString problem() const { return m_problem; }
    int count() const { return m_count; }

    // Starts watching the state file, and reads once.
    Q_INVOKABLE void start();
    // Runs the program and offers what it prints.
    Q_INVOKABLE void refresh();
    // Offers the notices of a JSON array as the program prints it. Returns
    // how many notices it held, or -1 if it is not such an array.
    Q_INVOKABLE int ingest(const QString &json);

Q_SIGNALS:
    void companionChanged();
    void commandChanged();
    void stateFileChanged();
    void problemChanged();
    void countChanged();

private:
    void watch();
    void setProblem(const QString &problem);
    void finished(int exitCode, QProcess::ExitStatus status);

    QPointer<Companion> m_companion;
    QStringList m_command;
    QString m_stateFile;
    QString m_problem;
    int m_count = 0;
    bool m_started = false;
    bool m_again = false;
    QProcess m_process;
    QFileSystemWatcher m_watcher;
    QTimer m_settle;
};
