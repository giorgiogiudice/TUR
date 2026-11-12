// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#pragma once

#include <QByteArray>
#include <QHash>
#include <QJsonObject>
#include <QList>
#include <QString>
#include <QStringList>

namespace Settings {

// A titled group of rows on a page. Not addressable.
struct Section
{
    QString id;
    QString title;
};

// One row of a page, as its registry describes it.
struct Row
{
    QString id;
    QString title;
    QStringList keywords;
    QString kind;       // switch, choice, number, text, list, action, information; empty if removed
    QString section;    // the identifier of a section of its page, or empty
    QString status;     // available or removed
    QString replacedBy; // an address, when a removed row has a successor
    int changeClass = -1; // the action class of changing it; -1 for information
    QString tool;         // the typed tool that changes it, when there is one
    QString sourceType;   // what backs it: file, dbus, command, kde-config, platform, program, address
    bool sourceAvailable = true;
    QString waitsFor; // what is missing, when the source is not available
    QString opens;    // the address a row of source type "address" leads to
    QJsonObject json; // the entry as the file has it

    bool isRemoved() const { return status == QLatin1String("removed"); }
};

// One page, as its registry describes it.
struct Page
{
    QString address;
    QString title;
    QStringList path;
    QStringList keywords;
    QString status;     // available, planned or removed
    QString replacedBy;
    QString program;    // the program whose registry holds the page
    bool listed = true;
    QString module;               // a module of KDE System Settings, for a kde page
    QStringList moduleArguments;
    QString verifiedWith;
    QList<Section> sections;
    QList<Row> rows;
    QJsonObject json;

    bool isAvailable() const { return status == QLatin1String("available"); }
    bool isRemoved() const { return status == QLatin1String("removed"); }
    const Row *row(const QString &id) const;
    QString sectionTitle(const QString &id) const;
};

// One match of a search.
struct Hit
{
    QString address;
    QString title;
    double score = 0; // from 0 to 1
    QStringList path; // the page's path
    QString section;  // the title of the row's section, or empty
    QString kind;     // "page", or the row's kind
    QString program;
};

// The settings registries of the programs on this system, merged
// (docs/architecture/desktop-experience.md, section 10.3). The format is
// schemas/settings-registry.schema.json; this loader applies the same rules
// and is the one the program trusts. It reads; it decides nothing.
//
// A file that breaks a rule is not loaded at all: half a registry would
// make some addresses resolve and others not, with nothing to say which.
class RegistryData
{
public:
    // Adds one registry file. `name` is how the file is called in a message.
    // Returns false, and says why in `problems`, if the file breaks a rule;
    // nothing of it is kept then.
    bool add(const QByteArray &json, const QString &name, QStringList *problems);

    // The rules that span files, checked once everything is added: an
    // address named by replaced_by, or by a row that opens another address,
    // exists and is not removed. Returns what it found; the registry stays
    // loaded.
    QStringList crossProblems() const;

    void clear();

    const QList<Page> &pages() const { return m_pages; }
    const Page *page(const QString &address) const;
    QStringList programs() const { return m_programs; }

    // The registry file of a program, as it was read.
    QByteArray document(const QString &program) const { return m_documents.value(program); }

    // The rows and pages that match some words, best first: a row or a page
    // that is there, on a page that can be opened. Every word must match the
    // title, a keyword, the section or the page. At most `limit` matches.
    QList<Hit> search(const QString &text, int limit) const;

    static constexpr int MaximumFileSize = 1024 * 1024;

private:
    QList<Page> m_pages;
    QHash<QString, int> m_index; // page address -> index in m_pages
    QStringList m_programs;
    QHash<QString, QByteArray> m_documents;
};

} // namespace Settings
