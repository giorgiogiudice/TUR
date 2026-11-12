// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#pragma once

#include <QByteArray>
#include <QHash>
#include <QObject>
#include <QString>
#include <QVariantMap>
#include <QtQml/qqmlregistration.h>

// What the system says it is, for Overview and About: the two files the image
// build writes (image/Containerfile).
//
//   /usr/lib/tur/release   TUR_VERSION, TUR_BUILD_ID, TUR_SOURCE_REVISION,
//                          TUR_BASE_IMAGE, TUR_BASE_DIGEST, TUR_BASE_VERSION
//   os-release             PRETTY_NAME, ID  (/etc/os-release, else
//                          /usr/lib/os-release, as os-release(5) says)
//
// It reads; it changes nothing and runs nothing. With `fixture` set it reads
// DIR/tur-release and DIR/os-release instead, so that the pages can be built,
// tested and photographed where there is no TUR system (design, 15.2).
//
// A value that is not in the file is empty, and the page says "Not recorded".
// Nothing is filled in.
class SystemFacts : public QObject
{
    Q_OBJECT
    QML_ELEMENT

    Q_PROPERTY(QString fixture READ fixture WRITE setFixture NOTIFY changed FINAL)

    Q_PROPERTY(QString version READ version NOTIFY changed FINAL)
    Q_PROPERTY(QString build READ build NOTIFY changed FINAL)
    Q_PROPERTY(QString sourceRevision READ sourceRevision NOTIFY changed FINAL)
    Q_PROPERTY(QString baseImage READ baseImage NOTIFY changed FINAL)
    Q_PROPERTY(QString baseDigest READ baseDigest NOTIFY changed FINAL)
    Q_PROPERTY(QString baseVersion READ baseVersion NOTIFY changed FINAL)

    // PRETTY_NAME and ID of os-release.
    Q_PROPERTY(QString systemName READ systemName NOTIFY changed FINAL)
    Q_PROPERTY(QString systemId READ systemId NOTIFY changed FINAL)

    // The files that were read, or would have been.
    Q_PROPERTY(QString releasePath READ releasePath NOTIFY changed FINAL)
    Q_PROPERTY(QString osReleasePath READ osReleasePath NOTIFY changed FINAL)

    // Empty when TUR's release file was read. Otherwise the content of the
    // kit's two-layer message: code, title, reason, detail.
    Q_PROPERTY(QVariantMap failure READ failure NOTIFY changed FINAL)

    // The versions of this program and of the Qt it runs on.
    Q_PROPERTY(QString programVersion READ programVersion CONSTANT FINAL)
    Q_PROPERTY(QString qtVersion READ qtVersion CONSTANT FINAL)

public:
    explicit SystemFacts(QObject *parent = nullptr);

    QString fixture() const { return m_fixture; }
    void setFixture(const QString &fixture);

    QString version() const { return m_release.value(QStringLiteral("TUR_VERSION")); }
    QString build() const { return m_release.value(QStringLiteral("TUR_BUILD_ID")); }
    QString sourceRevision() const { return m_release.value(QStringLiteral("TUR_SOURCE_REVISION")); }
    QString baseImage() const { return m_release.value(QStringLiteral("TUR_BASE_IMAGE")); }
    QString baseDigest() const { return m_release.value(QStringLiteral("TUR_BASE_DIGEST")); }
    QString baseVersion() const { return m_release.value(QStringLiteral("TUR_BASE_VERSION")); }
    QString systemName() const { return m_os.value(QStringLiteral("PRETTY_NAME")); }
    QString systemId() const { return m_os.value(QStringLiteral("ID")); }
    QString releasePath() const { return m_releasePath; }
    QString osReleasePath() const { return m_osPath; }
    QVariantMap failure() const { return m_failure; }
    QString programVersion() const;
    QString qtVersion() const;

    Q_INVOKABLE void reload();

    // The KEY=VALUE lines of a file in the syntax of os-release(5): comments
    // begin with '#'; a value may be in double quotes, with backslash
    // escapes, or in single quotes. A line that is not of that form is left
    // out. A key that occurs twice keeps its last value, as a shell would.
    static QHash<QString, QString> parse(const QByteArray &text);

    static constexpr int MaximumFileSize = 64 * 1024;

Q_SIGNALS:
    void changed();

private:
    QString m_fixture;
    QHash<QString, QString> m_release;
    QHash<QString, QString> m_os;
    QString m_releasePath;
    QString m_osPath;
    QVariantMap m_failure;
};
