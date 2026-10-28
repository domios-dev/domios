use super::SqliteMemStore;
use crate::backend::{ExportCursor, ExportPage, StoreRow, StoreTable, StoreValue, TableCounts};
use rusqlite::{
    types::{Value, ValueRef},
    Connection, OptionalExtension,
};
use std::{
    collections::BTreeMap,
    path::{Component, Path},
};

impl StoreTable {
    fn sqlite_name(self) -> &'static str {
        match self {
            Self::Items => "mem_items",
            Self::History => "mem_item_history",
            Self::Entities => "entities",
            Self::Relations => "relations",
            Self::Aliases => "entity_aliases",
            Self::Pins => "graph_pinned_positions",
            Self::Fts => "mem_fts",
            Self::Embedder => "mem_embedder",
            Self::Vectors => "mem_vectors",
            Self::SolutionMetadata => "solution_metadata",
            Self::SourceTombstones => "mem_source_tombstones",
            Self::PrecompactPurges => "mem_precompact_purges",
            Self::PrecompactBlobs => "mem_precompact_purge_blobs",
        }
    }
    fn order_key(self) -> &'static str {
        match self {
            Self::Items | Self::History | Self::Entities | Self::Relations => "id",
            Self::Aliases => "alias_id",
            Self::Pins | Self::Vectors => "mem_id",
            Self::Fts => "rowid",
            Self::Embedder => "singleton",
            Self::SolutionMetadata | Self::PrecompactPurges => "item_id",
            Self::SourceTombstones => "source_key",
            Self::PrecompactBlobs => "item_id, path",
        }
    }
}

fn exists(conn: &Connection, table: StoreTable) -> Result<bool, String> {
    conn.query_row(
        "SELECT 1 FROM sqlite_master WHERE name=?1",
        [table.sqlite_name()],
        |_| Ok(true),
    )
    .optional()
    .map(|v| v.unwrap_or(false))
    .map_err(|e| e.to_string())
}

fn value(value: ValueRef<'_>) -> Result<StoreValue, String> {
    Ok(match value {
        ValueRef::Null => StoreValue::Null,
        ValueRef::Integer(n) => StoreValue::Integer(n),
        ValueRef::Real(n) => StoreValue::Real(n),
        ValueRef::Blob(bytes) => StoreValue::Bytes(bytes.to_vec()),
        ValueRef::Text(bytes) => StoreValue::Text(
            std::str::from_utf8(bytes)
                .map_err(|e| e.to_string())?
                .to_owned(),
        ),
    })
}

fn blob_path(root: &Path, name: &str) -> Result<std::path::PathBuf, String> {
    let mut parts = Path::new(name).components();
    if !matches!(parts.next(), Some(Component::Normal(_))) || parts.next().is_some() {
        return Err("invalid memory blob name".into());
    }
    Ok(root.join(name))
}

impl SqliteMemStore {
    pub(super) fn export_rows(
        &self,
        after: Option<&ExportCursor>,
        limit: usize,
    ) -> Result<ExportPage, String> {
        if limit == 0 || limit > 10_000 {
            return Err("export page limit must be 1 to 10000".into());
        }
        let table = after.map_or(StoreTable::Items, |c| c.table);
        let offset = after.map_or(0, |c| c.offset);
        let conn = self.conn.lock();
        let mut rows = Vec::new();
        if exists(&conn, table)? {
            let projection = if table == StoreTable::Fts {
                "rowid, *"
            } else {
                "*"
            };
            let sql = format!(
                "SELECT {projection} FROM {} ORDER BY {} LIMIT ?1 OFFSET ?2",
                table.sqlite_name(),
                table.order_key()
            );
            let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
            let columns: Vec<_> = stmt.column_names().into_iter().map(str::to_owned).collect();
            let mut cursor = stmt
                .query(rusqlite::params![
                    limit as i64 + 1,
                    i64::try_from(offset).map_err(|e| e.to_string())?
                ])
                .map_err(|e| e.to_string())?;
            while let Some(row) = cursor.next().map_err(|e| e.to_string())? {
                let mut values = BTreeMap::new();
                for (i, name) in columns.iter().enumerate() {
                    values.insert(
                        name.clone(),
                        value(row.get_ref(i).map_err(|e| e.to_string())?)?,
                    );
                }
                let mut blobs = BTreeMap::new();
                if let Some(StoreValue::Text(name)) = values.get("body_blob_path") {
                    blobs.insert(
                        name.clone(),
                        std::fs::read(blob_path(&self.blobs_dir, name)?)
                            .map_err(|e| e.to_string())?,
                    );
                }
                rows.push(StoreRow { values, blobs });
            }
        }
        let next = if rows.len() > limit {
            rows.pop();
            Some(ExportCursor {
                table,
                offset: offset + rows.len() as u64,
            })
        } else {
            StoreTable::ALL
                .iter()
                .position(|t| *t == table)
                .and_then(|i| StoreTable::ALL.get(i + 1))
                .map(|table| ExportCursor {
                    table: *table,
                    offset: 0,
                })
        };
        Ok(ExportPage { table, rows, next })
    }

