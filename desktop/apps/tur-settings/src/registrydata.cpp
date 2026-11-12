// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#include "registrydata.h"

#include "address.h"

#include <QChar>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonParseError>
#include <QJsonValue>
#include <QRegularExpression>
#include <QSet>

#include <algorithm>
#include <cmath>
#include <utility>

namespace Settings {

namespace {

const QStringList &kinds()
{
    static const QStringList list = { QStringLiteral("switch"), QStringLiteral("choice"), QStringLiteral("number"),
                                      QStringLiteral("text"),   QStringLiteral("list"),   QStringLiteral("action"),
                                      QStringLiteral("information") };
    return list;
}

bool matches(const char *pattern, const QString &text)
{
    return QRegularExpression(QString::fromLatin1(pattern)).match(text).hasMatch();
}

bool isBusName(const QString &text)
{
    return text.size() <= 255 && matches("\\A[A-Za-z_][A-Za-z0-9_]*(?:\\.[A-Za-z_][A-Za-z0-9_]*)+\\z", text);
}

bool isText(const QString &text)
{
    if (text.size() > 200 || text.trimmed().isEmpty())
        return false;
    for (const QChar ch : text) {
        const char16_t code = ch.unicode();
        if (code < 0x20 || (code >= 0x7f && code <= 0x9f))
            return false;
    }
    return true;
}

// Reads the values of one registry file and collects what is wrong with
// them. Every message says where: "pages[2].rows[0].title".
class Reader
{
public:
    Reader(const QString &name, QStringList *problems)
        : m_name(name)
        , m_problems(problems)
    {
    }

    bool ok() const { return m_ok; }

    void fail(const QString &where, const QString &what)
    {
        m_ok = false;
        if (m_problems)
            m_problems->append(m_name + QLatin1String(": ") + where + QLatin1String(": ") + what);
    }

    // Every required key is there and no key is unknown.
    void keys(const QJsonObject &object, const QString &where, const QStringList &required, const QStringList &optional)
    {
        for (const QString &key : required) {
            if (!object.contains(key))
                fail(where, QStringLiteral("\"%1\" is missing").arg(key));
        }
        for (auto it = object.constBegin(); it != object.constEnd(); ++it) {
            if (!required.contains(it.key()) && !optional.contains(it.key()))
                fail(where, QStringLiteral("\"%1\" is not a key of the format").arg(it.key()));
        }
    }

    // A string, if the key is there; empty otherwise.
    QString string(const QJsonObject &object, const QString &key, const QString &where)
    {
        if (!object.contains(key))
            return QString();
        const QJsonValue value = object.value(key);
        if (!value.isString() || value.toString().isEmpty() || value.toString().size() > 255) {
            fail(where + QLatin1Char('.') + key, QStringLiteral("is not a string of 1 to 255 characters"));
            return QString();
        }
        return value.toString();
    }

    // A source string: not blank, at most 200 characters, no control
    // characters.
    QString text(const QJsonObject &object, const QString &key, const QString &where)
    {
        if (!object.contains(key))
            return QString();
        const QJsonValue value = object.value(key);
        if (!value.isString() || !isText(value.toString())) {
            fail(where + QLatin1Char('.') + key,
                 QStringLiteral("is not text: a string that is not blank, of at most 200 characters, without "
                                "control characters"));
            return QString();
        }
        return value.toString();
    }

    QStringList texts(const QJsonObject &object, const QString &key, const QString &where, int most, bool unique)
    {
        QStringList out;
        if (!object.contains(key))
            return out;
        const QString here = where + QLatin1Char('.') + key;
        const QJsonValue value = object.value(key);
        if (!value.isArray()) {
            fail(here, QStringLiteral("is not a list"));
            return out;
        }
        const QJsonArray array = value.toArray();
        if (array.size() > most)
            fail(here, QStringLiteral("has more than %1 items").arg(most));
        for (const QJsonValue &item : array) {
            if (!item.isString() || !isText(item.toString())) {
                fail(here, QStringLiteral("holds something that is not text"));
                continue;
            }
            if (unique && out.contains(item.toString()))
                fail(here, QStringLiteral("holds \"%1\" twice").arg(item.toString()));
            out.append(item.toString());
        }
        return out;
    }

