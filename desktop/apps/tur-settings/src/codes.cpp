// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#include "codes.h"

#include <QCoreApplication>

namespace Settings {

QString codeText(Code code)
{
    return QStringLiteral("TUR-DESKTOP-SETTINGS-%1").arg(int(code), 3, 10, QLatin1Char('0'));
}

QString errorName(Code code)
{
    const char *name = "Failed";
    switch (code) {
    case Code::UnknownAddress:
        name = "UnknownAddress";
        break;
    case Code::RemovedAddress:
        name = "RemovedAddress";
        break;
    case Code::PageNotAvailable:
        name = "PageNotAvailable";
        break;
    case Code::NotAnAddress:
        name = "NotAnAddress";
        break;
    case Code::OwnerUnreachable:
        name = "OwnerUnreachable";
        break;
    case Code::RowNotShown:
        name = "RowNotShown";
        break;
    case Code::RegistryUnreadable:
        name = "RegistryUnreadable";
        break;
    case Code::SystemDescriptionUnreadable:
        name = "SystemDescriptionUnreadable";
        break;
    }
    return QStringLiteral("org.tur.Settings1.Error.") + QLatin1String(name);
}

QString titleOf(Code code)
{
    switch (code) {
    case Code::UnknownAddress:
        return QCoreApplication::translate("Settings", "No setting has this address.");
    case Code::RemovedAddress:
        return QCoreApplication::translate("Settings", "This setting is no longer there.");
    case Code::PageNotAvailable:
        return QCoreApplication::translate("Settings", "This page is not part of the system yet.");
    case Code::NotAnAddress:
        return QCoreApplication::translate("Settings", "This is not the address of a setting.");
    case Code::OwnerUnreachable:
        return QCoreApplication::translate("Settings", "The program that holds this setting could not be opened.");
    case Code::RowNotShown:
        return QCoreApplication::translate("Settings", "The page does not show this setting.");
    case Code::RegistryUnreadable:
        return QCoreApplication::translate("Settings", "A list of settings could not be read.");
    case Code::SystemDescriptionUnreadable:
        return QCoreApplication::translate("Settings", "TUR could not read what this system is.");
    }
    return QString();
}

QVariantMap failure(Code code, const QString &reason, const QString &detail)
{
    QVariantMap out;
    out.insert(QStringLiteral("ok"), false);
    out.insert(QStringLiteral("code"), codeText(code));
    out.insert(QStringLiteral("title"), titleOf(code));
    out.insert(QStringLiteral("reason"), reason);
    out.insert(QStringLiteral("detail"), detail);
    return out;
}

QString errorMessage(Code code, const QString &reason)
{
    return codeText(code) + QLatin1String(": ") + reason;
}

} // namespace Settings
