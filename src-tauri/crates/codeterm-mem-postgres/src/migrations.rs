use crate::{db_error, StoreResult};
use postgres::Client;

pub(crate) const VERSION: i64 = 2;

pub(crate) fn verify_version(
    client: &mut impl postgres::GenericClient,
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

pub(crate) fn run(
    client: &mut Client,
    schema: &str,
    fence: Option<&crate::fence::SchemaFence>,
) -> StoreResult<()> {
    let mut tx = client.transaction().map_err(db_error)?;
    if let Some(fence) = fence {
        fence.admit(&mut tx)?;
    }
    tx.query_one(
        "SELECT pg_advisory_xact_lock(hashtextextended('domios_mem:bootstrap', 0))",
        &[],
    )
    .map_err(db_error)?;
    tx.query_one(
        "SELECT pg_advisory_xact_lock(hashtextextended($1, 0))",
        &[&schema],
    )
    .map_err(db_error)?;
    tx.batch_execute(&format!(
        "CREATE SCHEMA IF NOT EXISTS {schema}; SET LOCAL search_path TO {schema}, public;"
    ))
    .map_err(db_error)?;
    let stamped: bool = tx.query_one(
        "SELECT to_regclass($1) IS NOT NULL", &[&format!("{schema}.mem_schema_version")],
    ).map_err(db_error)?.get(0);
    if stamped {
        let versions: Vec<i64> = tx.query(&format!("SELECT version FROM {schema}.mem_schema_version FOR UPDATE"), &[])
            .map_err(db_error)?.into_iter().map(|row| row.get(0)).collect();
        if versions != [VERSION] {
            return Err(format!("unsupported Postgres memory schema version {versions:?}; expected {VERSION}; older schemas cannot be upgraded"));
        }
    } else {
        let occupied: bool = tx.query_one(
            "SELECT EXISTS(SELECT 1 FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname=$1)",
            &[&schema],
        ).map_err(db_error)?.get(0);
        if occupied {
            return Err("unstamped Postgres memory schema refused; create a fresh schema".into());
        }
        tx.batch_execute("CREATE EXTENSION IF NOT EXISTS vector WITH SCHEMA public;")
            .map_err(db_error)?;
        tx.batch_execute(include_str!("migrations/001_storage.sql")).map_err(db_error)?;
        tx.batch_execute("CREATE TABLE mem_schema_version (
            singleton BIGINT PRIMARY KEY CHECK(singleton=1), version BIGINT NOT NULL);
            INSERT INTO mem_schema_version VALUES(1,2);").map_err(db_error)?;
    }
    tx.commit().map_err(db_error)
}
