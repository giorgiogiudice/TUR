// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#include "overlayservice.h"

#include "busadaptor.h"
#include "overlaystate.h"
#include "windowlocator.h"

#include <QDBusConnectionInterface>
#include <QDBusMessage>
#include <QDBusServiceWatcher>
#include <QRandomGenerator>
#include <QTimer>

#include <algorithm>
#include <array>
#include <cmath>
#include <utility>

namespace {

// org.tur.PaxOverlay1's errors (dbus/org.tur.PaxOverlay1.xml).
struct ErrorKind
{
    const char *name;
    const char *code;
};
constexpr std::array<ErrorKind, 6> errors { {
    { "", "" },
    { "org.tur.PaxOverlay1.Error.NoLease", "TUR-DESKTOP-PAXOVERLAY-001" },
    { "org.tur.PaxOverlay1.Error.InvalidArgument", "TUR-DESKTOP-PAXOVERLAY-002" },
    { "org.tur.PaxOverlay1.Error.WindowNotFound", "TUR-DESKTOP-PAXOVERLAY-003" },
    { "org.tur.PaxOverlay1.Error.UnknownHighlight", "TUR-DESKTOP-PAXOVERLAY-004" },
    { "org.tur.PaxOverlay1.Error.LimitReached", "TUR-DESKTOP-PAXOVERLAY-005" },
} };
constexpr int NoLease = 1;
constexpr int InvalidArgument = 2;
constexpr int WindowNotFound = 3;
constexpr int UnknownHighlight = 4;
constexpr int LimitReached = 5;

// A coordinate or a size that a screen could have.
constexpr double largest = 100000;

bool sane(const PaxOverlay::RectD &rect)
{
    for (double value : { rect.x, rect.y, rect.width, rect.height }) {
        if (!std::isfinite(value) || std::abs(value) > largest)
            return false;
    }
    return rect.width > 0 && rect.height > 0;
}

QString newToken()
{
    std::array<quint32, 4> words {};
    QRandomGenerator::system()->fillRange(words.data(), words.size());
    QString token;
    for (quint32 word : words)
        token += QStringLiteral("%1").arg(word, 8, 16, QLatin1Char('0'));
    return token;
}

// The window the caller names, as WindowLocator::find takes it; for the
// error message.
QString describeWindow(uint pid, const QString &appId, const QString &title)
{
    return QStringLiteral("pid %1, app id \"%2\", title \"%3\"").arg(pid).arg(appId, title.left(80));
}

} // namespace

OverlayService::OverlayService(OverlayState *state, WindowLocator *locator, QObject *parent)
    : QObject(parent)
    , m_state(state)
    , m_locator(locator)
    , m_bus(QStringLiteral("tur-pax-overlay-not-connected"))
{
    new PaxOverlay1Adaptor(this);
    connect(m_state, &OverlayState::pointerArrived, this, [this] {
        const QString token = std::exchange(m_pointerLease, QString());
        const auto it = m_leases.constFind(token);
        if (it != m_leases.constEnd())
            sendTo(it->owner, QStringLiteral("PointerArrived"), { token });
    });
    connect(m_state, &OverlayState::activeChanged, this, &OverlayService::updateShown);
    connect(m_state, &OverlayState::activityChanged, this, &OverlayService::updateShown);
    connect(m_state, &OverlayState::onChanged, this, &OverlayService::updateShown);
    connect(m_locator, &WindowLocator::changed, this, &OverlayService::windowChanged);
    connect(m_locator, &WindowLocator::removed, this, &OverlayService::windowRemoved);
}

OverlayService::~OverlayService() = default;

bool OverlayService::registerOn(const QDBusConnection &bus, QString *problem)
{
    m_bus = bus;
    if (!m_bus.isConnected()) {
        *problem = QStringLiteral("There is no session bus: %1").arg(m_bus.lastError().message());
        return false;
    }
    PaxOverlay::registerBusTypes();
    m_watcher = new QDBusServiceWatcher(this);
    m_watcher->setConnection(m_bus);
    m_watcher->setWatchMode(QDBusServiceWatcher::WatchForUnregistration);
    connect(m_watcher, &QDBusServiceWatcher::serviceUnregistered, this, &OverlayService::ownerLeft);
    if (!m_bus.registerObject(PaxOverlay::Bus::path(), this, QDBusConnection::ExportAdaptors)) {
        *problem = QStringLiteral("The object %1 could not be put on the bus.").arg(PaxOverlay::Bus::path());
        return false;
    }
    if (!m_bus.registerService(PaxOverlay::Bus::service())) {
        m_bus.unregisterObject(PaxOverlay::Bus::path());
        *problem = QStringLiteral("The name %1 is taken: is tur-pax-overlay already running?")
                       .arg(PaxOverlay::Bus::service());
        return false;
    }
    return true;
}

