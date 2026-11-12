// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#include "settingshost.h"

#include "busadaptors.h"
#include "bustypes.h"

#include <QCoreApplication>
#include <QDBusConnectionInterface>
#include <QDBusError>
#include <QDBusPendingCall>
#include <QDBusPendingCallWatcher>
#include <QGuiApplication>
#include <QJsonDocument>
#include <QJsonObject>
#include <QProcess>

using Settings::Address;
using Settings::Code;
using Settings::Page;
using Settings::Row;

namespace {

// How long another program is given to show a row. Longer than D-Bus's own
// default would be pointless; shorter than starting a program is too short.
constexpr int ForwardTimeoutMs = 20000;

void polish(QQuickItem *item)
{
    item->ensurePolished();
    const QList<QQuickItem *> children = item->childItems();
    for (QQuickItem *child : children)
        polish(child);
}

bool isRow(const QQuickItem *item)
{
    return item->property("isSettingRow").toBool();
}

void collectRows(QQuickItem *item, QVariantList &out)
{
    if (isRow(item))
        out.append(QVariant::fromValue(item));
    const QList<QQuickItem *> children = item->childItems();
    for (QQuickItem *child : children)
        collectRows(child, out);
}

} // namespace

SettingsHost::SettingsHost(QObject *parent)
    : QObject(parent)
    , m_bus(QStringLiteral("tur-settings-not-connected"))
{
}

SettingsHost::~SettingsHost()
{
    stopBus();
}

void SettingsHost::setRegistry(Registry *registry)
{
    if (registry == m_registry)
        return;
    m_registry = registry;
    Q_EMIT registryChanged();
}

void SettingsHost::setSystemSettingsProgram(const QString &program)
{
    if (program == m_systemSettings)
        return;
    m_systemSettings = program;
    Q_EMIT systemSettingsProgramChanged();
}

void SettingsHost::setCurrentAddress(const QString &address)
{
    if (address == m_currentAddress)
        return;
    m_currentAddress = address;
    Q_EMIT currentAddressChanged();
}

QVariantMap SettingsHost::optionsFrom(const QVariantMap &given, bool highlightByDefault)
{
    QVariantMap out;
    out.insert(QStringLiteral("highlight"), given.value(QStringLiteral("highlight"), highlightByDefault).toBool());
    bool number = false;
    const double duration = given.value(QStringLiteral("duration"), 0.0).toDouble(&number);
    // A day at most: a highlight that nobody clears does not stay for ever
    // by accident of a large number.
    out.insert(QStringLiteral("duration"), number && duration > 0 ? qMin(duration, 86400.0) : 0.0);
    out.insert(QStringLiteral("dim"), given.value(QStringLiteral("dim"), false).toBool());
    out.insert(QStringLiteral("pax"), given.value(QStringLiteral("pax"), false).toBool());
    return out;
}

SettingsHost::Outcome SettingsHost::failed(Code code, const QString &reason, const QString &detail) const
{
    Outcome out;
    out.ok = false;
    out.code = code;
    out.reason = reason;
    out.detail = detail;
    return out;
}

QVariantMap SettingsHost::toView(const Outcome &outcome) const
{
    if (!outcome.ok)
        return Settings::failure(outcome.code, outcome.reason, outcome.detail);
    QVariantMap out = outcome.result;
    out.insert(QStringLiteral("ok"), true);
    return out;
}

SettingsHost::Outcome SettingsHost::startSystemSettings(const QString &module, const QStringList &arguments)
{
    QStringList command;
    if (!module.isEmpty()) {
        command.append(module);
        if (!arguments.isEmpty())
            command << QStringLiteral("--args") << arguments.join(QLatin1Char(' '));
    }
    // The program and its arguments are a vector; nothing goes through a
    // shell. It is started on its own: it outlives this program.
    if (m_systemSettings.isEmpty() || !QProcess::startDetached(m_systemSettings, command)) {
        return failed(Code::OwnerUnreachable, tr("KDE System Settings could not be started."),
                      QStringLiteral("%1 %2").arg(m_systemSettings, command.join(QLatin1Char(' '))).trimmed());
    }
    Outcome out;
    out.ok = true;
    return out;
}

