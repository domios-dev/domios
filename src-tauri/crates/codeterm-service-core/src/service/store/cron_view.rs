use super::types::*;
use crate::cron::store::{CronJob, CronStore};

pub const CRON_ID_PREFIX: &str = "cron_";

pub fn cron_id_to_service_id(cron_id: &str) -> String {
    format!("{}{}", CRON_ID_PREFIX, cron_id)
}

#[allow(dead_code)] // inverse of cron_id_to_service_id; kept as a symmetric pair
pub fn service_id_to_cron_id(service_id: &str) -> &str {
    service_id
        .strip_prefix(CRON_ID_PREFIX)
        .unwrap_or(service_id)
}

pub fn cron_to_service(job: &CronJob) -> Service {
    Service {
        id: cron_id_to_service_id(&job.id),
        name: job.name.clone(),
        kind: ServiceKind::Cron,
        protected: job.protected,
        description: job.description.clone(),
        host: None,
        url: None,
        port: None,
        process_label: None,
        script_path: None,
        health_mode: HealthMode::Http,
        health_path: None,
        health_json_field: None,
        health_ok_values: None,
        health_degraded_values: None,
        health_interval: 0,
        health_ttl: 0,
        field_mapping: None,
        push_enabled: false,
        schedule: Some(job.schedule.clone()),
        next_run: job.next_run.map(|n| n as i64),
        last_run_status: job.last_run.as_ref().map(|r| format!("{:?}", r.status)),
        registered_at: job.created_at as i64,
        updated_at: job.created_at as i64,
        enabled: job.enabled,
        health: HealthSnapshot::default(),
    }
}

pub fn list_local(svc: &super::ServicesStore, cron: &CronStore) -> rusqlite::Result<Vec<Service>> {
    let mut out = svc.list(None, None)?;
    out.extend(cron.list().iter().map(cron_to_service));
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}
