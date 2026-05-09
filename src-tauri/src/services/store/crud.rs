use rusqlite::{params, OptionalExtension};
use std::time::{SystemTime, UNIX_EPOCH};

use super::types::*;
use super::ServicesStore;

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

fn slug(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

fn kind_to_str(k: &ServiceKind) -> &'static str {
    match k {
        ServiceKind::Cron => "cron",
        ServiceKind::Daemon => "daemon",
    }
}

fn mode_to_str(m: &HealthMode) -> &'static str {
    match m {
        HealthMode::Http => "http",
        HealthMode::Tcp => "tcp",
        HealthMode::Process => "process",
        HealthMode::Script => "script",
    }
}

fn parse_kind(s: &str) -> rusqlite::Result<ServiceKind> {
    match s {
        "cron" => Ok(ServiceKind::Cron),
        "daemon" => Ok(ServiceKind::Daemon),
        other => Err(rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Text,
            Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("unknown kind: {}", other),
            )),
        )),
    }
}

fn parse_mode(s: &str) -> rusqlite::Result<HealthMode> {
    match s {
        "http" => Ok(HealthMode::Http),
        "tcp" => Ok(HealthMode::Tcp),
        "process" => Ok(HealthMode::Process),
        "script" => Ok(HealthMode::Script),
        other => Err(rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Text,
            Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("unknown mode: {}", other),
            )),
        )),
    }
}

