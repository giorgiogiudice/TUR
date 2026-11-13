// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#include "apps.h"

#include "appsfixture.h"
#include "appsprocess.h"

#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonParseError>
#include <QMetaObject>

#include <utility>

namespace {

// How much of tur-apps' standard error the technical layer shows.
constexpr qsizetype ShownErrorCharacters = 2000;

QString technical(const AppsReply &reply, const QString &note = QString())
{
    QStringList lines;
    if (!reply.commandLine.isEmpty())
        lines << QStringLiteral("Command: ") + reply.commandLine;
    if (reply.failure == AppsReply::None)
        lines << QStringLiteral("Exit status: %1").arg(reply.exitStatus);
    if (!reply.detail.isEmpty())
        lines << reply.detail;
    const QString errors = QString::fromUtf8(reply.errorOutput).trimmed();
    if (!errors.isEmpty())
        lines << QStringLiteral("Standard error:\n") + errors.left(ShownErrorCharacters);
    if (!note.isEmpty())
        lines << note;
    return lines.join(QLatin1Char('\n'));
}

// The codes after which trying again cannot help: the answer would be the
// same.
bool retryable(const QString &code)
{
    static const QStringList final {
        QStringLiteral("TUR-DESKTOP-APPS-002"), QStringLiteral("TUR-DESKTOP-APPS-003"),
        QStringLiteral("TUR-DESKTOP-APPS-004"), QStringLiteral("TUR-DESKTOP-APPS-005"),
        QStringLiteral("TUR-DESKTOP-APPS-012"), QStringLiteral("TUR-DESKTOP-APPS-014"),
        QStringLiteral("TUR-DESKTOP-APPS-017"), QStringLiteral("TUR-DESKTOP-CENTRE-006"),
        QStringLiteral("TUR-DESKTOP-CENTRE-007"),
    };
    return !final.contains(code);
}

QVariantMap message(const QString &code, const QString &title, const QString &reason, const QString &detail)
{
    return QVariantMap {
        { QStringLiteral("code"), code },
        { QStringLiteral("title"), title },
        { QStringLiteral("reason"), reason },
        { QStringLiteral("detail"), detail },
        { QStringLiteral("retryable"), retryable(code) },
    };
}

// Reads a one-document answer (`plan --json`, `list --json`, `sources
// --json`): the document, or the failure.
QVariantMap readDocument(const AppsReply &reply, QJsonObject *document)
{
    if (reply.failure != AppsReply::None)
        return Apps::failureOf(reply);
    QJsonParseError error;
    const QJsonDocument parsed = QJsonDocument::fromJson(reply.output, &error);
    const QJsonObject object = parsed.object();
    if (error.error == QJsonParseError::NoError && object.value(QLatin1String("error")).isObject())
        return Apps::failureOf(reply, object.value(QLatin1String("error")).toObject());
    if (error.error != QJsonParseError::NoError || !parsed.isObject() || reply.exitStatus != 0)
        return message(Apps::Unreadable, Apps::tr("tur-apps' answer could not be read"),
                       Apps::tr("It did not print the document the application centre expects."),
                       technical(reply, error.error != QJsonParseError::NoError ? error.errorString() : QString()));
    *document = object;
    return QVariantMap();
}

} // namespace

// --- AppsOperation -------------------------------------------------------------

AppsOperation::AppsOperation(Apps *apps, const QString &applicationId, Kind kind, const QString &installerFile,
                             bool deleteData)
    : QObject(apps)
    , m_apps(apps)
    , m_id(applicationId)
    , m_kind(kind)
    , m_installerFile(installerFile)
    , m_deleteData(deleteData)
{
}

QString AppsOperation::applicationId() const
{
    return m_id;
}

AppsOperation::Kind AppsOperation::kind() const
{
    return m_kind;
}

AppsOperation::Phase AppsOperation::phase() const
{
    return m_phase;
}

QVariantMap AppsOperation::plan() const
{
    return m_plan;
}

QString AppsOperation::installerFile() const
{
    return m_installerFile;
}

bool AppsOperation::deleteData() const
{
    return m_deleteData;
}

QString AppsOperation::step() const
{
    return m_step;
}

QString AppsOperation::activity() const
{
    return m_activity;
}

double AppsOperation::done() const
{
    return m_done;
}

double AppsOperation::total() const
{
    return m_total;
}

