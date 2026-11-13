// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#include "catalogue.h"

#include "fixturesource.h"
#include "processsource.h"

#include <QJsonDocument>
#include <QJsonObject>
#include <QMetaObject>

#include <algorithm>
#include <utility>

namespace {

QVariantMap recommendedOf(const QVariantMap &report)
{
    const QVariantList options = report.value(QStringLiteral("options")).toList();
    for (const QVariant &option : options) {
        const QVariantMap map = option.toMap();
        if (map.value(QStringLiteral("recommended")).toBool())
            return map;
    }
    return QVariantMap();
}

} // namespace

// --- SearchSession ---------------------------------------------------------

SearchSession::SearchSession(Catalogue *catalogue)
    : QObject(catalogue)
    , m_catalogue(catalogue)
    , m_hits(new HitsModel(this))
{
}

QString SearchSession::query() const
{
    return m_query;
}

void SearchSession::setQuery(const QString &query)
{
    const QString wanted = query.trimmed();
    if (wanted == m_query)
        return;
    m_query = wanted;
    Q_EMIT queryChanged();
    start();
}

SearchSession::Status SearchSession::status() const
{
    return m_status;
}

QString SearchSession::answeredQuery() const
{
    return m_answeredQuery;
}

HitsModel *SearchSession::hits() const
{
    return m_hits;
}

QVariantMap SearchSession::failure() const
{
    return m_failure;
}

bool SearchSession::settled() const
{
    return m_status != Searching && m_outstanding == 0;
}

void SearchSession::retry()
{
    start();
}

void SearchSession::setStatus(Status status)
{
    if (status == m_status)
        return;
    const bool was = settled();
    m_status = status;
    Q_EMIT statusChanged();
    if (was != settled())
        Q_EMIT settledChanged();
}

void SearchSession::setOutstanding(int outstanding)
{
    const bool was = settled();
    m_outstanding = outstanding;
    if (was != settled())
        Q_EMIT settledChanged();
}

void SearchSession::start()
{
    // Whatever is still on its way belongs to an older question.
    const quint64 generation = ++m_generation;
    if (m_request != 0) {
        m_catalogue->forget(m_request);
        m_request = 0;
    }
    setOutstanding(0);

    if (!m_failure.isEmpty()) {
        m_failure.clear();
        Q_EMIT failureChanged();
    }

    if (m_query.isEmpty()) {
        m_hits->reset({});
        if (!m_answeredQuery.isEmpty()) {
            m_answeredQuery.clear();
            Q_EMIT answeredQueryChanged();
        }
        setStatus(Idle);
        return;
    }

    // The rows of the last answer stay until the new one arrives.
    setStatus(Searching);
    m_request = m_catalogue->ask(CatalogueRequest::Search, m_query, [this, generation](const CatalogueReply &reply) {
        if (generation != m_generation)
            return;
        m_request = 0;
        answered(reply);
    });
}

void SearchSession::answered(const CatalogueReply &reply)
{
    Answers::SearchAnswer answer;
    Answers::Failure failure;
    if (!Answers::readSearch(reply, &answer, &failure)) {
        fail(failure);
        return;
    }

    m_hits->reset(answer.hits);
    if (m_answeredQuery != m_query) {
        m_answeredQuery = m_query;
        Q_EMIT answeredQueryChanged();
    }
    setOutstanding(int(answer.hits.size()));
    setStatus(answer.hits.isEmpty() ? NothingFound : Results);

    // The route and the rating of each row: what the router recommends for
    // this machine. The rows are on screen already.
    const quint64 generation = m_generation;
    for (const Answers::Hit &hit : std::as_const(answer.hits)) {
        const QString id = hit.id;
        m_catalogue->details(id, [this, generation, id](const Catalogue::Details &details) {
            if (generation != m_generation)
                return;
            if (!details.ok) {
                fail(details.failure);
                return;
            }
            m_hits->setDetails(id, details.entry, recommendedOf(details.report));
            setOutstanding(m_outstanding - 1);
        });
    }
}

