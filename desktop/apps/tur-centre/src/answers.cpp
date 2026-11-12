// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#include "answers.h"

#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QJsonParseError>
#include <QJsonValue>
#include <QRegularExpression>

namespace {

// How much of the program's standard error the technical layer shows.
constexpr qsizetype ShownErrorCharacters = 2000;

const QRegularExpression &codePattern()
{
    static const QRegularExpression pattern(QStringLiteral("^TUR-[A-Z]+(?:-[A-Z0-9]+)+-[0-9]{3}$"));
    return pattern;
}

// The first problem found in a document, and the helpers that look.
class Shape
{
public:
    bool ok() const { return m_problem.isEmpty(); }
    QString problem() const { return m_problem; }

    void fail(const QString &where, const QString &what)
    {
        if (m_problem.isEmpty())
            m_problem = where + QLatin1String(": ") + what;
    }

    QString string(const QJsonObject &object, const char *key, const QString &where)
    {
        const QJsonValue value = object.value(QLatin1String(key));
        if (!value.isString())
            fail(where + QLatin1Char('.') + QLatin1String(key), QStringLiteral("is not text"));
        return value.toString();
    }

    void optionalString(const QJsonObject &object, const char *key, const QString &where)
    {
        const QJsonValue value = object.value(QLatin1String(key));
        if (!(value.isString() || value.isNull() || value.isUndefined()))
            fail(where + QLatin1Char('.') + QLatin1String(key), QStringLiteral("is neither text nor null"));
    }

    void boolean(const QJsonObject &object, const char *key, const QString &where)
    {
        if (!object.value(QLatin1String(key)).isBool())
            fail(where + QLatin1Char('.') + QLatin1String(key), QStringLiteral("is not true or false"));
    }

    QJsonArray array(const QJsonObject &object, const char *key, const QString &where)
    {
        const QJsonValue value = object.value(QLatin1String(key));
        if (!value.isArray())
            fail(where + QLatin1Char('.') + QLatin1String(key), QStringLiteral("is not a list"));
        return value.toArray();
    }

    QJsonObject object(const QJsonObject &parent, const char *key, const QString &where)
    {
        const QJsonValue value = parent.value(QLatin1String(key));
        if (!value.isObject())
            fail(where + QLatin1Char('.') + QLatin1String(key), QStringLiteral("is not an object"));
        return value.toObject();
    }

    QJsonObject element(const QJsonValue &value, const QString &where)
    {
        if (!value.isObject())
            fail(where, QStringLiteral("is not an object"));
        return value.toObject();
    }

    // A value of a fixed vocabulary: one this program has words for.
    QString word(const QJsonObject &object, const char *key, const QStringList &known, const QString &where)
    {
        const QString value = string(object, key, where);
        if (ok() && !known.contains(value))
            fail(where + QLatin1Char('.') + QLatin1String(key),
                 QStringLiteral("is \"%1\", which this version of the application centre has no words for")
                     .arg(value));
        return value;
    }

private:
    QString m_problem;
};

QString at(const QString &list, qsizetype index)
{
    return QStringLiteral("%1[%2]").arg(list).arg(index);
}

// The catalogue's own errors on standard error: "error[CODE]: message".
bool errorLine(const QByteArray &errorOutput, QString *code, QString *message)
{
    static const QRegularExpression line(
        QStringLiteral("^error\\[(TUR-[A-Z]+(?:-[A-Z0-9]+)+-[0-9]{3})\\]: (.+)$"),
        QRegularExpression::MultilineOption);
    const QRegularExpressionMatch match = line.match(QString::fromUtf8(errorOutput));
    if (!match.hasMatch())
        return false;
    *code = match.captured(1);
    *message = match.captured(2).trimmed();
    return true;
}

void checkRatingSource(Shape &shape, const QJsonObject &source, const QString &where)
{
    const QString kind = shape.word(source, "kind", Answers::ratingSourceKinds(), where);
    if (kind == QLatin1String("external")) {
        shape.string(source, "name", where);
        shape.string(source, "url", where);
        shape.string(source, "date", where);
        shape.optionalString(source, "source_rating", where);
        shape.optionalString(source, "version_tested", where);
    } else if (kind == QLatin1String("tur-test")) {
        shape.string(source, "report", where);
        shape.string(source, "date", where);
        shape.optionalString(source, "version_tested", where);
    }
}

} // namespace

QVariantMap Answers::Failure::toMap() const
{
    return QVariantMap {
        { QStringLiteral("code"), code },
        { QStringLiteral("title"), title },
        { QStringLiteral("reason"), reason },
        { QStringLiteral("detail"), detail },
        { QStringLiteral("retryable"), retryable },
    };
}