bool AppsOperation::cancellable() const
{
    if (m_kind != Install || m_phase != Running || m_cancelRequested)
        return false;
    // Where tur-apps honours a cancel (its src/cancel.rs): before the first
    // step; while downloading and checking; while flatpak installs. Not
    // once tur-runtime installs, and not while the installation is
    // recorded.
    if (m_step.isEmpty() || m_step == QLatin1String("download") || m_step == QLatin1String("verify"))
        return true;
    if (m_step == QLatin1String("install"))
        return m_plan.value(QStringLiteral("route")).toString() == QLatin1String("flatpak");
    return false;
}

bool AppsOperation::cancelRequested() const
{
    return m_cancelRequested;
}

QVariantMap AppsOperation::failure() const
{
    return m_failure;
}

QVariantMap AppsOperation::result() const
{
    return m_result;
}

void AppsOperation::setPhase(Phase phase)
{
    m_phase = phase;
    Q_EMIT changed();
}

void AppsOperation::start()
{
    AppsRequest request;
    request.subject = m_id;
    request.installerFile = m_installerFile;
    switch (m_kind) {
    case Install:
        if (m_phase == Planning) {
            request.command = AppsRequest::Plan;
            m_request = m_apps->submit(request, nullptr, [this](const AppsReply &reply) {
                m_request = 0;
                QJsonObject document;
                const QVariantMap failure = readDocument(reply, &document);
                if (!failure.isEmpty()) {
                    fail(failure);
                    m_apps->operationEnded(this);
                    return;
                }
                m_plan = document.toVariantMap();
                setPhase(Planned);
            });
            return;
        }
        request.command = AppsRequest::Install;
        break;
    case Update:
        request.command = AppsRequest::Update;
        break;
    case Remove:
        request.command = AppsRequest::Remove;
        request.deleteData = m_deleteData;
        break;
    }
    m_phase = Running;
    Q_EMIT changed();
    Q_EMIT m_apps->operationsChanged();
    m_request = m_apps->submit(
        request, [this](const QByteArray &line) { receive(line); },
        [this](const AppsReply &reply) {
            m_request = 0;
            end(reply);
        });
}

void AppsOperation::confirm()
{
    if (m_kind != Install || m_phase != Planned)
        return;
    start();
}

void AppsOperation::cancel()
{
    if (!cancellable())
        return;
    m_cancelRequested = true;
    m_apps->cancelOperation(this);
    Q_EMIT changed();
}

void AppsOperation::dismiss()
{
    if (m_phase == Running || m_phase == Planning)
        return;
    m_apps->dropOperation(this);
}

void AppsOperation::retry()
{
    if (m_phase != Failed)
        return;
    switch (m_kind) {
    case Install:
        m_apps->plan(m_id, m_installerFile);
        break;
    case Update:
        m_apps->update(m_id);
        break;
    case Remove:
        m_apps->remove(m_id, m_deleteData);
        break;
    }
}

void AppsOperation::receive(const QByteArray &line)
{
    const QJsonObject event = QJsonDocument::fromJson(line).object();
    const QString kind = event.value(QLatin1String("event")).toString();
    if (kind == QLatin1String("plan")) {
        m_plan = event.value(QLatin1String("plan")).toObject().toVariantMap();
    } else if (kind == QLatin1String("step-started")) {
        m_step = event.value(QLatin1String("step")).toString();
        m_activity.clear();
        m_done = 0;
        m_total = -1;
    } else if (kind == QLatin1String("activity")) {
        m_activity = event.value(QLatin1String("text")).toString();
    } else if (kind == QLatin1String("progress")) {
        m_done = event.value(QLatin1String("done")).toDouble();
        const QJsonValue total = event.value(QLatin1String("total"));
        m_total = total.isDouble() && total.toDouble() > 0 ? total.toDouble() : -1;
    } else if (kind == QLatin1String("result")) {
        m_result = event.value(QLatin1String("result")).toObject().toVariantMap();
        m_sawResult = true;
    } else if (kind == QLatin1String("error")) {
        m_eventError = event.value(QLatin1String("error")).toObject().toVariantMap();
    } else {
        // step-finished, and any kind a later tur-apps adds: nothing to show.
        return;
    }
    Q_EMIT changed();
}