void SearchSession::fail(const Answers::Failure &failure)
{
    // Nothing more of this question is wanted.
    ++m_generation;
    m_hits->reset({});
    m_failure = failure.toMap();
    Q_EMIT failureChanged();
    setOutstanding(0);
    setStatus(Failed);
}

// --- ApplicationSession ----------------------------------------------------

ApplicationSession::ApplicationSession(Catalogue *catalogue)
    : QObject(catalogue)
    , m_catalogue(catalogue)
{
}

QString ApplicationSession::applicationId() const
{
    return m_id;
}

void ApplicationSession::setApplicationId(const QString &id)
{
    if (id == m_id)
        return;
    m_id = id;
    Q_EMIT applicationIdChanged();
    load();
}

ApplicationSession::Status ApplicationSession::status() const
{
    return m_status;
}

QVariantMap ApplicationSession::entry() const
{
    return m_entry;
}

QVariantList ApplicationSession::options() const
{
    return m_options;
}

QVariantMap ApplicationSession::recommended() const
{
    return m_recommended;
}

QVariantList ApplicationSession::others() const
{
    return m_others;
}

bool ApplicationSession::noRoute() const
{
    return m_noRoute;
}

bool ApplicationSession::hostAssumed() const
{
    return m_hostAssumed;
}

QVariantMap ApplicationSession::failure() const
{
    return m_failure;
}

QVariantList ApplicationSession::reports(const QVariantMap &option) const
{
    QVariantList out;
    const QVariant index = option.value(QStringLiteral("edition_index"));
    if (!index.isValid() || index.isNull())
        return out;
    const QVariantList editions = m_entry.value(QStringLiteral("editions")).toList();
    const int position = index.toInt();
    if (position < 0 || position >= editions.size())
        return out;
    const QString route = option.value(QStringLiteral("route")).toString();
    const QVariantList records = editions.at(position).toMap().value(QStringLiteral("records")).toList();
    for (const QVariant &record : records) {
        if (record.toMap().value(QStringLiteral("route")).toString() == route)
            out.append(record);
    }
    return out;
}

void ApplicationSession::retry()
{
    load();
}

void ApplicationSession::setStatus(Status status)
{
    if (status == m_status)
        return;
    m_status = status;
    Q_EMIT statusChanged();
}

void ApplicationSession::load()
{
    const quint64 generation = ++m_generation;

    // The status first, and the content only later: a view that shows a
    // loaded page lets go of it when the status changes, and the page must
    // not see its content taken away while it is being let go of.
    setStatus(m_id.isEmpty() ? Idle : Loading);

    if (m_id.isEmpty()) {
        QMetaObject::invokeMethod(
            this,
            [this, generation] {
                if (generation == m_generation)
                    show(CatalogueDetails());
            },
            Qt::QueuedConnection);
        return;
    }

    m_catalogue->details(m_id, [this, generation](const Catalogue::Details &details) {
        if (generation != m_generation)
            return;
        show(details);
        setStatus(details.ok ? Ready : Failed);
    });
}

// Replaces what the page shows with `details`: an application, a failure, or
// nothing.
void ApplicationSession::show(const CatalogueDetails &details)
{
    m_entry.clear();
    m_options.clear();
    m_recommended.clear();
    m_others.clear();
    m_noRoute = false;
    m_hostAssumed = false;
    m_failure.clear();

    if (details.ok) {
        m_entry = details.entry;
        m_options = details.report.value(QStringLiteral("options")).toList();
        for (const QVariant &option : std::as_const(m_options)) {
            const QVariantMap map = option.toMap();
            if (map.value(QStringLiteral("recommended")).toBool())
                m_recommended = map;
            else
                m_others.append(map);
        }
        m_noRoute = m_recommended.value(QStringLiteral("route")).toString() == QLatin1String("no-route");
        m_hostAssumed = details.report.value(QStringLiteral("host_assumed")).toBool();
    } else if (!details.failure.code.isEmpty()) {
        m_failure = details.failure.toMap();
    }
    Q_EMIT loaded();
}

