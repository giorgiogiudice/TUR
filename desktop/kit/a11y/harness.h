// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#pragma once

#include <QColor>
#include <QObject>
#include <QQuickItem>
#include <QString>
#include <QStringList>
#include <QVariant>
#include <QVariantList>
#include <QVariantMap>
#include <QtQml/qqmlregistration.h>

#include <memory>

class PseudoTranslator;
class QJSEngine;
class QQmlEngine;

// The test harness of module Tur.Kit.Testing: what a Qt Quick Test needs to
// check a page the way desktop-experience.md, section 13, asks. Every TUR
// program's tests use it; it is not installed in the image.
//
//  - the direction and text-length harness: right-to-left and the
//    pseudo-locale (L5, L6);
//  - the contrast walk: what colour is really behind each text and each
//    control boundary (X5);
//  - the accessibility walk: the tab stops of a page and what each exposes to
//    the accessibility layer (X1, X3);
//  - clipped text, for the 200 % text test (X8).
class Harness : public QObject
{
    Q_OBJECT
    QML_ELEMENT
    QML_SINGLETON

    Q_PROPERTY(bool rightToLeft READ rightToLeft WRITE setRightToLeft NOTIFY rightToLeftChanged FINAL)
    Q_PROPERTY(Pseudo pseudo READ pseudo WRITE setPseudo NOTIFY pseudoChanged FINAL)

    // Where screenshots go: TUR_SCREENSHOT_DIR, or empty when nothing should
    // be saved.
    Q_PROPERTY(QString outputDir READ outputDir CONSTANT FINAL)

    // "1x", "1.5x": the scale the process renders at (QT_SCALE_FACTOR).
    Q_PROPERTY(QString scaleLabel READ scaleLabel CONSTANT FINAL)
    Q_PROPERTY(double devicePixelRatio READ devicePixelRatio CONSTANT FINAL)

    // The kit's tokens.json, for tests that compare the live tokens with it.
    Q_PROPERTY(QString tokensFile READ tokensFile CONSTANT FINAL)

public:
    enum Pseudo {
        None,    // strings as written
        Long,    // every string at least 40 % longer
        RtlLong, // longer, and in a right-to-left script
    };
    Q_ENUM(Pseudo)

    explicit Harness(QQmlEngine *engine);
    ~Harness() override;

    static Harness *create(QQmlEngine *qmlEngine, QJSEngine *jsEngine);

    bool rightToLeft() const;
    void setRightToLeft(bool rightToLeft);

    Pseudo pseudo() const;
    void setPseudo(Pseudo pseudo);

    QString outputDir() const;
    QString scaleLabel() const;
    double devicePixelRatio() const;
    QString tokensFile() const;

    // What the test is running on: Qt version, platform, renderer, style,
    // fonts. Printed by the tests so that a CI log says what drew a picture.
    Q_INVOKABLE QVariantMap environment() const;

    // WCAG 2.2 contrast ratio of two opaque colours, from 1 to 21.
    Q_INVOKABLE double contrast(const QColor &first, const QColor &second) const;

    // Walks every visible item in the window `item` is in, in the order they
    // are painted, and reports each piece of text and each control boundary
    // with the colour that is really behind it:
    //
    //   kind        "text" or "boundary"
    //   ratio       the contrast ratio, or 0 if no background was found
    //   foreground, background    the two colours, as "#rrggbb"
    //   exempt      true if the item is disabled (WCAG exempts it)
    //   problem     empty, or why the pair could not be judged
    //   where       the item, as a path of type names, and its text
    //
    // A boundary is a Rectangle's border. Borders drawn in one of the
    // `decorative` colours (line.subtle) are left out.
    Q_INVOKABLE QVariantList contrastFindings(QQuickItem *item, const QVariantList &decorative) const;

    // The items under `root` that Tab stops at: visible, enabled, and asking
    // for tab focus. In the order of the item tree.
    Q_INVOKABLE QVariantList tabStops(QQuickItem *root) const;

    // What the accessibility layer (the interface a screen reader's bridge
    // reads) reports for an item: valid, name, description, role, roleName,
    // focusable, checkable, checked, disabled.
    Q_INVOKABLE QVariantMap accessible(QQuickItem *item) const;

    // Text items under `root` that do not show all of their text: elided, or
    // wider or taller than the room they were given.
    Q_INVOKABLE QStringList clippedText(QQuickItem *root) const;

    // The item's rectangle in the window, as x, y, width, height.
    Q_INVOKABLE QVariantMap sceneRect(QQuickItem *item) const;

    // The item as a short path of type names, for failure messages.
    Q_INVOKABLE QString describe(QQuickItem *item) const;

    // A JSON file as a JavaScript value; undefined if it cannot be read.
    Q_INVOKABLE QVariant readJson(const QString &path) const;

Q_SIGNALS:
    void rightToLeftChanged();
    void pseudoChanged();

private:
    QQmlEngine *m_engine;
    std::unique_ptr<PseudoTranslator> m_translator;
    Pseudo m_pseudo = None;
    bool m_rightToLeft = false;
};
