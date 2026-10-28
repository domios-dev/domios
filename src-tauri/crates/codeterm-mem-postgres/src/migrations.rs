use crate::{db_error, StoreResult};
use postgres::Client;

pub(crate) const VERSION: i64 = 2;

pub(crate) fn verify_version(
    client: &mut Client,
    schema: &str,
) -> Result<(), crate::PostgresOpenError> {
    let table = format!("{schema}.mem_schema_version");
    let exists: bool = client
        .query_one("SELECT to_regclass($1) IS NOT NULL", &[&table])
        .map_err(|_| crate::PostgresOpenError::Connection)?
        .get(0);
    let found = if exists {
        client
            .query_opt(
                &format!("SELECT version FROM {table} WHERE singleton=1"),
                &[],
            )
            .map_err(|_| crate::PostgresOpenError::Connection)?
            .map(|row| row.get(0))
    } else {
        None
    };
    if found != Some(VERSION) {
        return Err(crate::PostgresOpenError::SchemaVersionMismatch {
            found,
            expected: VERSION,
        });
    }
    Ok(())
}

const STEPS: &[&str] = &[
    include_str!("migrations/001_storage.sql"),
    include_str!("migrations/002_authorship.sql"),
];

pub(crate) fn run(client: &mut Client, schema: &str) -> StoreResult<()> {
    let mut tx = client.transaction().map_err(db_error)?;
    tx.query_one(
        "SELECT pg_advisory_xact_lock(hashtextextended($1, 0))",
        &[&schema],
    )
    .map_err(db_error)?;
    tx.batch_execute(&format!(
        "CREATE SCHEMA IF NOT EXISTS {schema}; SET LOCAL search_path TO {schema}, public;"
    ))
    .map_err(db_error)?;
    tx.batch_execute(
        "CREATE EXTENSION IF NOT EXISTS vector WITH SCHEMA public;
         CREATE TABLE IF NOT EXISTS mem_schema_version (
            singleton BIGINT PRIMARY KEY CHECK(singleton=1), version BIGINT NOT NULL);
         INSERT INTO mem_schema_version VALUES (1,0) ON CONFLICT DO NOTHING;",
    )
    .map_err(db_error)?;
    let version: i64 = tx
        .query_one(
            "SELECT version FROM mem_schema_version WHERE singleton=1 FOR UPDATE",
            &[],
        )
        .map_err(db_error)?
        .get(0);
    if version < 0 || version as usize > STEPS.len() {
        return Err("unsupported Postgres memory schema version".into());
    }
    for (index, sql) in STEPS.iter().enumerate().skip(version as usize) {
        tx.batch_execute(sql).map_err(db_error)?;
        tx.execute(
            "UPDATE mem_schema_version SET version=$1 WHERE singleton=1",
            &[&((index + 1) as i64)],
        )
        .map_err(db_error)?;
    }
    tx.commit().map_err(db_error)
}
