//! One tab's usage, resolved the way its usage bubble resolves it so the CLI and the UI agree.
//! Mirrors `tabUsageSource` (usageTracking.ts) and `latestUsageSnapshot`/`accountUsageSnapshot` (runtimeUsage.ts).

use super::{latest, USAGE};
use crate::multiplexer::types::TabBacking;
use crate::service::agent_runtime::launch_profile;
use crate::service::ServiceContext;
use codeterm_plugin::agent_runtime::{limits_source, RuntimeProfileLimitsSource};
use codeterm_types::agent::context::TurnContext;
use serde::Serialize;
use serde_json::{json, Value};
use std::time::Duration;

/// A balance stays current for this long (`balanceNeedsRead` in balanceModel.ts).
const BALANCE_STALE_MS: i64 = 20 * 60 * 1000;
/// Quota is read again after this long (`quotaReadDue` in quotaModel.ts).
const QUOTA_READ_INTERVAL_MS: i64 = 60_000;
/// Bounds a stuck endpoint; the read keeps running and its row lands in the store.
const READ_THROUGH_WAIT: Duration = Duration::from_secs(15);

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum TabLimitsSource {
    Meter {
        provider: String,
    },
    Account {
        provider: String,
    },
    Balance {
        provider: String,
        account_id: String,
    },
    Quota {
        provider: String,
        account_id: String,
    },
    Unavailable {
        provider: String,
        reason: String,
    },
}

impl TabLimitsSource {
    /// A chat-backend tab follows its launch profile's declaration; the kind alone decides.
    fn resolve(
        provider: &str,
        chat_backend: bool,
        runtime_usage: bool,
        declared: Option<(&str, RuntimeProfileLimitsSource)>,
    ) -> Self {
        let provider = provider.to_owned();
        match declared.filter(|_| chat_backend) {
            Some((_, RuntimeProfileLimitsSource::Account { provider })) => {
                Self::Account { provider }
            }
            Some((profile, RuntimeProfileLimitsSource::Balance { .. })) => Self::Balance {
                provider,
                account_id: limits_source::balance_account_key(profile),
            },
            Some((profile, RuntimeProfileLimitsSource::Quota { .. })) => Self::Quota {
                provider,
                account_id: limits_source::balance_account_key(profile),
            },
            Some((_, RuntimeProfileLimitsSource::Unavailable { reason })) => {
                Self::Unavailable { provider, reason }
            }
            None if chat_backend && !runtime_usage => Self::Unavailable {
                reason: format!("{provider} does not report usage limits"),
                provider,
            },
            None => Self::Meter { provider },
        }
    }

    /// The provider whose Usage Tracking toggle gates this reading.
    fn tracking_provider(&self) -> &str {
        match self {
            Self::Meter { provider }
            | Self::Account { provider }
            | Self::Balance { provider, .. }
            | Self::Quota { provider, .. }
            | Self::Unavailable { provider, .. } => provider,
        }
    }
}

struct TabFacts {
    provider: String,
    thread_id: Option<String>,
    /// Present only while the tab's runtime binding is authoritative; it gates the session's own reading.
    nonce: Option<String>,
    profile: Option<String>,
    context: Option<TurnContext>,
    source: TabLimitsSource,
}

fn tab_facts(svc: &ServiceContext, tab_id: &str) -> Result<TabFacts, String> {
    let (provider, thread_id, nonce, profile, context, chat_backend) = {
        let mux = svc.mux.read();
        let tab = mux.get_tab(tab_id).ok_or("Tab not found")?;
        let provider = tab.agent_type.clone().ok_or("Tab has no provider")?;
        let nonce = tab
            .session_binding_nonce
            .clone()
            .filter(|value| tab.reported_session_binding_nonce.as_ref() == Some(value));
        (
            provider,
            tab.agent_session_id.clone(),
            nonce,
            launch_profile::of_tab(tab),
            tab.context.clone(),
            matches!(tab.backing, TabBacking::ChatBackend { .. }),
        )
    };
    let capability = svc
        .agent_registry
        .with_provider(&provider, |p| p.agent_runtime())
        .flatten();
    let runtime_usage = capability.as_ref().is_some_and(|runtime| runtime.usage);
    let declared = profile
        .as_deref()
        .zip(capability)
        .and_then(|(id, runtime)| {
            let source = runtime.launch_profiles?.remove(id)?.limits_source?;
            Some((id, source))
        });
    let source = TabLimitsSource::resolve(&provider, chat_backend, runtime_usage, declared);
    Ok(TabFacts {
        provider,
        thread_id,
        nonce,
        profile,
        context,
        source,
    })
}

fn rows(value: &Value) -> &[Value] {
    match value.as_array() {
        Some(rows) => rows.as_slice(),
        None => &[],
    }
}

