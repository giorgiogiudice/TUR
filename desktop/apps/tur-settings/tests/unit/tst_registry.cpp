// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
//
// Addresses and the settings registry: the grammar of an address; the
// registries the program carries, read by the program's own loader; every
// rule of the format, on a registry that breaks it; and the search.

#include "address.h"
#include "registry.h"
#include "registrydata.h"

#include <QFile>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QTest>

#include <functional>

using Settings::Address;
using Settings::Hit;
using Settings::Page;
using Settings::RegistryData;
using Settings::Row;

namespace {

const QString source = QStringLiteral(TUR_SETTINGS_SOURCE);
const QString testData = QStringLiteral(TUR_SETTINGS_TEST_DATA);

QByteArray fileContent(const QString &path)
{
    QFile file(path);
    if (!file.open(QIODevice::ReadOnly))
        qFatal("%s cannot be read", qPrintable(path));
    return file.readAll();
}

// A registry that is right, for the tests to break one thing in.
QJsonObject good()
{
    const char *text = R"({
      "licence": "SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0",
      "schema_version": 1,
      "program": "org.tur.Example",
      "pages": [
        {
          "page": "tur.example",
          "title": "Example",
          "path": ["Example"],
          "keywords": ["sample"],
          "sections": [{ "id": "main", "title": "Main" }],
          "rows": [
            { "id": "one", "title": "One", "keywords": ["first"], "kind": "switch", "section": "main",
              "source": { "type": "dbus", "available": true, "bus": "session", "service": "org.tur.Example" },
              "change": { "class": 1, "tool": "example_set_one" } },
            { "id": "two", "title": "Two", "kind": "information", "section": "main",
              "source": { "type": "file", "available": false, "waits_for": "The file", "path": "/etc/two" } },
            { "id": "old", "title": "Old", "status": "removed", "replaced_by": "tur.example#one" }
          ]
        },
        { "page": "tur.example.later", "title": "Later", "path": ["Example", "Later"], "status": "planned" },
        { "page": "kde.kcm_example", "title": "Example module", "path": ["System Settings", "Example module"],
          "open": { "module": "kcm_example" }, "verified_with": "plasma-6.8.2" }
      ]
    })";
    const QJsonDocument document = QJsonDocument::fromJson(text);
    if (!document.isObject())
        qFatal("the test's own registry is not JSON");
    return document.object();
}

using Edit = std::function<void(QJsonObject &)>;

// Changes one page of a registry.
Edit inPage(int index, const std::function<void(QJsonObject &)> &edit)
{
    return [index, edit](QJsonObject &root) {
        QJsonArray pages = root.value(QStringLiteral("pages")).toArray();
        QJsonObject page = pages.at(index).toObject();
        edit(page);
        pages.replace(index, page);
        root.insert(QStringLiteral("pages"), pages);
    };
}

// Changes one row of the first page.
Edit inRow(int index, const std::function<void(QJsonObject &)> &edit)
{
    return inPage(0, [index, edit](QJsonObject &page) {
        QJsonArray rows = page.value(QStringLiteral("rows")).toArray();
        QJsonObject row = rows.at(index).toObject();
        edit(row);
        rows.replace(index, row);
        page.insert(QStringLiteral("rows"), rows);
    });
}

Edit set(const QString &key, const QJsonValue &value)
{
    return [key, value](QJsonObject &object) { object.insert(key, value); };
}

Edit drop(const QString &key)
{
    return [key](QJsonObject &object) { object.remove(key); };
}

QJsonObject object(const char *json)
{
    return QJsonDocument::fromJson(json).object();
}

} // namespace

Q_DECLARE_METATYPE(Edit)

class TestRegistry : public QObject
{
    Q_OBJECT

private Q_SLOTS:
    // --- Addresses -----------------------------------------------------------

