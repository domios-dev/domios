//! Turn a completed relay response into the peer's own verdict: success data or
//! a structured failure. Pure — no I/O, no process exit.

use super::MeshSpawnFailure;
use codeterm_mesh::contract::delivery;

#[cfg(test)]
mod tests;

/// Whether repeating the identical request can ever succeed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RefusalClass {
    Deterministic,
    Transient,
    Unknown,
}

impl RefusalClass {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Deterministic => "deterministic",
            Self::Transient => "transient",
            Self::Unknown => "unknown",
        }
    }
}

const PEER_LACKS_DELAYED_RECURRENCE: &str = "peer_lacks_delayed_recurrence";

/// Class from the peer's own code first, then the HTTP status. A 5xx with no
/// code is `Unknown`: only an explicit signal may promise a retry can work.
pub(crate) fn refusal_class(code: Option<&str>, status: u16) -> RefusalClass {
    match code {
        Some(
            delivery::PEER_RELAY_NOT_AUTHORIZED
            | delivery::PEER_RELAY_PROTOCOL_MISMATCH
            | delivery::PAYLOAD_TOO_LARGE
            | delivery::TAB_NOT_REACHABLE_ON_PEER
            | delivery::EXEC_AGENT_TAB_REFUSED
            | delivery::SHELL_EXITED_REFUSED
            | PEER_LACKS_DELAYED_RECURRENCE,
        ) => RefusalClass::Deterministic,
        Some(
            delivery::MESH_DELIVERY_WINDOW_SATURATED
            | delivery::TAB_BUSY_REFUSED
            | delivery::PEER_LOCAL_API_UNREACHABLE
            | delivery::PEER_UNREACHABLE
            | delivery::MESH_TRANSPORT_NOTHING_SENT,
        ) => RefusalClass::Transient,
        Some(
            delivery::SUBMIT_UNCONFIRMED
            | delivery::TARGET_RESTARTING
            | delivery::MESH_TRANSPORT_SUBMIT_UNCONFIRMED
            | delivery::RELAY_DELIVERY_DEADLINE_EXCEEDED,
        ) => RefusalClass::Unknown,
        _ => match status {
            429 | 502..=504 => RefusalClass::Transient,
            400..=499 => RefusalClass::Deterministic,
            _ => RefusalClass::Unknown,
        },
    }
}

/// The peer instance that produced a relayed answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AnsweringPeer {
    pub(crate) instance_id: String,
    pub(crate) instance_name: String,
}

impl AnsweringPeer {
    pub(crate) fn from_envelope(envelope: &serde_json::Value) -> Option<Self> {
        let peer = envelope.get("peer")?;
        let text = |key: &str| peer.get(key).and_then(|v| v.as_str()).map(str::to_string);
        Some(Self {
            instance_id: text("instance_id")?,
            instance_name: text("instance_name")?,
        })
    }

    pub(crate) fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "instance_id": self.instance_id,
            "instance_name": self.instance_name,
        })
    }

    pub(crate) fn short_id(&self) -> &str {
        self.instance_id.get(..8).unwrap_or(&self.instance_id)
    }
}

/// Success data plus the peer that answered.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RelayAnswer {
    pub(crate) data: serde_json::Value,
    pub(crate) peer: Option<AnsweringPeer>,
}

/// Fallback next-step hint by HTTP status class, used when the peer's
/// response carries no `retry_hint` of its own.
fn generic_retry_hint(status: u16) -> String {
    match status {
        429 => "rate limited — back off and retry shortly".to_string(),
        502..=504 => {
            "transient — retry once; if it repeats, check the peer daemon is up (`domios mesh peers`)"
                .to_string()
        }
        500..=599 => "the peer failed the request and did not say whether a retry can help — check \
             `domios mesh peers` and the peer's log before retrying"
            .to_string(),
        400..=499 => "request rejected — verify the requested peer, tab, and operation".to_string(),
        _ => "retry the spawn".to_string(),
    }
}

fn is_cgnat_ip(token: &str) -> bool {
    let octets: Vec<&str> = token.split('.').collect();
    if octets.len() != 4 {
        return false;
    }
    let parsed: Option<Vec<u8>> = octets.iter().map(|o| o.parse::<u8>().ok()).collect();
    matches!(parsed.as_deref(), Some([100, second, _, _]) if (64..=127).contains(second))
}

