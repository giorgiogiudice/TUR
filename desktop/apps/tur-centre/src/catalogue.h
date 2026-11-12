// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#pragma once

#include "answers.h"
#include "cataloguesource.h"
#include "hitsmodel.h"

#include <QHash>
#include <QList>
#include <QObject>
#include <QString>
#include <QVariantList>
#include <QVariantMap>
#include <QtQml/qqmlregistration.h>

#include <functional>

class Catalogue;

// What the catalogue and the router say about one application.
struct CatalogueDetails
{
    bool ok = false;
    QVariantMap entry;  // `show`
    QVariantMap report; // `route`
    Answers::Failure failure;
};

// The search of the application centre (desktop-experience.md, section 8.3).
// Setting `query` asks the catalogue; the rows arrive in `hits`, and what
// the router says about each application arrives after them.
class SearchSession : public QObject
{
    Q_OBJECT
    QML_ELEMENT
    QML_UNCREATABLE("A search belongs to a Catalogue.")

    Q_PROPERTY(QString query READ query WRITE setQuery NOTIFY queryChanged FINAL)
    Q_PROPERTY(Status status READ status NOTIFY statusChanged FINAL)
    // The query the rows, or the statement that nothing was found, answer.
    // It trails `query` while a search is under way.
    Q_PROPERTY(QString answeredQuery READ answeredQuery NOTIFY answeredQueryChanged FINAL)
    Q_PROPERTY(HitsModel *hits READ hits CONSTANT FINAL)
    // The content of the two-layer message when status is Failed.
    Q_PROPERTY(QVariantMap failure READ failure NOTIFY failureChanged FINAL)
    // True when nothing is awaited: the search has answered and every row
    // has its details.
    Q_PROPERTY(bool settled READ settled NOTIFY settledChanged FINAL)

public:
    enum Status {
        Idle,         // nothing was asked
        Searching,    // asked, not answered yet
        Results,      // the catalogue found something
        NothingFound, // the catalogue found nothing
        Failed,       // the catalogue could not be asked, or its answer not read
    };
    Q_ENUM(Status)

    explicit SearchSession(Catalogue *catalogue);

    QString query() const;
    void setQuery(const QString &query);
    Status status() const;
    QString answeredQuery() const;
    HitsModel *hits() const;
    QVariantMap failure() const;
    bool settled() const;

    // Asks again, for the same query.
    Q_INVOKABLE void retry();

Q_SIGNALS:
    void queryChanged();
    void statusChanged();
    void answeredQueryChanged();
    void failureChanged();
    void settledChanged();

private:
    void start();
    void answered(const CatalogueReply &reply);
    void fail(const Answers::Failure &failure);
    void setStatus(Status status);
    void setOutstanding(int outstanding);

    Catalogue *m_catalogue;
    HitsModel *m_hits;
    QString m_query;
    QString m_answeredQuery;
    Status m_status = Idle;
    QVariantMap m_failure;
    quint64 m_generation = 0;
    quint64 m_request = 0;
    int m_outstanding = 0;
};

// One application's page (sections 8.4 and 8.8): its catalogue entry and the
// router's answer for this machine. Setting `applicationId` asks for both.
class ApplicationSession : public QObject
{
    Q_OBJECT
    QML_ELEMENT
    QML_UNCREATABLE("An application's page belongs to a Catalogue.")

    Q_PROPERTY(QString applicationId READ applicationId WRITE setApplicationId NOTIFY applicationIdChanged FINAL)
    Q_PROPERTY(Status status READ status NOTIFY statusChanged FINAL)
    // The catalogue's entry: `tur-catalogue show --json`, as it is.
    Q_PROPERTY(QVariantMap entry READ entry NOTIFY loaded FINAL)
    // The router's options, in the router's order, each as it is.
    Q_PROPERTY(QVariantList options READ options NOTIFY loaded FINAL)
    // The option the router marks `recommended`.
    Q_PROPERTY(QVariantMap recommended READ recommended NOTIFY loaded FINAL)
    // Every other option, in the router's order.
    Q_PROPERTY(QVariantList others READ others NOTIFY loaded FINAL)
    // The router states that nothing runs the application on this machine.
    Q_PROPERTY(bool noRoute READ noRoute NOTIFY loaded FINAL)
    // The router was not told what this machine has, and assumed it.
    Q_PROPERTY(bool hostAssumed READ hostAssumed NOTIFY loaded FINAL)
    Q_PROPERTY(QVariantMap failure READ failure NOTIFY loaded FINAL)

public:
    enum Status {
        Idle,
        Loading,
        Ready,
        Failed,
    };
    Q_ENUM(Status)

    explicit ApplicationSession(Catalogue *catalogue);