    void an_address_is_read_data()
    {
        QTest::addColumn<QString>("text");
        QTest::addColumn<QString>("page");
        QTest::addColumn<QString>("row");

        QTest::newRow("a page") << "tur.overview" << "tur.overview" << "";
        QTest::newRow("a page in a group") << "tur.pax.privacy" << "tur.pax.privacy" << "";
        QTest::newRow("a row") << "tur.pax.privacy#private-mode" << "tur.pax.privacy" << "private-mode";
        QTest::newRow("a page of KDE's") << "kde.kcm_nightlight" << "kde.kcm_nightlight" << "";
        QTest::newRow("a row of KDE's") << "kde.kcm_nightlight#switching-times" << "kde.kcm_nightlight"
                                        << "switching-times";
        QTest::newRow("digits and a hyphen") << "tur.wi-fi_2#row-2" << "tur.wi-fi_2" << "row-2";
        QTest::newRow("a link") << "tur-settings:tur.about#version" << "tur.about" << "version";
        QTest::newRow("a link to a page") << "tur-settings:tur.about" << "tur.about" << "";
        QTest::newRow("a link with the sign encoded") << "tur-settings:tur.about%23version" << "tur.about"
                                                      << "version";
    }

    void an_address_is_read()
    {
        QFETCH(QString, text);
        QFETCH(QString, page);
        QFETCH(QString, row);
        const Address address = Address::parse(text);
        QVERIFY(address.isValid());
        QCOMPARE(address.page, page);
        QCOMPARE(address.row, row);
        QCOMPARE(address.hasRow(), !row.isEmpty());
        QCOMPARE(address.toString(), row.isEmpty() ? page : page + QLatin1Char('#') + row);
        QCOMPARE(address.nameSpace(), page.left(3));
        // Written out, it reads back as itself.
        QCOMPARE(Address::parse(address.toString()).toString(), address.toString());
    }

    void what_is_not_an_address_is_not_read_data()
    {
        QTest::addColumn<QString>("text");

        QTest::newRow("empty") << "";
        QTest::newRow("a namespace alone") << "tur";
        QTest::newRow("a namespace and a dot") << "tur.";
        QTest::newRow("another namespace") << "gnome.display";
        QTest::newRow("no namespace") << "overview";
        QTest::newRow("upper case") << "tur.Overview";
        QTest::newRow("upper case in the row") << "tur.overview#Version";
        QTest::newRow("an empty segment") << "tur..overview";
        QTest::newRow("a trailing dot") << "tur.overview.";
        QTest::newRow("an empty row") << "tur.overview#";
        QTest::newRow("two rows") << "tur.overview#a#b";
        QTest::newRow("a dot in the row") << "tur.overview#a.b";
        QTest::newRow("an underscore in the row") << "tur.overview#a_b";
        QTest::newRow("a space") << "tur.over view";
        QTest::newRow("spaces round it") << " tur.overview ";
        QTest::newRow("a line break after it") << "tur.overview\n";
        QTest::newRow("a slash") << "tur.overview/../x";
        QTest::newRow("a letter outside ASCII") << QStringLiteral("tur.übersicht");
        QTest::newRow("a link of another program") << "tur-centre:home";
        QTest::newRow("a link with slashes") << "tur-settings://tur.overview";
        QTest::newRow("a link to nothing") << "tur-settings:";
        QTest::newRow("the sign encoded outside a link") << "tur.about%23version";
        QTest::newRow("a page too long") << QStringLiteral("tur.") + QString(Address::MaximumPageLength, QLatin1Char('a'));
        QTest::newRow("a row too long") << QStringLiteral("tur.a#") + QString(Address::MaximumRowLength + 1, QLatin1Char('a'));
    }

    void what_is_not_an_address_is_not_read()
    {
        QFETCH(QString, text);
        const Address address = Address::parse(text);
        QVERIFY2(!address.isValid(), qPrintable(text));
        QVERIFY(address.page.isEmpty());
        QVERIFY(address.row.isEmpty());
        QVERIFY(address.toString().isEmpty());
    }

    // --- The registries the program carries ---------------------------------------

