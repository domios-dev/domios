use crate::{db_error, vectors, PostgresMemStore, StoreResult};
use codeterm_mem::backend::{
    ExportCursor, ExportPage, StoreRow, StoreTable, StoreValue, TableCounts,
};
use postgres::{
    types::{ToSql, Type},
    Row, Transaction,
};
use std::collections::BTreeMap;

pub(crate) fn name(table: StoreTable) -> &'static str {
    match table {
        StoreTable::Items => "mem_items",
        StoreTable::History => "mem_item_history",
        StoreTable::Entities => "entities",
        StoreTable::Relations => "relations",
        StoreTable::Aliases => "entity_aliases",
        StoreTable::Pins => "graph_pinned_positions",
        StoreTable::Fts => "mem_fts",
        StoreTable::Embedder => "mem_embedder",
        StoreTable::Vectors => "mem_vectors",
        StoreTable::SolutionMetadata => "solution_metadata",
        StoreTable::BlueprintCallScopes => "blueprint_call_scopes",
        StoreTable::BlueprintCallRevisions => "blueprint_call_revisions",
        StoreTable::BlueprintIndex => "blueprint_index",
        StoreTable::BlueprintVectors => "blueprint_vectors",
        StoreTable::BlueprintCleanupReceipt => "blueprint_v1_cleanup_receipt",
        StoreTable::SourceTombstones => "mem_source_tombstones",
        StoreTable::PrecompactPurges => "mem_precompact_purges",
        StoreTable::PrecompactBlobs => "mem_precompact_purge_blobs",
        StoreTable::VectorPending => "mem_vector_pending",
        StoreTable::LessonAudit => "lesson_audit",
        StoreTable::GraphPurgeLog => "graph_purge_log",
    }
}

fn ordering(table: StoreTable) -> &'static str {
    match table {
        StoreTable::Items
        | StoreTable::History
        | StoreTable::Entities
        | StoreTable::Relations
        | StoreTable::BlueprintCleanupReceipt
        | StoreTable::LessonAudit
        | StoreTable::GraphPurgeLog => "id",
        StoreTable::Aliases => "alias_id",
        StoreTable::Pins | StoreTable::Vectors | StoreTable::VectorPending => "mem_id",
        StoreTable::Fts => "rowid",
        StoreTable::Embedder => "singleton",
        StoreTable::SolutionMetadata
        | StoreTable::BlueprintCallScopes
        | StoreTable::BlueprintIndex
        | StoreTable::PrecompactPurges => "item_id",
        StoreTable::BlueprintCallRevisions => "item_id, hash, area, team",
        StoreTable::BlueprintVectors => "item_id, slot, ordinal",
        StoreTable::SourceTombstones => "source_key",
        StoreTable::PrecompactBlobs => "item_id, path",
    }
}

pub(crate) fn exists(tx: &mut Transaction<'_>, table: StoreTable) -> StoreResult<bool> {
    Ok(tx
        .query_one("SELECT to_regclass($1) IS NOT NULL", &[&name(table)])
        .map_err(db_error)?
        .get(0))
}

fn row_values(row: &Row) -> StoreResult<BTreeMap<String, StoreValue>> {
    row.columns()
        .iter()
        .enumerate()
        .map(|(i, column)| {
            let value = match *column.type_() {
                Type::INT8 => row
                    .try_get::<_, Option<i64>>(i)
                    .map_err(db_error)?
                    .map(StoreValue::Integer),
                Type::FLOAT8 => row
                    .try_get::<_, Option<f64>>(i)
                    .map_err(db_error)?
                    .map(StoreValue::Real),
                Type::TEXT => row
                    .try_get::<_, Option<String>>(i)
                    .map_err(db_error)?
                    .map(StoreValue::Text),
                Type::BYTEA => row
                    .try_get::<_, Option<Vec<u8>>>(i)
                    .map_err(db_error)?
                    .map(StoreValue::Bytes),
                _ if column.type_().name() == "vector" => row
                    .try_get::<_, Option<pgvector::Vector>>(i)
                    .map_err(db_error)?
                    .map(|v| {
                        StoreValue::Bytes(codeterm_mem::embedder::index::encode(v.as_slice()))
                    }),
                _ => return Err("unsupported Postgres transfer column".into()),
            };
            Ok((column.name().to_owned(), value.unwrap_or(StoreValue::Null)))
        })
        .collect()
}

