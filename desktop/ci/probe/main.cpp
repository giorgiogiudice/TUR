// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
//
// tur-probe: the one-page program of the toolchain proof (task D-01). It
// shows a page, says what it is running on as one line of JSON, and can save
// a picture of what it drew. desktop/ci/probe.sh runs it in the ways that
// settle the assumptions of desktop-experience.md, section 16. It is a CI
// tool and is not installed in the image.

#include "turkit.h"

#include <QCommandLineOption>
#include <QCommandLineParser>
#include <QCoreApplication>
#include <QFont>
#include <QFontDatabase>
#include <QFontInfo>
#include <QGuiApplication>
#include <QImage>
#include <QJSValue>
#include <QJsonDocument>
#include <QJsonObject>
#include <QJsonValue>
#include <QList>
#include <QQmlEngine>
#include <QQmlError>
#include <QQuickItem>
#include <QQuickStyle>
#include <QQuickView>
#include <QQuickWindow>
#include <QString>
#include <QStyleHints>
#include <QTextStream>
#include <QTimer>
#include <QVariant>
#include <QVariantMap>

namespace {

QString schemeName(Qt::ColorScheme scheme)
{
    switch (scheme) {
    case Qt::ColorScheme::Dark:
        return QStringLiteral("dark");
    case Qt::ColorScheme::Light:
        return QStringLiteral("light");
    default:
        return QStringLiteral("unknown");
    }
}

} // namespace

int main(int argc, char *argv[])
{
    QGuiApplication app(argc, argv);
    QCoreApplication::setApplicationName(QStringLiteral("tur-probe"));

    QCommandLineParser parser;
    parser.setApplicationDescription(QStringLiteral("TUR's toolchain proof: shows one page and reports what drew it."));
    parser.addHelpOption();
    const QCommandLineOption standalone(QStringLiteral("standalone"),
                                        QStringLiteral("Use the look of TUR's programs outside a session."));
    const QCommandLineOption page(QStringLiteral("page"), QStringLiteral("The page to show: Probe or BreezeProbe."),
                                  QStringLiteral("name"), QStringLiteral("Probe"));
    const QCommandLineOption grab(QStringLiteral("grab"), QStringLiteral("Save a picture of the page to this file."),
                                  QStringLiteral("file"));
    const QCommandLineOption strict(QStringLiteral("strict"), QStringLiteral("Fail if QML reported a warning."));
    parser.addOptions({ standalone, page, grab, strict });
    parser.process(app);

    // What the platform theme chose before this program chose anything:
    // under Plasma a QGuiApplication is given org.kde.breeze.
    const QString styleAtStart = QQuickStyle::name();
    if (parser.isSet(standalone))
        TurKit::useStandaloneLook();

    QQuickView view;
    int warnings = 0;
    QObject::connect(view.engine(), &QQmlEngine::warnings, &app, [&warnings](const QList<QQmlError> &list) {
        warnings += int(list.size());
    });
    view.loadFromModule("Tur.Probe", parser.value(page));
    if (view.status() != QQuickView::Ready) {
        QTextStream err(stderr);
        const QList<QQmlError> errors = view.errors();
        for (const QQmlError &error : errors)
            err << "tur-probe: " << error.toString() << '\n';
        return 2;
    }
    view.show();

    const QString grabFile = parser.value(grab);
    const bool failOnWarnings = parser.isSet(strict);
    QTimer::singleShot(700, &app, [&] {
        QJsonObject facts;
        facts.insert(QStringLiteral("page"), parser.value(page));
        facts.insert(QStringLiteral("qt"), QString::fromLatin1(qVersion()));
        facts.insert(QStringLiteral("platform"), QGuiApplication::platformName());
        facts.insert(QStringLiteral("platformThemeRequested"), qEnvironmentVariable("QT_QPA_PLATFORMTHEME"));
        facts.insert(QStringLiteral("styleAtStart"), styleAtStart);
        facts.insert(QStringLiteral("style"), QQuickStyle::name());
        facts.insert(QStringLiteral("sceneGraphBackend"), QQuickWindow::sceneGraphBackend());
        facts.insert(QStringLiteral("colorScheme"), schemeName(QGuiApplication::styleHints()->colorScheme()));
        const QFont font = QGuiApplication::font();
        facts.insert(QStringLiteral("fontRequested"), font.family());
        facts.insert(QStringLiteral("font"), QFontInfo(font).family());
        facts.insert(QStringLiteral("fontPointSize"), font.pointSizeF());
        facts.insert(QStringLiteral("fixedFont"),
                     QFontInfo(QFontDatabase::systemFont(QFontDatabase::FixedFont)).family());
        facts.insert(QStringLiteral("qmlWarnings"), warnings);

        if (const QQuickItem *root = view.rootObject()) {
            QVariant extra = root->property("facts");
            if (extra.userType() == qMetaTypeId<QJSValue>())
                extra = extra.value<QJSValue>().toVariant();
            const QVariantMap map = extra.toMap();
            for (auto it = map.cbegin(); it != map.cend(); ++it)
                facts.insert(it.key(), QJsonValue::fromVariant(it.value()));
        }

        if (!grabFile.isEmpty()) {
            const QImage image = view.grabWindow();
            const bool saved = !image.isNull() && image.save(grabFile);
            facts.insert(QStringLiteral("grab"),
                         saved ? QStringLiteral("%1x%2").arg(image.width()).arg(image.height())
                               : QStringLiteral("FAILED"));
        }

        QTextStream(stdout) << "TUR-PROBE " << QJsonDocument(facts).toJson(QJsonDocument::Compact) << '\n';
        app.exit(failOnWarnings && warnings > 0 ? 3 : 0);
    });
    return app.exec();
}