OverlayService::Outcome OverlayService::refuse(int code, const QString &message) const
{
    Outcome out;
    out.ok = false;
    out.errorName = QString::fromLatin1(errors.at(code).name);
    out.errorMessage = QString::fromLatin1(errors.at(code).code) + QStringLiteral(": ") + message;
    return out;
}

OverlayService::Lease *OverlayService::leaseOf(const QString &caller, const QString &token)
{
    auto it = m_leases.find(token);
    // Another connection's lease is answered as if it did not exist, so that
    // nobody can learn whether a token is in use.
    if (it == m_leases.end() || it->owner != caller)
        return nullptr;
    return &it.value();
}

std::optional<QRectF> OverlayService::globalRect(const QString &screen, const PaxOverlay::RectD &rect,
                                                 Outcome *error) const
{
    if (!sane(rect)) {
        *error = refuse(InvalidArgument, QStringLiteral("A rectangle needs a positive width and height and "
                                                        "coordinates within %1 pixels.")
                                             .arg(largest));
        return std::nullopt;
    }
    QRectF out = rect.toRect();
    if (!screen.isEmpty()) {
        const auto it = m_outputs.constFind(screen);
        if (it == m_outputs.constEnd()) {
            *error = refuse(InvalidArgument, QStringLiteral("There is no screen called \"%1\". Screens: %2.")
                                                 .arg(screen.left(80), m_outputs.keys().join(QStringLiteral(", "))));
            return std::nullopt;
        }
        out.translate(it->topLeft());
    }
    return out;
}

std::optional<QPointF> OverlayService::globalPoint(const QString &screen, double x, double y, Outcome *error) const
{
    const auto rect = globalRect(screen, PaxOverlay::RectD { x, y, 1, 1 }, error);
    if (!rect)
        return std::nullopt;
    return rect->topLeft();
}

std::optional<OverlayService::Outcome> OverlayService::checkText(const QVariantMap &options) const
{
    for (const QString &key : { QStringLiteral("caption"), QStringLiteral("step") }) {
        if (options.value(key).toString().size() > maxTextLength) {
            return refuse(InvalidArgument,
                          QStringLiteral("The %1 is longer than %2 characters.").arg(key).arg(maxTextLength));
        }
    }
    const QString anchor = options.value(QStringLiteral("anchor")).toString();
    if (!anchor.isEmpty() && anchor != QLatin1String("center") && anchor != QLatin1String("start")
        && anchor != QLatin1String("end")) {
        return refuse(InvalidArgument, QStringLiteral("An anchor is center, start or end."));
    }
    return std::nullopt;
}

QRectF OverlayService::screenAt(const QRectF &rect) const
{
    for (const QRectF &output : m_outputs) {
        if (output.contains(rect.center()))
            return output;
    }
    return QRectF();
}

// --- The calls ----------------------------------------------------------------

OverlayService::Outcome OverlayService::acquire(const QString &caller, const QString &purpose)
{
    if (m_leases.size() >= maxLeases)
        return refuse(LimitReached, QStringLiteral("%1 leases are held already.").arg(maxLeases));
    Lease lease;
    lease.token = newToken();
    lease.owner = caller;
    lease.purpose = purpose.left(100);
    m_leases.insert(lease.token, lease);
    m_state->setOn(true);
    if (m_watcher && !m_watcher->watchedServices().contains(caller))
        m_watcher->addWatchedService(caller);
    // The caller may already have left: then the watcher never hears of it.
    if (m_bus.isConnected() && m_bus.interface() && !caller.isEmpty()
        && !m_bus.interface()->isServiceRegistered(caller).value()) {
        const QString token = lease.token;
        QMetaObject::invokeMethod(this, [this, token] { endLease(token, QStringLiteral("owner-gone")); },
                                  Qt::QueuedConnection);
    }
    Q_EMIT leaseBegan(caller, lease.purpose);
    Outcome out;
    out.value = lease.token;
    return out;
}

