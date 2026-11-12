// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#include "systemfacts.h"

#include "codes.h"

#include <QCoreApplication>
#include <QFile>
#include <QFileInfo>
#include <QList>
#include <QRegularExpression>
#include <QStringList>

#include <utility>

namespace {

// Reads a file of KEY=VALUE lines. False, with the reason, if it cannot be
// read.
bool readKeyValues(const QString &path, QHash<QString, QString> *out, QString *error)
{
    QFile file(path);
    if (!file.open(QIODevice::ReadOnly)) {
        *error = file.errorString();
        return false;
    }
    *out = SystemFacts::parse(file.read(SystemFacts::MaximumFileSize));
    return true;
}

} // namespace

SystemFacts::SystemFacts(QObject *parent)
    : QObject(parent)
{
    reload();
}

void SystemFacts::setFixture(const QString &fixture)
{
    if (fixture == m_fixture)
        return;
    m_fixture = fixture;
    reload();
}

QString SystemFacts::programVersion() const
{
    return QCoreApplication::applicationVersion();
}

QString SystemFacts::qtVersion() const
{
    return QString::fromLatin1(qVersion());
}

QHash<QString, QString> SystemFacts::parse(const QByteArray &text)
{
    QHash<QString, QString> out;
    static const QRegularExpression keyPattern(QStringLiteral("\\A[A-Za-z_][A-Za-z0-9_]*\\z"));
    const QList<QByteArray> lines = text.split('\n');
    for (const QByteArray &raw : lines) {
        const QString line = QString::fromUtf8(raw).trimmed();
        if (line.isEmpty() || line.startsWith(QLatin1Char('#')))
            continue;
        const qsizetype equals = line.indexOf(QLatin1Char('='));
        if (equals <= 0)
            continue;
        const QString key = line.left(equals);
        if (!keyPattern.match(key).hasMatch())
            continue;
        QString value = line.mid(equals + 1);
        if (value.size() >= 2 && value.startsWith(QLatin1Char('"')) && value.endsWith(QLatin1Char('"'))) {
            // Inside double quotes a backslash protects ", $, ` and itself.
            const QString inner = value.mid(1, value.size() - 2);
            value.clear();
            for (qsizetype i = 0; i < inner.size(); ++i) {
                const QChar ch = inner.at(i);
                if (ch == QLatin1Char('\\') && i + 1 < inner.size()
                    && QStringLiteral("\"$`\\").contains(inner.at(i + 1))) {
                    value += inner.at(++i);
                } else {
                    value += ch;
                }
            }
        } else if (value.size() >= 2 && value.startsWith(QLatin1Char('\'')) && value.endsWith(QLatin1Char('\''))) {
            value = value.mid(1, value.size() - 2);
        }
        out.insert(key, value);
    }
    return out;
}

void SystemFacts::reload()
{
    m_release.clear();
    m_os.clear();
    m_failure.clear();

    QStringList osCandidates;
    if (m_fixture.isEmpty()) {
        m_releasePath = QStringLiteral("/usr/lib/tur/release");
        osCandidates = { QStringLiteral("/etc/os-release"), QStringLiteral("/usr/lib/os-release") };
    } else {
        m_releasePath = m_fixture + QLatin1String("/tur-release");
        osCandidates = { m_fixture + QLatin1String("/os-release") };
    }

    QString osError;
    m_osPath = osCandidates.constFirst();
    for (const QString &candidate : std::as_const(osCandidates)) {
        m_osPath = candidate;
        osError.clear();
        if (readKeyValues(candidate, &m_os, &osError))
            break;
    }

    QString releaseError;
    if (!readKeyValues(m_releasePath, &m_release, &releaseError)) {
        QStringList detail = { QStringLiteral("%1: %2").arg(m_releasePath, releaseError) };
        if (!osError.isEmpty())
            detail.append(QStringLiteral("%1: %2").arg(m_osPath, osError));
        else
            detail.append(QStringLiteral("%1: ID=%2").arg(m_osPath, systemId()));
        m_failure = Settings::failure(
            Settings::Code::SystemDescriptionUnreadable,
            tr("The file that records TUR's version and build could not be read. Either this is not a TUR system, "
               "or the system is damaged."),
            detail.join(QLatin1Char('\n')));
    }
    Q_EMIT changed();
}
