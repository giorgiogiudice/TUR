// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#include "busadaptor.h"

#include "overlayservice.h"

#include <QDBusConnection>

namespace {

// Answers the call with the outcome's error instead of the slot's value.
// Returns true if it did.
bool refused(OverlayService *service, const QDBusMessage &message, const OverlayService::Outcome &outcome)
{
    if (outcome.ok)
        return false;
    message.setDelayedReply(true);
    service->bus().send(message.createErrorReply(outcome.errorName, outcome.errorMessage));
    return true;
}

} // namespace

PaxOverlay1Adaptor::PaxOverlay1Adaptor(OverlayService *service)
    : QDBusAbstractAdaptor(service)
    , m_service(service)
{
    setAutoRelaySignals(false);
}

bool PaxOverlay1Adaptor::shown() const
{
    return m_service->shown();
}

QStringList PaxOverlay1Adaptor::screens() const
{
    return m_service->screens();
}

QString PaxOverlay1Adaptor::Acquire(const QString &purpose, const QDBusMessage &message)
{
    const auto outcome = m_service->acquire(message.service(), purpose);
    if (refused(m_service, message, outcome))
        return {};
    return outcome.value.toString();
}

void PaxOverlay1Adaptor::Release(const QString &lease, const QDBusMessage &message)
{
    refused(m_service, message, m_service->release(message.service(), lease));
}

void PaxOverlay1Adaptor::SetActivity(const QString &lease, const QString &state, const QVariantMap &detail,
                                     const QDBusMessage &message)
{
    refused(m_service, message, m_service->setActivity(message.service(), lease, state, detail));
}

void PaxOverlay1Adaptor::MovePointer(const QString &lease, const QString &screen, double x, double y,
                                     const QVariantMap &options, const QDBusMessage &message)
{
    refused(m_service, message, m_service->movePointer(message.service(), lease, screen, x, y, options));
}

void PaxOverlay1Adaptor::PointAt(const QString &lease, const QString &screen, const PaxOverlay::RectD &rect,
                                 const QVariantMap &options, const QDBusMessage &message)
{
    refused(m_service, message, m_service->pointAt(message.service(), lease, screen, rect, options));
}

void PaxOverlay1Adaptor::PointInWindow(const QString &lease, uint pid, const QString &app_id, const QString &title,
                                       const PaxOverlay::RectD &rect, const QVariantMap &options,
                                       const QDBusMessage &message)
{
    refused(m_service, message,
            m_service->pointInWindow(message.service(), lease, pid, app_id, title, rect, options));
}

void PaxOverlay1Adaptor::PulsePointer(const QString &lease, const QString &kind, const QDBusMessage &message)
{
    refused(m_service, message, m_service->pulsePointer(message.service(), lease, kind));
}

void PaxOverlay1Adaptor::HidePointer(const QString &lease, const QDBusMessage &message)
{
    refused(m_service, message, m_service->hidePointer(message.service(), lease));
}

uint PaxOverlay1Adaptor::HighlightRect(const QString &lease, const QString &screen, const PaxOverlay::RectD &rect,
                                       const QVariantMap &options, const QDBusMessage &message)
{
    const auto outcome = m_service->highlightRect(message.service(), lease, screen, rect, options);
    if (refused(m_service, message, outcome))
        return 0;
    return outcome.value.toUInt();
}

uint PaxOverlay1Adaptor::HighlightInWindow(const QString &lease, uint pid, const QString &app_id,
                                           const QString &title, const PaxOverlay::RectD &rect,
                                           const QVariantMap &options, const QDBusMessage &message)
{
    const auto outcome = m_service->highlightInWindow(message.service(), lease, pid, app_id, title, rect, options);
    if (refused(m_service, message, outcome))
        return 0;
    return outcome.value.toUInt();
}

void PaxOverlay1Adaptor::SetCaption(const QString &lease, uint id, const QString &text, const QStringList &controls,
                                    const QDBusMessage &message)
{
    refused(m_service, message, m_service->setCaption(message.service(), lease, id, text, controls));
}

void PaxOverlay1Adaptor::Clear(const QString &lease, uint id, const QDBusMessage &message)
{
    refused(m_service, message, m_service->clear(message.service(), lease, id));
}

void PaxOverlay1Adaptor::Hide(const QDBusMessage &message)
{
    Q_UNUSED(message)
    m_service->hide();
}