    void the_program_s_registries_load()
    {
        RegistryData data;
        QStringList problems;
        QVERIFY2(data.add(fileContent(source + QStringLiteral("/registry/org.tur.Settings.json")),
                          QStringLiteral("org.tur.Settings.json"), &problems),
                 qPrintable(problems.join(QLatin1Char('\n'))));
        QVERIFY2(data.add(fileContent(source + QStringLiteral("/registry/org.kde.systemsettings.json")),
                          QStringLiteral("org.kde.systemsettings.json"), &problems),
                 qPrintable(problems.join(QLatin1Char('\n'))));
        QVERIFY(problems.isEmpty());
        QCOMPARE(data.crossProblems(), QStringList());
        QCOMPARE(data.programs(), QStringList({ QStringLiteral("org.tur.Settings"), QStringLiteral("org.kde.systemsettings") }));

        // The pages of the design's table (10.2), each with its address.
        const QStringList expected = {
            QStringLiteral("tur.overview"),      QStringLiteral("tur.updates"),       QStringLiteral("tur.recovery"),
            QStringLiteral("tur.apps"),          QStringLiteral("tur.windows"),       QStringLiteral("tur.android"),
            QStringLiteral("tur.macos"),         QStringLiteral("tur.storage"),       QStringLiteral("tur.devices"),
            QStringLiteral("tur.pax.general"),   QStringLiteral("tur.pax.voice"),     QStringLiteral("tur.pax.privacy"),
            QStringLiteral("tur.pax.approvals"), QStringLiteral("tur.pax.memory"),    QStringLiteral("tur.pax.schedules"),
            QStringLiteral("tur.pax.spending"),  QStringLiteral("tur.pax.activity"),  QStringLiteral("tur.diagnostics"),
            QStringLiteral("tur.appearance"),    QStringLiteral("tur.about"),
        };
        QStringList own;
        for (const Page &page : data.pages()) {
            if (page.program == QLatin1String("org.tur.Settings"))
                own.append(page.address);
        }
        QCOMPARE(own, expected);

        // Three are built; the others are reserved and have no rows.
        for (const Page &page : data.pages()) {
            if (page.program != QLatin1String("org.tur.Settings"))
                continue;
            const bool built = page.address == QLatin1String("tur.overview")
                               || page.address == QLatin1String("tur.appearance")
                               || page.address == QLatin1String("tur.about");
            QCOMPARE(page.status, built ? QStringLiteral("available") : QStringLiteral("planned"));
            QCOMPARE(page.rows.isEmpty(), !built);
            QCOMPARE(page.listed, page.address != QLatin1String("tur.macos"));
        }

        const Page *privacy = data.page(QStringLiteral("tur.pax.privacy"));
        QVERIFY(privacy);
        QCOMPARE(privacy->path, QStringList({ QStringLiteral("P@x"), QStringLiteral("Privacy") }));

        // A row that leads to a page of KDE's names a page that is there.
        const Row *colours = data.page(QStringLiteral("tur.appearance"))->row(QStringLiteral("colours"));
        QVERIFY(colours);
        QCOMPARE(colours->kind, QStringLiteral("action"));
        QCOMPARE(colours->opens, QStringLiteral("kde.kcm_colors"));
        QCOMPARE(colours->changeClass, 1);
        const Page *module = data.page(colours->opens);
        QVERIFY(module);
        QCOMPARE(module->module, QStringLiteral("kcm_colors"));
        QCOMPARE(module->program, QStringLiteral("org.kde.systemsettings"));

        // What has no backend yet says so, and says what it waits for.
        const Row *variant = data.page(QStringLiteral("tur.appearance"))->row(QStringLiteral("variant"));
        QVERIFY(variant);
        QCOMPARE(variant->sourceAvailable, false);
        QVERIFY(!variant->waitsFor.isEmpty());
        QCOMPARE(variant->sourceType, QStringLiteral("kde-config"));
    }