// A relay failure that names a Tailscale address (100.64/10 CGNAT or *.ts.net)
// usually means the local Tailscale path dropped, not that the peer is down.
pub(crate) fn tailscale_unreachable_hint(text: &str) -> Option<String> {
    let ts_dns = text.contains(".ts.net");
    let cgnat = text
        .split(|c: char| !c.is_ascii_digit() && c != '.')
        .any(is_cgnat_ip);
    (ts_dns || cgnat).then(|| {
        "the peer is a Tailscale address that timed out — Tailscale may have dropped the path (the \
         peer is likely fine). Try `tailscale ping <peer>` or restart Tailscale on this machine, \
         then confirm the peer is up."
            .to_string()
    })
}

/// The peer's own delivery verdict, read off the relayed body.
#[derive(Default)]
struct PeerVerdict {
    code: Option<String>,
    /// Three-valued: absent means unknown, never zero.
    bytes_written: Option<bool>,
    message: Option<String>,
    outcome: Option<String>,
    output: Option<String>,
    output_truncated: bool,
}

fn peer_delivery_verdict(data: &serde_json::Value) -> PeerVerdict {
    let error = data.get("error").filter(|e| !e.is_null()).unwrap_or(data);
    let text = |key: &str| error.get(key).and_then(|v| v.as_str()).map(str::to_string);
    let outcome = data
        .get("outcome")
        .and_then(|v| v.as_str())
        .or_else(|| error.get("outcome").and_then(|v| v.as_str()))
        .map(str::to_string);
    PeerVerdict {
        code: text("code"),
        bytes_written: error
            .get("bytes_written")
            .or_else(|| data.get("bytes_written"))
            .and_then(|v| v.as_bool()),
        message: text("message").or_else(|| error.as_str().map(str::to_string)),
        outcome,
        output: data
            .get("output")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        output_truncated: data.get("output_truncated").and_then(|v| v.as_bool()) == Some(true),
    }
}

fn delivery_outcome(
    relayed: (&str, &str),
    explicit: Option<String>,
    code: Option<&str>,
    bytes_written: Option<bool>,
) -> Option<String> {
    if explicit.is_some() {
        return explicit;
    }
    if code == Some(delivery::SUBMIT_UNCONFIRMED) {
        return Some(delivery::OUTCOME_SUBMIT_UNCONFIRMED.into());
    }
    if !super::is_delivery_relay(relayed.0, relayed.1) && code.is_none() {
        return None;
    }
    match (code, bytes_written) {
        (_, Some(false)) => Some(delivery::OUTCOME_REFUSED.into()),
        (
            Some(
                delivery::TARGET_RESTARTING
                | delivery::MESH_TRANSPORT_SUBMIT_UNCONFIRMED
                | delivery::RELAY_DELIVERY_DEADLINE_EXCEEDED,
            ),
            _,
        ) => Some(delivery::OUTCOME_UNKNOWN.into()),
        (_, Some(true)) => Some(delivery::OUTCOME_UNKNOWN.into()),
        _ if super::is_delivery_relay(relayed.0, relayed.1) => {
            Some(delivery::OUTCOME_UNKNOWN.into())
        }
        _ => None,
    }
}

/// What a caller may safely do next, given what the payload is known to have
/// done and whether the refusal can ever clear. An unknown outcome is never
/// advertised as a free retry.
fn delivery_retry_hint(code: Option<&str>, bytes_written: Option<bool>, status: u16) -> String {
    match (code, bytes_written) {
        (Some(_), Some(true)) => "the payload already reached the target PTY — check the target \
             before resending; a retry may deliver it twice"
            .to_string(),
        (Some(_), Some(false)) => match refusal_class(code, status) {
            RefusalClass::Deterministic => "refused, nothing was written — the same request will \
                 be refused again; change the request, the target, or the peer's access level"
                .to_string(),
            RefusalClass::Transient => {
                "nothing was written to the target — safe to retry once".to_string()
            }
            RefusalClass::Unknown => "nothing was written; the peer did not say whether the \
                 refusal can clear — check the target before resending"
                .to_string(),
        },
        (Some(_), None) => "the delivery outcome is unknown — confirm on the target before \
             resending; a blind retry may deliver it twice"
            .to_string(),
        _ => generic_retry_hint(status),
    }
}

fn relay_retry_hint(relayed: (&str, &str), status: u16) -> String {
    let path = relayed.1.split('?').next().unwrap_or(relayed.1);
    if status == 404 && relayed.0.eq_ignore_ascii_case("GET") && path.ends_with("/text") {
        "the tab has no PTY; choose a PTY-backed tab".to_string()
    } else if super::is_delivery_relay(relayed.0, relayed.1) && matches!(status, 500..=599) {
        "the delivery outcome is unknown — confirm on the target before resending; a blind retry may deliver it twice".to_string()
    } else {
        generic_retry_hint(status)
    }
}

