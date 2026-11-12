// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#include "harness.h"

#include "pseudotranslator.h"

#include <QAccessible>
#include <QCoreApplication>
#include <QFile>
#include <QFont>
#include <QFontInfo>
#include <QGuiApplication>
#include <QImage>
#include <QJsonDocument>
#include <QJsonParseError>
#include <QList>
#include <QMetaEnum>
#include <QPointF>
#include <QQmlEngine>
#include <QQuickStyle>
#include <QQuickWindow>
#include <QRect>
#include <QRectF>
#include <QScreen>

#include <algorithm>
#include <cmath>

namespace {

// One item as the scene graph paints it: after everything behind it.
struct Painted
{
    QQuickItem *item;
    qreal opacity; // its own multiplied by every ancestor's
};

bool isOpaque(qreal opacity)
{
    return opacity > 0.999;
}

// Items in painting order: children with a negative z, then the item, then
// its other children, each group in order of z and then of declaration.
void collectPainted(QQuickItem *item, qreal opacity, QList<Painted> &out)
{
    if (!item->isVisible())
        return;
    const qreal own = opacity * item->opacity();
    QList<QQuickItem *> children = item->childItems();
    std::stable_sort(children.begin(), children.end(),
                     [](const QQuickItem *a, const QQuickItem *b) { return a->z() < b->z(); });
    auto child = children.cbegin();
    for (; child != children.cend() && (*child)->z() < 0; ++child)
        collectPainted(*child, own, out);
    out.append(Painted { item, own });
    for (; child != children.cend(); ++child)
        collectPainted(*child, own, out);
}

QRectF sceneRectOf(const QQuickItem *item)
{
    return item->mapRectToScene(QRectF(0, 0, item->width(), item->height()));
}

bool isText(const QQuickItem *item)
{
    return item->inherits("QQuickText") || item->inherits("QQuickTextInput") || item->inherits("QQuickTextEdit");
}

bool isRectangle(const QQuickItem *item)
{
    return item->inherits("QQuickRectangle");
}

QString typeName(const QQuickItem *item)
{
    // "Button_QMLTYPE_12" and "QQuickRectangle" read better without the
    // decoration.
    QString name = QString::fromLatin1(item->metaObject()->className());
    const qsizetype cut = name.indexOf(QLatin1String("_QML"));
    if (cut > 0)
        name.truncate(cut);
    if (name.startsWith(QLatin1String("QQuick")))
        name.remove(0, 6);
    if (!item->objectName().isEmpty())
        name += QLatin1Char('#') + item->objectName();
    return name;
}

QString pathOf(const QQuickItem *item)
{
    QStringList parts;
    for (const QQuickItem *at = item; at && parts.size() < 6; at = at->parentItem())
        parts.prepend(typeName(at));
    return parts.join(QLatin1String(" > "));
}

QString shortened(QString text)
{
    text.replace(QLatin1Char('\n'), QLatin1Char(' '));
    if (text.size() > 48)
        text = text.left(45) + QLatin1String("...");
    return text;
}

// The colour of the topmost opaque Rectangle that is painted before
// `painted[index]` and covers `point`. Invalid if there is none.
QColor colourBehind(const QList<Painted> &painted, qsizetype index, const QPointF &point)
{
    for (qsizetype i = index - 1; i >= 0; --i) {
        const Painted &below = painted.at(i);
        if (!isRectangle(below.item) || !isOpaque(below.opacity))
            continue;
        const QColor fill = below.item->property("color").value<QColor>();
        if (!fill.isValid() || fill.alpha() != 255)
            continue;
        if (sceneRectOf(below.item).contains(point))
            return fill;
    }
    return QColor();
}

double channel(double value)
{
    return value <= 0.04045 ? value / 12.92 : std::pow((value + 0.055) / 1.055, 2.4);
}

double luminance(const QColor &colour)
{
    return 0.2126 * channel(colour.redF()) + 0.7152 * channel(colour.greenF()) + 0.0722 * channel(colour.blueF());
}

double ratio(const QColor &first, const QColor &second)
{
    double lighter = luminance(first);
    double darker = luminance(second);
    if (lighter < darker)
        std::swap(lighter, darker);
    return (lighter + 0.05) / (darker + 0.05);
}

QVariantMap finding(const QString &kind, const QQuickItem *item, const QString &text, const QColor &foreground,
                    const QColor &background, const QString &problem)
{
    QVariantMap out;
    out.insert(QStringLiteral("kind"), kind);
    out.insert(QStringLiteral("foreground"), foreground.name());
    out.insert(QStringLiteral("background"), background.isValid() ? background.name() : QString());
    out.insert(QStringLiteral("ratio"), problem.isEmpty() ? ratio(foreground, background) : 0.0);
    out.insert(QStringLiteral("exempt"), !item->isEnabled());
    out.insert(QStringLiteral("problem"), problem);
    out.insert(QStringLiteral("where"),
               text.isEmpty() ? pathOf(item) : pathOf(item) + QLatin1String(" \"") + shortened(text) + QLatin1Char('"'));
    return out;
}

void collectTabStops(QQuickItem *item, QVariantList &out)
{
    if (!item->isVisible() || !item->isEnabled())
        return;
    if (item->activeFocusOnTab())
        out.append(QVariant::fromValue(item));
    const QList<QQuickItem *> children = item->childItems();
    for (QQuickItem *child : children)
        collectTabStops(child, out);
}

void collectClipped(QQuickItem *item, QStringList &out)
{
    if (!item->isVisible())
        return;
    if (item->inherits("QQuickText")) {
        const QString text = item->property("text").toString();
        if (!text.isEmpty()) {
            const bool truncated = item->property("truncated").toBool();
            const bool tooWide = item->property("contentWidth").toReal() > item->width() + 1.0;
            const bool tooTall = item->property("contentHeight").toReal() > item->height() + 1.0;
            if (truncated || tooWide || tooTall) {
                out.append(pathOf(item) + QLatin1String(" \"") + shortened(text) + QLatin1String("\": ")
                           + (truncated ? QLatin1String("elided") : tooWide ? QLatin1String("wider than its room")
                                                                             : QLatin1String("taller than its room"))
                           + QStringLiteral(" (text %1 by %2 px in %3 by %4 px, %5 line(s))")
                                 .arg(item->property("contentWidth").toReal())
                                 .arg(item->property("contentHeight").toReal())
                                 .arg(item->width())
                                 .arg(item->height())
                                 .arg(item->property("lineCount").toInt()));
            }
        }
    }
    const QList<QQuickItem *> children = item->childItems();
    for (QQuickItem *child : children)
        collectClipped(child, out);
}

} // namespace