    void another_program_s_registry_is_merged()
    {
        Registry registry;
        QCOMPARE(registry.problems(), QStringList());
        const int before = registry.revision();
        QVERIFY(registry.entry(QStringLiteral("tur.example#thing")).isEmpty());

        registry.setDirectory(testData + QStringLiteral("/registry"));
        QCOMPARE(registry.problems(), QStringList());
        QVERIFY(registry.revision() > before);
        const QVariantMap thing = registry.entry(QStringLiteral("tur.example#thing"));
        QCOMPARE(thing.value(QStringLiteral("program")).toString(), QStringLiteral("org.tur.Example"));
        QCOMPARE(thing.value(QStringLiteral("title")).toString(), QStringLiteral("A thing of another program"));
        QCOMPARE(thing.value(QStringLiteral("tool")).toString(), QStringLiteral("example_set_thing"));
        QCOMPARE(registry.entry(QStringLiteral("tur.example#old-thing")).value(QStringLiteral("status")).toString(),
                 QStringLiteral("removed"));
        QCOMPARE(registry.entry(QStringLiteral("tur.example.gone")).value(QStringLiteral("status")).toString(),
                 QStringLiteral("removed"));

        // The list at the side is this program's own pages and nobody else's.
        const QVariantList navigation = registry.navigation();
        QCOMPARE(navigation.size(), 20); // nineteen pages and the heading of P@x's group
        for (const QVariant &item : navigation)
            QVERIFY(!item.toMap().value(QStringLiteral("address")).toString().startsWith(QLatin1String("tur.example")));

        // An address is looked up as it is written, not as a link.
        QVERIFY(registry.entry(QStringLiteral("tur-settings:tur.about")).isEmpty());
        QVERIFY(!registry.entry(QStringLiteral("tur.about")).isEmpty());
        QCOMPARE(registry.sectionTitle(QStringLiteral("tur.about"), QStringLiteral("technical")),
                 QStringLiteral("Technical detail"));
        QCOMPARE(registry.rowsOf(QStringLiteral("tur.overview")).size(), 5);
        QVERIFY(registry.rowsOf(QStringLiteral("tur.updates")).isEmpty());

        // A directory that is not there adds nothing and is no failure.
        registry.setDirectory(QStringLiteral("/nonexistent/tur/settings"));
        QCOMPARE(registry.problems(), QStringList());
        QVERIFY(registry.entry(QStringLiteral("tur.example#thing")).isEmpty());
    }

    // --- Every rule of the format ---------------------------------------------------

    void a_registry_that_is_right_loads()
    {
        RegistryData data;
        QStringList problems;
        QVERIFY2(data.add(QJsonDocument(good()).toJson(), QStringLiteral("good.json"), &problems),
                 qPrintable(problems.join(QLatin1Char('\n'))));
        QCOMPARE(data.crossProblems(), QStringList());
        QCOMPARE(data.pages().size(), 3);
        const Page *page = data.page(QStringLiteral("tur.example"));
        QVERIFY(page);
        QCOMPARE(page->rows.size(), 3);
        QVERIFY(page->row(QStringLiteral("old"))->isRemoved());
        QCOMPARE(page->row(QStringLiteral("old"))->replacedBy, QStringLiteral("tur.example#one"));
        QCOMPARE(page->row(QStringLiteral("one"))->tool, QStringLiteral("example_set_one"));
        QCOMPARE(page->row(QStringLiteral("two"))->changeClass, -1);
    }

