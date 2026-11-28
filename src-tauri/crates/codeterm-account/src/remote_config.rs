//! Firebase Remote Config switches kept in memory. A failed fetch clears both switches.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde_json::{json, Value};

use crate::InstallationId;

/// One fetch at startup and one per interval keeps far inside the free quota.
pub const REFRESH_INTERVAL: Duration = Duration::from_secs(5 * 60 * 60);
const FETCH_TIMEOUT: Duration = Duration::from_secs(10);

static PURCHASES_ENABLED: AtomicBool = AtomicBool::new(false);
static CLOUD_MACHINES_ENABLED: AtomicBool = AtomicBool::new(false);

/// Purchases are on only when the template holds exactly the string "true".
pub fn purchases_enabled_in(response: &Value) -> bool {
    response
        .pointer("/entries/purchases_enabled")
        .and_then(Value::as_str)
        == Some("true")
}

fn cloud_machines_enabled_in(response: &Value) -> bool {
    response
        .pointer("/entries/cloud_machines_enabled")
        .and_then(Value::as_str)
        == Some("true")
}

pub fn cloud_machines_enabled() -> bool {
    CLOUD_MACHINES_ENABLED.load(Ordering::Acquire)
}

/// The last fetched value; false until a fetch says otherwise.
pub fn purchases_enabled() -> bool {
    PURCHASES_ENABLED.load(Ordering::Acquire)
}

/// Fetches once and stores the result; returns whether the switch changed.
pub async fn refresh(installation: &InstallationId) -> bool {
    let response = fetch(installation).await.unwrap_or(Value::Null);
    let purchases = purchases_enabled_in(&response);
    let cloud = cloud_machines_enabled_in(&response);
    let purchases_changed = PURCHASES_ENABLED.swap(purchases, Ordering::AcqRel) != purchases;
    let cloud_changed = CLOUD_MACHINES_ENABLED.swap(cloud, Ordering::AcqRel) != cloud;
    purchases_changed || cloud_changed
}

async fn fetch(installation: &InstallationId) -> Option<Value> {
    let vendor = crate::build_config::vendor();
    let (key, project, app) = (vendor.api_key?, vendor.project_id?, vendor.app_id?);
    let mut url = url::Url::parse(&format!(
        "https://firebaseremoteconfig.googleapis.com/v1/projects/{project}/namespaces/firebase:fetch"
    ))
    .ok()?;
    url.query_pairs_mut().append_pair("key", key);
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(FETCH_TIMEOUT)
        .build()
        .ok()?;
    let response = client
        .post(url)
        .json(&json!({"appId": app, "appInstanceId": installation.as_str()}))
        .send()
        .await
        .ok()?;
    if !response.status().is_success() {
        log::debug!(
            "[remote-config] fetch refused: HTTP {}",
            response.status().as_u16()
        );
        return None;
    }
    response.json().await.ok()
}

#[cfg(test)]
mod tests;