/// `runtimeLimitsSnapshot`: the session's own reading in the snapshot's shape.
fn runtime_limits_view(snapshot: &Value, reading: &Value) -> Value {
    let windows: &[Value] = if reading["billing_mode"] == "subscription" {
        rows(&reading["rate_limits"]["value"])
    } else {
        &[]
    };
    let named = |name: &str, position: usize| {
        windows
            .iter()
            .find(|window| window["window"] == name)
            .or(windows.get(position))
            .cloned()
            .unwrap_or(Value::Null)
    };
    let (primary, secondary) = (named("primary", 0), named("secondary", 1));
    let captured_at = [
        &reading["rate_limits"]["captured_at"],
        &reading["token_usage"]["captured_at"],
        &reading["captured_at"],
        &snapshot["captured_at"],
    ]
    .into_iter()
    .find(|value| !value.is_null())
    .cloned()
    .unwrap_or(Value::Null);
    let mut view = snapshot.clone();
    view["id"] = json!(0);
    view["account_id"] = Value::Null;
    view["captured_at"] = captured_at;
    view["parsed_ok"] = json!(true);
    view["billing_mode"] = reading["billing_mode"].clone();
    view["limits_reported"] = json!(!windows.is_empty());
    view["session_pct"] = primary["usedPercent"].clone();
    view["session_resets_at"] = primary["resetsAt"].clone();
    view["weekly_pct"] = secondary["usedPercent"].clone();
    view["weekly_resets_at"] = secondary["resetsAt"].clone();
    for cleared in [
        "spend_cents",
        "spend_limit_cents",
        "spend_remaining_cents",
        "spend_resets_at",
        "failure_reason",
        "probe_command",
    ] {
        view[cleared] = Value::Null;
    }
    view
}

/// The merge `latestUsageSnapshot` makes once it has a reading: a newer good subscription row speaks, otherwise the reading.
fn merged_view(snapshot: &Value, reading: &Value) -> Value {
    let limits_at = reading["rate_limits"]["captured_at"].as_i64().unwrap_or(0);
    let row_speaks = reading["billing_mode"] == "subscription"
        && snapshot["parsed_ok"] == true
        && snapshot["captured_at"].as_i64().unwrap_or(0) >= limits_at;
    let mut view = if row_speaks {
        snapshot.clone()
    } else {
        runtime_limits_view(snapshot, reading)
    };
    view["resets"] = reading["resets"].clone();
    view["account_credits"] = reading["account_credits"].clone();
    view
}

/// `latestUsageSnapshot` for this tab: its own reading, or nothing while siblings report and it does not.
fn own_view(snapshot: &Value, tab_id: &str, thread_id: Option<&str>) -> Value {
    let own = rows(&snapshot["runtime_usage"])
        .iter()
        .find(|row| row["tab_id"] == tab_id && row["thread_id"].as_str() == thread_id);
    match own {
        Some(reading) => merged_view(snapshot, reading),
        None if snapshot.get("runtime_usage").is_some() => Value::Null,
        None => snapshot.clone(),
    }
}

/// `accountUsageSnapshot`: a borrowing tab shows the native account through its freshest subscription limits.
fn account_view(snapshot: &Value) -> Value {
    let readings = rows(&snapshot["runtime_usage"]);
    if readings.is_empty() {
        return snapshot.clone();
    }
    let limits_at = |row: &Value| row["rate_limits"]["captured_at"].as_i64().unwrap_or(0);
    let mut freshest: Option<&Value> = None;
    for row in readings
        .iter()
        .filter(|row| row["billing_mode"] == "subscription" && !row["rate_limits"].is_null())
    {
        if freshest.is_none_or(|latest| limits_at(row) > limits_at(latest)) {
            freshest = Some(row);
        }
    }
    match freshest {
        Some(reading) => merged_view(snapshot, reading),
        None if snapshot["parsed_ok"] == true => snapshot.clone(),
        None => Value::Null,
    }
}

/// `hasQuota` for a meter; for a plugin read, the shape its bubble requires before it shows anything.
fn has_reading(source: &TabLimitsSource, view: &Value) -> bool {
    if view["parsed_ok"] != true {
        return false;
    }
    match source {
        TabLimitsSource::Balance { .. } => view["usage_details"]["balance"].is_object(),
        TabLimitsSource::Quota { .. } => view["usage_details"]["quota"].is_object(),
        TabLimitsSource::Unavailable { .. } => false,
        TabLimitsSource::Meter { .. } | TabLimitsSource::Account { .. } => {
            view["limits_reported"] == true
                || ["session_pct", "weekly_pct", "spend_cents"]
                    .iter()
                    .any(|meter| !view[*meter].is_null())
        }
    }
}

fn unavailable_reason(
    source: &TabLimitsSource,
    profile: Option<&str>,
    tracking: bool,
    view: &Value,
) -> String {
    if !tracking {
        return format!(
            "Usage tracking is disabled for {}",
            source.tracking_provider()
        );
    }
    if let Some(failure) = view["failure_reason"].as_str() {
        return failure.to_owned();
    }
    match source {
        TabLimitsSource::Unavailable { reason, .. } => reason.clone(),
        TabLimitsSource::Balance { .. } => "The balance has not been read yet".into(),
        TabLimitsSource::Quota { .. } => "Usage limits have not been read yet".into(),
        TabLimitsSource::Account { provider } => {
            format!("No {provider} account limits have been read yet")
        }
        TabLimitsSource::Meter { provider } => match profile {
            Some(profile) => format!(
                "Launch profile {profile} declares no limits source and its session has reported none"
            ),
            None => format!("{provider} has reported no usage limits for this tab"),
        },
    }
}

