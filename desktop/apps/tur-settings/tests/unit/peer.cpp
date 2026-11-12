// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
//
// A stand-in for another TUR program with addressable content, for the tests
// of tur-settings: it owns org.tur.Example on the session bus and answers
// org.tur.Addressable1 at /org/tur/Addressable1, the way such a program is
// to (dbus/org.tur.Addressable1.xml). tur-settings, asked for an address of
// tests/data/registry/org.tur.Example.json, passes the call on to it.
//
// What it answers is fixed, so that a test can tell that the answer came
// from here: the rectangle (1, 2, 3, 4) and the process of this program. An
// address it does not have is refused with the error a real program gives.
//
// Not part of tur-settings and not shipped.

#include <QCoreApplication>
#include <QDBusAbstractAdaptor>
#include <QDBusArgument>
#include <QDBusConnection>
#include <QDBusMessage>
#include <QDBusMetaType>
#include <QObject>
#include <QString>
#include <QTextStream>
#include <QVariantMap>

struct Rect
{
    double x = 0;
    double y = 0;
    double width = 0;
    double height = 0;
};
Q_DECLARE_METATYPE(Rect)

QDBusArgument &operator<<(QDBusArgument &argument, const Rect &rect)
{
    argument.beginStructure();
    argument << rect.x << rect.y << rect.width << rect.height;
    argument.endStructure();
    return argument;
}

const QDBusArgument &operator>>(const QDBusArgument &argument, Rect &rect)
{
    argument.beginStructure();
    argument >> rect.x >> rect.y >> rect.width >> rect.height;
    argument.endStructure();
    return argument;
}

class Addressable : public QDBusAbstractAdaptor
{
    Q_OBJECT
    Q_CLASSINFO("D-Bus Interface", "org.tur.Addressable1")

public:
    explicit Addressable(QObject *parent)
        : QDBusAbstractAdaptor(parent)
    {
    }

public Q_SLOTS:
    QVariantMap Reveal(const QString &address, const QVariantMap &options, const QDBusMessage &message)
    {
        if (!address.startsWith(QLatin1String("tur.example"))) {
            message.setDelayedReply(true);
            QDBusConnection::sessionBus().send(
                message.createErrorReply(QStringLiteral("org.tur.Settings1.Error.UnknownAddress"),
                                         QStringLiteral("TUR-DESKTOP-SETTINGS-001: the example program has no ")
                                             + address));
            return QVariantMap();
        }
        QVariantMap result;
        result.insert(QStringLiteral("address"), address);
        result.insert(QStringLiteral("program"), QStringLiteral("org.tur.Example"));
        result.insert(QStringLiteral("located"), true);
        result.insert(QStringLiteral("rect"), QVariant::fromValue(Rect { 1, 2, 3, 4 }));
        result.insert(QStringLiteral("highlighted"), options.value(QStringLiteral("highlight"), true).toBool());
        result.insert(QStringLiteral("pid"), uint(QCoreApplication::applicationPid()));
        result.insert(QStringLiteral("app_id"), QStringLiteral("org.tur.Example"));
        result.insert(QStringLiteral("window_title"), QStringLiteral("Example"));
        return result;
    }

    void ClearHighlight() { }

    QString List() { return QStringLiteral("{}"); }

Q_SIGNALS:
    void RowChanged(const QString &address);
};

int main(int argc, char *argv[])
{
    QCoreApplication app(argc, argv);
    qDBusRegisterMetaType<Rect>();

    QDBusConnection bus = QDBusConnection::sessionBus();
    QObject object;
    new Addressable(&object);
    if (!bus.isConnected() || !bus.registerObject(QStringLiteral("/org/tur/Addressable1"), &object)
        || !bus.registerService(QStringLiteral("org.tur.Example"))) {
        QTextStream(stderr) << "tst_settings_peer: could not take org.tur.Example on the session bus\n";
        return 1;
    }
    return app.exec();
}

#include "peer.moc"
