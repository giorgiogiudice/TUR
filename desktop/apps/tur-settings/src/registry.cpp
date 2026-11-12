// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#include "registry.h"

#include "address.h"

#include <QDir>
#include <QFile>
#include <QFileInfo>

using Settings::Address;
using Settings::Hit;
using Settings::Page;
using Settings::Row;

namespace {

// The registries this program carries: its own, and the few pages of KDE
// System Settings that its pages link to.
const char *const Embedded[] = {
    ":/tur/settings/registry/org.tur.Settings.json",
    ":/tur/settings/registry/org.kde.systemsettings.json",
};

QByteArray readAll(const QString &path, QString *error)
{
    QFile file(path);
    if (!file.open(QIODevice::ReadOnly)) {
        *error = file.errorString();
        return QByteArray();
    }
    // One byte more than the limit, so that the loader can say "too large".
    return file.read(Settings::RegistryData::MaximumFileSize + 1);
}

} // namespace

Registry::Registry(QObject *parent)
    : QObject(parent)
{
    reload();
}

QString Registry::ownProgram()
{
    return QStringLiteral("org.tur.Settings");
}

void Registry::setDirectory(const QString &directory)
{
    if (directory == m_directory)
        return;
    m_directory = directory;
    reload();
}

void Registry::reload()
{
    m_data.clear();
    m_problems.clear();

    for (const char *path : Embedded) {
        QString error;
        const QString name = QString::fromLatin1(path);
        const QByteArray json = readAll(name, &error);
        if (!error.isEmpty())
            m_problems.append(name + QLatin1String(": ") + error);
        else
            m_data.add(json, QFileInfo(name).fileName(), &m_problems);
    }

    if (!m_directory.isEmpty()) {
        const QDir directory(m_directory);
        const QStringList names = directory.entryList({ QStringLiteral("*.json") }, QDir::Files, QDir::Name);
        for (const QString &name : names) {
            // A program's registry is named after the program. The ones this
            // program carries are not read a second time from a directory
            // they were also installed in.
            if (m_data.programs().contains(QFileInfo(name).completeBaseName()))
                continue;
            QString error;
            const QByteArray json = readAll(directory.filePath(name), &error);
            if (!error.isEmpty())
                m_problems.append(name + QLatin1String(": ") + error);
            else
                m_data.add(json, name, &m_problems);
        }
    }
    m_problems += m_data.crossProblems();

    ++m_revision;
    Q_EMIT changed();
}

QVariantList Registry::navigation() const
{
    QVariantList out;
    QString group;
    for (const Page &page : m_data.pages()) {
        if (page.program != ownProgram() || !page.listed || page.isRemoved())
            continue;
        const QString inGroup = page.path.size() > 1 ? page.path.constFirst() : QString();
        if (inGroup != group && !inGroup.isEmpty()) {
            QVariantMap heading;
            heading.insert(QStringLiteral("header"), true);
            heading.insert(QStringLiteral("address"), QString());
            heading.insert(QStringLiteral("title"), inGroup);
            heading.insert(QStringLiteral("group"), inGroup);
            heading.insert(QStringLiteral("depth"), 0);
            heading.insert(QStringLiteral("available"), false);
            out.append(heading);
        }
        group = inGroup;
        QVariantMap item;
        item.insert(QStringLiteral("header"), false);
        item.insert(QStringLiteral("address"), page.address);
        item.insert(QStringLiteral("title"), page.title);
        item.insert(QStringLiteral("group"), inGroup);
        item.insert(QStringLiteral("depth"), inGroup.isEmpty() ? 0 : 1);
        item.insert(QStringLiteral("available"), page.isAvailable());
        out.append(item);
    }
    return out;
}

QVariantMap Registry::entry(const QString &address, int) const
{
    QVariantMap out;
    const Address parsed = Address::parse(address);
    if (!parsed.isValid() || address != parsed.toString())
        return out;
    const Page *page = m_data.page(parsed.page);
    if (!page)
        return out;
    const Row *row = parsed.hasRow() ? page->row(parsed.row) : nullptr;
    if (parsed.hasRow() && !row)
        return out;

    out.insert(QStringLiteral("address"), address);
    out.insert(QStringLiteral("page"), page->address);
    out.insert(QStringLiteral("row"), parsed.row);
    out.insert(QStringLiteral("program"), page->program);
    out.insert(QStringLiteral("pageTitle"), page->title);
    out.insert(QStringLiteral("pageStatus"), page->status);
    out.insert(QStringLiteral("path"), page->path);
    out.insert(QStringLiteral("module"), page->module);
    if (!row) {
        out.insert(QStringLiteral("status"), page->status);
        out.insert(QStringLiteral("replacedBy"), page->replacedBy);
        out.insert(QStringLiteral("title"), page->title);
        out.insert(QStringLiteral("kind"), QStringLiteral("page"));
        out.insert(QStringLiteral("changeClass"), -1);
        return out;
    }
    out.insert(QStringLiteral("status"), row->isRemoved() ? row->status : page->status);
    out.insert(QStringLiteral("replacedBy"), row->replacedBy);
    out.insert(QStringLiteral("title"), row->title);
    out.insert(QStringLiteral("kind"), row->kind);
    out.insert(QStringLiteral("section"), row->section);
    out.insert(QStringLiteral("sectionTitle"), page->sectionTitle(row->section));
    out.insert(QStringLiteral("changeClass"), row->changeClass);
    out.insert(QStringLiteral("tool"), row->tool);
    out.insert(QStringLiteral("sourceType"), row->sourceType);
    out.insert(QStringLiteral("sourceAvailable"), row->sourceAvailable);
    out.insert(QStringLiteral("waitsFor"), row->waitsFor);
    out.insert(QStringLiteral("opens"), row->opens);
    return out;
}

QString Registry::sectionTitle(const QString &page, const QString &section, int) const
{
    const Page *entry = m_data.page(page);
    return entry ? entry->sectionTitle(section) : QString();
}

QStringList Registry::rowsOf(const QString &page) const
{
    QStringList out;
    if (const Page *entry = m_data.page(page)) {
        for (const Row &row : entry->rows) {
            if (!row.isRemoved())
                out.append(entry->address + QLatin1Char('#') + row.id);
        }
    }
    return out;
}

QStringList Registry::ownPages() const
{
    QStringList out;
    for (const Page &page : m_data.pages()) {
        if (page.program == ownProgram() && page.isAvailable())
            out.append(page.address);
    }
    return out;
}

QVariantList Registry::search(const QString &text, int limit, int) const
{
    QVariantList out;
    const QList<Hit> hits = m_data.search(text, limit);
    for (const Hit &hit : hits) {
        QVariantMap item;
        item.insert(QStringLiteral("address"), hit.address);
        item.insert(QStringLiteral("title"), hit.title);
        item.insert(QStringLiteral("score"), hit.score);
        item.insert(QStringLiteral("path"), hit.path);
        item.insert(QStringLiteral("section"), hit.section);
        item.insert(QStringLiteral("kind"), hit.kind);
        item.insert(QStringLiteral("program"), hit.program);
        out.append(item);
    }
    return out;
}