OverlayService::Outcome OverlayService::release(const QString &caller, const QString &token)
{
    if (!leaseOf(caller, token))
        return refuse(NoLease, QStringLiteral("This connection holds no such lease."));
    endLease(token, QStringLiteral("released"));
    return {};
}

OverlayService::Outcome OverlayService::setActivity(const QString &caller, const QString &token, const QString &state,
                                                    const QVariantMap &detail)
{
    Lease *lease = leaseOf(caller, token);
    if (!lease)
        return refuse(NoLease, QStringLiteral("This connection holds no such lease."));
    if (!OverlayState::isActivity(state)) {
        return refuse(InvalidArgument, QStringLiteral("\"%1\" is not a state. States: %2.")
                                           .arg(state.left(40), OverlayState::activities().join(QStringLiteral(", "))));
    }
    QString scope = detail.value(QStringLiteral("scope"), QStringLiteral("window")).toString();
    if (scope != QLatin1String("window") && scope != QLatin1String("screen"))
        return refuse(InvalidArgument, QStringLiteral("A scope is window or screen."));

    QString frameWindow;
    if (scope == QLatin1String("window")) {
        const uint pid = detail.value(QStringLiteral("pid")).toUInt();
        const QString appId = detail.value(QStringLiteral("app_id")).toString();
        const QString title = detail.value(QStringLiteral("title")).toString();
        if (pid != 0 || !appId.isEmpty() || !title.isEmpty()) {
            if (const auto place = m_locator->find(pid, appId, title))
                frameWindow = place->uuid;
        }
        // A window that cannot be found, or none named: the frame goes round
        // the screens and the pill says "this screen". Saying more than is
        // true is the safe error for an indicator of what P@x can see.
        if (frameWindow.isEmpty())
            scope = QStringLiteral("screen");
    }
    lease->activity = state;
    lease->scope = scope;
    lease->frameWindow = frameWindow;
    showLoudest();
    return {};
}

void OverlayService::showLoudest()
{
    const Lease *loudest = nullptr;
    for (const Lease &lease : std::as_const(m_leases)) {
        if (!loudest || OverlayState::loudness(lease.activity) > OverlayState::loudness(loudest->activity))
            loudest = &lease;
    }
    if (!loudest) {
        m_state->setActivity(QStringLiteral("idle"), QStringLiteral("window"), QRectF());
        return;
    }
    QRectF frame;
    if (!loudest->frameWindow.isEmpty()) {
        if (const auto place = m_locator->place(loudest->frameWindow))
            frame = place->frame;
    }
    m_state->setActivity(loudest->activity, frame.isEmpty() ? QStringLiteral("screen") : loudest->scope, frame);
}

void OverlayService::sendPointer(Lease &lease, const QPointF &target, const QVariantMap &options)
{
    m_pointerLease = lease.token;
    m_pointerHolder = lease.token;
    m_state->movePointer(target, options.value(QStringLiteral("slow")).toBool());
}

OverlayService::Outcome OverlayService::movePointer(const QString &caller, const QString &token, const QString &screen,
                                                    double x, double y, const QVariantMap &options)
{
    Lease *lease = leaseOf(caller, token);
    if (!lease)
        return refuse(NoLease, QStringLiteral("This connection holds no such lease."));
    Outcome error;
    const auto point = globalPoint(screen, x, y, &error);
    if (!point)
        return error;
    sendPointer(*lease, *point, options);
    return {};
}

OverlayService::Outcome OverlayService::pointAt(const QString &caller, const QString &token, const QString &screen,
                                                const PaxOverlay::RectD &rect, const QVariantMap &options)
{
    Lease *lease = leaseOf(caller, token);
    if (!lease)
        return refuse(NoLease, QStringLiteral("This connection holds no such lease."));
    if (const auto bad = checkText(options))
        return *bad;
    Outcome error;
    const auto global = globalRect(screen, rect, &error);
    if (!global)
        return error;
    sendPointer(*lease, OverlayState::anchorOf(*global, options.value(QStringLiteral("anchor")).toString()), options);
    return {};
}