Harness::Harness(QQmlEngine *engine)
    : QObject(nullptr)
    , m_engine(engine)
{
    m_rightToLeft = QGuiApplication::layoutDirection() == Qt::RightToLeft;
}

Harness::~Harness()
{
    // The application outlives the engine of one test file: leave it as it
    // was found.
    if (m_translator)
        QCoreApplication::removeTranslator(m_translator.get());
    QGuiApplication::setLayoutDirection(Qt::LeftToRight);
}

Harness *Harness::create(QQmlEngine *qmlEngine, QJSEngine *)
{
    return new Harness(qmlEngine);
}

bool Harness::rightToLeft() const
{
    return m_rightToLeft;
}

void Harness::setRightToLeft(bool rightToLeft)
{
    QGuiApplication::setLayoutDirection(rightToLeft ? Qt::RightToLeft : Qt::LeftToRight);
    if (rightToLeft == m_rightToLeft)
        return;
    m_rightToLeft = rightToLeft;
    Q_EMIT rightToLeftChanged();
}

Harness::Pseudo Harness::pseudo() const
{
    return m_pseudo;
}

void Harness::setPseudo(Pseudo pseudo)
{
    if (pseudo == m_pseudo)
        return;
    m_pseudo = pseudo;

    // Installing a translator makes QGuiApplication work out the layout
    // direction again, but only if nobody set it; put back what the test
    // asked for.
    if (m_translator) {
        QCoreApplication::removeTranslator(m_translator.get());
        m_translator.reset();
    }
    if (pseudo != None) {
        m_translator = std::make_unique<PseudoTranslator>(pseudo == RtlLong);
        QCoreApplication::installTranslator(m_translator.get());
    }
    QGuiApplication::setLayoutDirection(m_rightToLeft ? Qt::RightToLeft : Qt::LeftToRight);
    if (m_engine)
        m_engine->retranslate();
    Q_EMIT pseudoChanged();
}