void AppsOperation::end(const AppsReply &reply)
{
    if (reply.failure != AppsReply::None) {
        fail(Apps::failureOf(reply));
    } else if (m_sawResult) {
        setPhase(Succeeded);
    } else if (!m_eventError.isEmpty()) {
        const QVariantMap failure = Apps::failureOf(reply, QJsonObject::fromVariantMap(m_eventError));
        if (m_cancelRequested && failure.value(QStringLiteral("code")).toString() == Apps::NothingDone) {
            m_failure = failure;
            setPhase(Cancelled);
        } else {
            fail(failure);
        }
    } else {
        fail(message(Apps::Unreadable, Apps::tr("tur-apps ended without saying how it went"),
                     Apps::tr("Its last line was neither a result nor an error. The list of what is installed shows what is there now."),
                     technical(reply)));
    }
    m_apps->operationEnded(this);
}

void AppsOperation::fail(const QVariantMap &failure)
{
    m_failure = failure;
    setPhase(Failed);
}

// --- Apps ----------------------------------------------------------------------

Apps::Apps(QObject *parent)
    : QObject(parent)
{
}

Apps::~Apps()
{
    // The source first: it may wait for a change that is under way, and
    // calls nothing back while it does.
    if (m_source) {
        m_source->disconnect(this);
        delete m_source;
        m_source = nullptr;
    }
}

QString Apps::fixture() const
{
    return m_fixture;
}

void Apps::setFixture(const QString &fixture)
{
    if (fixture == m_fixture)
        return;
    m_fixture = fixture;
    dropSource();
    Q_EMIT configurationChanged();
}

QString Apps::program() const
{
    return m_program;
}

void Apps::setProgram(const QString &program)
{
    if (program == m_program)
        return;
    m_program = program;
    dropSource();
    Q_EMIT configurationChanged();
}

QString Apps::catalogueDirectory() const
{
    return m_catalogueDirectory;
}

void Apps::setCatalogueDirectory(const QString &directory)
{
    if (directory == m_catalogueDirectory)
        return;
    m_catalogueDirectory = directory;
    dropSource();
    Q_EMIT configurationChanged();
}

QString Apps::hostFacts() const
{
    return m_hostFacts;
}

void Apps::setHostFacts(const QString &file)
{
    if (file == m_hostFacts)
        return;
    m_hostFacts = file;
    dropSource();
    Q_EMIT configurationChanged();
}

QString Apps::preferences() const
{
    return m_preferences;
}

void Apps::setPreferences(const QString &file)
{
    if (file == m_preferences)
        return;
    m_preferences = file;
    dropSource();
    Q_EMIT configurationChanged();
}

QVariantList Apps::installed() const
{
    return m_installed;
}

Apps::Status Apps::installedStatus() const
{
    return m_installedStatus;
}

QVariantMap Apps::installedFailure() const
{
    return m_installedFailure;
}

bool Apps::updatesChecked() const
{
    return m_updatesChecked;
}

QStringList Apps::installedProblems() const
{
    return m_installedProblems;
}

QVariantList Apps::sources() const
{
    return m_sources;
}

Apps::Status Apps::sourcesStatus() const
{
    return m_sourcesStatus;
}

QVariantMap Apps::sourcesFailure() const
{
    return m_sourcesFailure;
}

QString Apps::sourceChanging() const
{
    return m_sourceChanging;
}

QVariantMap Apps::operations() const
{
    QVariantMap out;
    for (auto it = m_operations.cbegin(); it != m_operations.cend(); ++it)
        out.insert(it.key(), QVariant::fromValue(it.value()));
    return out;
}

bool Apps::busy() const
{
    for (const AppsOperation *operation : m_operations) {
        if (operation->phase() == AppsOperation::Running)
            return true;
    }
    return false;
}

void Apps::useSource(AppsSource *source)
{
    dropSource();
    m_source = source;
    m_source->setParent(this);
    connect(m_source, &AppsSource::line, this, &Apps::lineArrived);
    connect(m_source, &AppsSource::finished, this, &Apps::answered);
}

AppsSource *Apps::source()
{
    if (m_source)
        return m_source;
    if (!m_fixture.isEmpty()) {
        m_source = new AppsFixture(m_fixture, this);
    } else {
        AppsProcess::Settings settings;
        settings.program = m_program;
        settings.catalogueDirectory = m_catalogueDirectory;
        settings.hostFacts = m_hostFacts;
        settings.preferences = m_preferences;
        m_source = new AppsProcess(settings, this);
    }
    connect(m_source, &AppsSource::line, this, &Apps::lineArrived);
    connect(m_source, &AppsSource::finished, this, &Apps::answered);
    return m_source;
}