OverlayService::Outcome OverlayService::pointInWindow(const QString &caller, const QString &token, uint pid,
                                                      const QString &appId, const QString &title,
                                                      const PaxOverlay::RectD &rect, const QVariantMap &options)
{
    Lease *lease = leaseOf(caller, token);
    if (!lease)
        return refuse(NoLease, QStringLiteral("This connection holds no such lease."));
    if (const auto bad = checkText(options))
        return *bad;
    Outcome error;
    if (!globalRect(QString(), rect, &error))
        return error;
    const auto place = m_locator->find(pid, appId, title);
    if (!place) {
        return refuse(WindowNotFound, QStringLiteral("No window matches %1 (%2).")
                                          .arg(describeWindow(pid, appId, title), m_locator->mechanism()));
    }
    const QRectF global = rect.toRect().translated(place->client.topLeft());
    sendPointer(*lease, OverlayState::anchorOf(global, options.value(QStringLiteral("anchor")).toString()), options);
    return {};
}

OverlayService::Outcome OverlayService::pulsePointer(const QString &caller, const QString &token, const QString &kind)
{
    if (!leaseOf(caller, token))
        return refuse(NoLease, QStringLiteral("This connection holds no such lease."));
    if (kind != QLatin1String("click"))
        return refuse(InvalidArgument, QStringLiteral("The one kind of pulse is \"click\"."));
    m_state->pulse();
    return {};
}

OverlayService::Outcome OverlayService::hidePointer(const QString &caller, const QString &token)
{
    if (!leaseOf(caller, token))
        return refuse(NoLease, QStringLiteral("This connection holds no such lease."));
    m_pointerLease.clear();
    m_pointerHolder.clear();
    m_state->hidePointer();
    return {};
}

uint OverlayService::addHighlight(Lease &lease, const QRectF &global, const QRectF &dimRect,
                                  const QVariantMap &options)
{
    const bool point = options.value(QStringLiteral("point")).toBool();
    QVariantMap shown {
        { QStringLiteral("dim"), options.value(QStringLiteral("dim")).toBool() },
        { QStringLiteral("dimRect"), dimRect },
        { QStringLiteral("caption"), options.value(QStringLiteral("caption")).toString() },
        { QStringLiteral("step"), options.value(QStringLiteral("step")).toString() },
        { QStringLiteral("afterPointer"), point },
    };
    if (point)
        sendPointer(lease, OverlayState::anchorOf(global, options.value(QStringLiteral("anchor")).toString()), options);
    const uint id = m_state->addHighlight(global, shown);
    lease.highlights.insert(id);

    const double duration = options.value(QStringLiteral("duration")).toDouble();
    if (std::isfinite(duration) && duration > 0) {
        const int milliseconds = int(std::min(duration, 3600.0) * 1000);
        QTimer::singleShot(milliseconds, this, [this, id] {
            if (m_state->hasHighlight(id))
                dropHighlight(id, QStringLiteral("expired"));
        });
    }
    return id;
}

OverlayService::Outcome OverlayService::highlightRect(const QString &caller, const QString &token,
                                                      const QString &screen, const PaxOverlay::RectD &rect,
                                                      const QVariantMap &options)
{
    Lease *lease = leaseOf(caller, token);
    if (!lease)
        return refuse(NoLease, QStringLiteral("This connection holds no such lease."));
    if (lease->highlights.size() >= maxHighlightsPerLease)
        return refuse(LimitReached, QStringLiteral("A lease has at most %1 highlights.").arg(maxHighlightsPerLease));
    if (const auto bad = checkText(options))
        return *bad;
    Outcome error;
    const auto global = globalRect(screen, rect, &error);
    if (!global)
        return error;
    Outcome out;
    out.value = addHighlight(*lease, *global, screenAt(*global), options);
    return out;
}

OverlayService::Outcome OverlayService::highlightInWindow(const QString &caller, const QString &token, uint pid,
                                                          const QString &appId, const QString &title,
                                                          const PaxOverlay::RectD &rect, const QVariantMap &options)
{
    Lease *lease = leaseOf(caller, token);
    if (!lease)
        return refuse(NoLease, QStringLiteral("This connection holds no such lease."));
    if (lease->highlights.size() >= maxHighlightsPerLease)
        return refuse(LimitReached, QStringLiteral("A lease has at most %1 highlights.").arg(maxHighlightsPerLease));
    if (const auto bad = checkText(options))
        return *bad;
    Outcome error;
    if (!globalRect(QString(), rect, &error))
        return error;
    const auto place = m_locator->find(pid, appId, title);
    if (!place) {
        return refuse(WindowNotFound, QStringLiteral("No window matches %1 (%2).")
                                          .arg(describeWindow(pid, appId, title), m_locator->mechanism()));
    }
    const QRectF global = rect.toRect().translated(place->client.topLeft());
    Outcome out;
    const uint id = addHighlight(*lease, global, place->client, options);
    m_tracked.insert(id, TrackedHighlight { token, place->uuid, rect.toRect(), place->client.size(),
                                            options.value(QStringLiteral("dim")).toBool() });
    out.value = id;
    // It may be covered already.
    windowChanged(place->uuid);
    return out;
}

