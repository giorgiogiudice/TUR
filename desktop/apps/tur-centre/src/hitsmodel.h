// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#pragma once

#include "answers.h"

#include <QAbstractListModel>
#include <QByteArray>
#include <QHash>
#include <QList>
#include <QString>
#include <QVariantMap>
#include <QtQml/qqmlregistration.h>

// The rows of a search: one for each application the catalogue found, in
// the catalogue's order (desktop-experience.md, section 8.3: "Order is the
// catalogue's"). A row is shown as soon as the search answers; what the
// router says about the application arrives afterwards and fills the row in,
// so that results never wait for the slowest answer.
class HitsModel : public QAbstractListModel
{
    Q_OBJECT
    QML_ELEMENT
    QML_UNCREATABLE("The rows of a search belong to the catalogue's search.")

    Q_PROPERTY(int count READ count NOTIFY countChanged FINAL)

public:
    enum Role {
        IdRole = Qt::UserRole + 1,
        NameRole,
        PublisherRole,
        CategoryRole,
        // False until the catalogue's entry and the router's answer for the
        // application have arrived.
        DetailedRole,
        // The catalogue's entry (`show`), or an empty map.
        EntryRole,
        // The option the router recommends for this machine, or an empty map.
        RecommendedRole,
    };

    explicit HitsModel(QObject *parent = nullptr);

    int rowCount(const QModelIndex &parent = QModelIndex()) const override;
    QVariant data(const QModelIndex &index, int role) const override;
    QHash<int, QByteArray> roleNames() const override;

    int count() const;

    void reset(const QList<Answers::Hit> &hits);
    void setDetails(const QString &id, const QVariantMap &entry, const QVariantMap &recommended);
    QStringList ids() const;

Q_SIGNALS:
    void countChanged();

private:
    struct Row
    {
        Answers::Hit hit;
        bool detailed = false;
        QVariantMap entry;
        QVariantMap recommended;
    };

    QList<Row> m_rows;
};