impl ServicesStore {
    pub fn create(&self, input: ServiceCreate) -> rusqlite::Result<String> {
        let id = format!(
            "svc_{}_{}",
            slug(&input.name),
            &uuid::Uuid::new_v4().to_string()[..8]
        );
        let now = now_ms();
        let conn = self.conn();
        let conn = conn.lock();
        conn.execute(
            "INSERT INTO services (
                id, name, kind, description, url, host, port,
                process_label, script_path, health_mode, health_path,
                health_interval, health_ttl, field_mapping, push_enabled,
                registered_at, updated_at, enabled
             ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18
             )",
            params![
                id,
                input.name,
                kind_to_str(&input.kind),
                input.description,
                input.url,
                input.host,
                input.port,
                input.process_label,
                input.script_path,
                mode_to_str(&input.health_mode),
                input.health_path,
                input.health_interval.unwrap_or(30) as i64,
                60_i64,
                input
                    .field_mapping
                    .as_ref()
                    .map(|m| serde_json::to_string(m).unwrap()),
                input.push_enabled.unwrap_or(false),
                now,
                now,
                true,
            ],
        )?;
        Ok(id)
    }

    pub fn get(&self, id: &str) -> rusqlite::Result<Option<Service>> {
        let conn = self.conn();
        let conn = conn.lock();
        conn.query_row(
            "SELECT id, name, kind, description, url, host, port,
                    process_label, script_path, health_mode, health_path,
                    health_interval, health_ttl, field_mapping, push_enabled,
                    registered_at, updated_at, enabled
             FROM services WHERE id = ?1",
            [id],
            row_to_service,
        )
        .optional()
    }

    pub fn list(
        &self,
        kind: Option<ServiceKind>,
        enabled: Option<bool>,
    ) -> rusqlite::Result<Vec<Service>> {
        let conn = self.conn();
        let conn = conn.lock();
        let mut sql = String::from(
            "SELECT id, name, kind, description, url, host, port,
                    process_label, script_path, health_mode, health_path,
                    health_interval, health_ttl, field_mapping, push_enabled,
                    registered_at, updated_at, enabled
             FROM services WHERE 1=1",
        );
        if let Some(k) = &kind {
            sql.push_str(&format!(" AND kind = '{}'", kind_to_str(k)));
        }
        if let Some(e) = enabled {
            sql.push_str(&format!(" AND enabled = {}", if e { 1 } else { 0 }));
        }
        sql.push_str(" ORDER BY name");
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map([], row_to_service)?;
        rows.collect()
    }

    pub fn update(&self, id: &str, patch: ServiceUpdate) -> rusqlite::Result<()> {
        let now = now_ms();
        let conn = self.conn();
        let conn = conn.lock();
        if let Some(v) = patch.name {
            conn.execute(
                "UPDATE services SET name=?1, updated_at=?2 WHERE id=?3",
                params![v, now, id],
            )?;
        }
        if let Some(v) = patch.description {
            conn.execute(
                "UPDATE services SET description=?1, updated_at=?2 WHERE id=?3",
                params![v, now, id],
            )?;
        }
        if let Some(v) = patch.url {
            conn.execute(
                "UPDATE services SET url=?1, updated_at=?2 WHERE id=?3",
                params![v, now, id],
            )?;
        }
        if let Some(v) = patch.host {
            conn.execute(
                "UPDATE services SET host=?1, updated_at=?2 WHERE id=?3",
                params![v, now, id],
            )?;
        }
        if let Some(v) = patch.port {
            conn.execute(
                "UPDATE services SET port=?1, updated_at=?2 WHERE id=?3",
                params![v, now, id],
            )?;
        }
        if let Some(v) = patch.process_label {
            conn.execute(
                "UPDATE services SET process_label=?1, updated_at=?2 WHERE id=?3",
                params![v, now, id],
            )?;
        }
        if let Some(v) = patch.script_path {
            conn.execute(
                "UPDATE services SET script_path=?1, updated_at=?2 WHERE id=?3",
                params![v, now, id],
            )?;
        }
        if let Some(v) = patch.health_mode {
            conn.execute(
                "UPDATE services SET health_mode=?1, updated_at=?2 WHERE id=?3",
                params![mode_to_str(&v), now, id],
            )?;
        }
        if let Some(v) = patch.health_path {
            conn.execute(
                "UPDATE services SET health_path=?1, updated_at=?2 WHERE id=?3",
                params![v, now, id],
            )?;
        }
        if let Some(v) = patch.health_interval {
            conn.execute(
                "UPDATE services SET health_interval=?1, updated_at=?2 WHERE id=?3",
                params![v as i64, now, id],
            )?;
        }
        if let Some(v) = patch.field_mapping {
            conn.execute(
                "UPDATE services SET field_mapping=?1, updated_at=?2 WHERE id=?3",
                params![serde_json::to_string(&v).unwrap(), now, id],
            )?;
        }
        if let Some(v) = patch.push_enabled {
            conn.execute(
                "UPDATE services SET push_enabled=?1, updated_at=?2 WHERE id=?3",
                params![v, now, id],
            )?;
        }
        if let Some(v) = patch.enabled {
            conn.execute(
                "UPDATE services SET enabled=?1, updated_at=?2 WHERE id=?3",
                params![v, now, id],
            )?;
        }
        Ok(())
    }

    pub fn delete(&self, id: &str) -> rusqlite::Result<()> {
        let conn = self.conn();
        let conn = conn.lock();
        conn.execute("DELETE FROM services WHERE id=?1", [id])?;
        Ok(())
    }
}

fn row_to_service(row: &rusqlite::Row) -> rusqlite::Result<Service> {
    let kind_str: String = row.get(2)?;
    let mode_str: String = row.get(9)?;
    let field_mapping_str: Option<String> = row.get(13)?;
    let field_mapping = field_mapping_str.and_then(|s| serde_json::from_str(&s).ok());

    Ok(Service {
        id: row.get(0)?,
        name: row.get(1)?,
        kind: parse_kind(&kind_str)?,
        description: row.get(3)?,
        url: row.get(4)?,
        host: row.get(5)?,
        port: row.get::<_, Option<i64>>(6)?.map(|p| p as u16),
        process_label: row.get(7)?,
        script_path: row.get(8)?,
        health_mode: parse_mode(&mode_str)?,
        health_path: row.get(10)?,
        health_interval: row.get::<_, i64>(11)? as u32,
        health_ttl: row.get::<_, i64>(12)? as u32,
        field_mapping,
        push_enabled: row.get(14)?,
        registered_at: row.get(15)?,
        updated_at: row.get(16)?,
        enabled: row.get(17)?,
        health: HealthSnapshot::default(),
        schedule: None,
        next_run: None,
        last_run_status: None,
    })
}