    void a_registry_that_breaks_a_rule_is_not_loaded_data()
    {
        QTest::addColumn<Edit>("edit");
        QTest::addColumn<QString>("says");

        const auto row = [](const char *name, const Edit &edit, const char *says) {
            QTest::newRow(name) << edit << QString::fromLatin1(says);
        };

        row("an unknown key", set(QStringLiteral("colour"), QStringLiteral("blue")), "is not a key of the format");
        row("another version", set(QStringLiteral("schema_version"), 2), "schema_version");
        row("no version", drop(QStringLiteral("schema_version")), "\"schema_version\" is missing");
        row("no program", drop(QStringLiteral("program")), "\"program\" is missing");
        row("a program that is not an identifier", set(QStringLiteral("program"), QStringLiteral("settings")),
            "application identifier");
        row("no pages", drop(QStringLiteral("pages")), "\"pages\" is missing");
        row("an empty list of pages", set(QStringLiteral("pages"), QJsonArray()), "is empty");
        row("a page that is not an object", set(QStringLiteral("pages"), QJsonArray { QStringLiteral("tur.x") }),
            "is not an object");

        row("a page address in upper case", inPage(0, set(QStringLiteral("page"), QStringLiteral("tur.Example"))),
            "is not the address of a page");
        row("a page outside the two namespaces", inPage(0, set(QStringLiteral("page"), QStringLiteral("gnome.example"))),
            "is not the address of a page");
        row("a page address with a row", inPage(0, set(QStringLiteral("page"), QStringLiteral("tur.example#one"))),
            "is not the address of a page");
        row("a page with no title", inPage(0, drop(QStringLiteral("title"))), "\"title\" is missing");
        row("a blank title", inPage(0, set(QStringLiteral("title"), QStringLiteral("   "))), "is not text");
        row("a title with a line break", inPage(0, set(QStringLiteral("title"), QStringLiteral("Exa\nmple"))),
            "is not text");
        row("a path that does not end in the title",
            inPage(0, set(QStringLiteral("path"), QJsonArray { QStringLiteral("Something else") })),
            "does not end in the page's title");
        row("no path", inPage(0, drop(QStringLiteral("path"))), "\"path\" is missing");
        row("a keyword twice",
            inPage(0, set(QStringLiteral("keywords"), QJsonArray { QStringLiteral("a"), QStringLiteral("a") })),
            "twice");
        row("an unknown status", inPage(0, set(QStringLiteral("status"), QStringLiteral("hidden"))),
            "is not available, planned or removed");
        row("replaced_by on a page that is there",
            inPage(0, set(QStringLiteral("replaced_by"), QStringLiteral("tur.example.later"))),
            "only a removed page");
        row("listed that is not a boolean", inPage(0, set(QStringLiteral("listed"), QStringLiteral("no"))),
            "is not true or false");
        row("a page twice", [](QJsonObject &root) {
            QJsonArray pages = root.value(QStringLiteral("pages")).toArray();
            pages.append(pages.at(1));
            root.insert(QStringLiteral("pages"), pages);
        }, "is in this file twice");
        row("a section twice", inPage(0, [](QJsonObject &page) {
            QJsonArray sections = page.value(QStringLiteral("sections")).toArray();
            sections.append(sections.at(0));
            page.insert(QStringLiteral("sections"), sections);
        }), "is declared twice");
        row("a KDE page that does not say how it is opened", inPage(2, drop(QStringLiteral("open"))),
            "says how it is opened");
        row("a TUR page with open", inPage(0, set(QStringLiteral("open"), object(R"({"module": "kcm_x"})"))),
            "is opened by its address");
        row("a module that is not an identifier",
            inPage(2, set(QStringLiteral("open"), object(R"({"module": "kcm example; rm -rf"})"))),
            "is not the identifier of a module");
        row("an argument with a space",
            inPage(2, set(QStringLiteral("open"), object(R"({"module": "kcm_example", "arguments": ["a b"]})"))),
            "arguments without spaces");
        row("a planned page with a row", inPage(1, set(QStringLiteral("rows"), QJsonArray { object(
                                                        R"({"id": "x", "title": "X", "kind": "information"})") })),
            "so the row cannot be there");

        row("a row with no identifier", inRow(0, drop(QStringLiteral("id"))), "\"id\" is missing");
        row("a row identifier with a dot", inRow(0, set(QStringLiteral("id"), QStringLiteral("one.two"))),
            "is not a row identifier");
        row("a row identifier used twice", inRow(1, set(QStringLiteral("id"), QStringLiteral("one"))),
            "is used twice on the page");
        row("a row with no kind", inRow(0, drop(QStringLiteral("kind"))), "\"kind\" is missing");
        row("an unknown kind", inRow(0, set(QStringLiteral("kind"), QStringLiteral("slider"))), "is not a kind of row");
        row("a row in a section its page does not have", inRow(0, set(QStringLiteral("section"), QStringLiteral("other"))),
            "names no section of its page");
        row("a row with no section on a page that has sections", inRow(0, drop(QStringLiteral("section"))),
            "names no section of its page");
        row("a switch that does not say what changing it is", inRow(0, drop(QStringLiteral("change"))),
            "says what changing it is");
        row("information with a change", inRow(1, set(QStringLiteral("change"), object(R"({"class": 0})"))),
            "information is only read");
        row("class 5", inRow(0, set(QStringLiteral("change"), object(R"({"class": 5})"))), "is not an action class");
        row("a class that is not a whole number", inRow(0, set(QStringLiteral("change"), object(R"({"class": 1.5})"))),
            "is not an action class");
        row("a class as text", inRow(0, set(QStringLiteral("change"), object(R"({"class": "1"})"))),
            "is not an action class");
        row("a tool that is not a name", inRow(0, set(QStringLiteral("change"), object(R"({"class": 1, "tool": "Set One"})"))),
            "is not the name of a tool");
        row("a source of an unknown type", inRow(0, set(QStringLiteral("source"), object(R"({"type": "magic", "available": true})"))),
            "is not a kind of source");
        row("a source that does not say whether it is there",
            inRow(0, set(QStringLiteral("source"), object(R"({"type": "dbus", "bus": "session", "service": "org.tur.Example"})"))),
            "\"available\" is missing");
        row("a missing source that does not say what it waits for",
            inRow(1, set(QStringLiteral("source"), object(R"({"type": "file", "available": false, "path": "/etc/two"})"))),
            "says what it waits for");
        row("a source that is there and waits",
            inRow(0, set(QStringLiteral("source"), object(R"({"type": "dbus", "available": true, "waits_for": "x", "bus": "session", "service": "org.tur.Example"})"))),
            "waits for nothing");
        row("a file that is not an absolute path",
            inRow(1, set(QStringLiteral("source"), object(R"({"type": "file", "available": true, "path": "etc/two"})"))),
            "is not an absolute path");
        row("a command given as one string",
            inRow(1, set(QStringLiteral("source"), object(R"({"type": "command", "available": true, "program": "/bin/ls -l"})"))),
            "its arguments are a list");
        row("a bus that is neither",
            inRow(0, set(QStringLiteral("source"), object(R"({"type": "dbus", "available": true, "bus": "private", "service": "org.tur.Example"})"))),
            "is neither session nor system");
        row("a source with a key of another kind",
            inRow(0, set(QStringLiteral("source"), object(R"({"type": "dbus", "available": true, "bus": "session", "service": "org.tur.Example", "path": "/x"})"))),
            "is not a key of the format");
        row("a row that opens what is not an address",
            inRow(0, set(QStringLiteral("source"), object(R"({"type": "address", "available": true, "opens": "Colours"})"))),
            "is not an address");
        row("a removed row that still has a kind", inRow(2, set(QStringLiteral("kind"), QStringLiteral("switch"))),
            "a removed row has no \"kind\"");
        row("replaced_by on a row that is there", inRow(0, set(QStringLiteral("replaced_by"), QStringLiteral("tur.example#two"))),
            "only a removed row");
        row("an empty locator", inRow(0, set(QStringLiteral("locator"), QJsonObject())), "how the control is found");
    }

