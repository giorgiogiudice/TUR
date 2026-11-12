// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#include "pseudotranslator.h"

#include <QChar>
#include <QStringList>

#include <cmath>
#include <cstring>

namespace {

// How much longer a translation may be than its English source.
constexpr double Expansion = 1.4;

// One Hebrew letter for each Latin letter, a to z. The result is not Hebrew;
// it is text that a layout engine must treat as right-to-left.
constexpr char16_t Hebrew[26] = {
    u'א', u'ב', u'צ', u'ד', u'ע', u'פ', u'ג', u'ה', u'י',
    u'ז', u'ק', u'ל', u'מ', u'נ', u'ו', u'פ', u'ק', u'ר',
    u'ס', u'ת', u'ו', u'ב', u'ש', u'ס', u'י', u'ז',
};

QChar mapped(QChar ch, bool rightToLeft)
{
    if (!rightToLeft)
        return ch;
    const char16_t code = ch.toLower().unicode();
    if (code >= u'a' && code <= u'z')
        return QChar(Hebrew[code - u'a']);
    return ch;
}

} // namespace

PseudoTranslator::PseudoTranslator(bool rightToLeft, QObject *parent)
    : QTranslator(parent)
    , m_rightToLeft(rightToLeft)
{
}

bool PseudoTranslator::isEmpty() const
{
    // An empty translator is not consulted.
    return false;
}

QString PseudoTranslator::translate(const char *context, const char *sourceText, const char *, int) const
{
    // QGuiApplication asks its translators which way the language runs.
    if (context && sourceText && std::strcmp(context, "QGuiApplication") == 0
        && std::strcmp(sourceText, "QT_LAYOUT_DIRECTION") == 0) {
        return m_rightToLeft ? QStringLiteral("RTL") : QStringLiteral("LTR");
    }
    return transform(QString::fromUtf8(sourceText), m_rightToLeft);
}

QString PseudoTranslator::transform(const QString &source, bool rightToLeft)
{
    if (source.isEmpty())
        return source;

    QString out;
    out.reserve(source.size() * 2);
    int letters = 0;
    for (qsizetype i = 0; i < source.size(); ++i) {
        const QChar ch = source.at(i);
        if (ch == u'%' && i + 1 < source.size()) {
            // A placeholder: %1 to %99, %n, %L1. Copy it as it is.
            qsizetype j = i + 1;
            if (source.at(j) == u'L')
                ++j;
            if (j < source.size() && (source.at(j).isDigit() || source.at(j) == u'n')) {
                while (j < source.size() && (source.at(j).isDigit() || source.at(j) == u'n'))
                    ++j;
                out += source.mid(i, j - i);
                i = j - 1;
                continue;
            }
        }
        if (ch.isLetter())
            ++letters;
        out += mapped(ch, rightToLeft);
    }

    // Lengthen with whole words, so that the text wraps where real text would.
    const QStringList filler = { QStringLiteral("lorem"), QStringLiteral("ipsum"), QStringLiteral("dolor"),
                                 QStringLiteral("sit"), QStringLiteral("amet") };
    const int wanted = int(std::ceil(letters * Expansion));
    int have = letters;
    for (int word = 0; have < wanted; ++word) {
        const QString &next = filler.at(word % filler.size());
        out += QLatin1Char(' ');
        for (const QChar ch : next)
            out += mapped(ch, rightToLeft);
        have += int(next.size());
    }
    return out;
}
