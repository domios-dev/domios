use std::collections::{HashMap, VecDeque};
use std::sync::LazyLock;

use axum::http::StatusCode;
use codeterm_service_core::mesh::{
    account_mesh::ledger::LinkAdmission,
    crown::AuthenticatedAgent,
    manager::MeshManager,
    types::{AccessLevel, RelayResponse},
};
use codeterm_service_core::notification::vet::{created_result, parse_push_intent, vet_title};
use codeterm_service_core::service::notification::PushIntent;
use parking_lot::Mutex;
use serde::Deserialize;

use crate::auth::{peer_grant::PeerGrant, AuthenticatedOrigin};
use crate::routes::AppState;

pub const MAX_PER_PEER_NOTIFICATIONS: usize = 10;
pub const PER_PEER_NOTIFICATION_WINDOW_MS: u64 = 60_000;
const MAX_TRACKED_PEERS: usize = 1024;

pub const PEER_NOTIFICATION_RATE_LIMITED: &str = "peer_notification_rate_limited";
pub const REMOTE_NOTIFICATION_REJECTED: &str = "remote_notification_rejected";

const FORBIDDEN_REMOTE_NOTIFICATION_FIELDS: &[&str] = &[
    "source_peer",
    "tab_id",
    "workspace_id",
    "target_tab_id",
    "chat_message_id",
    "offer",
    "offer_accept",
    "actions",
];

#[derive(Deserialize)]
struct RemoteNotificationBody {
    title: String,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    severity: Option<String>,
    #[serde(default)]
    sticky: Option<bool>,
    #[serde(default)]
    level: Option<String>,
    #[serde(default)]
    event_key: Option<String>,
}

struct ParsedRemoteNotification {
    title: String,
    body: Option<String>,
    severity: Option<String>,
    sticky: Option<bool>,
    intent: PushIntent,
}

struct PeerNotificationRateLimiter {
    max_per_peer: usize,
    window_ms: u64,
    max_tracked_peers: usize,
    buckets: HashMap<String, VecDeque<u64>>,
}

impl PeerNotificationRateLimiter {
    fn new(max_per_peer: usize, window_ms: u64, max_tracked_peers: usize) -> Self {
        Self {
            max_per_peer,
            window_ms,
            max_tracked_peers,
            buckets: HashMap::new(),
        }
    }

    fn evict_expired(bucket: &mut VecDeque<u64>, window_ms: u64, now_ms: u64) {
        while bucket
            .front()
            .is_some_and(|&ts| ts.saturating_add(window_ms) <= now_ms)
        {
            bucket.pop_front();
        }
    }

    fn check_and_record(&mut self, peer_id: &str, now_ms: u64) -> bool {
        if self.max_per_peer == 0 {
            return false;
        }
        if let Some(bucket) = self.buckets.get_mut(peer_id) {
            Self::evict_expired(bucket, self.window_ms, now_ms);
            if bucket.len() >= self.max_per_peer {
                return false;
            }
            bucket.push_back(now_ms);
            return true;
        }
        if self.buckets.len() >= self.max_tracked_peers {
            let window_ms = self.window_ms;
            self.buckets.retain(|_, bucket| {
                Self::evict_expired(bucket, window_ms, now_ms);
                !bucket.is_empty()
            });
            if self.buckets.len() >= self.max_tracked_peers {
                if let Some(oldest_peer) = self
                    .buckets
                    .iter()
                    .min_by_key(|(_, bucket)| bucket.back().copied().unwrap_or(0))
                    .map(|(peer, _)| peer.clone())
                {
                    self.buckets.remove(&oldest_peer);
                }
            }
        }
        let mut bucket = VecDeque::with_capacity(self.max_per_peer.min(16));
        bucket.push_back(now_ms);
        self.buckets.insert(peer_id.to_owned(), bucket);
        true
    }
}

static RATE_LIMITER: LazyLock<Mutex<PeerNotificationRateLimiter>> = LazyLock::new(|| {
    Mutex::new(PeerNotificationRateLimiter::new(
        MAX_PER_PEER_NOTIFICATIONS,
        PER_PEER_NOTIFICATION_WINDOW_MS,
        MAX_TRACKED_PEERS,
    ))
});

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or(0)
}

fn parse_remote_notification_body(
    raw_body: &serde_json::Value,
) -> Result<ParsedRemoteNotification, String> {
    let Some(obj) = raw_body.as_object() else {
        return Err("Remote notification body must be a JSON object".into());
    };
    for field in FORBIDDEN_REMOTE_NOTIFICATION_FIELDS {
        if obj.get(*field).is_some_and(|value| !value.is_null()) {
            return Err(format!("Remote notification must not supply '{field}'"));
        }
    }
    let parsed: RemoteNotificationBody = serde_json::from_value(raw_body.clone())
        .map_err(|error| format!("Invalid remote notification body: {error}"))?;
    let title = vet_title(&parsed.title)?;
    let intent = parse_push_intent(parsed.level.as_deref(), parsed.event_key)?;
    Ok(ParsedRemoteNotification {
        title,
        body: parsed.body,
        severity: parsed.severity,
        sticky: parsed.sticky,
        intent,
    })
}

