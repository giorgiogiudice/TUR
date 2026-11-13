// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#include "keyboard.h"

#include "transcript.h"

#include <QByteArrayList>
#include <QDBusConnection>
#include <QDBusMessage>
#include <QDBusMetaType>
#include <QDir>
#include <QFile>
#include <QHash>
#include <QRegularExpression>
#include <QSaveFile>
#include <QStandardPaths>
#include <QStringList>

#include <algorithm>
#include <utility>

namespace {

// When xkeyboard-config's list is not there: the layouts of the languages
// the design names first, so that the screen is never empty.
QVariantList fallbackLayouts()
{
    const QList<std::pair<QString, QString>> known {
        { QStringLiteral("us"), QStringLiteral("English (US)") },
        { QStringLiteral("gb"), QStringLiteral("English (UK)") },
        { QStringLiteral("de"), QStringLiteral("German") },
        { QStringLiteral("es"), QStringLiteral("Spanish") },
        { QStringLiteral("fr"), QStringLiteral("French") },
        { QStringLiteral("it"), QStringLiteral("Italian") },
    };
    QVariantList out;
    for (const auto &[code, name] : known)
        out << QVariantMap { { QStringLiteral("code"), code }, { QStringLiteral("name"), name } };
    return out;
}

} // namespace

KeyboardLayouts::KeyboardLayouts(QObject *parent)
    : QObject(parent)
{
    qDBusRegisterMetaType<QByteArrayList>();
    qDBusRegisterMetaType<QHash<QString, QByteArrayList>>();
    load();
}

QVariantList KeyboardLayouts::parseRules(const QString &text)
{
    QVariantList out;
    bool inLayouts = false;
    static const QRegularExpression entry(QStringLiteral("^\\s+([A-Za-z0-9_-]+)\\s+(.+?)\\s*$"));
    const QStringList lines = text.split(QLatin1Char('\n'));
    for (const QString &line : lines) {
        if (line.startsWith(QLatin1Char('!'))) {
            inLayouts = line.trimmed() == QStringLiteral("! layout");
            continue;
        }
        if (!inLayouts)
            continue;
        const QRegularExpressionMatch match = entry.match(line);
        if (match.hasMatch())
            out << QVariantMap { { QStringLiteral("code"), match.captured(1) }, { QStringLiteral("name"), match.captured(2) } };
    }
    std::sort(out.begin(), out.end(), [](const QVariant &a, const QVariant &b) {
        return QString::localeAwareCompare(a.toMap().value(QStringLiteral("name")).toString(),
                                           b.toMap().value(QStringLiteral("name")).toString())
               < 0;
    });
    return out;
}

void KeyboardLayouts::load()
{
    QFile file(m_rulesFile);
    m_layouts.clear();
    if (file.open(QIODevice::ReadOnly | QIODevice::Text))
        m_layouts = parseRules(QString::fromUtf8(file.readAll()));
    if (m_layouts.isEmpty())
        m_layouts = fallbackLayouts();

    // What the live user has now: the first layout of kxkbrc, or US.
    QFile config(QDir(configDirectory()).filePath(QStringLiteral("kxkbrc")));
    if (config.open(QIODevice::ReadOnly | QIODevice::Text)) {
        bool inLayout = false;
        const QStringList lines = QString::fromUtf8(config.readAll()).split(QLatin1Char('\n'));
        for (const QString &line : lines) {
            const QString trimmed = line.trimmed();
            if (trimmed.startsWith(QLatin1Char('['))) {
                inLayout = trimmed == QStringLiteral("[Layout]");
                continue;
            }
            if (inLayout && trimmed.startsWith(QStringLiteral("LayoutList="))) {
                const QString first = trimmed.mid(11).split(QLatin1Char(',')).value(0).trimmed();
                if (!first.isEmpty())
                    m_current = first;
            }
        }
    }
    Q_EMIT layoutsChanged();
    Q_EMIT currentChanged();
}

QString KeyboardLayouts::configDirectory() const
{
    if (!m_configDirectory.isEmpty())
        return m_configDirectory;
    return QStandardPaths::writableLocation(QStandardPaths::GenericConfigLocation);
}

void KeyboardLayouts::setConfigDirectory(const QString &directory)
{
    if (directory == m_configDirectory)
        return;
    m_configDirectory = directory;
    Q_EMIT configDirectoryChanged();
    load();
}

void KeyboardLayouts::setRulesFile(const QString &file)
{
    if (file == m_rulesFile)
        return;
    m_rulesFile = file;
    load();
}

int KeyboardLayouts::indexOf(const QString &code) const
{
    for (qsizetype i = 0; i < m_layouts.size(); ++i) {
        if (m_layouts.at(i).toMap().value(QStringLiteral("code")).toString() == code)
            return int(i);
    }
    return -1;
}

QString KeyboardLayouts::layoutGroup(const QString &code)
{
    return QStringLiteral("[Layout]\nLayoutList=%1\nUse=true\n").arg(code);
}

QString KeyboardLayouts::withGroup(const QString &text, const QString &group)
{
    QStringList kept;
    bool skipping = false;
    const QStringList lines = text.split(QLatin1Char('\n'));
    for (const QString &line : lines) {
        const QString trimmed = line.trimmed();
        if (trimmed.startsWith(QLatin1Char('[')))
            skipping = trimmed == QStringLiteral("[Layout]");
        if (!skipping)
            kept << line;
    }
    while (!kept.isEmpty() && kept.last().trimmed().isEmpty())
        kept.removeLast();
    QString out = kept.join(QLatin1Char('\n'));
    if (!out.isEmpty())
        out += QStringLiteral("\n\n");
    return out + group;
}

bool KeyboardLayouts::apply(const QString &code)
{
    if (indexOf(code) < 0)
        return false;
    const QString directory = configDirectory();
    QDir().mkpath(directory);
    const QString path = QDir(directory).filePath(QStringLiteral("kxkbrc"));
    QString text;
    {
        QFile old(path);
        if (old.open(QIODevice::ReadOnly | QIODevice::Text))
            text = QString::fromUtf8(old.readAll());
    }
    QSaveFile file(path);
    if (!file.open(QIODevice::WriteOnly | QIODevice::Text))
        return false;
    file.write(withGroup(text, layoutGroup(code)).toUtf8());
    if (!file.commit())
        return false;
    m_current = code;
    Q_EMIT currentChanged();

    // KConfig's change notification, as KConfig itself sends it when a
    // program writes with KConfig::Notify.
    QDBusConnection bus = QDBusConnection::sessionBus();
    if (bus.isConnected()) {
        QDBusMessage message = QDBusMessage::createSignal(QStringLiteral("/kxkbrc"), QStringLiteral("org.kde.kconfig.notify"),
                                                          QStringLiteral("ConfigChanged"));
        QHash<QString, QByteArrayList> changes;
        changes.insert(QStringLiteral("Layout"), QByteArrayList { QByteArrayLiteral("LayoutList"), QByteArrayLiteral("Use") });
        message.setArguments({ QVariant::fromValue(changes) });
        bus.send(message);
    }
    TranscriptFile::write(QStringLiteral("keyboard layout=%1").arg(code));
    return true;
}