    void a_registry_that_breaks_a_rule_is_not_loaded()
    {
        QFETCH(Edit, edit);
        QFETCH(QString, says);
        QJsonObject registry = good();
        edit(registry);

        RegistryData data;
        QStringList problems;
        QVERIFY2(!data.add(QJsonDocument(registry).toJson(), QStringLiteral("broken.json"), &problems),
                 "the registry was loaded");
        const QString all = problems.join(QLatin1Char('\n'));
        QVERIFY2(all.contains(says), qPrintable(QStringLiteral("it should say \"%1\"; it says:\n%2").arg(says, all)));
        // Every problem names the file.
        for (const QString &problem : std::as_const(problems))
            QVERIFY2(problem.startsWith(QLatin1String("broken.json: ")), qPrintable(problem));
        // Nothing of a file that breaks a rule is kept: not even its pages
        // that are right.
        QVERIFY(data.pages().isEmpty());
        QVERIFY(data.programs().isEmpty());
        QVERIFY(!data.page(QStringLiteral("tur.example.later")));
    }

    void what_is_not_a_registry_at_all_is_not_loaded_data()
    {
        QTest::addColumn<QByteArray>("content");
        QTest::addColumn<QString>("says");

        QTest::newRow("empty") << QByteArray() << "is not JSON";
        QTest::newRow("not JSON") << QByteArray("pages: tur.overview") << "is not JSON";
        QTest::newRow("a list") << QByteArray("[1, 2]") << "is not a JSON object";
        QTest::newRow("cut short") << QJsonDocument(good()).toJson().left(200) << "is not JSON";
        QTest::newRow("too large") << QByteArray(RegistryData::MaximumFileSize + 1, ' ') << "is larger than";
    }