const QStringList &Answers::routes()
{
    static const QStringList values {
        QStringLiteral("flatpak"), QStringLiteral("native"), QStringLiteral("distro-container"),
        QStringLiteral("proton"), QStringLiteral("wine"), QStringLiteral("windows-guest"),
        QStringLiteral("android-runtime"), QStringLiteral("darling"), QStringLiteral("web"),
        QStringLiteral("no-route"),
    };
    return values;
}

const QStringList &Answers::ratings()
{
    static const QStringList values {
        QStringLiteral("good"), QStringLiteral("usable"), QStringLiteral("poor"), QStringLiteral("broken"),
        QStringLiteral("unknown"),
    };
    return values;
}

const QStringList &Answers::outlooks()
{
    static const QStringList values { QStringLiteral("clear"), QStringLiteral("doubtful"), QStringLiteral("blocked") };
    return values;
}

const QStringList &Answers::severities()
{
    static const QStringList values {
        QStringLiteral("note"), QStringLiteral("warning"), QStringLiteral("obstacle"), QStringLiteral("blocker"),
    };
    return values;
}

const QStringList &Answers::ratingSourceKinds()
{
    static const QStringList values { QStringLiteral("unrated"), QStringLiteral("external"),
                                      QStringLiteral("tur-test") };
    return values;
}

QString Answers::technicalDetail(const CatalogueReply &reply, const QString &note)
{
    QStringList lines;
    if (!reply.commandLine.isEmpty())
        lines << QStringLiteral("Command: %1").arg(reply.commandLine);
    if (reply.failure == CatalogueReply::None)
        lines << QStringLiteral("Exit status: %1").arg(reply.exitStatus);
    if (!reply.detail.isEmpty())
        lines << reply.detail;
    if (!note.isEmpty())
        lines << note;
    QString errors = QString::fromUtf8(reply.errorOutput).trimmed();
    if (!errors.isEmpty()) {
        if (errors.size() > ShownErrorCharacters)
            errors = errors.left(ShownErrorCharacters) + QStringLiteral(" ...");
        lines << QStringLiteral("Standard error: %1").arg(errors);
    }
    return lines.join(QLatin1Char('\n'));
}

namespace {

Answers::Failure unreadable(const CatalogueReply &reply, const QString &why)
{
    Answers::Failure failure;
    failure.code = Answers::Unreadable;
    failure.title = QCoreApplication::translate("Answers", "The application catalogue could not be read.");
    failure.reason = QCoreApplication::translate(
        "Answers",
        "The program that reads it answered in a form this version of the application centre does not understand.");
    failure.detail = Answers::technicalDetail(reply, why);
    return failure;
}

// The document the program printed, as a JSON object. False, with the
// failure filled in, when there is none, when the asking failed, and when
// the document is the catalogue's report of an error.
bool document(const CatalogueReply &reply, QJsonObject *object, Answers::Failure *failure)
{
    const QString couldNotBeRead =
        QCoreApplication::translate("Answers", "The application catalogue could not be read.");

    switch (reply.failure) {
    case CatalogueReply::None:
        break;
    case CatalogueReply::NotStarted:
        failure->code = Answers::NotStarted;
        failure->title = couldNotBeRead;
        failure->reason = QCoreApplication::translate("Answers", "The program that reads it could not be started.");
        failure->detail = Answers::technicalDetail(reply);
        return false;
    case CatalogueReply::TimedOut:
        failure->code = Answers::TimedOut;
        failure->title = couldNotBeRead;
        failure->reason = QCoreApplication::translate(
            "Answers", "The program that reads it did not answer in time, and was stopped.");
        failure->detail = Answers::technicalDetail(reply);
        return false;
    case CatalogueReply::TooLarge:
        failure->code = Answers::TooLarge;
        failure->title = couldNotBeRead;
        failure->reason = QCoreApplication::translate(
            "Answers", "The program that reads it printed more than the application centre accepts, and was stopped.");
        failure->detail = Answers::technicalDetail(reply);
        return false;
    case CatalogueReply::Crashed:
        failure->code = Answers::Crashed;
        failure->title = couldNotBeRead;
        failure->reason = QCoreApplication::translate("Answers", "The program that reads it stopped unexpectedly.");
        failure->detail = Answers::technicalDetail(reply);
        return false;
    case CatalogueReply::NotRecorded:
        failure->code = Answers::NotRecorded;
        failure->title = QCoreApplication::translate("Answers", "There is no recorded answer for this.");
        failure->reason = QCoreApplication::translate(
            "Answers",
            "The application centre was started with recorded answers in place of the catalogue, and none was "
            "recorded for this request.");
        failure->detail = Answers::technicalDetail(reply);
        failure->retryable = false;
        return false;
    case CatalogueReply::Refused:
        failure->code = Answers::Refused;
        failure->title = QCoreApplication::translate("Answers", "That cannot be looked up in the catalogue.");
        failure->reason = QCoreApplication::translate(
            "Answers", "It is too long, or it contains characters that are not text.");
        failure->detail = Answers::technicalDetail(reply);
        failure->retryable = false;
        return false;
    }

    if (reply.output.trimmed().isEmpty()) {
        // With --json the catalogue still reports a catalogue it cannot
        // load on standard error, as text.
        QString code;
        QString message;
        if (errorLine(reply.errorOutput, &code, &message)) {
            failure->code = code;
            failure->title = couldNotBeRead;
            failure->reason = message;
            failure->detail = Answers::technicalDetail(reply);
            return false;
        }
        *failure = unreadable(reply, QStringLiteral("The program printed nothing on standard output."));
        return false;
    }

    QJsonParseError error;
    const QJsonDocument parsed = QJsonDocument::fromJson(reply.output, &error);
    if (error.error != QJsonParseError::NoError) {
        *failure = unreadable(reply, QStringLiteral("Not JSON: %1 at byte %2 of %3.")
                                         .arg(error.errorString())
                                         .arg(error.offset)
                                         .arg(reply.output.size()));
        return false;
    }
    if (!parsed.isObject()) {
        *failure = unreadable(reply, QStringLiteral("The answer is JSON, but not an object."));
        return false;
    }

    const QJsonObject root = parsed.object();
    if (root.contains(QLatin1String("error"))) {
        const QJsonObject reported = root.value(QLatin1String("error")).toObject();
        const QString code = reported.value(QLatin1String("code")).toString();
        const QString title = reported.value(QLatin1String("title")).toString();
        const QString message = reported.value(QLatin1String("message")).toString();
        if (!codePattern().match(code).hasMatch() || title.isEmpty() || message.isEmpty()) {
            *failure = unreadable(reply, QStringLiteral("error: lacks a code, a title or a message."));
            return false;
        }
        // The catalogue's own code and words (specification 29.3).
        failure->code = code;
        failure->title = title;
        failure->reason = message;
        failure->detail = Answers::technicalDetail(reply);
        failure->retryable = code != Answers::UnknownApplication && code != Answers::NoMatch;
        return false;
    }

    *object = root;
    return true;
}

} // namespace