pub(crate) fn export_in(
    tx: &mut Transaction<'_>,
    after: Option<&ExportCursor>,
    limit: usize,
) -> StoreResult<ExportPage> {
    if !(1..=10_000).contains(&limit) {
        return Err("export page limit must be 1 to 10000".into());
    }
    let table = after.map_or(StoreTable::Items, |cursor| cursor.table);
    let offset = after.map_or(0, |cursor| cursor.offset);
    let mut rows = Vec::new();
    if exists(tx, table)? {
        let projection = if table == StoreTable::Fts {
            "rowid,mem_id,title,summary,body,tags"
        } else {
            "*"
        };
        let sql = format!(
            "SELECT {projection} FROM {} ORDER BY {} LIMIT $1 OFFSET $2",
            name(table),
            ordering(table)
        );
        let offset_bind = i64::try_from(offset).map_err(|_| "export offset overflow")?;
        for row in tx
            .query(&sql, &[&((limit + 1) as i64), &offset_bind])
            .map_err(db_error)?
        {
            let values = row_values(&row)?;
            let mut blobs = BTreeMap::new();
            if let Some(StoreValue::Text(name)) = values.get("body_blob_path") {
                let body: Vec<u8> = tx
                    .query_one("SELECT body FROM mem_blobs WHERE name=$1", &[name])
                    .map_err(db_error)?
                    .get(0);
                blobs.insert(name.clone(), body);
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
            .position(|v| *v == table)
            .and_then(|index| StoreTable::ALL.get(index + 1))
            .map(|table| ExportCursor {
                table: *table,
                offset: 0,
            })
    };
    Ok(ExportPage { table, rows, next })
}

fn parameter(value: &StoreValue, ty: &Type) -> StoreResult<Box<dyn ToSql + Sync + Send>> {
    Ok(match (value, ty) {
        (StoreValue::Null, &Type::INT8) => Box::new(None::<i64>),
        (StoreValue::Null, &Type::FLOAT8) => Box::new(None::<f64>),
        (StoreValue::Null, &Type::BYTEA) => Box::new(None::<Vec<u8>>),
        (StoreValue::Null, &Type::TEXT) => Box::new(None::<String>),
        (StoreValue::Integer(n), &Type::INT8) => Box::new(*n),
        (StoreValue::Real(n), &Type::FLOAT8) => Box::new(*n),
        (StoreValue::Text(s), &Type::TEXT) => Box::new(s.clone()),
        (StoreValue::Bytes(b), &Type::BYTEA) => Box::new(b.clone()),
        (StoreValue::Bytes(b), ty) if ty.name() == "vector" => {
            Box::new(pgvector::Vector::from(vectors::decode(b)?))
        }
        _ => return Err("invalid Postgres transfer value type".into()),
    })
}

pub(crate) fn import_in(tx: &mut Transaction<'_>, page: &ExportPage) -> StoreResult<()> {
    if page.rows.is_empty() {
        return Ok(());
    }
    if page.table == StoreTable::Embedder {
        if page.rows.len() != 1 {
            return Err("expected one embedder stamp".into());
        }
        let values = &page.rows[0].values;
        let text = |key: &str| match values.get(key) {
            Some(StoreValue::Text(s)) => Ok(s.clone()),
            _ => Err(format!("missing {key}")),
        };
        let dimension = match values.get("dimension") {
            Some(StoreValue::Integer(n)) => {
                usize::try_from(*n).map_err(|_| "invalid vector dimension")?
            }
            _ => return Err("missing vector dimension".into()),
        };
        vectors::initialize_in(
            tx,
            &codeterm_mem::embedder::EmbedderIdentity {
                provider: text("provider")?,
                model: text("model")?,
                dimension,
            },
        )?;
    }
    let projection = if page.table == StoreTable::Fts {
        "rowid,mem_id,title,summary,body,tags"
    } else {
        "*"
    };
    let shape = tx
        .prepare(&format!(
            "SELECT {projection} FROM {} LIMIT 0",
            name(page.table)
        ))
        .map_err(db_error)?;
    let columns = shape.columns();
    for row in &page.rows {
        if columns.len() != row.values.len()
            || !columns
                .iter()
                .all(|column| row.values.contains_key(column.name()))
        {
            return Err(format!("invalid columns for {:?}", page.table));
        }
        for (name, bytes) in &row.blobs {
            tx.execute(
                "INSERT INTO mem_blobs(name,body) VALUES ($1,$2) ON CONFLICT DO NOTHING",
                &[name, bytes],
            )
            .map_err(db_error)?;
            let stored: Vec<u8> = tx
                .query_one("SELECT body FROM mem_blobs WHERE name=$1", &[name])
                .map_err(db_error)?
                .get(0);
            if stored != *bytes {
                return Err("memory blob content conflict".into());
            }
        }
        if let Some(StoreValue::Text(name)) = row.values.get("body_blob_path") {
            if !row.blobs.contains_key(name) {
                return Err("memory blob body missing".into());
            }
        }
        let mut params = Vec::new();
        for column in columns {
            params.push(parameter(&row.values[column.name()], column.type_())?);
        }
        let names = columns
            .iter()
            .map(|column| format!("\"{}\"", column.name()))
            .collect::<Vec<_>>()
            .join(",");
        let placeholders = (1..=columns.len())
            .map(|i| format!("${i}"))
            .collect::<Vec<_>>()
            .join(",");
        let conflict = match page.table {
            StoreTable::Embedder | StoreTable::BlueprintCleanupReceipt => {
                let key = if page.table == StoreTable::Embedder {
                    "singleton"
                } else {
                    "id"
                };
                let assignments = columns
                    .iter()
                    .filter(|c| c.name() != key)
                    .map(|c| format!("\"{0}\"=excluded.\"{0}\"", c.name()))
                    .collect::<Vec<_>>()
                    .join(",");
                format!(" ON CONFLICT ({key}) DO UPDATE SET {assignments}")
            }
            _ => String::new(),
        };
        let sql = format!(
            "INSERT INTO {} ({names}) VALUES ({placeholders}){conflict}",
            name(page.table)
        );
        let refs: Vec<&(dyn ToSql + Sync)> = params.iter().map(|p| p.as_ref() as _).collect();
        tx.execute(&sql, &refs).map_err(db_error)?;
    }
    if matches!(
        page.table,
        StoreTable::History
            | StoreTable::Relations
            | StoreTable::Fts
            | StoreTable::LessonAudit
            | StoreTable::GraphPurgeLog
    ) {
        let key = if page.table == StoreTable::Fts {
            "rowid"
        } else {
            "id"
        };
        tx.query_one(&format!("SELECT setval(pg_get_serial_sequence($1,$2), coalesce(max({key}),0)+1, false) FROM {}", name(page.table)), &[&name(page.table), &key])
            .map_err(db_error)?;
    }
    Ok(())
}

pub(crate) fn counts_in(tx: &mut Transaction<'_>) -> StoreResult<TableCounts> {
    StoreTable::ALL
        .into_iter()
        .map(|table| {
            let count: i64 = if exists(tx, table)? {
                tx.query_one(&format!("SELECT count(*) FROM {}", name(table)), &[])
                    .map_err(db_error)?
                    .get(0)
            } else {
                0
            };
            Ok((table, count as u64))
        })
        .collect()
}

impl PostgresMemStore {
    pub fn export_page(
        &self,
        after: Option<&ExportCursor>,
        limit: usize,
    ) -> StoreResult<ExportPage> {
        self.transaction(|tx| export_in(tx, after, limit))
    }

    pub fn import_page(&self, page: &ExportPage) -> StoreResult<()> {
        self.transaction(|tx| import_in(tx, page))
    }

    pub fn table_counts(&self) -> StoreResult<TableCounts> {
        self.transaction(counts_in)
    }
}