    void what_is_not_a_registry_at_all_is_not_loaded()
    {
        QFETCH(QByteArray, content);
        QFETCH(QString, says);
        RegistryData data;
        QStringList problems;
        QVERIFY(!data.add(content, QStringLiteral("x.json"), &problems));
        QCOMPARE(problems.size(), 1);
        QVERIFY2(problems.constFirst().contains(says), qPrintable(problems.constFirst()));
        QVERIFY(data.pages().isEmpty());
    }

    void a_page_cannot_be_registered_twice()
    {
        RegistryData data;
        QStringList problems;
        QVERIFY(data.add(QJsonDocument(good()).toJson(), QStringLiteral("first.json"), &problems));

        // The same program again.
        QVERIFY(!data.add(QJsonDocument(good()).toJson(), QStringLiteral("again.json"), &problems));
        QVERIFY2(problems.join(QLatin1Char('\n')).contains(QLatin1String("is already loaded")),
                 qPrintable(problems.join(QLatin1Char('\n'))));

        // Another program that claims one of its pages.
        problems.clear();
        QJsonObject other = good();
        other.insert(QStringLiteral("program"), QStringLiteral("org.tur.Other"));
        QVERIFY(!data.add(QJsonDocument(other).toJson(), QStringLiteral("other.json"), &problems));
        QVERIFY2(problems.join(QLatin1Char('\n')).contains(QLatin1String("is already registered by org.tur.Example")),
                 qPrintable(problems.join(QLatin1Char('\n'))));
        QCOMPARE(data.pages().size(), 3);
        QCOMPARE(data.page(QStringLiteral("tur.example"))->program, QStringLiteral("org.tur.Example"));
    }