SettingsHost::Outcome SettingsHost::perform(const QString &text, const QVariantMap &given, Verb verb)
{
    if (text.isEmpty() && verb == Verb::Open) {
        // Nothing to go to: the window is only asked to come forward.
        Q_EMIT presentRequested();
        Outcome out;
        out.ok = true;
        return out;
    }

    const Address address = Address::parse(text);
    if (!address.isValid()) {
        return failed(Code::NotAnAddress,
                      tr("An address is a page, such as tur.appearance, or a row on a page, such as "
                         "tur.appearance#variant."),
                      text.left(200));
    }
    if (!m_registry)
        return failed(Code::RegistryUnreadable, tr("This program has no list of settings."));

    const QString shown = address.toString();
    const Page *page = m_registry->data().page(address.page);
    const Row *row = page && address.hasRow() ? page->row(address.row) : nullptr;
    const QString unknown = tr("%1 is not the address of a settings page or of a row on one.").arg(shown);
    if (!page)
        return failed(Code::UnknownAddress, unknown);

    if (page->isRemoved() || (row && row->isRemoved())) {
        const QString successor = row && row->isRemoved() ? row->replacedBy : page->replacedBy;
        return failed(Code::RemovedAddress,
                      successor.isEmpty() ? tr("%1 was removed and nothing replaced it.").arg(shown)
                                          : tr("%1 was removed. It was replaced by %2.").arg(shown, successor),
                      successor);
    }
    // A page that is reserved has no rows yet: whatever row is asked for,
    // the answer is that the page is not there, not that the row is unknown.
    if (!page->isAvailable())
        return failed(Code::PageNotAvailable, tr("%1 is reserved for a page that has not been built.").arg(shown));
    if (address.hasRow() && !row)
        return failed(Code::UnknownAddress, unknown);

    const QVariantMap options = optionsFrom(given, verb == Verb::Reveal);
    Outcome out;
    out.address = address;

    if (page->program != Registry::ownProgram()) {
        if (!page->module.isEmpty()) {
            // A page of KDE System Settings is opened exactly. A row of one
            // cannot be pointed at from outside yet (design, 10.4; task
            // D-20): the caller is told so, not given a made-up rectangle.
            out = startSystemSettings(page->module, page->moduleArguments);
            out.address = address;
            if (out.ok) {
                out.result.insert(QStringLiteral("address"), shown);
                out.result.insert(QStringLiteral("page"), address.page);
                out.result.insert(QStringLiteral("row"), address.row);
                out.result.insert(QStringLiteral("program"), page->program);
                out.result.insert(QStringLiteral("located"), false);
                out.result.insert(QStringLiteral("highlighted"), false);
            }
            return out;
        }
        out.ok = true;
        out.forwardService = page->program;
        return out;
    }

    m_answered = false;
    m_shown.clear();
    Q_EMIT showRequested(address.page, address.row, options);
    if (!m_answered || !m_shown.value(QStringLiteral("found")).toBool()) {
        return failed(Code::RowNotShown,
                      m_answered ? tr("%1 is registered, and the page does not show it.").arg(shown)
                                 : tr("The window is not ready to show %1.").arg(shown));
    }
    Q_EMIT presentRequested();

    Settings::RectD rect;
    rect.x = m_shown.value(QStringLiteral("x")).toDouble();
    rect.y = m_shown.value(QStringLiteral("y")).toDouble();
    rect.width = m_shown.value(QStringLiteral("width")).toDouble();
    rect.height = m_shown.value(QStringLiteral("height")).toDouble();

    out.ok = true;
    out.result.insert(QStringLiteral("address"), shown);
    out.result.insert(QStringLiteral("page"), address.page);
    out.result.insert(QStringLiteral("row"), address.row);
    out.result.insert(QStringLiteral("program"), Registry::ownProgram());
    out.result.insert(QStringLiteral("located"), true);
    out.result.insert(QStringLiteral("rect"), QVariant::fromValue(rect));
    out.result.insert(QStringLiteral("highlighted"), m_shown.value(QStringLiteral("highlighted")).toBool());
    out.result.insert(QStringLiteral("focused"), m_shown.value(QStringLiteral("focused")).toBool());
    out.result.insert(QStringLiteral("pid"), uint(QCoreApplication::applicationPid()));
    const QString desktopFile = QGuiApplication::desktopFileName();
    out.result.insert(QStringLiteral("app_id"), desktopFile.isEmpty() ? Registry::ownProgram() : desktopFile);
    out.result.insert(QStringLiteral("window_title"), m_shown.value(QStringLiteral("windowTitle")).toString());
    out.result.insert(QStringLiteral("scale"), m_shown.value(QStringLiteral("scale"), 1.0).toDouble());
    return out;
}

