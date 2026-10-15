use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::service::store::{HealthSnapshot, HealthStatus};

/// Probe a URL and map the response to a HealthSnapshot.
///
/// Status resolution (in order):
/// 1. Non-2xx HTTP → Down.
/// 2. Empty body or non-JSON body + 2xx → Healthy (link is up, no body to check).
/// 3. JSON body, `json_field` key present → compare value (case-insensitive) against
///    `ok_values` / `degraded_values`. Unknown value → Down.
/// 4. JSON body, `json_field` key absent → Healthy (field not required by this service).
///
/// Defaults when params are `None`:
/// - `json_field`: `"status"`
/// - `ok_values`: `["ok", "healthy", "up"]`
/// - `degraded_values`: `["degraded", "starting"]`
pub async fn probe_http(
    url: &str,
    timeout_ms: u64,
    json_field: Option<&str>,
    ok_values: Option<&[String]>,
    degraded_values: Option<&[String]>,
) -> std::io::Result<HealthSnapshot> {
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
            let parsed: Option<serde_json::Value> = serde_json::from_str(&body).ok();

            let status = match &parsed {
                None => HealthStatus::Healthy, // empty or non-JSON body — trust 2xx
                Some(p) => {
                    let field = json_field.unwrap_or("status");
                    match p.get(field).and_then(|v| v.as_str()) {
                        None => HealthStatus::Healthy, // field absent — trust 2xx
                        Some(val) => {
                            let upper = val.to_ascii_uppercase();
                            let is_ok = ok_values
                                .map(|vs| vs.iter().any(|v| v.to_ascii_uppercase() == upper))
                                .unwrap_or_else(|| {
                                    matches!(upper.as_str(), "OK" | "HEALTHY" | "UP")
                                });
                            let is_deg = degraded_values
                                .map(|vs| vs.iter().any(|v| v.to_ascii_uppercase() == upper))
                                .unwrap_or_else(|| {
                                    matches!(upper.as_str(), "DEGRADED" | "STARTING")
                                });
                            if is_ok {
                                HealthStatus::Healthy
                            } else if is_deg {
                                HealthStatus::Degraded
                            } else {
                                HealthStatus::Down
                            }
                        }
                    }
                }
            };

            let p = parsed.unwrap_or(serde_json::json!({}));
            Ok(HealthSnapshot {
                status,
                probed_at: Some(now_ms()),
                latency_ms: Some(latency_ms),
                version: p.get("version").and_then(|v| v.as_str()).map(String::from),
                uptime_secs: p.get("uptime_secs").and_then(|v| v.as_u64()),
                extra: p,
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
    async fn status_ok_is_healthy() {
        let mut server = Server::new_async().await;
        let _m = server
            .mock("GET", "/healthz")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"status":"ok","version":"1.2","uptime_secs":3600}"#)
            .create_async()
            .await;
        let snap = probe_http(&format!("{}/healthz", server.url()), 5000, None, None, None)
            .await
            .unwrap();
        assert_eq!(snap.status, HealthStatus::Healthy);
        assert_eq!(snap.version.as_deref(), Some("1.2"));
        assert_eq!(snap.uptime_secs, Some(3600));
    }

    #[tokio::test]
    async fn sonarqube_up_is_healthy() {
        let mut server = Server::new_async().await;
        let _m = server
            .mock("GET", "/api/system/status")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"status":"UP","version":"26.5.0"}"#)
            .create_async()
            .await;
        let snap = probe_http(
            &format!("{}/api/system/status", server.url()),
            5000,
            None,
            None,
            None,
        )
        .await
        .unwrap();
        assert_eq!(snap.status, HealthStatus::Healthy);
    }

    #[tokio::test]
    async fn custom_field_and_values() {
        let mut server = Server::new_async().await;
        let _m = server
            .mock("GET", "/health")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"state":"running","version":"2.0"}"#)
            .create_async()
            .await;
        let ok = vec!["running".to_string()];
        let snap = probe_http(
            &format!("{}/health", server.url()),
            5000,
            Some("state"),
            Some(&ok),
            None,
        )
        .await
        .unwrap();
        assert_eq!(snap.status, HealthStatus::Healthy);
    }

    #[tokio::test]
    async fn empty_body_is_healthy() {
        let mut server = Server::new_async().await;
        let _m = server
            .mock("GET", "/ping")
            .with_status(200)
            .with_body("")
            .create_async()
            .await;
        let snap = probe_http(&format!("{}/ping", server.url()), 5000, None, None, None)
            .await
            .unwrap();
        assert_eq!(snap.status, HealthStatus::Healthy);
    }

    #[tokio::test]
    async fn html_body_is_healthy() {
        let mut server = Server::new_async().await;
        let _m = server
            .mock("GET", "/")
            .with_status(200)
            .with_header("content-type", "text/html")
            .with_body("<html><body>OK</body></html>")
            .create_async()
            .await;
        let snap = probe_http(&format!("{}/", server.url()), 5000, None, None, None)
            .await
            .unwrap();
        assert_eq!(snap.status, HealthStatus::Healthy);
    }

    #[tokio::test]
    async fn missing_field_is_healthy() {
        let mut server = Server::new_async().await;
        let _m = server
            .mock("GET", "/api")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"version":"3.0","uptime":99}"#)
            .create_async()
            .await;
        let snap = probe_http(&format!("{}/api", server.url()), 5000, None, None, None)
            .await
            .unwrap();
        assert_eq!(snap.status, HealthStatus::Healthy);
    }

    #[tokio::test]
    async fn unknown_value_is_down() {
        let mut server = Server::new_async().await;
        let _m = server
            .mock("GET", "/healthz")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"status":"maintenance"}"#)
            .create_async()
            .await;
        let snap = probe_http(&format!("{}/healthz", server.url()), 5000, None, None, None)
            .await
            .unwrap();
        assert_eq!(snap.status, HealthStatus::Down);
    }

    #[tokio::test]
    async fn http_500_is_down() {
        let mut server = Server::new_async().await;
        let _m = server
            .mock("GET", "/healthz")
            .with_status(500)
            .create_async()
            .await;
        let snap = probe_http(&format!("{}/healthz", server.url()), 5000, None, None, None)
            .await
            .unwrap();
        assert_eq!(snap.status, HealthStatus::Down);
    }

    #[tokio::test]
    async fn timeout_is_down() {
        let snap = probe_http("http://192.0.2.1:1/healthz", 200, None, None, None)
            .await
            .unwrap();
        assert_eq!(snap.status, HealthStatus::Down);
    }
}