// --- Catalogue ---------------------------------------------------------------

Catalogue::Catalogue(QObject *parent)
    : QObject(parent)
    , m_search(new SearchSession(this))
    , m_application(new ApplicationSession(this))
{
}

Catalogue::~Catalogue() = default;

QString Catalogue::fixture() const
{
    return m_fixture;
}

void Catalogue::setFixture(const QString &fixture)
{
    if (fixture == m_fixture)
        return;
    m_fixture = fixture;
    dropSource();
    Q_EMIT configurationChanged();
}

QString Catalogue::program() const
{
    return m_program;
}

void Catalogue::setProgram(const QString &program)
{
    if (program == m_program)
        return;
    m_program = program;
    dropSource();
    Q_EMIT configurationChanged();
}

QString Catalogue::catalogueDirectory() const
{
    return m_catalogueDirectory;
}

void Catalogue::setCatalogueDirectory(const QString &directory)
{
    if (directory == m_catalogueDirectory)
        return;
    m_catalogueDirectory = directory;
    dropSource();
    Q_EMIT configurationChanged();
}

QString Catalogue::hostFacts() const
{
    return m_hostFacts;
}

void Catalogue::setHostFacts(const QString &file)
{
    if (file == m_hostFacts)
        return;
    m_hostFacts = file;
    dropSource();
    Q_EMIT configurationChanged();
}

QString Catalogue::preferences() const
{
    return m_preferences;
}

void Catalogue::setPreferences(const QString &file)
{
    if (file == m_preferences)
        return;
    m_preferences = file;
    dropSource();
    Q_EMIT configurationChanged();
}

int Catalogue::timeout() const
{
    return m_timeout;
}

void Catalogue::setTimeout(int milliseconds)
{
    milliseconds = std::max(1, milliseconds);
    if (milliseconds == m_timeout)
        return;
    m_timeout = milliseconds;
    dropSource();
    Q_EMIT configurationChanged();
}

SearchSession *Catalogue::search() const
{
    return m_search;
}

ApplicationSession *Catalogue::application() const
{
    return m_application;
}

QVariantMap Catalogue::knownVocabulary() const
{
    return QVariantMap {
        { QStringLiteral("routes"), Answers::routes() },
        { QStringLiteral("ratings"), Answers::ratings() },
        { QStringLiteral("outlooks"), Answers::outlooks() },
        { QStringLiteral("severities"), Answers::severities() },
        { QStringLiteral("rating_source_kinds"), Answers::ratingSourceKinds() },
    };
}

void Catalogue::setSource(CatalogueSource *source)
{
    dropSource();
    m_source = source;
    m_source->setParent(this);
    connect(m_source, &CatalogueSource::finished, this, &Catalogue::answered);
}

CatalogueSource *Catalogue::source()
{
    if (m_source)
        return m_source;
    if (!m_fixture.isEmpty()) {
        m_source = new FixtureSource(m_fixture, this);
    } else {
        ProcessSource::Settings settings;
        settings.program = m_program;
        settings.catalogueDirectory = m_catalogueDirectory;
        settings.hostFacts = m_hostFacts;
        settings.preferences = m_preferences;
        settings.timeoutMs = m_timeout;
        m_source = new ProcessSource(settings, this);
    }
    connect(m_source, &CatalogueSource::finished, this, &Catalogue::answered);
    return m_source;
}

// A change of configuration: what was asked of the old source, and what it
// answered, no longer holds.
void Catalogue::dropSource()
{
    ++m_epoch;
    m_handlers.clear();
    m_pending.clear();
    m_known.clear();
    if (m_source) {
        m_source->disconnect(this);
        m_source->deleteLater();
        m_source = nullptr;
    }
    if (!m_statistics.isEmpty() || m_checkingStatistics) {
        m_statistics.clear();
        m_checkingStatistics = false;
        Q_EMIT statisticsChanged();
    }
}

