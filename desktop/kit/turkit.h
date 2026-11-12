// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#pragma once

// libturkit: the small C++ side of TUR's shared UI kit. The kit itself is the
// QML module "Tur.Kit"; see docs/architecture/desktop-kit.md.
namespace TurKit {

// For the programs that run outside a user's session (first-run setup, the
// installer, the trusted screen, P@x's overlay) and for tests: Qt Quick
// Controls' Basic style, TUR's interface font and the dark variant, whatever
// the machine's theme says. Call it after constructing the QGuiApplication
// and before loading any QML. Programs inside the session do not call it: there the desktop chooses
// the style (org.kde.breeze) and the font (desktop-experience.md, 5.7).
void useStandaloneLook();

} // namespace TurKit
