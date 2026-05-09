use std::net::{SocketAddr, ToSocketAddrs};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::services::store::{HealthSnapshot, HealthStatus};

pub async fn probe_tcp(host: &str, port: u16, timeout_ms: u64) -> std::io::Result<HealthSnapshot> {
    let host = host.to_string();
    let timeout = Duration::from_millis(timeout_ms);
    tokio::task::spawn_blocking(move || probe_blocking(&host, port, timeout))
        .await
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?
}

fn probe_blocking(host: &str, port: u16, timeout: Duration) -> std::io::Result<HealthSnapshot> {
    let started = Instant::now();
    let target = format!("{}:{}", host, port);
    let addr: Option<SocketAddr> = target.to_socket_addrs().ok().and_then(|mut it| it.next());

    let snap_down = || HealthSnapshot {
        status: HealthStatus::Down,
        probed_at: Some(now_ms()),
        latency_ms: Some(started.elapsed().as_millis() as u64),
        ..Default::default()
    };

    let Some(addr) = addr else {
        return Ok(snap_down());
    };

    match std::net::TcpStream::connect_timeout(&addr, timeout) {
        Ok(_) => Ok(HealthSnapshot {
            status: HealthStatus::Healthy,
            probed_at: Some(now_ms()),
            latency_ms: Some(started.elapsed().as_millis() as u64),
            ..Default::default()
        }),
        Err(_) => Ok(snap_down()),
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
    use std::net::TcpListener;

    #[tokio::test]
    async fn tcp_probe_open_port_is_healthy() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let snap = probe_tcp("127.0.0.1", port, 1000).await.unwrap();
        assert_eq!(snap.status, HealthStatus::Healthy);
        assert!(snap.latency_ms.is_some());
    }

    #[tokio::test]
    async fn tcp_probe_closed_port_is_down() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let snap = probe_tcp("127.0.0.1", port, 500).await.unwrap();
        assert_eq!(snap.status, HealthStatus::Down);
    }

    #[tokio::test]
    async fn tcp_probe_unroutable_is_down() {
        let snap = probe_tcp("192.0.2.1", 1, 200).await.unwrap();
        assert_eq!(snap.status, HealthStatus::Down);
    }
}