    pub(super) fn import_rows(&self, page: &ExportPage) -> Result<(), String> {
        if page.rows.is_empty() {
            return Ok(());
        }
        let conn = self.conn.lock();
        let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
        if page.table == StoreTable::Embedder {
            if page.rows.len() != 1 {
                return Err("expected one embedder stamp".into());
            }
            let row = &page.rows[0].values;
            let text = |key: &str| match row.get(key) {
                Some(StoreValue::Text(v)) => Ok(v.clone()),
                _ => Err(format!("missing {key}")),
            };
            let dimension = match row.get("dimension") {
                Some(StoreValue::Integer(n)) => usize::try_from(*n).map_err(|e| e.to_string())?,
                _ => return Err("missing vector dimension".into()),
            };
            tx.execute_batch(crate::embedder::index::STAMP_SCHEMA)
                .map_err(|e| e.to_string())?;
            crate::embedder::pending::init(&tx)?;
            crate::embedder::index::recreate_in(
                &tx,
                &crate::embedder::EmbedderIdentity {
                    provider: text("provider")?,
                    model: text("model")?,
                    dimension,
                },
            )?;
        }
        let sql = format!("SELECT * FROM {} LIMIT 0", page.table.sqlite_name());
        let stmt = tx.prepare(&sql).map_err(|e| e.to_string())?;
        let mut columns: Vec<_> = stmt.column_names().into_iter().map(str::to_owned).collect();
        drop(stmt);
        if page.table == StoreTable::Fts {
            columns.push("rowid".into());
        }
        for row in &page.rows {
            if row.values.len() != columns.len()
                || !columns.iter().all(|name| row.values.contains_key(name))
            {
                return Err(format!("invalid columns for {:?}", page.table));
            }
            for (name, bytes) in &row.blobs {
                let path = blob_path(&self.blobs_dir, name)?;
                if path.exists() {
                    if std::fs::read(&path).map_err(|e| e.to_string())? != *bytes {
                        return Err("memory blob content conflict".into());
                    }
                } else {
                    std::fs::write(path, bytes).map_err(|e| e.to_string())?;
                }
            }
            let columns = row
                .values
                .keys()
                .map(|name| format!("\"{name}\""))
                .collect::<Vec<_>>()
                .join(",");
            let placeholders = vec!["?"; row.values.len()].join(",");
            let verb = if page.table == StoreTable::Embedder {
                "INSERT OR REPLACE"
            } else {
                "INSERT"
            };
            let sql = format!(
                "{verb} INTO {} ({columns}) VALUES ({placeholders})",
                page.table.sqlite_name()
            );
            let values = row.values.values().map(|v| match v {
                StoreValue::Null => Value::Null,
                StoreValue::Integer(n) => Value::Integer(*n),
                StoreValue::Real(n) => Value::Real(*n),
                StoreValue::Text(s) => Value::Text(s.clone()),
                StoreValue::Bytes(b) => Value::Blob(b.clone()),
            });
            tx.execute(&sql, rusqlite::params_from_iter(values))
                .map_err(|e| e.to_string())?;
        }
        tx.commit().map_err(|e| e.to_string())
    }

    pub(super) fn counts(&self) -> Result<TableCounts, String> {
        let conn = self.conn.lock();
        StoreTable::ALL
            .into_iter()
            .map(|table| {
                let count = if exists(&conn, table)? {
                    conn.query_row(
                        &format!("SELECT COUNT(*) FROM {}", table.sqlite_name()),
                        [],
                        |r| r.get(0),
                    )
                    .map_err(|e| e.to_string())?
                } else {
                    0
                };
                Ok((table, count))
            })
            .collect()
    }
}
