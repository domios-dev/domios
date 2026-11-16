use super::{KpiEventInput, KpiEventKind, KpiLedger, UsageRebuildOutcome};
use rusqlite::{params, OptionalExtension};

impl KpiLedger {
    pub fn remember_usage_provider(&self, source: &str, provider: &str) -> Result<(), String> {
        let source = super::normalize_usage_source_key(source);
        let mut conn = self.conn.lock();
        let tx = conn.transaction().map_err(|e| e.to_string())?;
        if usage_owner(&tx, &source)?.is_some_and(|owner| owner != provider) {
            return Err("usage source already belongs to another provider".into());
        }
        let identity = super::source_keys::source_key_sql("source_key", &source);
        tx.execute(
            &format!(
                "DELETE FROM kpi_usage_source_owners WHERE {identity} = ?1 AND source_key != ?1"
            ),
            [&source],
        )
        .map_err(|e| e.to_string())?;
        tx.execute("INSERT INTO kpi_usage_source_owners (source_key, provider) VALUES (?1, ?2) ON CONFLICT(source_key) DO NOTHING", params![source, provider]).map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())
    }

    pub fn usage_provider(&self, source: &str) -> Result<Option<String>, String> {
        usage_owner(&self.conn.lock(), source)
    }

    pub fn record_runtime_usage(&self, event: &KpiEventInput) -> Result<(), String> {
        let key = event
            .dedup_key
            .as_deref()
            .ok_or("runtime usage requires a response key")?;
        if event.kind != KpiEventKind::TokenUsage || event.tokens.is_none() {
            return Err("runtime archive accepts reported token usage only".into());
        }
        let mut conn = self.conn.lock();
        let tx = conn.transaction().map_err(|e| e.to_string())?;
        let previous: Option<String> = tx
            .query_row(
                "SELECT event_json FROM kpi_runtime_usage WHERE response_key = ?1",
                [key],
                |r| r.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        let mut archived = match previous {
            Some(json) => {
                let mut previous: KpiEventInput =
                    serde_json::from_str(&json).map_err(|e| e.to_string())?;
                if previous.provider != event.provider {
                    return Err("runtime response belongs to another provider".into());
                }
                previous.tokens = previous
                    .tokens
                    .zip(event.tokens)
                    .map(|(held, next)| held.max_each(next));
                previous
            }
            None => event.clone(),
        };
        archived.bytes = archived.tokens.map_or(0, |tokens| tokens.total());
        let json = serde_json::to_string(&archived).map_err(|e| e.to_string())?;
        tx.execute(
            "INSERT INTO kpi_runtime_usage (response_key, event_json) VALUES (?1, ?2)
            ON CONFLICT(response_key) DO UPDATE SET event_json = excluded.event_json",
            params![key, json],
        )
        .map_err(|e| e.to_string())?;
        Self::insert_with(&tx, &archived)?;
        tx.commit().map_err(|e| e.to_string())?;
        self.committed();
        Ok(())
    }

    pub fn rebuild_runtime_usage(&self) -> Result<UsageRebuildOutcome, String> {
        let mut outcome = UsageRebuildOutcome::default();
        let mut conn = self.conn.lock();
        let tx = conn.transaction().map_err(|e| e.to_string())?;
        {
            let mut stmt = tx
                .prepare("SELECT event_json FROM kpi_runtime_usage ORDER BY response_key")
                .map_err(|e| e.to_string())?;
            let rows = stmt
                .query_map([], |r| r.get::<_, String>(0))
                .map_err(|e| e.to_string())?;
            for row in rows {
                let event: KpiEventInput = serde_json::from_str(&row.map_err(|e| e.to_string())?)
                    .map_err(|e| e.to_string())?;
                outcome.responses += 1;
                outcome.plans.extend(event.plan_id.clone());
                Self::insert_with(&tx, &event)?;
            }
        }
        tx.commit().map_err(|e| e.to_string())?;
        self.committed();
        outcome.sources = u64::from(outcome.responses > 0);
        Ok(outcome)
    }
}

fn usage_owner(conn: &rusqlite::Connection, source: &str) -> Result<Option<String>, String> {
    let source = super::normalize_usage_source_key(source);
    let identity = super::source_keys::source_key_sql("source_key", &source);
    let mut stmt = conn
        .prepare(&format!(
            "SELECT DISTINCT provider FROM kpi_usage_source_owners WHERE {identity} = ?1 LIMIT 2"
        ))
        .map_err(|e| e.to_string())?;
    let owners = stmt
        .query_map([source], |row| row.get::<_, String>(0))
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    match owners.len() {
        0 => Ok(None),
        1 => Ok(owners.into_iter().next()),
        _ => Err("usage source has conflicting recorded providers".into()),
    }
}

#[cfg(test)]
mod tests;