SettingsHost::Outcome SettingsHost::resolve(const QString &text) const
{
    const Address address = Address::parse(text);
    if (!address.isValid()) {
        return failed(Code::NotAnAddress,
                      tr("An address is a page, such as tur.appearance, or a row on a page, such as "
                         "tur.appearance#variant."),
                      text.left(200));
    }
    const Page *page = m_registry ? m_registry->data().page(address.page) : nullptr;
    const Row *row = page && address.hasRow() ? page->row(address.row) : nullptr;
    if (!page || (address.hasRow() && !row)) {
        return failed(Code::UnknownAddress,
                      tr("%1 is not the address of a settings page or of a row on one.").arg(address.toString()));
    }

    // The entry as its registry has it. A page is given without its rows:
    // List gives the whole registry.
    QJsonObject pageEntry = page->json;
    pageEntry.remove(QStringLiteral("rows"));
    QJsonObject entry;
    entry.insert(QStringLiteral("address"), address.toString());
    entry.insert(QStringLiteral("program"), page->program);
    entry.insert(QStringLiteral("page"), pageEntry);
    if (row)
        entry.insert(QStringLiteral("row"), row->json);

    Outcome out;
    out.ok = true;
    out.address = address;
    out.result.insert(QStringLiteral("json"), QString::fromUtf8(QJsonDocument(entry).toJson(QJsonDocument::Compact)));
    return out;
}

QVariantMap SettingsHost::go(const QString &address)
{
    const Address parsed = Address::parse(address);
    return parsed.isValid() && parsed.hasRow() ? reveal(address, QVariantMap()) : open(address);
}

// For the window, which does not wait: the call is sent to the program that
// owns the address, and what that program makes of it is its own to show.
SettingsHost::Outcome SettingsHost::passedOn(const Outcome &outcome, const QVariantMap &options)
{
    if (!outcome.ok || outcome.forwardService.isEmpty())
        return outcome;
    if (!forward(outcome.forwardService, outcome.address.toString(), options, {})) {
        return failed(Code::OwnerUnreachable,
                      tr("%1 belongs to the program %2, and there is no session bus to reach it on.")
                          .arg(outcome.address.toString(), outcome.forwardService));
    }
    Outcome out = outcome;
    out.result.insert(QStringLiteral("address"), outcome.address.toString());
    out.result.insert(QStringLiteral("program"), outcome.forwardService);
    out.result.insert(QStringLiteral("forwarded"), true);
    return out;
}

QVariantMap SettingsHost::open(const QString &address)
{
    return toView(passedOn(perform(address, QVariantMap(), Verb::Open),
                           QVariantMap { { QStringLiteral("highlight"), false } }));
}

QVariantMap SettingsHost::reveal(const QString &address, const QVariantMap &options)
{
    const Outcome outcome = passedOn(perform(address, options, Verb::Reveal), options);
    QVariantMap out = toView(outcome);
    // A view reads the rectangle as four numbers.
    if (outcome.ok && outcome.result.contains(QStringLiteral("rect"))) {
        const auto rect = outcome.result.value(QStringLiteral("rect")).value<Settings::RectD>();
        out.insert(QStringLiteral("x"), rect.x);
        out.insert(QStringLiteral("y"), rect.y);
        out.insert(QStringLiteral("width"), rect.width);
        out.insert(QStringLiteral("height"), rect.height);
        out.remove(QStringLiteral("rect"));
    }
    return out;
}

void SettingsHost::clearHighlight()
{
    Q_EMIT clearRequested();
}

QVariantMap SettingsHost::openSystemSettings()
{
    return toView(startSystemSettings(QString(), QStringList()));
}