bool Answers::readSearch(const CatalogueReply &reply, SearchAnswer *answer, Failure *failure)
{
    QJsonObject root;
    if (!document(reply, &root, failure))
        return false;

    Shape shape;
    SearchAnswer out;
    out.query = shape.string(root, "query", QStringLiteral("search"));
    const QJsonArray hits = shape.array(root, "hits", QStringLiteral("search"));
    for (qsizetype i = 0; i < hits.size() && shape.ok(); ++i) {
        const QString where = at(QStringLiteral("hits"), i);
        const QJsonObject object = shape.element(hits.at(i), where);
        Hit hit;
        hit.id = shape.string(object, "id", where);
        hit.name = shape.string(object, "name", where);
        hit.category = shape.string(object, "category", where);
        hit.publisher = shape.string(object, "publisher", where);
        hit.matched = shape.string(object, "matched", where);
        hit.kind = shape.string(object, "kind", where);
        if (!object.value(QLatin1String("score")).isDouble())
            shape.fail(where + QLatin1String(".score"), QStringLiteral("is not a number"));
        hit.score = object.value(QLatin1String("score")).toInt();
        if (shape.ok() && (hit.id.isEmpty() || hit.name.isEmpty()))
            shape.fail(where, QStringLiteral("has no identifier or no name"));
        out.hits.append(hit);
    }
    if (!shape.ok()) {
        *failure = unreadable(reply, shape.problem());
        return false;
    }
    *answer = out;
    return true;
}

