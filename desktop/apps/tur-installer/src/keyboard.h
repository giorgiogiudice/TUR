// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#pragma once

#include <QObject>
#include <QString>
#include <QVariantList>
#include <QtQml/qqmlregistration.h>

// The keyboard layout of the live session (desktop-experience.md, 11.5,
// screen 1). The list is xkeyboard-config's (rules/evdev.lst). Choosing one
// writes the [Layout] group of the live user's kxkbrc and tells KWin through
// KConfig's own change notification (org.kde.kconfig.notify, ConfigChanged,
// on /kxkbrc), which KWin's keyboard layout follows. No KDE library is linked
// for it: the file is plain, and the signal is sent with Qt D-Bus.
//
// The choice applies to the live session, and so to the confirmation phrase
// typed in it. The installed system asks for its keyboard at its first start.
class KeyboardLayouts : public QObject
{
    Q_OBJECT
    QML_ELEMENT

    // [{ code, name }], by name.
    Q_PROPERTY(QVariantList layouts READ layouts CONSTANT FINAL)
    Q_PROPERTY(QString current READ current NOTIFY currentChanged FINAL)
    // Where kxkbrc is written: the user's configuration directory unless set.
    Q_PROPERTY(QString configDirectory READ configDirectory WRITE setConfigDirectory NOTIFY configDirectoryChanged FINAL)
    // The list file; xkeyboard-config's unless set.
    Q_PROPERTY(QString rulesFile READ rulesFile WRITE setRulesFile NOTIFY layoutsChanged FINAL)

public:
    explicit KeyboardLayouts(QObject *parent = nullptr);

    QVariantList layouts() const { return m_layouts; }
    QString current() const { return m_current; }
    QString configDirectory() const;
    void setConfigDirectory(const QString &directory);
    QString rulesFile() const { return m_rulesFile; }
    void setRulesFile(const QString &file);

    // The index of `code` in `layouts`, or -1.
    Q_INVOKABLE int indexOf(const QString &code) const;
    // Makes `code` the layout. False if it is not one of `layouts` or the
    // file could not be written.
    Q_INVOKABLE bool apply(const QString &code);

    // The [Layout] group for one layout, as written. For tests.
    static QString layoutGroup(const QString &code);
    // `text` with its [Layout] group replaced by `group`; other groups kept.
    static QString withGroup(const QString &text, const QString &group);
    // The layouts of an evdev.lst, { code, name }, by name.
    static QVariantList parseRules(const QString &text);

Q_SIGNALS:
    void currentChanged();
    void configDirectoryChanged();
    void layoutsChanged();

private:
    void load();

    QVariantList m_layouts;
    QString m_current = QStringLiteral("us");
    QString m_configDirectory;
    QString m_rulesFile = QStringLiteral("/usr/share/X11/xkb/rules/evdev.lst");
};