void SettingsHost::answer(const QVariantMap &shown)
{
    m_shown = shown;
    m_answered = true;
}

void SettingsHost::rowChanged(const QString &address)
{
    if (m_addressableAdaptor && Address::parse(address).hasRow())
        Q_EMIT m_addressableAdaptor->RowChanged(address);
}

QQuickItem *SettingsHost::findRow(QQuickItem *root, const QString &address) const
{
    if (!root)
        return nullptr;
    if (isRow(root) && root->property("address").toString() == address)
        return root;
    const QList<QQuickItem *> children = root->childItems();
    for (QQuickItem *child : children) {
        if (QQuickItem *found = findRow(child, address))
            return found;
    }
    return nullptr;
}

QVariantList SettingsHost::rows(QQuickItem *root) const
{
    QVariantList out;
    if (root)
        collectRows(root, out);
    return out;
}

void SettingsHost::settle(QQuickItem *root) const
{
    if (!root)
        return;
    // A layout learns its size from what is inside it and gives its children
    // their places. One pass moves that one level; a page is a few levels of
    // layouts with cards between them.
    for (int pass = 0; pass < 8; ++pass)
        polish(root);
}

bool SettingsHost::startBus()
{
    if (m_onBus)
        return true;
    Settings::registerBusTypes();

    QDBusConnection bus = QDBusConnection::sessionBus();
    if (!bus.isConnected()) {
        m_busProblem = tr("There is no session bus: %1").arg(bus.lastError().message());
        Q_EMIT busChanged();
        return false;
    }
    m_bus = bus;

    if (!m_settingsObject) {
        m_settingsObject = new QObject(this);
        m_settingsAdaptor = new Settings::Settings1Adaptor(m_settingsObject, this);
        m_addressableObject = new QObject(this);
        m_addressableAdaptor = new Settings::Addressable1Adaptor(m_addressableObject, this);
    }
    // The objects first, the names after: whoever sees the name finds the
    // objects there.
    const bool objects = m_bus.registerObject(Settings::Bus::settingsPath(), m_settingsObject)
                         && m_bus.registerObject(Settings::Bus::addressablePath(), m_addressableObject);
    if (!objects || !m_bus.registerService(Settings::Bus::settingsService())
        || !m_bus.registerService(Registry::ownProgram())) {
        m_busProblem = objects ? tr("Another Settings window already answers on the session bus.")
                               : tr("The program could not be put on the session bus: %1").arg(m_bus.lastError().message());
        m_bus.unregisterService(Settings::Bus::settingsService());
        m_bus.unregisterService(Registry::ownProgram());
        m_bus.unregisterObject(Settings::Bus::settingsPath());
        m_bus.unregisterObject(Settings::Bus::addressablePath());
        Q_EMIT busChanged();
        return false;
    }
    m_onBus = true;
    m_busProblem.clear();
    Q_EMIT busChanged();
    return true;
}

void SettingsHost::stopBus()
{
    if (!m_onBus)
        return;
    m_bus.unregisterService(Settings::Bus::settingsService());
    m_bus.unregisterService(Registry::ownProgram());
    m_bus.unregisterObject(Settings::Bus::settingsPath());
    m_bus.unregisterObject(Settings::Bus::addressablePath());
    m_onBus = false;
    Q_EMIT busChanged();
}

bool SettingsHost::forward(const QString &service, const QString &address, const QVariantMap &options,
                           const std::function<void(const QDBusMessage &)> &done)
{
    QDBusConnection bus = m_onBus ? m_bus : QDBusConnection::sessionBus();
    if (!bus.isConnected())
        return false;
    QDBusMessage call = QDBusMessage::createMethodCall(service, Settings::Bus::addressablePath(),
                                                       Settings::Bus::addressableInterface(), QStringLiteral("Reveal"));
    call << address << options;
    auto *watcher = new QDBusPendingCallWatcher(bus.asyncCall(call, ForwardTimeoutMs), this);
    connect(watcher, &QDBusPendingCallWatcher::finished, this, [done](QDBusPendingCallWatcher *finished) {
        if (done)
            done(finished->reply());
        finished->deleteLater();
    });
    return true;
}
