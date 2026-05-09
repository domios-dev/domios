//! services.db schema initialization.

use rusqlite::Connection;

pub const SCHEMA_VERSION: u32 = 1;

pub fn init_schema(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS services (
            id              TEXT PRIMARY KEY,
            name            TEXT NOT NULL,
            kind            TEXT NOT NULL,
            description     TEXT,
            url             TEXT,
            host            TEXT,
            port            INTEGER,
            process_label   TEXT,
            script_path     TEXT,
            health_mode     TEXT NOT NULL,
            health_path     TEXT,
            health_interval INTEGER NOT NULL DEFAULT 30,
            health_ttl      INTEGER NOT NULL DEFAULT 60,
            field_mapping   TEXT,
            push_enabled    BOOLEAN NOT NULL DEFAULT 0,
            registered_at   INTEGER NOT NULL,
            updated_at      INTEGER NOT NULL,
            enabled         BOOLEAN NOT NULL DEFAULT 1
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

        CREATE TABLE IF NOT EXISTS schema_version (version INTEGER NOT NULL);
    "#,
    )?;

    let mut stmt = conn.prepare("SELECT version FROM schema_version LIMIT 1")?;
    let exists: bool = stmt.exists([])?;
    if !exists {
        conn.execute(
            "INSERT INTO schema_version (version) VALUES (?1)",
            [SCHEMA_VERSION],
        )?;
    }
    Ok(())
}
