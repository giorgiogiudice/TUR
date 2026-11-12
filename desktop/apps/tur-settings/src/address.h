// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#pragma once

#include <QString>

namespace Settings {

// The address of a settings page, or of a row on one
// (docs/architecture/desktop-experience.md, section 10.3):
//
//   address   = page [ "#" row ]
//   page      = namespace "." segment *( "." segment )
//   namespace = "tur" | "kde"
//   segment   = 1*( a-z | 0-9 | "-" | "_" )
//   row       = 1*( a-z | 0-9 | "-" )
//
// An address is lower-case ASCII, is never translated and keeps its meaning
// for ever. On the command line and in a link it may be written with the
// scheme of the program's desktop file in front: tur-settings:tur.about#version.
struct Address
{
    QString page; // "tur.pax.privacy"; empty if the text was not an address
    QString row;  // "private-mode"; empty for a page

    bool isValid() const { return !page.isEmpty(); }
    bool hasRow() const { return !row.isEmpty(); }

    // "tur" or "kde".
    QString nameSpace() const;

    // page, or page#row.
    QString toString() const;

    // Reads an address. Anything that does not follow the grammar exactly is
    // not an address: nothing is trimmed, lower-cased or guessed. The scheme
    // "tur-settings:" in front is allowed, and "%23" for "#" after it, because
    // that is how a link may arrive.
    static Address parse(const QString &text);

    static bool isPage(const QString &text);
    static bool isRowId(const QString &text);

    static constexpr int MaximumPageLength = 120;
    static constexpr int MaximumRowLength = 64;
};

} // namespace Settings
