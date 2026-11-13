// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#pragma once

#include <QObject>
#include <QString>
#include <QtQml/qqmlregistration.h>

// The installer's state lines, "TUR-WIZARD: ...". A person never needs them.
// They go to the file TUR_WIZARD_TRANSCRIPT names, or on the installer medium
// to /run/tur-installer/wizard/transcript, from where a service copies them
// to the first serial port (tests/iso reads them while it drives the
// installer with the keyboard), and the engine copies them to the installed
// disk with the rest of the log. Nothing is written anywhere else, and
// nothing a person types is written except the confirmation phrase's
// comparison result.
namespace TranscriptFile {
// Appends one state line. Thread-unsafe; the installer has one thread.
void write(const QString &line);
// Where lines go; empty when nowhere.
QString path();
}

// The same, for QML: Transcript.write("screen name=welcome").
class Transcript : public QObject
{
    Q_OBJECT
    QML_ELEMENT
    QML_SINGLETON

public:
    explicit Transcript(QObject *parent = nullptr);

    Q_INVOKABLE void write(const QString &line);
};