    QString applicationId() const;
    void setApplicationId(const QString &id);
    Status status() const;
    QVariantMap entry() const;
    QVariantList options() const;
    QVariantMap recommended() const;
    QVariantList others() const;
    bool noRoute() const;
    bool hostAssumed() const;
    QVariantMap failure() const;

    // Every record the catalogue holds for the way of running that `option`
    // is: "All reports" (section 8.2).
    Q_INVOKABLE QVariantList reports(const QVariantMap &option) const;

    Q_INVOKABLE void retry();

Q_SIGNALS:
    void applicationIdChanged();
    void statusChanged();
    void loaded();

private:
    void load();
    void show(const CatalogueDetails &details);
    void setStatus(Status status);

    Catalogue *m_catalogue;
    QString m_id;
    Status m_status = Idle;
    QVariantMap m_entry;
    QVariantList m_options;
    QVariantMap m_recommended;
    QVariantList m_others;
    bool m_noRoute = false;
    bool m_hostAssumed = false;
    QVariantMap m_failure;
    quint64 m_generation = 0;
};

// What the application centre's views read: the catalogue and the router,
// through whichever CatalogueSource the configuration selects. It decides
// nothing about routing; it asks, checks the shape of the answer, and hands
// it on.
//
//     Catalogue { fixture: "/path/to/recorded/answers" }      tests, pictures, --fixture
//     Catalogue { program: "tur-catalogue"; hostFacts: ... }  the real thing
class Catalogue : public QObject
{
    Q_OBJECT
    QML_ELEMENT

    // A directory of recorded answers. When set, no program is run.
    Q_PROPERTY(QString fixture READ fixture WRITE setFixture NOTIFY configurationChanged FINAL)
    // The catalogue program: a name found on the PATH, or a path.
    Q_PROPERTY(QString program READ program WRITE setProgram NOTIFY configurationChanged FINAL)
    // --catalogue, --host and --prefs of the program; each may be empty.
    Q_PROPERTY(QString catalogueDirectory READ catalogueDirectory WRITE setCatalogueDirectory NOTIFY
                   configurationChanged FINAL)
    Q_PROPERTY(QString hostFacts READ hostFacts WRITE setHostFacts NOTIFY configurationChanged FINAL)
    Q_PROPERTY(QString preferences READ preferences WRITE setPreferences NOTIFY configurationChanged FINAL)
    // The longest a run of the program may last, in milliseconds.
    Q_PROPERTY(int timeout READ timeout WRITE setTimeout NOTIFY configurationChanged FINAL)

    Q_PROPERTY(SearchSession *search READ search CONSTANT FINAL)
    Q_PROPERTY(ApplicationSession *application READ application CONSTANT FINAL)

public:
    using Details = CatalogueDetails;
    using ReplyHandler = std::function<void(const CatalogueReply &)>;
    using DetailsHandler = std::function<void(const Details &)>;

    explicit Catalogue(QObject *parent = nullptr);
    ~Catalogue() override;

    QString fixture() const;
    void setFixture(const QString &fixture);
    QString program() const;
    void setProgram(const QString &program);
    QString catalogueDirectory() const;
    void setCatalogueDirectory(const QString &directory);
    QString hostFacts() const;
    void setHostFacts(const QString &file);
    QString preferences() const;
    void setPreferences(const QString &file);
    int timeout() const;
    void setTimeout(int milliseconds);

    SearchSession *search() const;
    ApplicationSession *application() const;

    // The values of the router's vocabulary this program has words for, by
    // list: routes, ratings, outlooks, severities, rating_source_kinds.
    Q_INVOKABLE QVariantMap knownVocabulary() const;

    // Uses this source instead of the configured one. For tests. The
    // catalogue owns it.
    void setSource(CatalogueSource *source);

    // Asks one question. The handler is called once, later, unless forget()
    // is called with the returned number first.
    quint64 ask(CatalogueRequest::Command command, const QString &subject, ReplyHandler handler);
    void forget(quint64 id);

    // The entry and the route of one application, asked together and
    // remembered while the program runs. The handler is called once, later.
    void details(const QString &id, DetailsHandler handler);

Q_SIGNALS:
    void configurationChanged();

private:
    struct Pending
    {
        bool haveEntry = false;
        bool haveReport = false;
        Details details;
        QList<DetailsHandler> waiters;
    };

    CatalogueSource *source();
    void dropSource();
    void answered(quint64 id, const CatalogueReply &reply);
    void detailsArrived(const QString &id);

    QString m_fixture;
    QString m_program = QStringLiteral("tur-catalogue");
    QString m_catalogueDirectory;
    QString m_hostFacts;
    QString m_preferences;
    int m_timeout = 10000;

    CatalogueSource *m_source = nullptr;
    QHash<quint64, ReplyHandler> m_handlers;
    QHash<QString, Pending> m_pending;
    QHash<QString, Details> m_known;
    quint64 m_epoch = 0;

    SearchSession *m_search;
    ApplicationSession *m_application;
};
