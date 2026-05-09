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
            "SELECT s.id, s.name, s.kind, s.description, s.url, s.host, s.port,
                    s.process_label, s.script_path, s.health_mode, s.health_path,
                    s.health_interval, s.health_ttl, s.field_mapping, s.push_enabled,
                    s.registered_at, s.updated_at, s.enabled,
                    h.status, h.probed_at, h.latency_ms, h.response
             FROM services s
             LEFT JOIN (
                SELECT service_id, status, probed_at, latency_ms, response,
                       ROW_NUMBER() OVER (PARTITION BY service_id ORDER BY probed_at DESC) AS rn
                FROM service_health
             ) h ON h.service_id = s.id AND h.rn = 1
             WHERE s.id = ?1",
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
            "SELECT s.id, s.name, s.kind, s.description, s.url, s.host, s.port,
                    s.process_label, s.script_path, s.health_mode, s.health_path,
                    s.health_interval, s.health_ttl, s.field_mapping, s.push_enabled,
                    s.registered_at, s.updated_at, s.enabled,
                    h.status, h.probed_at, h.latency_ms, h.response
             FROM services s
             LEFT JOIN (
                SELECT service_id, status, probed_at, latency_ms, response,
                       ROW_NUMBER() OVER (PARTITION BY service_id ORDER BY probed_at DESC) AS rn
                FROM service_health
             ) h ON h.service_id = s.id AND h.rn = 1
             WHERE 1=1",
        );
        if let Some(k) = &kind {
            sql.push_str(&format!(" AND s.kind = '{}'", kind_to_str(k)));
        }
        if let Some(e) = enabled {
            sql.push_str(&format!(" AND s.enabled = {}", if e { 1 } else { 0 }));
        }
        sql.push_str(" ORDER BY s.name");
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

    let status_str: Option<String> = row.get(18).ok();
    let probed_at: Option<i64> = row.get(19).ok();
    let latency_ms: Option<i64> = row.get(20).ok();
    let response_str: Option<String> = row.get(21).ok().flatten();
    let extra_json: serde_json::Value = response_str
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or(serde_json::Value::Null);
    let version = extra_json.get("version").and_then(|v| v.as_str()).map(String::from);
    let uptime_secs = extra_json.get("uptime_secs").and_then(|v| v.as_u64());
    let health = HealthSnapshot {
        status: status_str.as_deref().map(parse_status).unwrap_or_default(),
        probed_at,
        latency_ms: latency_ms.map(|v| v as u64),
        version,
        uptime_secs,
        extra: extra_json,
    };

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
        health,
        schedule: None,
        next_run: None,
        last_run_status: None,
    })
}

fn parse_status(s: &str) -> HealthStatus {
    match s {
        "healthy" => HealthStatus::Healthy,
        "degraded" => HealthStatus::Degraded,
        "down" => HealthStatus::Down,
        _ => HealthStatus::Unknown,
    }
}