fn route_retry_hint(
    relayed: (&str, &str),
    data: &serde_json::Value,
    code: Option<&str>,
    bytes_written: Option<bool>,
    status: u16,
) -> String {
    if let Some(hint) = data
        .get("error")
        .and_then(|error| {
            error
                .get("retry_hint")
                .or_else(|| error.get("recovery"))
                .or_else(|| error.get("hint"))
        })
        .or_else(|| {
            data.get("recovery")
                .or_else(|| data.get("retry_hint"))
                .or_else(|| data.get("hint"))
        })
        .and_then(serde_json::Value::as_str)
    {
        return hint.to_string();
    }
    if code == Some(PEER_LACKS_DELAYED_RECURRENCE) {
        return "update the peer or send a one-shot explicitly".to_string();
    }
    if !super::is_delivery_relay(relayed.0, relayed.1)
        || (code.is_none() && data.get("error").is_none())
    {
        relay_retry_hint(relayed, status)
    } else {
        delivery_retry_hint(code, bytes_written, status)
    }
}

fn failure_of(
    peer_name: &str,
    peer: Option<&AnsweringPeer>,
    reason: String,
    hint: String,
    class: RefusalClass,
) -> MeshSpawnFailure {
    MeshSpawnFailure::new(peer_name, reason, hint).answered_by(peer, class)
}