void Apps::dropSource()
{
    m_lineHandlers.clear();
    m_replyHandlers.clear();
    m_listRequest = 0;
    m_sourcesRequest = 0;
    if (m_source) {
        m_source->disconnect(this);
        m_source->deleteLater();
        m_source = nullptr;
    }
}

quint64 Apps::submit(const AppsRequest &request, LineHandler onLine, ReplyHandler onEnd)
{
    const quint64 id = source()->submit(request);
    if (onLine)
        m_lineHandlers.insert(id, std::move(onLine));
    m_replyHandlers.insert(id, std::move(onEnd));
    return id;
}

void Apps::forget(quint64 id)
{
    m_lineHandlers.remove(id);
    m_replyHandlers.remove(id);
}

void Apps::lineArrived(quint64 id, const QByteArray &line)
{
    if (const auto handler = m_lineHandlers.constFind(id); handler != m_lineHandlers.constEnd()) {
        const LineHandler call = handler.value();
        call(line);
    }
}

void Apps::answered(quint64 id, const AppsReply &reply)
{
    m_lineHandlers.remove(id);
    const ReplyHandler handler = m_replyHandlers.take(id);
    if (handler)
        handler(reply);
}

void Apps::refresh(bool checkUpdates)
{
    listInstalled(checkUpdates, false);
}

void Apps::listInstalled(bool checkUpdates, bool afresh)
{
    // The same question already asked is not asked twice; a list that asks
    // for updates answers a plain one too. After a change, a list asked
    // before it would be stale, and is asked again.
    if (!afresh && m_listRequest && (m_listChecksUpdates || !checkUpdates))
        return;
    if (m_listRequest)
        forget(m_listRequest);
    m_listChecksUpdates = checkUpdates;
    AppsRequest request;
    request.command = checkUpdates ? AppsRequest::ListWithUpdates : AppsRequest::List;
    m_installedStatus = Loading;
    Q_EMIT installedChanged();
    m_listRequest = submit(request, nullptr, [this](const AppsReply &reply) {
        m_listRequest = 0;
        QJsonObject document;
        const QVariantMap failure = readDocument(reply, &document);
        if (!failure.isEmpty()) {
            m_installedFailure = failure;
            m_installedStatus = Failed;
        } else {
            m_installed = document.value(QLatin1String("applications")).toArray().toVariantList();
            m_updatesChecked = document.value(QLatin1String("updates_checked")).toBool();
            m_installedProblems.clear();
            for (const QJsonValue &problem : document.value(QLatin1String("problems")).toArray())
                m_installedProblems.append(problem.toString());
            m_installedFailure.clear();
            m_installedStatus = Ready;
        }
        Q_EMIT installedChanged();
    });
}

void Apps::refreshSources()
{
    if (m_sourcesRequest)
        forget(m_sourcesRequest);
    AppsRequest request;
    request.command = AppsRequest::Sources;
    m_sourcesStatus = Loading;
    Q_EMIT sourcesChanged();
    m_sourcesRequest = submit(request, nullptr, [this](const AppsReply &reply) {
        m_sourcesRequest = 0;
        QJsonObject document;
        const QVariantMap failure = readDocument(reply, &document);
        if (!failure.isEmpty()) {
            m_sourcesFailure = failure;
            m_sourcesStatus = Failed;
        } else {
            m_sources = document.value(QLatin1String("sources")).toArray().toVariantList();
            m_sourcesStatus = Ready;
        }
        Q_EMIT sourcesChanged();
    });
}

void Apps::setSource(const QString &name, bool on, const QString &thenPlan, const QString &installerFile)
{
    if (!m_sourceChanging.isEmpty())
        return;
    AppsRequest request;
    request.command = on ? AppsRequest::EnableSource : AppsRequest::DisableSource;
    request.subject = name;
    m_sourceChanging = name;
    m_sourcesFailure.clear();
    Q_EMIT sourcesChanged();
    submit(request, nullptr, [this, thenPlan, installerFile](const AppsReply &reply) {
        QJsonObject document;
        const QVariantMap failure = readDocument(reply, &document);
        m_sourceChanging.clear();
        refreshSources();
        // After the list was asked for again: its answer must not hide
        // why the switch failed.
        if (!failure.isEmpty()) {
            m_sourcesFailure = failure;
            Q_EMIT sourcesChanged();
            return;
        }
        if (!thenPlan.isEmpty())
            plan(thenPlan, installerFile);
    });
}