fn tracking_visible(svc: &ServiceContext, provider: &str) -> bool {
    crate::config::provider_config_from(&svc.configuration(), provider).usage_tracking_enabled
        != Some(false)
}

/// The tab's usage from what is already stored; `read_for_tab` also requests a due plugin read first.
pub fn for_tab(svc: &ServiceContext, tab_id: &str) -> Result<Value, String> {
    let facts = tab_facts(svc, tab_id)?;
    let tracking = tracking_visible(svc, facts.source.tracking_provider());
    let view = if !tracking {
        Value::Null
    } else {
        match &facts.source {
            TabLimitsSource::Balance {
                provider,
                account_id,
            }
            | TabLimitsSource::Quota {
                provider,
                account_id,
            } => latest(svc, provider, Some(account_id.as_str()))?,
            TabLimitsSource::Account { provider } => account_view(&latest(svc, provider, None)?),
            TabLimitsSource::Meter { provider } => own_view(
                &latest(svc, provider, None)?,
                tab_id,
                facts.thread_id.as_deref(),
            ),
            TabLimitsSource::Unavailable { .. } => Value::Null,
        }
    };
    let mut result = if view.is_null() {
        json!({"provider": facts.source.tracking_provider(), "limits_reported": false})
    } else {
        view
    };
    if !has_reading(&facts.source, &result) {
        let reason = unavailable_reason(&facts.source, facts.profile.as_deref(), tracking, &result);
        result["unavailable_reason"] = json!(reason);
        result["limits_reported"] = json!(false);
    }
    result["limits_source"] = serde_json::to_value(&facts.source).map_err(|e| e.to_string())?;
    result["context"] = serde_json::to_value(&facts.context).map_err(|e| e.to_string())?;
    let own = USAGE
        .lock()
        .get(tab_id)
        .filter(|reading| {
            reading.provider == facts.provider
                && Some(&reading.nonce) == facts.nonce.as_ref()
                && (reading.thread_id == facts.thread_id || reading.thread_id.is_none())
        })
        .cloned();
    if let Some(mut reading) = own {
        if reading.thread_id.is_none() {
            reading.thread_id = facts.thread_id;
        }
        result["runtime_usage"] = json!([reading]);
    } else if let Some(object) = result.as_object_mut() {
        object.remove("runtime_usage");
    }
    Ok(result)
}

/// A missing or failed reading is always due; a good one once it is older than its source's interval.
fn plugin_read_due(captured: Option<(bool, i64)>, now_ms: i64, stale_after_ms: i64) -> bool {
    captured
        .is_none_or(|(parsed_ok, captured_at)| !parsed_ok || now_ms - captured_at > stale_after_ms)
}

fn due_plugin_read(svc: &ServiceContext, tab_id: &str) -> Result<Option<(String, String)>, String> {
    let facts = tab_facts(svc, tab_id)?;
    let (provider, account_id, stale_after_ms) = match &facts.source {
        TabLimitsSource::Balance {
            provider,
            account_id,
        } => (provider, account_id, BALANCE_STALE_MS),
        TabLimitsSource::Quota {
            provider,
            account_id,
        } => (provider, account_id, QUOTA_READ_INTERVAL_MS),
        _ => return Ok(None),
    };
    if !crate::config::usage_tracking_enabled_from(&svc.configuration(), provider) {
        return Ok(None);
    }
    let Some(profile) = limits_source::profile_from_balance_account_key(account_id) else {
        return Ok(None);
    };
    let captured = svc
        .usage_store
        .get_latest_or_failure(provider, Some(account_id.as_str()))?
        .map(|row| (row.parsed_ok, row.captured_at));
    Ok(plugin_read_due(
        captured,
        chrono::Utc::now().timestamp_millis(),
        stale_after_ms,
    )
    .then(|| (provider.clone(), profile.to_owned())))
}

/// The answer the CLI gets: a background tab has no bubble to request its due read, so this read does.
pub async fn read_for_tab(svc: &ServiceContext, tab_id: &str) -> Result<Value, String> {
    let (reader, tab) = (svc.clone(), tab_id.to_owned());
    let due = tokio::task::spawn_blocking(move || due_plugin_read(&reader, &tab))
        .await
        .map_err(|error| error.to_string())??;
    if let Some((provider, profile)) = due {
        let read = crate::usage::profile_limits::spawn_refresh(
            svc.clone(),
            svc.usage_store.clone(),
            provider,
            profile,
        );
        if let Some(read) = read {
            // Past the bound the answer keeps what is stored; the read itself carries on.
            let _ = tokio::time::timeout(READ_THROUGH_WAIT, read).await;
        }
    }
    let (reader, tab) = (svc.clone(), tab_id.to_owned());
    tokio::task::spawn_blocking(move || for_tab(&reader, &tab))
        .await
        .map_err(|error| error.to_string())?
}

#[cfg(test)]
mod tests;