QVariantMap Catalogue::statistics() const
{
    return m_statistics;
}

bool Catalogue::checkingStatistics() const
{
    return m_checkingStatistics;
}

void Catalogue::checkStatistics()
{
    if (m_checkingStatistics || !m_statistics.isEmpty())
        return;
    m_checkingStatistics = true;
    Q_EMIT statisticsChanged();
    ask(CatalogueRequest::Validate, QString(), [this](const CatalogueReply &reply) {
        m_checkingStatistics = false;
        // Only a complete answer counts: without the number, Home says
        // nothing about who rated what.
        if (reply.failure == CatalogueReply::None && reply.exitStatus == 0) {
            const QJsonObject document = QJsonDocument::fromJson(reply.output).object();
            const QJsonObject counts = document.value(QLatin1String("statistics")).toObject();
            if (document.value(QLatin1String("valid")).toBool()
                && counts.value(QLatin1String("rated_by_tur_tests")).isDouble())
                m_statistics = counts.toVariantMap();
        }
        Q_EMIT statisticsChanged();
    });
}

quint64 Catalogue::ask(CatalogueRequest::Command command, const QString &subject, ReplyHandler handler)
{
    CatalogueRequest request;
    request.command = command;
    request.subject = subject;
    const quint64 id = source()->submit(request);
    m_handlers.insert(id, std::move(handler));
    return id;
}

void Catalogue::forget(quint64 id)
{
    if (m_handlers.remove(id) && m_source)
        m_source->cancel(id);
}

void Catalogue::answered(quint64 id, const CatalogueReply &reply)
{
    const ReplyHandler handler = m_handlers.take(id);
    if (handler)
        handler(reply);
}

void Catalogue::details(const QString &id, DetailsHandler handler)
{
    if (const auto known = m_known.constFind(id); known != m_known.constEnd()) {
        // Later, like every other answer.
        const Details details = known.value();
        const quint64 epoch = m_epoch;
        QMetaObject::invokeMethod(
            this,
            [this, epoch, details, handler] {
                if (epoch == m_epoch)
                    handler(details);
            },
            Qt::QueuedConnection);
        return;
    }

    if (const auto pending = m_pending.find(id); pending != m_pending.end()) {
        pending->waiters.append(std::move(handler));
        return;
    }

    Pending pending;
    pending.waiters.append(std::move(handler));
    m_pending.insert(id, pending);

    ask(CatalogueRequest::Show, id, [this, id](const CatalogueReply &reply) {
        const auto pending = m_pending.find(id);
        if (pending == m_pending.end())
            return;
        Answers::Failure failure;
        if (!Answers::readEntry(reply, &pending->details.entry, &failure) && pending->details.failure.code.isEmpty())
            pending->details.failure = failure;
        pending->haveEntry = true;
        detailsArrived(id);
    });
    ask(CatalogueRequest::Route, id, [this, id](const CatalogueReply &reply) {
        const auto pending = m_pending.find(id);
        if (pending == m_pending.end())
            return;
        Answers::Failure failure;
        if (!Answers::readRoutes(reply, &pending->details.report, &failure) && pending->details.failure.code.isEmpty())
            pending->details.failure = failure;
        pending->haveReport = true;
        detailsArrived(id);
    });
}

void Catalogue::detailsArrived(const QString &id)
{
    const auto found = m_pending.find(id);
    if (found == m_pending.end() || !found->haveEntry || !found->haveReport)
        return;
    Pending pending = m_pending.take(id);
    pending.details.ok = pending.details.failure.code.isEmpty();
    // A failure is not remembered: asking again may succeed.
    if (pending.details.ok)
        m_known.insert(id, pending.details);
    for (const DetailsHandler &waiter : std::as_const(pending.waiters))
        waiter(pending.details);
}