AppsOperation *Apps::begin(const QString &applicationId, AppsOperation::Kind kind, const QString &installerFile,
                           bool deleteData)
{
    if (AppsOperation *current = m_operations.value(applicationId, nullptr)) {
        // One thing at a time for one application.
        if (current->phase() == AppsOperation::Running || current->phase() == AppsOperation::Planning)
            return current;
        m_operations.remove(applicationId);
        current->deleteLater();
    }
    auto *operation = new AppsOperation(this, applicationId, kind, installerFile, deleteData);
    m_operations.insert(applicationId, operation);
    Q_EMIT operationsChanged();
    operation->start();
    return operation;
}

AppsOperation *Apps::plan(const QString &applicationId, const QString &installerFile)
{
    return begin(applicationId, AppsOperation::Install, installerFile, false);
}

AppsOperation *Apps::update(const QString &applicationId)
{
    return begin(applicationId, AppsOperation::Update, QString(), false);
}

AppsOperation *Apps::remove(const QString &applicationId, bool deleteData)
{
    return begin(applicationId, AppsOperation::Remove, QString(), deleteData);
}

void Apps::updateAll()
{
    for (const QVariant &value : std::as_const(m_installed)) {
        const QVariantMap application = value.toMap();
        const QString id = application.value(QStringLiteral("id")).toString();
        const bool available = application.value(QStringLiteral("update")).toMap().value(QStringLiteral("available")).toBool();
        if (available && application.value(QStringLiteral("installed_by")).toString() == QLatin1String("tur-apps")
            && !m_updateQueue.contains(id))
            m_updateQueue.append(id);
    }
    nextUpdate();
}

void Apps::nextUpdate()
{
    for (const AppsOperation *operation : std::as_const(m_operations)) {
        if (operation->kind() == AppsOperation::Update && operation->phase() == AppsOperation::Running)
            return;
    }
    while (!m_updateQueue.isEmpty()) {
        const QString id = m_updateQueue.takeFirst();
        AppsOperation *current = m_operations.value(id, nullptr);
        if (current && current->phase() == AppsOperation::Running)
            continue;
        update(id);
        return;
    }
}

AppsOperation *Apps::operation(const QString &applicationId) const
{
    return m_operations.value(applicationId, nullptr);
}

QVariantMap Apps::installedEntry(const QString &applicationId) const
{
    for (const QVariant &value : m_installed) {
        const QVariantMap application = value.toMap();
        if (application.value(QStringLiteral("id")).toString() == applicationId)
            return application;
    }
    return QVariantMap();
}

QVariantMap Apps::launch(const QString &applicationId)
{
    QString problem;
    if (source()->launch(applicationId, &problem))
        return QVariantMap();
    QVariantMap failure = message(NotLaunched, tr("The application could not be started"),
                                  tr("tur-apps, which starts it, could not be run."), problem);
    failure.insert(QStringLiteral("retryable"), true);
    return failure;
}

void Apps::holdAfter(int lines)
{
    if (auto *fixture = qobject_cast<AppsFixture *>(source()))
        fixture->setHold(lines);
}

void Apps::release()
{
    if (auto *fixture = qobject_cast<AppsFixture *>(source()))
        fixture->release();
}

QStringList Apps::launched() const
{
    if (auto *fixture = qobject_cast<AppsFixture *>(m_source))
        return fixture->launched();
    return QStringList();
}

void Apps::operationEnded(AppsOperation *operation)
{
    Q_EMIT operationsChanged();
    // What is installed has changed, or may have: a failure is taken back,
    // but a removal that left something behind has still removed some.
    if (operation->phase() != AppsOperation::Cancelled
        && !(operation->kind() == AppsOperation::Install && operation->phase() == AppsOperation::Failed
             && operation->plan().isEmpty()))
        listInstalled(m_updatesChecked, true);
    if (operation->kind() == AppsOperation::Update)
        nextUpdate();
}

void Apps::cancelOperation(AppsOperation *operation)
{
    if (operation->m_request && m_source)
        m_source->cancel(operation->m_request);
}

void Apps::dropOperation(AppsOperation *operation)
{
    if (m_operations.value(operation->applicationId()) != operation)
        return;
    m_operations.remove(operation->applicationId());
    if (operation->m_request)
        forget(operation->m_request);
    operation->deleteLater();
    Q_EMIT operationsChanged();
}

