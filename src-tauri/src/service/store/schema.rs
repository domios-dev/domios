use rusqlite::Connection;

pub const SCHEMA_VERSION: u32 = 3;
pub const SERVICE_HEALTH_RETENTION: i64 = 10_000;

pub(super) const COMPACT_SERVICE_HEALTH_SQL: &str = r#"
    DELETE FROM service_health
    WHERE rowid IN (
        SELECT rowid FROM service_health
        ORDER BY probed_at DESC, rowid DESC
        LIMIT -1 OFFSET ?1
    )
"#;

pub fn init_schema(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS services (
            id                      TEXT PRIMARY KEY,
            name                    TEXT NOT NULL,
            kind                    TEXT NOT NULL,
            description             TEXT,
            url                     TEXT,
            host                    TEXT,
            port                    INTEGER,
            process_label           TEXT,
            script_path             TEXT,
            health_mode             TEXT NOT NULL,
            health_path             TEXT,
            health_json_field       TEXT,
            health_ok_values        TEXT,
            health_degraded_values  TEXT,
            health_interval         INTEGER NOT NULL DEFAULT 30,
            health_ttl              INTEGER NOT NULL DEFAULT 60,
            field_mapping           TEXT,
            push_enabled            BOOLEAN NOT NULL DEFAULT 0,
            registered_at           INTEGER NOT NULL,
            updated_at              INTEGER NOT NULL,
            enabled                 BOOLEAN NOT NULL DEFAULT 1
        );
        CREATE INDEX IF NOT EXISTS idx_services_kind ON services(kind);
        CREATE INDEX IF NOT EXISTS idx_services_enabled ON services(enabled);

        CREATE TABLE IF NOT EXISTS service_health (
            service_id      TEXT NOT NULL,
            probed_at       INTEGER NOT NULL,
            status          TEXT NOT NULL,
            response        TEXT,
            latency_ms      INTEGER,
            PRIMARY KEY (service_id, probed_at),
            FOREIGN KEY (service_id) REFERENCES services(id) ON DELETE CASCADE
        );
        CREATE INDEX IF NOT EXISTS idx_health_service_time
            ON service_health(service_id, probed_at DESC);
        CREATE INDEX IF NOT EXISTS idx_health_time
            ON service_health(probed_at DESC);

        CREATE TABLE IF NOT EXISTS activity (
            id              INTEGER PRIMARY KEY AUTOINCREMENT,
            occurred_at     INTEGER NOT NULL,
            source          TEXT NOT NULL,
            source_id       TEXT,
            target          TEXT,
            level           TEXT NOT NULL,
            message         TEXT NOT NULL,
            meta            TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_activity_time ON activity(occurred_at DESC);
        CREATE INDEX IF NOT EXISTS idx_activity_source ON activity(source, occurred_at DESC);

        CREATE TABLE IF NOT EXISTS schema_version (
            id      INTEGER PRIMARY KEY CHECK (id = 1),
            version INTEGER NOT NULL
        );
    "#,
    )?;

    conn.execute(
        "INSERT OR IGNORE INTO schema_version (id, version) VALUES (1, ?1)",
        [SCHEMA_VERSION],
    )?;
    Ok(())
}

/// Idempotent migrations — safe to re-run on every open.
pub fn migrate_schema(conn: &Connection) -> rusqlite::Result<()> {
    for sql in &[
        "ALTER TABLE services ADD COLUMN health_json_field TEXT",
        "ALTER TABLE services ADD COLUMN health_ok_values TEXT",
        "ALTER TABLE services ADD COLUMN health_degraded_values TEXT",
    ] {
        let _ = conn.execute(sql, []); // silently ignore "duplicate column" on re-run
    }
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_health_time ON service_health(probed_at DESC)",
        [],
    )?;
    compact_service_health(conn)?;
    conn.execute(
        "INSERT OR REPLACE INTO schema_version (id, version) VALUES (1, ?1)",
        [SCHEMA_VERSION],
    )?;
    Ok(())
}

pub fn compact_service_health(conn: &Connection) -> rusqlite::Result<usize> {
    conn.execute(COMPACT_SERVICE_HEALTH_SQL, [SERVICE_HEALTH_RETENTION])
}