    void what_an_address_points_at_must_be_there_data()
    {
        QTest::addColumn<Edit>("edit");
        QTest::addColumn<QString>("says");

        QTest::newRow("replaced by nothing")
            << inRow(2, set(QStringLiteral("replaced_by"), QStringLiteral("tur.example#nothing")))
            << "tur.example#old: replaced_by names tur.example#nothing, which no registry has";
        QTest::newRow("replaced by itself")
            << inRow(2, set(QStringLiteral("replaced_by"), QStringLiteral("tur.example#old")))
            << "tur.example#old: replaced_by names the address itself";
        QTest::newRow("opens a page nobody has")
            << inRow(0, set(QStringLiteral("source"), object(R"({"type": "address", "available": true, "opens": "kde.kcm_missing"})")))
            << "tur.example#one: source.opens names kde.kcm_missing, which no registry has";
        QTest::newRow("opens what was removed")
            << inRow(0, set(QStringLiteral("source"), object(R"({"type": "address", "available": true, "opens": "tur.example#old"})")))
            << "tur.example#one: source.opens names tur.example#old, which is itself removed";
    }

    void what_an_address_points_at_must_be_there()
    {
        QFETCH(Edit, edit);
        QFETCH(QString, says);
        QJsonObject registry = good();
        edit(registry);
        RegistryData data;
        QStringList problems;
        QVERIFY2(data.add(QJsonDocument(registry).toJson(), QStringLiteral("x.json"), &problems),
                 qPrintable(problems.join(QLatin1Char('\n'))));
        QCOMPARE(data.crossProblems(), QStringList({ says }));
    }

    // --- Searching -----------------------------------------------------------------

    void search_finds_by_title_keyword_and_place_data()
    {
        QTest::addColumn<QString>("words");
        QTest::addColumn<QString>("first");

        // The title, whole.
        QTest::newRow("a title") << "Night Light" << "tur.appearance#night-light";
        QTest::newRow("a title in another case") << "FONTS" << "tur.appearance#fonts";
        // A keyword: what a person calls it.
        QTest::newRow("a keyword") << "dark mode" << "tur.appearance#variant";
        QTest::newRow("a keyword, American") << "color scheme" << "tur.appearance#colours";
        QTest::newRow("a keyword of a page") << "style" << "tur.appearance";
        // A word of a title counts for more than a keyword.
        QTest::newRow("a title before a keyword") << "theme" << "tur.appearance#global-theme";
        QTest::newRow("the beginning of a word") << "licen" << "tur.about#licence";
        QTest::newRow("a word of the title") << "revision" << "tur.about#source-revision";
        // The name of a page finds the page first, then its rows.
        QTest::newRow("a page") << "appearance" << "tur.appearance";
        QTest::newRow("a page and a row") << "about version" << "tur.about#version";
        QTest::newRow("a section") << "technical" << "tur.about#base-image";
        QTest::newRow("words in another order") << "light night" << "tur.appearance#night-light";
        QTest::newRow("punctuation and spaces") << "  fedora?  " << "tur.about#origin";
    }

    void search_finds_by_title_keyword_and_place()
    {
        QFETCH(QString, words);
        QFETCH(QString, first);
        Registry registry;
        const QList<Hit> hits = registry.data().search(words, 20);
        QVERIFY2(!hits.isEmpty(), "nothing was found");
        QStringList found;
        for (const Hit &hit : hits)
            found.append(QStringLiteral("%1 (%2)").arg(hit.address).arg(hit.score));
        QVERIFY2(hits.constFirst().address == first, qPrintable(found.join(QStringLiteral(", "))));

        // Best first, each score between nought and one, no address twice.
        QStringList seen;
        for (qsizetype i = 0; i < hits.size(); ++i) {
            QVERIFY(hits.at(i).score > 0 && hits.at(i).score <= 1.0);
            if (i > 0)
                QVERIFY(hits.at(i).score <= hits.at(i - 1).score);
            QVERIFY(!seen.contains(hits.at(i).address));
            seen.append(hits.at(i).address);
            QVERIFY(!hits.at(i).title.isEmpty());
            QVERIFY(!hits.at(i).path.isEmpty());
        }
    }

    void search_leaves_out_what_cannot_be_gone_to()
    {
        Registry registry;
        registry.setDirectory(testData + QStringLiteral("/registry"));
        // A page that is reserved is not found, though its title matches.
        for (const Hit &hit : registry.data().search(QStringLiteral("privacy"), 20))
            QVERIFY2(hit.address != QLatin1String("tur.pax.privacy"), "a page that is not built was found");
        QVERIFY(registry.data().search(QStringLiteral("recovery"), 20).isEmpty());
        // What was removed is not found.
        QVERIFY(registry.data().search(QStringLiteral("taken away"), 20).isEmpty());
        // Every word must match.
        QVERIFY(registry.data().search(QStringLiteral("fonts printer"), 20).isEmpty());
        QVERIFY(registry.data().search(QStringLiteral("zzz"), 20).isEmpty());
        // Nothing is asked: nothing is found.
        QVERIFY(registry.data().search(QString(), 20).isEmpty());
        QVERIFY(registry.data().search(QStringLiteral("  ?!  "), 20).isEmpty());
        QVERIFY(registry.data().search(QStringLiteral("version"), 0).isEmpty());
    }

    void search_is_limited_and_puts_tur_s_own_first()
    {
        Registry registry;
        QCOMPARE(registry.data().search(QStringLiteral("version"), 2).size(), 2);
        QVERIFY(registry.data().search(QStringLiteral("version"), 20).size() >= 3);

        // "Fonts" is the title of TUR's row and of KDE's page: equal scores,
        // TUR's first.
        const QList<Hit> fonts = registry.data().search(QStringLiteral("fonts"), 20);
        QVERIFY(fonts.size() >= 2);
        QCOMPARE(fonts.at(0).address, QStringLiteral("tur.appearance#fonts"));
        QCOMPARE(fonts.at(1).address, QStringLiteral("kde.kcm_fonts"));
        QCOMPARE(fonts.at(0).score, fonts.at(1).score);
        QCOMPARE(fonts.at(1).kind, QStringLiteral("page"));
        QCOMPARE(fonts.at(1).program, QStringLiteral("org.kde.systemsettings"));
        QCOMPARE(fonts.at(0).section, QStringLiteral("Everything else"));
        QCOMPARE(fonts.at(0).path, QStringList({ QStringLiteral("Appearance") }));
    }
};

QTEST_GUILESS_MAIN(TestRegistry)

#include "tst_registry.moc"
