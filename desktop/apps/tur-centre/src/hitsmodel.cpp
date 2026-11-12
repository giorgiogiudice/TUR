// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
#include "hitsmodel.h"

HitsModel::HitsModel(QObject *parent)
    : QAbstractListModel(parent)
{
}

int HitsModel::rowCount(const QModelIndex &parent) const
{
    return parent.isValid() ? 0 : int(m_rows.size());
}

QVariant HitsModel::data(const QModelIndex &index, int role) const
{
    if (!index.isValid() || index.row() < 0 || index.row() >= m_rows.size())
        return QVariant();
    const Row &row = m_rows.at(index.row());
    switch (role) {
    case IdRole:
        return row.hit.id;
    case NameRole:
    case Qt::DisplayRole:
        return row.hit.name;
    case PublisherRole:
        return row.hit.publisher;
    case CategoryRole:
        return row.hit.category;
    case DetailedRole:
        return row.detailed;
    case EntryRole:
        return row.entry;
    case RecommendedRole:
        return row.recommended;
    default:
        return QVariant();
    }
}

QHash<int, QByteArray> HitsModel::roleNames() const
{
    return {
        { IdRole, "applicationId" },
        { NameRole, "name" },
        { PublisherRole, "publisher" },
        { CategoryRole, "category" },
        { DetailedRole, "detailed" },
        { EntryRole, "entry" },
        { RecommendedRole, "recommended" },
    };
}

int HitsModel::count() const
{
    return int(m_rows.size());
}

void HitsModel::reset(const QList<Answers::Hit> &hits)
{
    const bool changed = hits.size() != m_rows.size();
    beginResetModel();
    m_rows.clear();
    m_rows.reserve(hits.size());
    for (const Answers::Hit &hit : hits) {
        Row row;
        row.hit = hit;
        m_rows.append(row);
    }
    endResetModel();
    if (changed)
        Q_EMIT countChanged();
}

void HitsModel::setDetails(const QString &id, const QVariantMap &entry, const QVariantMap &recommended)
{
    for (qsizetype i = 0; i < m_rows.size(); ++i) {
        Row &row = m_rows[i];
        if (row.hit.id != id)
            continue;
        row.detailed = true;
        row.entry = entry;
        row.recommended = recommended;
        const QModelIndex at = index(int(i));
        Q_EMIT dataChanged(at, at, { DetailedRole, EntryRole, RecommendedRole });
        return;
    }
}

QStringList HitsModel::ids() const
{
    QStringList out;
    for (const Row &row : m_rows)
        out.append(row.hit.id);
    return out;
}
