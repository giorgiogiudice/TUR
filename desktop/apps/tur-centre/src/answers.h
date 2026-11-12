// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#pragma once

#include "cataloguesource.h"

#include <QCoreApplication>
#include <QList>
#include <QString>
#include <QStringList>
#include <QVariantMap>

// Reads what the catalogue program answered.
//
// Three documents are understood: the answers of `tur-catalogue search`,
// `show` and `route` with --json (docs/architecture/catalogue-and-routing.md,
// "Command line"). Each is checked for its shape before anything is shown,
// and for its vocabulary: a route, a rating, an outlook or a severity that
// this program has no words for is refused, never shown raw and never shown
// as something else (desktop-experience.md, section 8.2).
//
// Whatever goes wrong becomes a Failure: the content of the kit's two-layer
// message. A failure the catalogue reports carries the catalogue's own code
// and words; a failure of the asking carries one of this program's codes.
class Answers
{
    Q_DECLARE_TR_FUNCTIONS(Answers)

public:
    // This program's own codes. Like the catalogue's, they are in the
    // DESKTOP family until a registry assigns them (specification 29.3).
    static inline const QString NotStarted = QStringLiteral("TUR-DESKTOP-CENTRE-001");
    static inline const QString TimedOut = QStringLiteral("TUR-DESKTOP-CENTRE-002");
    static inline const QString TooLarge = QStringLiteral("TUR-DESKTOP-CENTRE-003");
    static inline const QString Unreadable = QStringLiteral("TUR-DESKTOP-CENTRE-004");
    static inline const QString Crashed = QStringLiteral("TUR-DESKTOP-CENTRE-005");
    static inline const QString NotRecorded = QStringLiteral("TUR-DESKTOP-CENTRE-006");
    static inline const QString Refused = QStringLiteral("TUR-DESKTOP-CENTRE-007");

    // The catalogue's codes this program treats specially.
    static inline const QString UnknownApplication = QStringLiteral("TUR-DESKTOP-CATALOGUE-009");
    static inline const QString NoMatch = QStringLiteral("TUR-DESKTOP-CATALOGUE-010");
    static inline const QString NoRoute = QStringLiteral("TUR-DESKTOP-CATALOGUE-011");

    struct Failure
    {
        QString code;     // always present
        QString title;    // what happened, in one sentence
        QString reason;   // why
        QString detail;   // the technical layer: command, exit status, output
        bool retryable = true;

        QVariantMap toMap() const;
    };

    struct Hit
    {
        QString id;
        QString name;
        QString category;
        QString publisher;
        QString matched;
        QString kind;
        int score = 0;
    };

    struct SearchAnswer
    {
        QString query;
        QList<Hit> hits;
    };

    // Each returns true and fills the answer, or returns false and fills the
    // failure.
    static bool readSearch(const CatalogueReply &reply, SearchAnswer *answer, Failure *failure);
    static bool readEntry(const CatalogueReply &reply, QVariantMap *entry, Failure *failure);
    static bool readRoutes(const CatalogueReply &reply, QVariantMap *report, Failure *failure);

    // The values this program has words for. A test holds them against the
    // router's own lists (fixtures/vocabulary.json).
    static const QStringList &routes();
    static const QStringList &ratings();
    static const QStringList &outlooks();
    static const QStringList &severities();
    static const QStringList &ratingSourceKinds();

    // The technical layer of a failure: the command, how it ended, and the
    // beginning of what it wrote to standard error. Not translated.
    static QString technicalDetail(const CatalogueReply &reply, const QString &note = QString());
};
