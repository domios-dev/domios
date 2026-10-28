use super::*;
impl SqliteMemStore {
    pub(super) fn dream_worker_in(
        &self,
        ctx: &WriteContext,
        kind: &str,
        machine: &str,
    ) -> Result<serde_json::Value, String> {
        let conn = self.conn.lock();
        crate::store::with_author(ctx.default_system_author("dreaming:rem"), || match kind {
            "dreaming-light" => worker::worker_dreaming_light(&conn, machine)
                .and_then(|r| serde_json::to_value(r).map_err(|e| e.to_string())),
            "dreaming-deep" => worker::worker_dreaming_deep(&conn)
                .and_then(|r| serde_json::to_value(r).map_err(|e| e.to_string())),
            "dreaming-normalize" => worker::worker_dreaming_normalize(&conn),
            other => Err(format!("unknown worker kind: {other}")),
        })
    }
}
