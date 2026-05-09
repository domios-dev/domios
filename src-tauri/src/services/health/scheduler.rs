use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use rusqlite::{params, Connection};
use tauri::Emitter;

use crate::services::activity::{ActivityEvent, ActivityWriter};
use crate::services::store::{HealthMode, HealthSnapshot, HealthStatus, ServiceKind, ServicesStore};

const TICK_SECS: u64 = 30;
const HTTP_TIMEOUT_MS: u64 = 5_000;
const TCP_TIMEOUT_MS: u64 = 3_000;
const SCRIPT_TIMEOUT_MS: u64 = 30_000;

pub fn start(store: Arc<ServicesStore>, activity: ActivityWriter, app_handle: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        log::info!("[services] health scheduler started (tick {}s)", TICK_SECS);
        let mut last_status: HashMap<String, HealthStatus> = HashMap::new();
        loop {
            run_tick(&store, &activity, &app_handle, &mut last_status).await;
            tokio::time::sleep(Duration::from_secs(TICK_SECS)).await;
        }
    });
}

async fn run_tick(
    store: &Arc<ServicesStore>,
    activity: &ActivityWriter,
    app_handle: &tauri::AppHandle,
    last_status: &mut HashMap<String, HealthStatus>,
) {
    let services = match store.list(None, Some(true)) {
        Ok(s) => s,
        Err(e) => {
            log::error!("[services] list failed: {}", e);
            return;
        }
    };

    let conn = store.conn();
    for svc in services {
        if matches!(svc.kind, ServiceKind::Cron) {
            continue;
        }

        let snap = probe_one(&svc).await;
        persist_snapshot(&conn, &svc.id, &snap);

        let prev = last_status
            .get(&svc.id)
            .copied()
            .unwrap_or(HealthStatus::Unknown);
        if snap.status != prev {
            let _ = activity.record(ActivityEvent::health(
                &svc.id,
                (status_str(prev), status_str(snap.status)),
            ));
            let _ = app_handle.emit(
                "service-health-changed",
                serde_json::json!({"service_id": svc.id, "status": snap.status}),
            );
        }
        last_status.insert(svc.id.clone(), snap.status);
    }
}

async fn probe_one(svc: &crate::services::store::Service) -> HealthSnapshot {
    match svc.health_mode {
        HealthMode::Http => {
            let base = svc.url.clone().unwrap_or_default();
            let path = svc.health_path.as_deref().unwrap_or("/healthz");
            let url = format!("{}{}", base, path);
            super::http::probe_http(&url, HTTP_TIMEOUT_MS)
                .await
                .unwrap_or_default()
        }
        HealthMode::Tcp => super::tcp::probe_tcp(
            svc.host.as_deref().unwrap_or(""),
            svc.port.unwrap_or(0),
            TCP_TIMEOUT_MS,
        )
        .await
        .unwrap_or_default(),
        HealthMode::Process => {
            super::process::probe_process(svc.process_label.as_deref().unwrap_or(""))
                .unwrap_or_default()
        }
        HealthMode::Script => {
            super::script::probe_script(svc.script_path.as_deref().unwrap_or(""), SCRIPT_TIMEOUT_MS)
                .await
                .unwrap_or_default()
        }
    }
}

fn persist_snapshot(conn: &Arc<Mutex<Connection>>, service_id: &str, snap: &HealthSnapshot) {
    let probed_at = snap.probed_at.unwrap_or(0);
    let latency = snap.latency_ms.map(|v| v as i64);
    let response = serde_json::to_string(&snap.extra).ok();
    let res = {
        let c = conn.lock();
        c.execute(
            "INSERT OR REPLACE INTO service_health
                (service_id, probed_at, status, response, latency_ms)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                service_id,
                probed_at,
                status_str(snap.status),
                response,
                latency,
            ],
        )
    };
    if let Err(e) = res {
        log::warn!("[services] persist health snapshot {}: {}", service_id, e);
    }
}

fn status_str(s: HealthStatus) -> &'static str {
    match s {
        HealthStatus::Healthy => "healthy",
        HealthStatus::Degraded => "degraded",
        HealthStatus::Down => "down",
        HealthStatus::Unknown => "unknown",
    }
}
