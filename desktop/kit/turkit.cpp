// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#include "turkit.h"

#include "tokens/systemappearance.h"

#include <QFont>
#include <QGuiApplication>
#include <QQuickStyle>
#include <QString>

void TurKit::useStandaloneLook()
{
    QQuickStyle::setStyle(QStringLiteral("Basic"));

    // The interface face and size of desktop-experience.md, 5.3. Fontconfig
    // substitutes if Inter is not installed; the tests check that it is.
    QFont font(QStringLiteral("Inter"));
    font.setPointSizeF(10.0);
    QGuiApplication::setFont(font);

    // Dark unless the program says otherwise: there is no desktop to follow.
    SystemAppearance::setStandalone(true);
}