    // An array of objects, if the key is there.
    QJsonArray objects(const QJsonObject &object, const QString &key, const QString &where, int most)
    {
        if (!object.contains(key))
            return QJsonArray();
        const QString here = where + QLatin1Char('.') + key;
        const QJsonValue value = object.value(key);
        if (!value.isArray()) {
            fail(here, QStringLiteral("is not a list"));
            return QJsonArray();
        }
        const QJsonArray array = value.toArray();
        if (array.size() > most)
            fail(here, QStringLiteral("has more than %1 items").arg(most));
        for (qsizetype i = 0; i < array.size(); ++i) {
            if (!array.at(i).isObject()) {
                fail(QStringLiteral("%1[%2]").arg(here).arg(i), QStringLiteral("is not an object"));
                return QJsonArray();
            }
        }
        return array;
    }

private:
    QString m_name;
    QStringList *m_problems;
    bool m_ok = true;
};

void readSource(Reader &reader, const QJsonObject &source, const QString &where, Row &row)
{
    const QString type = reader.string(source, QStringLiteral("type"), where);
    row.sourceType = type;

    if (!source.value(QStringLiteral("available")).isBool())
        reader.fail(where + QLatin1String(".available"), QStringLiteral("is not true or false"));
    row.sourceAvailable = source.value(QStringLiteral("available")).toBool(true);
    row.waitsFor = reader.text(source, QStringLiteral("waits_for"), where);
    if (!row.sourceAvailable && !source.contains(QStringLiteral("waits_for")))
        reader.fail(where, QStringLiteral("a source that is not available says what it waits for (waits_for)"));
    if (row.sourceAvailable && source.contains(QStringLiteral("waits_for")))
        reader.fail(where, QStringLiteral("a source that is available waits for nothing (waits_for)"));

    const QStringList common = { QStringLiteral("type"), QStringLiteral("available") };
    const QStringList waits = { QStringLiteral("waits_for") };
    auto shape = [&](const QStringList &required, const QStringList &optional) {
        reader.keys(source, where, common + required, waits + optional);
    };

    if (type == QLatin1String("file")) {
        shape({ QStringLiteral("path") }, { QStringLiteral("key") });
        const QString path = reader.string(source, QStringLiteral("path"), where);
        if (!path.isEmpty() && !path.startsWith(QLatin1Char('/')))
            reader.fail(where + QLatin1String(".path"), QStringLiteral("is not an absolute path"));
        const QString key = reader.string(source, QStringLiteral("key"), where);
        if (!key.isEmpty() && !matches("\\A[A-Z][A-Z0-9_]*\\z", key))
            reader.fail(where + QLatin1String(".key"), QStringLiteral("is not a key of a file of KEY=VALUE lines"));
    } else if (type == QLatin1String("dbus")) {
        shape({ QStringLiteral("bus"), QStringLiteral("service") }, { QStringLiteral("member") });
        const QString bus = reader.string(source, QStringLiteral("bus"), where);
        if (!bus.isEmpty() && bus != QLatin1String("session") && bus != QLatin1String("system"))
            reader.fail(where + QLatin1String(".bus"), QStringLiteral("is neither session nor system"));
        const QString service = reader.string(source, QStringLiteral("service"), where);
        if (!service.isEmpty() && !isBusName(service))
            reader.fail(where + QLatin1String(".service"), QStringLiteral("is not a D-Bus name"));
        reader.string(source, QStringLiteral("member"), where);
    } else if (type == QLatin1String("command")) {
        shape({ QStringLiteral("program") }, { QStringLiteral("arguments") });
        const QString program = reader.string(source, QStringLiteral("program"), where);
        if (!program.isEmpty() && (!program.startsWith(QLatin1Char('/')) || program.contains(QLatin1Char(' '))))
            reader.fail(where + QLatin1String(".program"),
                        QStringLiteral("is not the absolute path of a program; its arguments are a list"));
        if (source.contains(QStringLiteral("arguments"))) {
            const QJsonValue arguments = source.value(QStringLiteral("arguments"));
            bool strings = arguments.isArray() && arguments.toArray().size() <= 16;
            const QJsonArray array = arguments.toArray();
            for (const QJsonValue &argument : array)
                strings = strings && argument.isString();
            if (!strings)
                reader.fail(where + QLatin1String(".arguments"), QStringLiteral("is not a list of at most 16 strings"));
        }
    } else if (type == QLatin1String("kde-config")) {
        shape({ QStringLiteral("file"), QStringLiteral("group"), QStringLiteral("key") }, {});
        reader.string(source, QStringLiteral("file"), where);
        reader.string(source, QStringLiteral("group"), where);
        reader.string(source, QStringLiteral("key"), where);
    } else if (type == QLatin1String("platform")) {
        shape({ QStringLiteral("hint") }, {});
        const QString hint = reader.string(source, QStringLiteral("hint"), where);
        if (!hint.isEmpty() && hint != QLatin1String("colour-scheme"))
            reader.fail(where + QLatin1String(".hint"), QStringLiteral("is not a hint this format knows"));
    } else if (type == QLatin1String("program")) {
        shape({ QStringLiteral("what") }, {});
        const QString what = reader.string(source, QStringLiteral("what"), where);
        if (!what.isEmpty() && !matches("\\A[a-z][a-z0-9-]*\\z", what))
            reader.fail(where + QLatin1String(".what"), QStringLiteral("is not a lower-case name"));
    } else if (type == QLatin1String("address")) {
        shape({ QStringLiteral("opens") }, {});
        row.opens = reader.string(source, QStringLiteral("opens"), where);
        if (!row.opens.isEmpty() && !Address::parse(row.opens).isValid())
            reader.fail(where + QLatin1String(".opens"), QStringLiteral("is not an address"));
    } else if (!type.isEmpty()) {
        reader.fail(where + QLatin1String(".type"), QStringLiteral("\"%1\" is not a kind of source").arg(type));
    }
}

Row readRow(Reader &reader, const QJsonObject &object, const QString &where, const Page &page)
{
    Row row;
    row.json = object;
    reader.keys(object, where, { QStringLiteral("id"), QStringLiteral("title") },
                { QStringLiteral("keywords"), QStringLiteral("kind"), QStringLiteral("section"),
                  QStringLiteral("source"), QStringLiteral("change"), QStringLiteral("locator"),
                  QStringLiteral("status"), QStringLiteral("replaced_by") });

    row.id = reader.string(object, QStringLiteral("id"), where);
    if (!row.id.isEmpty() && !Address::isRowId(row.id))
        reader.fail(where + QLatin1String(".id"),
                    QStringLiteral("\"%1\" is not a row identifier: lower-case letters, digits and hyphens").arg(row.id));
    row.title = reader.text(object, QStringLiteral("title"), where);
    row.keywords = reader.texts(object, QStringLiteral("keywords"), where, 24, true);

    row.status = object.contains(QStringLiteral("status")) ? reader.string(object, QStringLiteral("status"), where)
                                                           : QStringLiteral("available");
    if (row.status != QLatin1String("available") && row.status != QLatin1String("removed"))
        reader.fail(where + QLatin1String(".status"), QStringLiteral("is neither available nor removed"));

    row.replacedBy = reader.string(object, QStringLiteral("replaced_by"), where);
    if (!row.replacedBy.isEmpty() && !Address::parse(row.replacedBy).isValid())
        reader.fail(where + QLatin1String(".replaced_by"), QStringLiteral("is not an address"));

    if (row.isRemoved()) {
        // What is left of a removed row is its address, its last title and
        // its successor.
        for (const char *key : { "kind", "section", "source", "change", "locator" }) {
            if (object.contains(QLatin1String(key)))
                reader.fail(where, QStringLiteral("a removed row has no \"%1\"").arg(QLatin1String(key)));
        }
        return row;
    }
    if (!row.replacedBy.isEmpty())
        reader.fail(where, QStringLiteral("only a removed row is replaced by another (replaced_by)"));
    if (!page.isAvailable())
        reader.fail(where, QStringLiteral("its page is %1, so the row cannot be there").arg(page.status));

    row.kind = reader.string(object, QStringLiteral("kind"), where);
    if (!object.contains(QStringLiteral("kind")))
        reader.fail(where, QStringLiteral("\"kind\" is missing"));
    else if (!row.kind.isEmpty() && !kinds().contains(row.kind))
        reader.fail(where + QLatin1String(".kind"), QStringLiteral("\"%1\" is not a kind of row").arg(row.kind));

    row.section = reader.string(object, QStringLiteral("section"), where);
    if (!page.sections.isEmpty() && page.sectionTitle(row.section).isEmpty())
        reader.fail(where, QStringLiteral("it names no section of its page"));
    if (page.sections.isEmpty() && object.contains(QStringLiteral("section")))
        reader.fail(where, QStringLiteral("it names a section and its page declares none"));

    if (object.contains(QStringLiteral("source"))) {
        if (object.value(QStringLiteral("source")).isObject())
            readSource(reader, object.value(QStringLiteral("source")).toObject(), where + QLatin1String(".source"), row);
        else
            reader.fail(where + QLatin1String(".source"), QStringLiteral("is not an object"));
    }

    const bool information = row.kind == QLatin1String("information");
    if (object.contains(QStringLiteral("change"))) {
        const QString here = where + QLatin1String(".change");
        const QJsonObject change = object.value(QStringLiteral("change")).toObject();
        if (!object.value(QStringLiteral("change")).isObject())
            reader.fail(here, QStringLiteral("is not an object"));
        reader.keys(change, here, { QStringLiteral("class") }, { QStringLiteral("tool") });
        const QJsonValue number = change.value(QStringLiteral("class"));
        const double value = number.toDouble(-1);
        if (!number.isDouble() || value != std::floor(value) || value < 0 || value > 4)
            reader.fail(here + QLatin1String(".class"), QStringLiteral("is not an action class, 0 to 4"));
        else
            row.changeClass = int(value);
        row.tool = reader.string(change, QStringLiteral("tool"), here);
        if (!row.tool.isEmpty() && !matches("\\A[a-z][a-z0-9_]{0,63}\\z", row.tool))
            reader.fail(here + QLatin1String(".tool"), QStringLiteral("is not the name of a tool"));
        if (information)
            reader.fail(where, QStringLiteral("information is only read: it has no \"change\""));
    } else if (!information && !row.kind.isEmpty()) {
        reader.fail(where, QStringLiteral("a row that can be changed says what changing it is (change)"));
    }

    if (object.contains(QStringLiteral("locator"))) {
        const QString here = where + QLatin1String(".locator");
        const QJsonObject locator = object.value(QStringLiteral("locator")).toObject();
        if (!object.value(QStringLiteral("locator")).isObject() || locator.isEmpty())
            reader.fail(here, QStringLiteral("is not an object that says how the control is found"));
        reader.keys(locator, here, {},
                    { QStringLiteral("accessible_id"), QStringLiteral("role"), QStringLiteral("label_catalog"),
                      QStringLiteral("label_msgid") });
        for (auto it = locator.constBegin(); it != locator.constEnd(); ++it)
            reader.string(locator, it.key(), here);
    }
    return row;
}

Page readPage(Reader &reader, const QJsonObject &object, const QString &where, const QString &program)
{
    Page page;
    page.json = object;
    page.program = program;
    reader.keys(object, where, { QStringLiteral("page"), QStringLiteral("title"), QStringLiteral("path") },
                { QStringLiteral("keywords"), QStringLiteral("status"), QStringLiteral("replaced_by"),
                  QStringLiteral("listed"), QStringLiteral("open"), QStringLiteral("verified_with"),
                  QStringLiteral("sections"), QStringLiteral("rows") });

    page.address = reader.string(object, QStringLiteral("page"), where);
    if (!page.address.isEmpty() && !Address::isPage(page.address)) {
        reader.fail(where + QLatin1String(".page"),
                    QStringLiteral("\"%1\" is not the address of a page: tur or kde, then lower-case segments")
                        .arg(page.address));
    }
    page.title = reader.text(object, QStringLiteral("title"), where);
    page.path = reader.texts(object, QStringLiteral("path"), where, 4, false);
    if (object.contains(QStringLiteral("path")) && (page.path.isEmpty() || page.path.constLast() != page.title))
        reader.fail(where + QLatin1String(".path"), QStringLiteral("does not end in the page's title"));
    page.keywords = reader.texts(object, QStringLiteral("keywords"), where, 24, true);

    page.status = object.contains(QStringLiteral("status")) ? reader.string(object, QStringLiteral("status"), where)
                                                            : QStringLiteral("available");
    if (page.status != QLatin1String("available") && page.status != QLatin1String("planned")
        && page.status != QLatin1String("removed")) {
        reader.fail(where + QLatin1String(".status"), QStringLiteral("is not available, planned or removed"));
    }
    page.replacedBy = reader.string(object, QStringLiteral("replaced_by"), where);
    if (!page.replacedBy.isEmpty() && !Address::parse(page.replacedBy).isValid())
        reader.fail(where + QLatin1String(".replaced_by"), QStringLiteral("is not an address"));
    if (!page.replacedBy.isEmpty() && !page.isRemoved())
        reader.fail(where, QStringLiteral("only a removed page is replaced by another (replaced_by)"));

    if (object.contains(QStringLiteral("listed"))) {
        if (!object.value(QStringLiteral("listed")).isBool())
            reader.fail(where + QLatin1String(".listed"), QStringLiteral("is not true or false"));
        page.listed = object.value(QStringLiteral("listed")).toBool(true);
    }

    const bool kde = page.address.startsWith(QLatin1String("kde."));
    if (object.contains(QStringLiteral("open"))) {
        const QString here = where + QLatin1String(".open");
        const QJsonObject open = object.value(QStringLiteral("open")).toObject();
        if (!object.value(QStringLiteral("open")).isObject())
            reader.fail(here, QStringLiteral("is not an object"));
        reader.keys(open, here, { QStringLiteral("module") }, { QStringLiteral("arguments") });
        page.module = reader.string(open, QStringLiteral("module"), here);
        if (!page.module.isEmpty() && !matches("\\A[a-z][a-z0-9_-]{0,63}\\z", page.module))
            reader.fail(here + QLatin1String(".module"), QStringLiteral("is not the identifier of a module"));
        if (open.contains(QStringLiteral("arguments"))) {
            const QJsonValue arguments = open.value(QStringLiteral("arguments"));
            bool fine = arguments.isArray() && arguments.toArray().size() <= 8;
            const QJsonArray array = arguments.toArray();
            for (const QJsonValue &argument : array) {
                const QString text = argument.toString();
                fine = fine && argument.isString() && !text.isEmpty() && text.size() <= 255
                       && !matches("[\\s\\x00-\\x1f]", text);
                page.moduleArguments.append(text);
            }
            if (!fine) {
                reader.fail(here + QLatin1String(".arguments"),
                            QStringLiteral("is not a list of at most 8 arguments without spaces"));
            }
        }
        if (!kde)
            reader.fail(where, QStringLiteral("a page of a TUR program is opened by its address, not by \"open\""));
    } else if (kde && page.isAvailable()) {
        reader.fail(where, QStringLiteral("a page of KDE System Settings says how it is opened (open)"));
    }
    page.verifiedWith = reader.string(object, QStringLiteral("verified_with"), where);
    if (!page.verifiedWith.isEmpty() && !matches("\\A[a-z][a-z0-9-]*-[0-9][0-9A-Za-z.]*\\z", page.verifiedWith))
        reader.fail(where + QLatin1String(".verified_with"), QStringLiteral("is not a program and a version"));

    const QJsonArray sections = reader.objects(object, QStringLiteral("sections"), where, 16);
    for (qsizetype i = 0; i < sections.size(); ++i) {
        const QString here = QStringLiteral("%1.sections[%2]").arg(where).arg(i);
        const QJsonObject entry = sections.at(i).toObject();
        reader.keys(entry, here, { QStringLiteral("id"), QStringLiteral("title") }, {});
        Section section;
        section.id = reader.string(entry, QStringLiteral("id"), here);
        section.title = reader.text(entry, QStringLiteral("title"), here);
        if (!section.id.isEmpty() && !Address::isRowId(section.id))
            reader.fail(here + QLatin1String(".id"), QStringLiteral("is not an identifier"));
        if (!section.id.isEmpty() && !page.sectionTitle(section.id).isEmpty())
            reader.fail(here, QStringLiteral("the section %1 is declared twice").arg(section.id));
        page.sections.append(section);
    }

    const QJsonArray rows = reader.objects(object, QStringLiteral("rows"), where, 200);
    for (qsizetype i = 0; i < rows.size(); ++i) {
        const QString here = QStringLiteral("%1.rows[%2]").arg(where).arg(i);
        const Row row = readRow(reader, rows.at(i).toObject(), here, page);
        if (!row.id.isEmpty() && page.row(row.id))
            reader.fail(here, QStringLiteral("the identifier %1 is used twice on the page").arg(row.id));
        page.rows.append(row);
    }
    return page;
}

// --- Searching ---------------------------------------------------------------

// The words of a text, folded for comparison. "P@x" is one word.
QStringList wordsOf(const QString &text)
{
    QStringList out;
    QString word;
    const QString folded = text.toCaseFolded();
    for (const QChar ch : folded) {
        if (ch.isLetterOrNumber() || ch == QLatin1Char('@')) {
            word += ch;
        } else if (!word.isEmpty()) {
            out.append(word);
            word.clear();
        }
    }
    if (!word.isEmpty())
        out.append(word);
    return out;
}

// How well one word of the query matches a text: 1 if it is the whole text,
// less if it is one of its words, less again if a word begins with it, and
// least if it is only somewhere inside.
double wordMatch(const QString &word, const QString &text)
{
    const QStringList words = wordsOf(text);
    if (words.isEmpty())
        return 0;
    if (words.size() == 1 && words.constFirst() == word)
        return 1.0;
    double best = 0;
    for (const QString &candidate : words) {
        if (candidate == word)
            best = std::max(best, 0.9);
        else if (candidate.startsWith(word))
            best = std::max(best, 0.75);
        else if (word.size() >= 3 && candidate.contains(word))
            best = std::max(best, 0.5);
    }
    return best;
}

struct Field
{
    QString text;
    double weight;
};

// The score of an entry for a query: every word must match some field; the
// score is the mean of each word's best match, or the match of the whole
// query against one field if that is better.
double scoreOf(const QStringList &query, const QList<Field> &fields)
{
    double sum = 0;
    for (const QString &word : query) {
        double best = 0;
        for (const Field &field : fields)
            best = std::max(best, field.weight * wordMatch(word, field.text));
        if (best <= 0)
            return 0;
        sum += best;
    }
    double score = sum / double(query.size());
    if (query.size() > 1) {
        for (const Field &field : fields) {
            if (wordsOf(field.text) == query)
                score = std::max(score, field.weight);
        }
    }
    return std::round(score * 1000.0) / 1000.0;
}

} // namespace

const Row *Page::row(const QString &id) const
{
    for (const Row &row : rows) {
        if (row.id == id)
            return &row;
    }
    return nullptr;
}

QString Page::sectionTitle(const QString &id) const
{
    for (const Section &section : sections) {
        if (section.id == id)
            return section.title;
    }
    return QString();
}

void RegistryData::clear()
{
    m_pages.clear();
    m_index.clear();
    m_programs.clear();
    m_documents.clear();
}

bool RegistryData::add(const QByteArray &json, const QString &name, QStringList *problems)
{
    Reader reader(name, problems);
    if (json.size() > MaximumFileSize) {
        reader.fail(QStringLiteral("file"), QStringLiteral("is larger than %1 bytes").arg(MaximumFileSize));
        return false;
    }
    QJsonParseError error;
    const QJsonDocument document = QJsonDocument::fromJson(json, &error);
    if (error.error != QJsonParseError::NoError) {
        reader.fail(QStringLiteral("file"), QStringLiteral("is not JSON: %1 at byte %2").arg(error.errorString()).arg(error.offset));
        return false;
    }
    if (!document.isObject()) {
        reader.fail(QStringLiteral("file"), QStringLiteral("is not a JSON object"));
        return false;
    }
    const QJsonObject root = document.object();
    const QString top = QStringLiteral("registry");
    reader.keys(root, top, { QStringLiteral("schema_version"), QStringLiteral("program"), QStringLiteral("pages") },
                { QStringLiteral("licence") });

    const QJsonValue version = root.value(QStringLiteral("schema_version"));
    if (!version.isDouble() || version.toDouble() != 1.0)
        reader.fail(top + QLatin1String(".schema_version"), QStringLiteral("is not 1, the only version this program reads"));
    reader.string(root, QStringLiteral("licence"), top);

    const QString program = reader.string(root, QStringLiteral("program"), top);
    if (!program.isEmpty() && !isBusName(program))
        reader.fail(top + QLatin1String(".program"), QStringLiteral("is not an application identifier such as org.tur.Settings"));
    if (!program.isEmpty() && m_programs.contains(program))
        reader.fail(top + QLatin1String(".program"), QStringLiteral("the registry of %1 is already loaded").arg(program));

    const QJsonArray entries = reader.objects(root, QStringLiteral("pages"), top, 500);
    if (root.contains(QStringLiteral("pages")) && entries.isEmpty())
        reader.fail(top + QLatin1String(".pages"), QStringLiteral("is empty or is not a list of pages"));

    QList<Page> pages;
    QSet<QString> seen;
    for (qsizetype i = 0; i < entries.size(); ++i) {
        const QString where = QStringLiteral("pages[%1]").arg(i);
        const Page page = readPage(reader, entries.at(i).toObject(), where, program);
        if (!page.address.isEmpty()) {
            if (seen.contains(page.address))
                reader.fail(where, QStringLiteral("the page %1 is in this file twice").arg(page.address));
            if (m_index.contains(page.address)) {
                reader.fail(where, QStringLiteral("the page %1 is already registered by %2")
                                       .arg(page.address, m_pages.at(m_index.value(page.address)).program));
            }
            seen.insert(page.address);
        }
        pages.append(page);
    }

    if (!reader.ok())
        return false;
    for (const Page &page : std::as_const(pages)) {
        m_index.insert(page.address, int(m_pages.size()));
        m_pages.append(page);
    }
    m_programs.append(program);
    m_documents.insert(program, json);
    return true;
}

QStringList RegistryData::crossProblems() const
{
    QStringList out;
    auto judge = [&](const QString &own, const QString &field, const QString &target) {
        if (target.isEmpty())
            return;
        const Address address = Address::parse(target);
        const Page *page = this->page(address.page);
        const Row *row = page && address.hasRow() ? page->row(address.row) : nullptr;
        if (target == own)
            out.append(QStringLiteral("%1: %2 names the address itself").arg(own, field));
        else if (!page || (address.hasRow() && !row))
            out.append(QStringLiteral("%1: %2 names %3, which no registry has").arg(own, field, target));
        else if (page->isRemoved() || (row && row->isRemoved()))
            out.append(QStringLiteral("%1: %2 names %3, which is itself removed").arg(own, field, target));
    };
    for (const Page &page : m_pages) {
        judge(page.address, QStringLiteral("replaced_by"), page.replacedBy);
        for (const Row &row : page.rows) {
            const QString own = page.address + QLatin1Char('#') + row.id;
            judge(own, QStringLiteral("replaced_by"), row.replacedBy);
            judge(own, QStringLiteral("source.opens"), row.opens);
        }
    }
    return out;
}

const Page *RegistryData::page(const QString &address) const
{
    const auto it = m_index.constFind(address);
    return it == m_index.constEnd() ? nullptr : &m_pages.at(it.value());
}

QList<Hit> RegistryData::search(const QString &text, int limit) const
{
    QList<Hit> out;
    const QStringList query = wordsOf(text);
    if (query.isEmpty() || limit <= 0)
        return out;

    for (const Page &page : m_pages) {
        if (!page.isAvailable())
            continue;

        QList<Field> pageFields = { { page.title, 1.0 } };
        for (const QString &keyword : page.keywords)
            pageFields.append(Field { keyword, 0.8 });
        for (qsizetype i = 0; i + 1 < page.path.size(); ++i)
            pageFields.append(Field { page.path.at(i), 0.4 });
        const double pageScore = scoreOf(query, pageFields);
        if (pageScore > 0)
            out.append(Hit { page.address, page.title, pageScore, page.path, QString(), QStringLiteral("page"), page.program });

        for (const Row &row : page.rows) {
            if (row.isRemoved())
                continue;
            QList<Field> fields = { { row.title, 1.0 } };
            for (const QString &keyword : row.keywords)
                fields.append(Field { keyword, 0.8 });
            const QString section = page.sectionTitle(row.section);
            if (!section.isEmpty())
                fields.append(Field { section, 0.4 });
            for (const QString &step : page.path)
                fields.append(Field { step, 0.3 });
            const double score = scoreOf(query, fields);
            if (score > 0) {
                out.append(Hit { page.address + QLatin1Char('#') + row.id, row.title, score, page.path, section,
                                 row.kind, page.program });
            }
        }
    }

    // Best first; among equals, TUR's own before KDE's, then by address, so
    // that the order does not depend on the order of the files.
    std::stable_sort(out.begin(), out.end(), [](const Hit &a, const Hit &b) {
        if (a.score != b.score)
            return a.score > b.score;
        const bool aKde = a.address.startsWith(QLatin1String("kde."));
        const bool bKde = b.address.startsWith(QLatin1String("kde."));
        if (aKde != bKde)
            return bKde;
        return a.address < b.address;
    });
    if (out.size() > limit)
        out.resize(limit);
    return out;
}

} // namespace Settings