bool Answers::readEntry(const CatalogueReply &reply, QVariantMap *entry, Failure *failure)
{
    QJsonObject root;
    if (!document(reply, &root, failure))
        return false;

    Shape shape;
    const QString where = QStringLiteral("entry");
    shape.string(root, "id", where);
    shape.string(root, "name", where);
    shape.string(root, "publisher", where);
    shape.string(root, "category", where);
    shape.string(root, "entry_kind", where);
    shape.optionalString(root, "summary", where);
    shape.optionalString(root, "homepage", where);
    const QJsonArray editions = shape.array(root, "editions", where);
    for (qsizetype i = 0; i < editions.size() && shape.ok(); ++i) {
        const QString here = at(QStringLiteral("editions"), i);
        const QJsonObject edition = shape.element(editions.at(i), here);
        shape.string(edition, "kind", here);
        shape.optionalString(edition, "obtain", here);
        shape.optionalString(edition, "store_id", here);
        shape.optionalString(edition, "download_page", here);
        shape.optionalString(edition, "url", here);
        shape.optionalString(edition, "distribution", here);
        const QJsonArray records = shape.array(edition, "records", here);
        for (qsizetype r = 0; r < records.size() && shape.ok(); ++r) {
            const QString there = here + QLatin1Char('.') + at(QStringLiteral("records"), r);
            const QJsonObject record = shape.element(records.at(r), there);
            shape.word(record, "route", routes(), there);
            shape.word(record, "rating", ratings(), there);
            shape.string(record, "reported_by", there);
            shape.string(record, "date", there);
            shape.optionalString(record, "source_name", there);
            shape.optionalString(record, "source_url", there);
            shape.optionalString(record, "source_rating", there);
            shape.optionalString(record, "test_report", there);
            shape.optionalString(record, "version_tested", there);
        }
        const QJsonArray references = shape.array(edition, "references", here);
        for (qsizetype r = 0; r < references.size() && shape.ok(); ++r) {
            const QString there = here + QLatin1Char('.') + at(QStringLiteral("references"), r);
            const QJsonObject reference = shape.element(references.at(r), there);
            shape.string(reference, "title", there);
            shape.string(reference, "url", there);
            shape.string(reference, "checked", there);
        }
    }
    if (!shape.ok()) {
        *failure = unreadable(reply, shape.problem());
        return false;
    }
    *entry = root.toVariantMap();
    return true;
}

bool Answers::readRoutes(const CatalogueReply &reply, QVariantMap *report, Failure *failure)
{
    QJsonObject root;
    if (!document(reply, &root, failure))
        return false;

    Shape shape;
    const QString top = QStringLiteral("route");
    const QJsonObject application = shape.object(root, "application", top);
    shape.string(application, "id", QStringLiteral("application"));
    shape.string(application, "name", QStringLiteral("application"));
    shape.string(application, "category", QStringLiteral("application"));
    shape.boolean(root, "host_assumed", top);
    shape.word(root, "recommended", routes(), top);
    const QJsonArray options = shape.array(root, "options", top);
    if (shape.ok() && options.isEmpty())
        shape.fail(QStringLiteral("options"), QStringLiteral("is empty; the router never returns an empty answer"));

    int recommended = 0;
    for (qsizetype i = 0; i < options.size() && shape.ok(); ++i) {
        const QString where = at(QStringLiteral("options"), i);
        const QJsonObject option = shape.element(options.at(i), where);
        shape.word(option, "route", routes(), where);
        shape.word(option, "rating", ratings(), where);
        shape.word(option, "outlook", outlooks(), where);
        shape.string(option, "reason", where);
        shape.optionalString(option, "edition", where);
        shape.optionalString(option, "edition_kind", where);
        shape.boolean(option, "recommended", where);
        shape.boolean(option, "pinned", where);
        const QJsonValue index = option.value(QLatin1String("edition_index"));
        if (!(index.isDouble() || index.isNull()))
            shape.fail(where + QLatin1String(".edition_index"), QStringLiteral("is neither a number nor null"));
        if (option.value(QLatin1String("recommended")).toBool())
            ++recommended;

        checkRatingSource(shape, shape.object(option, "rating_source", where), where + QLatin1String(".rating_source"));

        const QJsonArray caveats = shape.array(option, "caveats", where);
        for (qsizetype c = 0; c < caveats.size() && shape.ok(); ++c) {
            const QString here = where + QLatin1Char('.') + at(QStringLiteral("caveats"), c);
            const QJsonObject caveat = shape.element(caveats.at(c), here);
            shape.string(caveat, "code", here);
            shape.word(caveat, "severity", severities(), here);
            shape.optionalString(caveat, "requirement", here);
            shape.string(caveat, "text", here);
        }

        const QJsonArray prerequisites = shape.array(option, "prerequisites", where);
        for (qsizetype p = 0; p < prerequisites.size() && shape.ok(); ++p) {
            const QString here = where + QLatin1Char('.') + at(QStringLiteral("prerequisites"), p);
            const QJsonObject prerequisite = shape.element(prerequisites.at(p), here);
            shape.string(prerequisite, "code", here);
            shape.string(prerequisite, "text", here);
        }

        const QJsonValue alternative = option.value(QLatin1String("alternative"));
        if (alternative.isObject()) {
            const QString here = where + QLatin1String(".alternative");
            shape.string(alternative.toObject(), "kind", here);
            shape.string(alternative.toObject(), "text", here);
        } else if (!alternative.isNull()) {
            shape.fail(where + QLatin1String(".alternative"), QStringLiteral("is neither an object nor null"));
        }
    }
    if (shape.ok() && recommended != 1)
        shape.fail(QStringLiteral("options"),
                   QStringLiteral("%1 options are marked recommended; exactly one must be").arg(recommended));

    if (!shape.ok()) {
        *failure = unreadable(reply, shape.problem());
        return false;
    }
    *report = root.toVariantMap();
    return true;
}