QString Harness::outputDir() const
{
    return qEnvironmentVariable("TUR_SCREENSHOT_DIR");
}

double Harness::devicePixelRatio() const
{
    return qGuiApp->devicePixelRatio();
}

QString Harness::scaleLabel() const
{
    return QString::number(devicePixelRatio(), 'g', 3) + QLatin1Char('x');
}

QString Harness::tokensFile() const
{
    return QStringLiteral(TUR_TOKENS_JSON);
}

QVariantMap Harness::environment() const
{
    QVariantMap out;
    out.insert(QStringLiteral("qt"), QString::fromLatin1(qVersion()));
    out.insert(QStringLiteral("platform"), QGuiApplication::platformName());
    out.insert(QStringLiteral("sceneGraphBackend"), QQuickWindow::sceneGraphBackend());
    out.insert(QStringLiteral("style"), QQuickStyle::name());
    out.insert(QStringLiteral("devicePixelRatio"), devicePixelRatio());
    if (const QScreen *screen = QGuiApplication::primaryScreen()) {
        out.insert(QStringLiteral("logicalDpi"), screen->logicalDotsPerInch());
        out.insert(QStringLiteral("screen"),
                   QStringLiteral("%1x%2").arg(screen->size().width()).arg(screen->size().height()));
    }
    const QFont font = QGuiApplication::font();
    out.insert(QStringLiteral("fontRequested"), font.family());
    out.insert(QStringLiteral("interfaceFont"), QFontInfo(font).family());
    out.insert(QStringLiteral("fontPointSize"), font.pointSizeF());
    out.insert(QStringLiteral("monoFont"), QFontInfo(QFont(QStringLiteral("JetBrains Mono"))).family());
    out.insert(QStringLiteral("accessibilityActive"), QAccessible::isActive());
    out.insert(QStringLiteral("layoutDirection"),
               QGuiApplication::layoutDirection() == Qt::RightToLeft ? QStringLiteral("rtl") : QStringLiteral("ltr"));
    return out;
}

double Harness::contrast(const QColor &first, const QColor &second) const
{
    return ratio(first, second);
}

QVariantList Harness::contrastFindings(QQuickItem *item, const QVariantList &decorative) const
{
    QVariantList out;
    if (!item || !item->window())
        return out;

    QList<QColor> ignored;
    for (const QVariant &colour : decorative)
        ignored.append(colour.value<QColor>());

    QList<Painted> painted;
    collectPainted(item->window()->contentItem(), 1.0, painted);

    for (qsizetype i = 0; i < painted.size(); ++i) {
        const QQuickItem *at = painted.at(i).item;
        const QRectF rect = sceneRectOf(at);
        if (rect.isEmpty())
            continue;

        if (isText(at)) {
            const QString text = at->property("text").toString();
            if (text.trimmed().isEmpty())
                continue;
            const QColor colour = at->property("color").value<QColor>();
            const QColor behind = colourBehind(painted, i, rect.center());
            QString problem;
            if (!isOpaque(painted.at(i).opacity) || colour.alpha() != 255)
                problem = QStringLiteral("the text is translucent");
            else if (!behind.isValid())
                problem = QStringLiteral("no opaque background was found behind the text");
            out.append(finding(QStringLiteral("text"), at, text, colour, behind, problem));
            continue;
        }

        if (isRectangle(at)) {
            const QObject *border = at->property("border").value<QObject *>();
            if (!border)
                continue;
            const qreal width = border->property("width").toReal();
            const QColor colour = border->property("color").value<QColor>();
            // A Rectangle that never set its border still reports a width of
            // one and opaque black. No token is black, and the boundary check
            // forbids colour literals, so that pair means "no border".
            const bool unset = qFuzzyCompare(width, 1.0) && colour == QColor(Qt::black);
            if (width <= 0 || unset || colour.alpha() != 255 || !isOpaque(painted.at(i).opacity))
                continue;
            if (ignored.contains(colour))
                continue;
            // What the border is seen against is what is behind its outer
            // edge; half-way down the left side stands for all of it.
            const QPointF edge(rect.left() + width / 2, rect.center().y());
            const QColor behind = colourBehind(painted, i, edge);
            const QString problem =
                behind.isValid() ? QString() : QStringLiteral("no opaque background was found behind the boundary");
            out.append(finding(QStringLiteral("boundary"), at, QString(), colour, behind, problem));
        }
    }
    return out;
}