fn verified_full_account_peer_name(
    mesh: &MeshManager,
    peer_id: &str,
    grant: &PeerGrant,
) -> Option<String> {
    if !matches!(grant.access(), AccessLevel::Full) || !grant.current(mesh) {
        return None;
    }
    let signed_in_account = mesh.account_state().signed_in_account()?;
    let link = mesh.link_record(peer_id)?;
    let LinkAdmission::Account { account_id, .. } = &link.admission;
    if account_id != signed_in_account {
        return None;
    }
    let peer = mesh.paired_peers.get(peer_id)?;
    if peer.access_level != AccessLevel::Full {
        return None;
    }
    let name = peer.instance_name.trim();
    if name.is_empty() {
        Some(peer_id.to_owned())
    } else {
        Some(name.to_owned())
    }
}

pub(super) async fn dispatch_remote_notification(
    state: &AppState,
    peer_id: &str,
    body: &serde_json::Value,
    origin: AuthenticatedOrigin,
    agent: Option<&AuthenticatedAgent>,
    grant: &PeerGrant,
) -> Result<(RelayResponse, bool), (StatusCode, String)> {
    if !origin.owner_trusted() || agent.is_some() {
        return Ok((
            super::relay_failure(
                StatusCode::FORBIDDEN.as_u16(),
                codeterm_mesh::contract::delivery::PEER_RELAY_NOT_AUTHORIZED,
                Some(false),
                "Remote notifications require an Owner or Mesh origin; remote agents are denied"
                    .into(),
            ),
            true,
        ));
    }

    let Some(peer_name) = verified_full_account_peer_name(&state.mesh.lock(), peer_id, grant)
    else {
        return Ok((super::grant_refusal(), true));
    };

    let parsed = match parse_remote_notification_body(body) {
        Ok(parsed) => parsed,
        Err(message) => {
            return Ok((
                super::relay_failure(
                    StatusCode::BAD_REQUEST.as_u16(),
                    REMOTE_NOTIFICATION_REJECTED,
                    Some(false),
                    message,
                ),
                true,
            ));
        }
    };

    if !RATE_LIMITER.lock().check_and_record(peer_id, now_ms()) {
        return Ok((
            super::relay_failure(
                StatusCode::TOO_MANY_REQUESTS.as_u16(),
                PEER_NOTIFICATION_RATE_LIMITED,
                Some(false),
                format!(
                    "Remote notification rate limit exceeded ({MAX_PER_PEER_NOTIFICATIONS} per {}s)",
                    PER_PEER_NOTIFICATION_WINDOW_MS / 1000
                ),
            ),
            true,
        ));
    }

    if verified_full_account_peer_name(&state.mesh.lock(), peer_id, grant).is_none() {
        return Ok((super::grant_refusal(), true));
    }

    let svc = state.svc.clone();
    let created = tokio::task::spawn_blocking(move || {
        svc.create_remote_peer_notification_reported(
            parsed.title,
            parsed.body,
            parsed.severity.as_deref(),
            parsed.sticky,
            peer_name,
            parsed.intent,
        )
    })
    .await
    .map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "blocking task failed".to_string(),
        )
    })?;

    match created {
        Ok((id, unseen)) => Ok((
            RelayResponse {
                ok: true,
                status: StatusCode::OK.as_u16(),
                data: Some(created_result(&id, unseen)),
                error: None,
            },
            false,
        )),
        Err(message) => Ok((
            super::relay_failure(
                StatusCode::BAD_REQUEST.as_u16(),
                REMOTE_NOTIFICATION_REJECTED,
                Some(false),
                message,
            ),
            true,
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn rate_limiter_enforces_per_peer_quota_isolation_window_refill_and_bounds() {
        let mut limiter = PeerNotificationRateLimiter::new(3, 60_000, 2);

        assert!(limiter.check_and_record("peer-a", 1_000));
        assert!(limiter.check_and_record("peer-a", 1_100));
        assert!(limiter.check_and_record("peer-a", 1_200));
        assert!(!limiter.check_and_record("peer-a", 1_300));

        // Peer B has an independent quota.
        assert!(limiter.check_and_record("peer-b", 1_300));
        assert!(limiter.check_and_record("peer-b", 1_400));

        // Window expiry refills peer A's oldest slot.
        assert!(!limiter.check_and_record("peer-a", 60_999));
        assert!(limiter.check_and_record("peer-a", 61_000));
        assert!(!limiter.check_and_record("peer-a", 61_050));

        // Bounded map evicts expired/oldest peers when max_tracked_peers is reached.
        assert!(limiter.check_and_record("peer-c", 61_050));
        assert!(limiter.buckets.len() <= 2);
    }

    #[test]
    fn parser_accepts_display_only_and_rejects_every_forbidden_field() {
        let valid = parse_remote_notification_body(&json!({
            "title": "Build completed",
            "body": "all green",
            "severity": "info",
            "level": "verbose",
            "sticky": false
        }))
        .expect("display-only payload must pass");
        assert_eq!(valid.title, "Build completed");
        assert_eq!(valid.body.as_deref(), Some("all green"));

        for (field, value) in [
            ("source_peer", json!("spoofed")),
            ("tab_id", json!("local-tab")),
            ("workspace_id", json!("local-ws")),
            ("target_tab_id", json!("local-tab")),
            ("chat_message_id", json!("msg-1")),
            ("offer", json!("desktop")),
            ("offer_accept", json!("Enable")),
            ("actions", json!([{"type": "answer", "label": "Yes"}])),
            ("actions", json!([])),
        ] {
            let mut payload = json!({ "title": "Remote notice" });
            payload[field] = value;
            let err = parse_remote_notification_body(&payload)
                .err()
                .expect("forbidden field must be rejected");
            assert!(
                err.contains(field),
                "expected error mentioning '{field}', got: {err}"
            );
        }
    }
}
