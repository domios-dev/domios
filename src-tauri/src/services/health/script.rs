use std::process::Stdio;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use tokio::process::Command;
use tokio::time::timeout;

use crate::services::store::{HealthSnapshot, HealthStatus};

pub async fn probe_script(path: &str, timeout_ms: u64) -> std::io::Result<HealthSnapshot> {
    let started = Instant::now();
    if path.trim().is_empty() {
        return Ok(HealthSnapshot {
            status: HealthStatus::Unknown,
            probed_at: Some(now_ms()),
            ..Default::default()
        });
    }

    let mut cmd = Command::new(path);
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let fut = async {
        let child = cmd.spawn();
        match child {
            Ok(c) => c.wait_with_output().await,
            Err(e) => Err(e),
        }
    };

    let res = timeout(Duration::from_millis(timeout_ms), fut).await;
    let latency_ms = started.elapsed().as_millis() as u64;

    match res {
        Err(_) => Ok(HealthSnapshot {
            status: HealthStatus::Down,
            probed_at: Some(now_ms()),
            latency_ms: Some(latency_ms),
            ..Default::default()
        }),
        Ok(Err(_)) => Ok(HealthSnapshot {
            status: HealthStatus::Down,
            probed_at: Some(now_ms()),
            latency_ms: Some(latency_ms),
            ..Default::default()
        }),
        Ok(Ok(out)) => {
            let status = if out.status.success() {
                HealthStatus::Healthy
            } else {
                HealthStatus::Down
            };
            let stdout = String::from_utf8_lossy(&out.stdout);
            let parsed: serde_json::Value =
                serde_json::from_str(stdout.trim()).unwrap_or(serde_json::json!({}));
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

    #[cfg(unix)]
    fn find_unix_tool(name: &str) -> Option<String> {
        for prefix in ["/bin/", "/usr/bin/"] {
            let p = format!("{}{}", prefix, name);
            if std::path::Path::new(&p).exists() {
                return Some(p);
            }
        }
        None
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn script_exit_zero_is_healthy() {
        let Some(tool) = find_unix_tool("true") else {
            return;
        };
        let snap = probe_script(&tool, 5000).await.unwrap();
        assert_eq!(snap.status, HealthStatus::Healthy);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn script_exit_nonzero_is_down() {
        let Some(tool) = find_unix_tool("false") else {
            return;
        };
        let snap = probe_script(&tool, 5000).await.unwrap();
        assert_eq!(snap.status, HealthStatus::Down);
    }

    #[tokio::test]
    async fn script_missing_path_is_down() {
        let snap = probe_script("/this/path/does/not/exist/zzz", 1000)
            .await
            .unwrap();
        assert_eq!(snap.status, HealthStatus::Down);
    }

    #[tokio::test]
    async fn empty_path_is_unknown() {
        let snap = probe_script("", 1000).await.unwrap();
        assert_eq!(snap.status, HealthStatus::Unknown);
    }
}