/// Classify a completed relay response into an answer or a structured failure.
pub(crate) fn classify_relay_answer(
    peer_name: &str,
    relayed: (&str, &str),
    status: reqwest::StatusCode,
    bytes: &[u8],
) -> Result<RelayAnswer, MeshSpawnFailure> {
    let val: serde_json::Value = serde_json::from_slice(bytes).unwrap_or(serde_json::Value::Null);
    let peer = AnsweringPeer::from_envelope(&val);
    let peer_name = peer
        .as_ref()
        .map(|p| p.instance_name.as_str())
        .unwrap_or(peer_name);

    // A local relay refusal can itself be JSON without being a RelayResponse.
    if !status.is_success() && val.get("ok").is_none() {
        if val.get("code").and_then(serde_json::Value::as_str) == Some(delivery::PEER_UNREACHABLE) {
            return Err(failure_of(
                peer_name,
                None,
                val.get("message")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("Peer is paired but not currently connected")
                    .to_string(),
                val.get("hint")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("Check `domios mesh peers` for connection progress.")
                    .to_string(),
                RefusalClass::Transient,
            )
            .with_delivery(Some(delivery::PEER_UNREACHABLE.to_string()), Some(false))
            .with_exec_observation(Some(delivery::OUTCOME_REFUSED.into()), None, false));
        }
        if val.get("code").and_then(serde_json::Value::as_str)
            == Some(delivery::PEER_RELAY_PROTOCOL_MISMATCH)
        {
            return Err(MeshSpawnFailure::refused_before_send(
                peer_name,
                delivery::PEER_RELAY_PROTOCOL_MISMATCH,
                val.get("message")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("Peer runs an incompatible Domios; update it through the updater"),
                "Update the peer through the updater, then retry.",
            ));
        }
        let preview = String::from_utf8_lossy(&bytes[..bytes.len().min(512)]);
        let refusal_reason = val.get("reason").and_then(serde_json::Value::as_str);
        let peer_offline = refusal_reason == Some("peer_name_no_online_record");
        let ambiguous = refusal_reason == Some("peer_name_ambiguous");
        let reason = if peer_offline {
            format!(
                "peer_offline: no online mesh route for {}",
                val.get("name")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or(peer_name)
            )
        } else if ambiguous {
            ambiguous_reason(&val, peer_name)
        } else if preview.trim().is_empty() {
            format!("HTTP {}", status.as_u16())
        } else {
            format!("HTTP {}: {}", status.as_u16(), preview.trim())
        };
        let (mut retry, class) = if peer_offline {
            (
                "bring the peer online, then retry".to_string(),
                RefusalClass::Transient,
            )
        } else if ambiguous {
            (
                "re-run with one candidate's instance id as --mesh".to_string(),
                RefusalClass::Deterministic,
            )
        } else {
            let class = if super::is_delivery_relay(relayed.0, relayed.1) && status.as_u16() >= 500
            {
                RefusalClass::Unknown
            } else {
                refusal_class(None, status.as_u16())
            };
            (relay_retry_hint(relayed, status.as_u16()), class)
        };
        if status.as_u16() >= 500 {
            if let Some(ts) = tailscale_unreachable_hint(&preview) {
                retry.push_str("\nhint: ");
                retry.push_str(&ts);
            }
        }
        let outcome = if peer_offline || ambiguous {
            Some(delivery::OUTCOME_REFUSED.into())
        } else if super::is_delivery_relay(relayed.0, relayed.1) && status.as_u16() >= 500 {
            Some(delivery::OUTCOME_UNKNOWN.into())
        } else {
            None
        };
        return Err(failure_of(peer_name, peer.as_ref(), reason, retry, class)
            .with_exec_observation(outcome, None, false));
    }

    let data = val.get("data").cloned().unwrap_or(serde_json::Value::Null);
    let PeerVerdict {
        code,
        bytes_written,
        message,
        outcome,
        output,
        output_truncated,
    } = peer_delivery_verdict(&data);
    let envelope_status = val
        .get("status")
        .and_then(|v| v.as_u64())
        .unwrap_or(status.as_u16() as u64) as u16;
    let outcome = delivery_outcome(relayed, outcome, code.as_deref(), bytes_written);

    // Relay returns 200+ok:false on auth/HTTP failures inside the envelope.
    if val.get("ok").and_then(|v| v.as_bool()) == Some(false) {
        let reason = val
            .get("error")
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .or_else(|| {
                val.get("error")?
                    .get("message")?
                    .as_str()
                    .map(str::to_string)
            })
            .filter(|reason| reason != &format!("HTTP {envelope_status}"))
            .or_else(|| match (&code, &message) {
                (Some(code), Some(message)) => Some(format!("{code}: {message}")),
                (Some(code), None) => Some(code.clone()),
                (None, Some(message)) => Some(message.clone()),
                _ => None,
            })
            .unwrap_or_else(|| format!("HTTP {envelope_status}"));
        let hint = route_retry_hint(
            relayed,
            &data,
            code.as_deref(),
            bytes_written,
            envelope_status,
        );
        let class = if super::is_delivery_relay(relayed.0, relayed.1)
            && bytes_written != Some(false)
            && code.is_none()
            && envelope_status >= 500
        {
            RefusalClass::Unknown
        } else {
            refusal_class(code.as_deref(), envelope_status)
        };
        return Err(failure_of(peer_name, peer.as_ref(), reason, hint, class)
            .with_delivery(code, bytes_written)
            .with_exec_observation(outcome, output, output_truncated));
    }

    let peer_refused = data.get("ok").and_then(|v| v.as_bool()) == Some(false);
    let rpc_error =
        relayed.1 != "/api/mesh/rpc" && data.get("error").is_some_and(|error| !error.is_null());
    if peer_refused || rpc_error {
        let reason = match (&code, &message) {
            (Some(code), Some(message)) => format!("{code}: {message}"),
            (Some(code), None) => code.clone(),
            (None, Some(message)) => message.clone(),
            _ => format!("peer refused the request (HTTP {envelope_status})"),
        };
        let hint = route_retry_hint(
            relayed,
            &data,
            code.as_deref(),
            bytes_written,
            envelope_status,
        );
        let class = refusal_class(code.as_deref(), envelope_status);
        return Err(failure_of(peer_name, peer.as_ref(), reason, hint, class)
            .with_delivery(code, bytes_written)
            .with_exec_observation(outcome, output, output_truncated));
    }

    Ok(RelayAnswer { data, peer })
}

/// Names every candidate so the caller can re-address by id.
fn ambiguous_reason(refusal: &serde_json::Value, target: &str) -> String {
    let listed: Vec<String> = refusal
        .get("candidates")
        .and_then(serde_json::Value::as_array)
        .map(|candidates| {
            candidates
                .iter()
                .map(|c| {
                    let text = |key: &str| c.get(key).and_then(|v| v.as_str()).unwrap_or("?");
                    format!("{} ({})", text("instance_name"), text("instance_id"))
                })
                .collect()
        })
        .unwrap_or_default();
    format!(
        "peer_name_ambiguous: '{}' matches {} online peers: {}",
        refusal
            .get("name")
            .and_then(serde_json::Value::as_str)
            .unwrap_or(target),
        listed.len(),
        listed.join(", ")
    )
}

/// Data-only view of [`classify_relay_answer`].
#[cfg(test)]
pub(super) fn classify_relay_response(
    peer_name: &str,
    relayed: (&str, &str),
    status: reqwest::StatusCode,
    bytes: &[u8],
) -> Result<serde_json::Value, MeshSpawnFailure> {
    classify_relay_answer(peer_name, relayed, status, bytes).map(|answer| answer.data)
}
