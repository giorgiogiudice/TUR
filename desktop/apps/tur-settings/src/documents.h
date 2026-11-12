// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#pragma once

#include <QObject>
#include <QString>
#include <QtQml/qqmlregistration.h>

// The texts the About page shows word for word: TUR's licence, its required
// notice and the third-party notices. They are the files at the top of the
// repository (LICENSE.md, NOTICE, THIRD_PARTY_NOTICES.md), compiled into the
// program, so that what is shown is what was shipped. Nothing here rewrites,
// shortens or renders them: a licence is shown as it is.
class Documents : public QObject
{
    Q_OBJECT
    QML_ELEMENT
    QML_SINGLETON

public:
    explicit Documents(QObject *parent = nullptr);

    // "licence", "notice" or "third-party". Empty for anything else.
    Q_INVOKABLE QString text(const QString &name) const;
};
