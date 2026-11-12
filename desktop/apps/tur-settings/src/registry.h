// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#pragma once

#include "registrydata.h"

#include <QObject>
#include <QString>
#include <QStringList>
#include <QVariantList>
#include <QVariantMap>
#include <QtQml/qqmlregistration.h>

// The settings registries, for the views: what a page and a row are called,
// where they are, and what backs them. One for the whole program.
//
// It holds this program's own registry, which is compiled in, the registry of
// the pages of KDE System Settings that TUR links to, and the registry of
// every other program found in `directory` (/usr/share/tur/settings on an
// installed system).
//
// The title of a row on screen is the title in the registry: a page does not
// write it again. So what search finds, what the assistant is told and what
// the person reads are one string.
class Registry : public QObject
{
    Q_OBJECT
    QML_ELEMENT
    QML_SINGLETON

    // A directory of further registry files (*.json). Empty for none.
    Q_PROPERTY(QString directory READ directory WRITE setDirectory NOTIFY changed FINAL)

    // Goes up each time the registries are read again. A binding that names
    // it is evaluated again then.
    Q_PROPERTY(int revision READ revision NOTIFY changed FINAL)

    // What was wrong with a file that was not loaded. Empty when all is well.
    Q_PROPERTY(QStringList problems READ problems NOTIFY changed FINAL)

    // The side list of this program: one item for each of its pages that is
    // listed, in the registry's order, with a heading before each group.
    //   header     true for a heading ("P@x")
    //   address    the page, for an item that is not a heading
    //   title      the source string
    //   group      the title of the group the item is in or is the heading
    //              of; empty for a page that is in none
    //   depth      0, or 1 for a page inside a group
    //   available  whether the page can be opened
    Q_PROPERTY(QVariantList navigation READ navigation NOTIFY changed FINAL)

public:
    explicit Registry(QObject *parent = nullptr);

    QString directory() const { return m_directory; }
    void setDirectory(const QString &directory);
    int revision() const { return m_revision; }
    QStringList problems() const { return m_problems; }
    QVariantList navigation() const;

    const Settings::RegistryData &data() const { return m_data; }

    // The program whose pages this program shows itself.
    static QString ownProgram();

    // Reads everything again.
    Q_INVOKABLE void reload();

    // A page or a row, by its address; an empty map if no registry has it.
    //   address, page, row, program, status ("available", "planned",
    //   "removed"), replacedBy, title, pageTitle, path, kind ("page" for a
    //   page), section, sectionTitle, changeClass (-1 if nothing changes),
    //   tool, sourceType, sourceAvailable, waitsFor, opens, module
    // `revision` is not used: naming Registry.revision there makes a binding
    // follow a reload.
    Q_INVOKABLE QVariantMap entry(const QString &address, int revision = 0) const;

    // The title of a section of a page; empty if there is none.
    Q_INVOKABLE QString sectionTitle(const QString &page, const QString &section, int revision = 0) const;

    // The addresses of the rows of a page that are there, in order.
    Q_INVOKABLE QStringList rowsOf(const QString &page) const;

    // The addresses of this program's pages that can be opened.
    Q_INVOKABLE QStringList ownPages() const;

    // What matches some words, best first: address, title, score, path,
    // section, kind, program.
    Q_INVOKABLE QVariantList search(const QString &text, int limit, int revision = 0) const;

Q_SIGNALS:
    void changed();

private:
    Settings::RegistryData m_data;
    QString m_directory;
    QStringList m_problems;
    int m_revision = 0;
};