QString Apps::titleOf(const QString &code, const QString &theirs)
{
    // tur-apps' codes (docs/architecture/app-install-backend.md), in the
    // application centre's words, so that they can be translated. A code
    // this version does not know keeps tur-apps' own title.
    static const QHash<QString, const char *> titles {
        { QStringLiteral("TUR-DESKTOP-APPS-001"), QT_TR_NOOP("Something this needs could not be read") },
        { QStringLiteral("TUR-DESKTOP-APPS-002"), QT_TR_NOOP("No application has this identifier") },
        { QStringLiteral("TUR-DESKTOP-APPS-003"), QT_TR_NOOP("No route runs this application on this computer") },
        { QStringLiteral("TUR-DESKTOP-APPS-004"), QT_TR_NOOP("Installing by this route is not built yet") },
        { QStringLiteral("TUR-DESKTOP-APPS-005"), QT_TR_NOOP("You need to supply the installer") },
        { QStringLiteral("TUR-DESKTOP-APPS-006"), QT_TR_NOOP("The file is not the one expected") },
        { QStringLiteral("TUR-DESKTOP-APPS-007"), QT_TR_NOOP("The download did not complete") },
        { QStringLiteral("TUR-DESKTOP-APPS-008"), QT_TR_NOOP("The source is not switched on") },
        { QStringLiteral("TUR-DESKTOP-APPS-009"), QT_TR_NOOP("A program this needs is not installed") },
        { QStringLiteral("TUR-DESKTOP-APPS-010"), QT_TR_NOOP("A step failed") },
        { QStringLiteral("TUR-DESKTOP-APPS-011"), QT_TR_NOOP("The application is not installed") },
        { QStringLiteral("TUR-DESKTOP-APPS-012"), QT_TR_NOOP("The application is already installed") },
        { QStringLiteral("TUR-DESKTOP-APPS-013"), QT_TR_NOOP("Nothing was done without your confirmation") },
        { QStringLiteral("TUR-DESKTOP-APPS-014"), QT_TR_NOOP("That way of running it is not offered here") },
        { QStringLiteral("TUR-DESKTOP-APPS-015"), QT_TR_NOOP("Something of the application is still there") },
        { QStringLiteral("TUR-DESKTOP-APPS-016"), QT_TR_NOOP("A record of an installed application is not valid") },
        { QStringLiteral("TUR-DESKTOP-APPS-017"), QT_TR_NOOP("An input is not valid") },
    };
    const auto found = titles.constFind(code);
    return found == titles.constEnd() ? theirs : tr(found.value());
}

QVariantMap Apps::failureOf(const AppsReply &reply, const QJsonObject &error)
{
    if (!error.isEmpty()) {
        const QString code = error.value(QLatin1String("code")).toString();
        QString note;
        if (error.value(QLatin1String("detail")).isObject() || error.value(QLatin1String("detail")).isArray())
            note = QStringLiteral("Detail: ")
                   + QString::fromUtf8(QJsonDocument(error.value(QLatin1String("detail")).toObject())
                                           .toJson(QJsonDocument::Compact))
                         .left(ShownErrorCharacters);
        return message(code.isEmpty() ? Unreadable : code,
                       titleOf(code, error.value(QLatin1String("title")).toString()),
                       error.value(QLatin1String("message")).toString(), technical(reply, note));
    }
    switch (reply.failure) {
    case AppsReply::NotStarted:
        return message(NotStarted, tr("The install backend could not be started"),
                       tr("The application centre installs through tur-apps, which is not installed or cannot be run."),
                       technical(reply));
    case AppsReply::TimedOut:
        return message(TimedOut, tr("tur-apps did not answer in time"),
                       tr("It was stopped. A source it asked may be slow or out of reach."), technical(reply));
    case AppsReply::TooLarge:
        return message(TooLarge, tr("tur-apps printed more than expected"),
                       tr("It was stopped, and its answer was not read."), technical(reply));
    case AppsReply::Crashed:
        return message(Crashed, tr("tur-apps ended abnormally"),
                       tr("What it was doing may not have finished. The list of what is installed shows what is there now."),
                       technical(reply));
    case AppsReply::NotRecorded:
        return message(NotRecorded, tr("No recorded answer for this"),
                       tr("The application centre is running on recorded answers, and none was recorded for this."),
                       technical(reply));
    case AppsReply::Refused:
        return message(Refused, tr("This cannot be passed to tur-apps"),
                       tr("The identifier or the file name is empty, too long, or has characters that cannot be passed on."),
                       technical(reply));
    case AppsReply::None:
        break;
    }
    return QVariantMap();
}