QVariantList Harness::tabStops(QQuickItem *root) const
{
    QVariantList out;
    if (root)
        collectTabStops(root, out);
    return out;
}

QVariantMap Harness::accessible(QQuickItem *item) const
{
    QVariantMap out;
    QAccessibleInterface *iface = item ? QAccessible::queryAccessibleInterface(item) : nullptr;
    const bool valid = iface && iface->isValid();
    out.insert(QStringLiteral("valid"), valid);
    if (!valid)
        return out;
    const QAccessible::Role role = iface->role();
    const QAccessible::State state = iface->state();
    out.insert(QStringLiteral("name"), iface->text(QAccessible::Name));
    out.insert(QStringLiteral("description"), iface->text(QAccessible::Description));
    out.insert(QStringLiteral("role"), int(role));
    out.insert(QStringLiteral("roleName"), QString::fromLatin1(QMetaEnum::fromType<QAccessible::Role>().valueToKey(role)));
    out.insert(QStringLiteral("focusable"), bool(state.focusable));
    out.insert(QStringLiteral("checkable"), bool(state.checkable));
    out.insert(QStringLiteral("checked"), bool(state.checked));
    out.insert(QStringLiteral("disabled"), bool(state.disabled));
    return out;
}

QStringList Harness::clippedText(QQuickItem *root) const
{
    QStringList out;
    if (root)
        collectClipped(root, out);
    return out;
}

QVariantMap Harness::sceneRect(QQuickItem *item) const
{
    QVariantMap out;
    if (!item)
        return out;
    const QRectF rect = sceneRectOf(item);
    out.insert(QStringLiteral("x"), rect.x());
    out.insert(QStringLiteral("y"), rect.y());
    out.insert(QStringLiteral("width"), rect.width());
    out.insert(QStringLiteral("height"), rect.height());
    return out;
}

QVariantMap Harness::grab(QQuickItem *item, const QString &file) const
{
    QVariantMap out;
    out.insert(QStringLiteral("saved"), false);
    QQuickWindow *window = item ? item->window() : nullptr;
    if (!window || window->width() <= 0)
        return out;

    const QImage whole = window->grabWindow();
    if (whole.isNull())
        return out;

    // The picture is in device pixels. The software renderer does not tell
    // the image its pixel ratio, so take it from the sizes.
    const qreal ratio = qreal(whole.width()) / qreal(window->width());
    const QRectF logical = sceneRectOf(item);
    const QRect pixels = QRectF(logical.x() * ratio, logical.y() * ratio, logical.width() * ratio,
                                logical.height() * ratio)
                             .toAlignedRect()
                             .intersected(whole.rect());
    QImage picture = whole.copy(pixels);
    picture.setDevicePixelRatio(1.0);

    out.insert(QStringLiteral("ratio"), ratio);
    out.insert(QStringLiteral("width"), picture.width());
    out.insert(QStringLiteral("height"), picture.height());
    out.insert(QStringLiteral("windowWidth"), whole.width());
    out.insert(QStringLiteral("windowHeight"), whole.height());
    if (!file.isEmpty())
        out.insert(QStringLiteral("saved"), picture.save(file));
    return out;
}

QString Harness::describe(QQuickItem *item) const
{
    return item ? pathOf(item) : QStringLiteral("(no item)");
}

QVariant Harness::readJson(const QString &path) const
{
    QFile file(path);
    if (!file.open(QIODevice::ReadOnly))
        return QVariant();
    QJsonParseError error;
    const QJsonDocument document = QJsonDocument::fromJson(file.readAll(), &error);
    if (error.error != QJsonParseError::NoError)
        return QVariant();
    return document.toVariant();
}