OverlayService::Outcome OverlayService::setCaption(const QString &caller, const QString &token, uint id,
                                                   const QString &text, const QStringList &controls)
{
    Q_UNUSED(controls)
    Lease *lease = leaseOf(caller, token);
    if (!lease)
        return refuse(NoLease, QStringLiteral("This connection holds no such lease."));
    if (!lease->highlights.contains(id))
        return refuse(UnknownHighlight, QStringLiteral("This lease has no highlight %1.").arg(id));
    if (text.size() > maxTextLength)
        return refuse(InvalidArgument, QStringLiteral("A caption is at most %1 characters.").arg(maxTextLength));
    m_state->setCaption(id, text);
    return {};
}

OverlayService::Outcome OverlayService::clear(const QString &caller, const QString &token, uint id)
{
    Lease *lease = leaseOf(caller, token);
    if (!lease)
        return refuse(NoLease, QStringLiteral("This connection holds no such lease."));
    if (id == 0) {
        const QSet<uint> all = lease->highlights;
        for (uint each : all) {
            m_tracked.remove(each);
            m_state->removeHighlight(each);
        }
        lease->highlights.clear();
        return {};
    }
    if (!lease->highlights.remove(id))
        return refuse(UnknownHighlight, QStringLiteral("This lease has no highlight %1.").arg(id));
    m_tracked.remove(id);
    m_state->removeHighlight(id);
    return {};
}

void OverlayService::showCommands(bool shown)
{
    m_state->setCommandsShown(shown);
}

bool OverlayService::commandsShown() const
{
    return m_state->commandsShown();
}

void OverlayService::hide()
{
    const QStringList tokens = m_leases.keys();
    for (const QString &token : tokens)
        endLease(token, QStringLiteral("hidden"));
    m_pointerLease.clear();
    m_pointerHolder.clear();
    m_tracked.clear();
    m_state->clear();
    m_state->setOn(false);
    m_state->setCommandsShown(false);
    updateShown();
}

// --- Keeping it true ------------------------------------------------------------

void OverlayService::endLease(const QString &token, const QString &reason)
{
    const auto it = m_leases.find(token);
    if (it == m_leases.end())
        return;
    const Lease lease = it.value();
    m_leases.erase(it);
    for (uint id : lease.highlights) {
        m_tracked.remove(id);
        m_state->removeHighlight(id);
    }
    if (m_pointerHolder == token || m_pointerLease == token || m_leases.isEmpty()) {
        m_pointerLease.clear();
        m_pointerHolder.clear();
        m_state->hidePointer();
    }
    if (m_leases.isEmpty()) {
        m_state->clear();
        m_state->setOn(false);
        m_state->setCommandsShown(false);
    }
    showLoudest();

    bool stillOwns = false;
    for (const Lease &other : std::as_const(m_leases))
        stillOwns = stillOwns || other.owner == lease.owner;
    if (!stillOwns && m_watcher)
        m_watcher->removeWatchedService(lease.owner);
    if (reason != QLatin1String("owner-gone"))
        sendTo(lease.owner, QStringLiteral("LeaseEnded"), { token, reason });
    Q_EMIT leaseEnded(lease.owner, reason);
    updateShown();
}

void OverlayService::ownerLeft(const QString &owner)
{
    QStringList gone;
    for (const Lease &lease : std::as_const(m_leases)) {
        if (lease.owner == owner)
            gone.append(lease.token);
    }
    for (const QString &token : std::as_const(gone))
        endLease(token, QStringLiteral("owner-gone"));
}

