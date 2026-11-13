// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#include "transcript.h"

#include <QDateTime>
#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QTextStream>

namespace TranscriptFile {

QString path()
{
    static const QString chosen = [] {
        const QString given = qEnvironmentVariable("TUR_WIZARD_TRANSCRIPT");
        if (!given.isEmpty())
            return given;
        const QFileInfo directory(QStringLiteral("/run/tur-installer/wizard"));
        if (directory.isDir() && directory.isWritable())
            return directory.filePath() + QStringLiteral("/transcript");
        return QString();
    }();
    return chosen;
}

void write(const QString &line)
{
    const QString target = path();
    if (target.isEmpty())
        return;
    QFile file(target);
    if (!file.open(QIODevice::WriteOnly | QIODevice::Append | QIODevice::Text))
        return;
    QString clean = line;
    clean.replace(QLatin1Char('\n'), QLatin1Char(' '));
    clean.replace(QLatin1Char('\r'), QLatin1Char(' '));
    QTextStream(&file) << "TUR-WIZARD: " << clean << '\n';
}

} // namespace TranscriptFile

Transcript::Transcript(QObject *parent)
    : QObject(parent)
{
}

void Transcript::write(const QString &line)
{
    TranscriptFile::write(line);
}
