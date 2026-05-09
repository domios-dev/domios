use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::services::store::{HealthSnapshot, HealthStatus};

pub async fn probe_http(url: &str, timeout_ms: u64) -> std::io::Result<HealthSnapshot> {
    let started = Instant::now();
    let client = reqwest::Client::builder()
        .timeout(Duration::from_millis(timeout_ms))
        .build()
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;

    match client.get(url).send().await {
        Ok(resp) => {
            let status_code = resp.status();
            let latency_ms = started.elapsed().as_millis() as u64;
            if !status_code.is_success() {
                return Ok(HealthSnapshot {
                    status: HealthStatus::Down,
                    probed_at: Some(now_ms()),
                    latency_ms: Some(latency_ms),
                    ..Default::default()
                });
            }
            let body = resp.text().await.unwrap_or_default();
            let parsed: serde_json::Value =
                serde_json::from_str(&body).unwrap_or(serde_json::json!({}));
            let status_str = parsed
                .get("status")
                .and_then(|v| v.as_str())
                .unwrap_or("ok");
            let status = match status_str {
                "ok" | "healthy" => HealthStatus::Healthy,
                "degraded" => HealthStatus::Degraded,
                _ => HealthStatus::Down,
            };
            Ok(HealthSnapshot {
                status,
                probed_at: Some(now_ms()),
                latency_ms: Some(latency_ms),
                version: parsed
                    .get("version")
                    .and_then(|v| v.as_str())
                    .map(String::from),
                uptime_secs: parsed.get("uptime_secs").and_then(|v| v.as_u64()),
                extra: parsed,
            })
        }
        Err(_) => Ok(HealthSnapshot {
            status: HealthStatus::Down,
            probed_at: Some(now_ms()),
            latency_ms: Some(started.elapsed().as_millis() as u64),
            ..Default::default()
        }),
    }
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockito::Server;

    #[tokio::test]
    async fn http_probe_200_ok_with_json() {
        let mut server = Server::new_async().await;
        let _m = server
            .mock("GET", "/healthz")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"status":"ok","version":"1.2","uptime_secs":3600}"#)
            .create_async()
            .await;

        let snap = probe_http(&format!("{}/healthz", server.url()), 5000)
            .await
            .unwrap();
        assert_eq!(snap.status, HealthStatus::Healthy);
        assert_eq!(snap.version.as_deref(), Some("1.2"));
        assert_eq!(snap.uptime_secs, Some(3600));
    }

    #[tokio::test]
    async fn http_probe_500_is_down() {
        let mut server = Server::new_async().await;
        let _m = server
            .mock("GET", "/healthz")
            .with_status(500)
            .create_async()
            .await;
        let snap = probe_http(&format!("{}/healthz", server.url()), 5000)
            .await
            .unwrap();
        assert_eq!(snap.status, HealthStatus::Down);
    }

    #[tokio::test]
    async fn http_probe_timeout_is_down() {
        let snap = probe_http("http://192.0.2.1:1/healthz", 200).await.unwrap();
        assert_eq!(snap.status, HealthStatus::Down);
    }
}