void OverlayService::dropHighlight(uint id, const QString &reason)
{
    QString owner;
    for (Lease &lease : m_leases) {
        if (lease.highlights.remove(id))
            owner = lease.owner;
    }
    m_tracked.remove(id);
    m_state->removeHighlight(id);
    if (!owner.isEmpty())
        sendTo(owner, QStringLiteral("HighlightLost"), { id, reason });
}

void OverlayService::windowChanged(const QString &uuid)
{
    // The frame follows its window.
    showLoudest();

    QList<uint> ids = m_tracked.keys();
    for (uint id : std::as_const(ids)) {
        const auto it = m_tracked.constFind(id);
        if (it == m_tracked.constEnd() || (!uuid.isEmpty() && it->window != uuid))
            continue;
        const TrackedHighlight tracked = it.value();
        const auto place = m_locator->place(tracked.window);
        if (!place) {
            dropHighlight(id, QStringLiteral("closed"));
            continue;
        }
        if (place->minimized) {
            dropHighlight(id, QStringLiteral("minimized"));
            continue;
        }
        // A window that changed size may have moved its content: the
        // rectangle may no longer be the row it was. A window that only
        // moved carries its content with it.
        if (std::abs(place->client.width() - tracked.clientSize.width()) > 0.5
            || std::abs(place->client.height() - tracked.clientSize.height()) > 0.5) {
            dropHighlight(id, QStringLiteral("resized"));
            continue;
        }
        const QRectF global = tracked.inWindow.translated(place->client.topLeft());
        if (m_locator->covered(tracked.window, global)) {
            dropHighlight(id, QStringLiteral("covered"));
            continue;
        }
        m_state->moveHighlight(id, global, place->client);
    }
}

void OverlayService::windowRemoved(const QString &uuid)
{
    QList<uint> ids;
    for (auto it = m_tracked.cbegin(); it != m_tracked.cend(); ++it) {
        if (it->window == uuid)
            ids.append(it.key());
    }
    for (uint id : std::as_const(ids))
        dropHighlight(id, QStringLiteral("closed"));
    showLoudest();
}

void OverlayService::setOutputs(const QMap<QString, QRectF> &outputs)
{
    m_outputs = outputs;
    m_state->setScreens(outputs.values());
    updateShown();
}

void OverlayService::setDrawnScreens(const QStringList &drawn)
{
    QStringList sorted = drawn;
    sorted.sort();
    if (sorted != m_drawn) {
        m_drawn = sorted;
        Q_EMIT screensChanged();
        if (m_bus.isConnected()) {
            QDBusMessage changed = QDBusMessage::createSignal(PaxOverlay::Bus::path(),
                                                              QStringLiteral("org.freedesktop.DBus.Properties"),
                                                              QStringLiteral("PropertiesChanged"));
            changed << PaxOverlay::Bus::interface() << QVariantMap { { QStringLiteral("Screens"), m_drawn } }
                    << QStringList();
            m_bus.send(changed);
        }
    }
    updateShown();
}

void OverlayService::updateShown()
{
    bool everywhere = !m_outputs.isEmpty();
    for (auto it = m_outputs.cbegin(); it != m_outputs.cend(); ++it)
        everywhere = everywhere && m_drawn.contains(it.key());
    // Shown while P@x is on (a lease is held), idle included: the pill with
    // the stop combination is drawn then too.
    const bool shown = (m_state->on() || m_state->activity() != QLatin1String("idle")) && everywhere;
    if (shown == m_shown)
        return;
    m_shown = shown;
    Q_EMIT shownChanged(shown);
    if (m_bus.isConnected()) {
        QDBusMessage changed = QDBusMessage::createSignal(PaxOverlay::Bus::path(),
                                                          QStringLiteral("org.freedesktop.DBus.Properties"),
                                                          QStringLiteral("PropertiesChanged"));
        changed << PaxOverlay::Bus::interface() << QVariantMap { { QStringLiteral("Shown"), shown } } << QStringList();
        m_bus.send(changed);
    }
}

void OverlayService::sendTo(const QString &owner, const QString &signal, const QVariantList &arguments)
{
    if (!m_bus.isConnected() || owner.isEmpty())
        return;
    // A signal addressed to one connection: a lease's token is not told to
    // anyone else on the bus.
    QDBusMessage message =
        QDBusMessage::createTargetedSignal(owner, PaxOverlay::Bus::path(), PaxOverlay::Bus::interface(), signal);
    message.setArguments(arguments);
    m_bus.send(message);
}
