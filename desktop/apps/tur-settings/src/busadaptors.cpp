// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#include "busadaptors.h"

#include "address.h"
#include "codes.h"
#include "registry.h"
#include "settingshost.h"

#include <QCoreApplication>
#include <QDBusConnection>
#include <QDBusMetaType>

#include <algorithm>

namespace Settings {

QDBusArgument &operator<<(QDBusArgument &argument, const SearchHit &hit)
{
    argument.beginStructure();
    argument << hit.address << hit.title << hit.score;
    argument.endStructure();
    return argument;
}

const QDBusArgument &operator>>(const QDBusArgument &argument, SearchHit &hit)
{
    argument.beginStructure();
    argument >> hit.address >> hit.title >> hit.score;
    argument.endStructure();
    return argument;
}

QDBusArgument &operator<<(QDBusArgument &argument, const RectD &rect)
{
    argument.beginStructure();
    argument << rect.x << rect.y << rect.width << rect.height;
    argument.endStructure();
    return argument;
}

const QDBusArgument &operator>>(const QDBusArgument &argument, RectD &rect)
{
    argument.beginStructure();
    argument >> rect.x >> rect.y >> rect.width >> rect.height;
    argument.endStructure();
    return argument;
}

void registerBusTypes()
{
    qDBusRegisterMetaType<SearchHit>();
    qDBusRegisterMetaType<QList<SearchHit>>();
    qDBusRegisterMetaType<RectD>();
}

namespace {

// Answers a call with an error instead of the value the slot returns.
void refuse(SettingsHost *host, const QDBusMessage &message, const SettingsHost::Outcome &outcome)
{
    message.setDelayedReply(true);
    host->bus().send(message.createErrorReply(errorName(outcome.code), errorMessage(outcome.code, outcome.reason)));
}

// Passes a call on to the program that owns the address, and answers the
// caller when that program has answered. `withResult` says whether the
// caller is given the other program's result (Reveal) or nothing (Open).
void passOn(SettingsHost *host, const QDBusMessage &message, const SettingsHost::Outcome &outcome,
            const QVariantMap &options, bool withResult)
{
    message.setDelayedReply(true);
    const QString service = outcome.forwardService;
    const QString address = outcome.address.toString();
    // A call that arrived over the bus can be passed on over it.
    host->forward(service, address, options, [host, message, service, address, withResult](const QDBusMessage &reply) {
        if (reply.type() == QDBusMessage::ReplyMessage) {
            host->bus().send(withResult ? message.createReply(reply.arguments()) : message.createReply());
            return;
        }
        // An error of the same family is passed on as it is: the other
        // program knows better than this one what went wrong.
        if (reply.errorName().startsWith(QLatin1String("org.tur."))) {
            host->bus().send(message.createErrorReply(reply.errorName(), reply.errorMessage()));
            return;
        }
        const QString reason = QCoreApplication::translate(
                                   "Settings", "%1 belongs to the program %2, which did not answer.")
                                   .arg(address, service);
        host->bus().send(message.createErrorReply(
            errorName(Code::OwnerUnreachable),
            errorMessage(Code::OwnerUnreachable, reason) + QLatin1String(" (") + reply.errorName() + QLatin1String(": ")
                + reply.errorMessage() + QLatin1Char(')')));
    });
}

} // namespace

Addressable1Adaptor::Addressable1Adaptor(QObject *object, SettingsHost *host)
    : QDBusAbstractAdaptor(object)
    , m_host(host)
{
}

QVariantMap Addressable1Adaptor::Reveal(const QString &address, const QVariantMap &options, const QDBusMessage &message)
{
    // This interface speaks for this program's own pages. The address of
    // another program, or of KDE System Settings, is asked of
    // org.tur.Settings1; here it is refused before anything is opened.
    const Page *page = m_host->registry() ? m_host->registry()->data().page(Address::parse(address).page) : nullptr;
    if (page && page->program != Registry::ownProgram()) {
        SettingsHost::Outcome notMine;
        notMine.code = Code::UnknownAddress;
        notMine.reason = QCoreApplication::translate("Settings", "%1 is not a setting of this program.").arg(address);
        refuse(m_host, message, notMine);
        return QVariantMap();
    }
    const SettingsHost::Outcome outcome = m_host->perform(address, options, SettingsHost::Verb::Reveal);
    if (!outcome.ok) {
        refuse(m_host, message, outcome);
        return QVariantMap();
    }
    return outcome.result;
}

void Addressable1Adaptor::ClearHighlight()
{
    m_host->clearHighlight();
}

QString Addressable1Adaptor::List()
{
    return m_host->registry() ? QString::fromUtf8(m_host->registry()->data().document(Registry::ownProgram()))
                              : QString();
}

Settings1Adaptor::Settings1Adaptor(QObject *object, SettingsHost *host)
    : QDBusAbstractAdaptor(object)
    , m_host(host)
{
}

QString Settings1Adaptor::address() const
{
    return m_host->currentAddress();
}

void Settings1Adaptor::Open(const QString &address, const QDBusMessage &message)
{
    const SettingsHost::Outcome outcome = m_host->perform(address, QVariantMap(), SettingsHost::Verb::Open);
    if (!outcome.ok)
        refuse(m_host, message, outcome);
    else if (!outcome.forwardService.isEmpty())
        passOn(m_host, message, outcome, QVariantMap { { QStringLiteral("highlight"), false } }, false);
}

QVariantMap Settings1Adaptor::Reveal(const QString &address, const QVariantMap &options, const QDBusMessage &message)
{
    const SettingsHost::Outcome outcome = m_host->perform(address, options, SettingsHost::Verb::Reveal);
    if (!outcome.ok) {
        refuse(m_host, message, outcome);
        return QVariantMap();
    }
    if (!outcome.forwardService.isEmpty()) {
        passOn(m_host, message, outcome, options, true);
        return QVariantMap();
    }
    return outcome.result;
}

void Settings1Adaptor::ClearHighlight()
{
    m_host->clearHighlight();
}

QString Settings1Adaptor::Resolve(const QString &address, const QDBusMessage &message)
{
    const SettingsHost::Outcome outcome = m_host->resolve(address);
    if (!outcome.ok) {
        refuse(m_host, message, outcome);
        return QString();
    }
    return outcome.result.value(QStringLiteral("json")).toString();
}

QList<SearchHit> Settings1Adaptor::Search(const QString &text, uint limit)
{
    QList<SearchHit> out;
    if (!m_host->registry())
        return out;
    // No limit asked for means twenty; nobody is given more than a hundred.
    const int most = limit == 0 ? 20 : int(std::min<uint>(limit, 100));
    const QList<Hit> hits = m_host->registry()->data().search(text, most);
    for (const Hit &hit : hits)
        out.append(SearchHit { hit.address, hit.title, hit.score });
    return out;
}

} // namespace Settings
