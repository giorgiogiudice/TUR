// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#pragma once

#include <QString>
#include <QTranslator>

// A translator that needs no catalogue: it rewrites every source string so
// that a layout can be tested for longer text and for a right-to-left script
// before any real translation exists (desktop-experience.md, 13.2, L5 and
// L6).
//
//  - Every string becomes at least 40 % longer.
//  - In the right-to-left mode Latin letters become Hebrew letters, so the
//    text really runs right to left and really needs a fallback font, and the
//    application's layout direction becomes right-to-left, as it would with a
//    real Hebrew or Arabic catalogue.
//  - Placeholders (%1, %n, %L1) are kept, so the result still formats.
//
// Text that never goes through qsTr (error codes, paths, commands) is
// untouched, which is the point of L8.
class PseudoTranslator : public QTranslator
{
    Q_OBJECT

public:
    explicit PseudoTranslator(bool rightToLeft, QObject *parent = nullptr);

    QString translate(const char *context, const char *sourceText, const char *disambiguation = nullptr,
                      int n = -1) const override;
    bool isEmpty() const override;

    // The rewriting itself, for tests.
    static QString transform(const QString &source, bool rightToLeft);

private:
    bool m_rightToLeft;
};
