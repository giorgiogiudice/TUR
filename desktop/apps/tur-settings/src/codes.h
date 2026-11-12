// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#pragma once

#include <QString>
#include <QVariantMap>

namespace Settings {

// The codes of this program, TUR-DESKTOP-SETTINGS-NNN (specification 29.3;
// desktop-experience.md, 10.3). A code keeps its meaning for ever. They are
// not registry entries in the sense of the specification yet: no registry of
// error codes exists, so their sentences are here until one does.
enum class Code {
    UnknownAddress = 1,              // well formed, and no registry has it
    RemovedAddress = 2,              // it was there once; the message names its successor
    PageNotAvailable = 3,            // reserved for a page that is not part of this system
    NotAnAddress = 4,                // does not follow the grammar
    OwnerUnreachable = 5,            // the program that owns the address did not start or did not answer
    RowNotShown = 6,                 // registered, and the page does not show it: a fault of this program
    RegistryUnreadable = 7,          // a registry file breaks the format
    SystemDescriptionUnreadable = 10 // /usr/lib/tur/release cannot be read
};

// "TUR-DESKTOP-SETTINGS-001".
QString codeText(Code code);

// The D-Bus error name: "org.tur.Settings1.Error.UnknownAddress".
QString errorName(Code code);

// What happened, in one sentence, for a person.
QString titleOf(Code code);

// The content of the kit's two-layer message, as the views take it: ok
// (false), code, title, reason, detail.
QVariantMap failure(Code code, const QString &reason, const QString &detail);

// The message of a D-Bus error: the code, a colon, and the reason. A caller
// reads the code from the start of the message.
QString errorMessage(Code code, const QString &reason);

} // namespace Settings
