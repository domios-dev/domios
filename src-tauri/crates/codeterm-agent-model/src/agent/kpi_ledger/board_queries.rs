use super::{KpiLedger, TokenCounts};
use rusqlite::params;

#[derive(Debug, Clone)]
pub struct DailyTokenUsage {
    pub timestamp: u64,
    pub plan_id: Option<String>,
    pub provider: String,
    pub model: Option<String>,
    pub tokens: TokenCounts,
    pub turns: u64,
}

impl KpiLedger {
    pub fn daily_token_usage(&self, from: u64, to: u64) -> Result<Vec<DailyTokenUsage>, String> {
        let conn = self.conn.lock();
        let mut statement = conn
            .prepare(
                "SELECT (ts / 86400) * 86400, plan_id, provider, model,
             COALESCE(SUM(input_tokens), 0), COALESCE(SUM(uncached_input_tokens), 0),
             COALESCE(SUM(output_tokens), 0), COUNT(*)
             FROM kpi_events WHERE kind = 'token_usage' AND ts >= ?1 AND ts < ?2
             AND ts / 86400 >= ?1 / 86400 AND ts / 86400 <= (?2 - 1) / 86400
             AND input_tokens IS NOT NULL
             GROUP BY ts / 86400, plan_id, provider, model",
            )
            .map_err(|error| format!("daily token usage prepare: {error}"))?;
        let rows = statement
            .query_map(params![from, to], |row| {
                Ok(DailyTokenUsage {
                    timestamp: row.get::<_, i64>(0)? as u64,
                    plan_id: row.get(1)?,
                    provider: row.get(2)?,
                    model: row.get(3)?,
                    tokens: TokenCounts {
                        input: row.get::<_, i64>(4)? as u64,
                        uncached_input: row.get::<_, i64>(5)? as u64,
                        output: row.get::<_, i64>(6)? as u64,
                    },
                    turns: row.get::<_, i64>(7)? as u64,
                })
            })
            .map_err(|error| format!("daily token usage query: {error}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("daily token usage row: {error}"))?;
        Ok(rows)
    }
}

#[cfg(test)]
mod tests;
