//! The purchase switch from Firebase Remote Config, kept in memory only. Any failure
//! means purchases stay off until the next scheduled fetch.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde_json::{json, Value};

use crate::InstallationId;

/// One fetch at startup and one per interval keeps far inside the free quota.
pub const REFRESH_INTERVAL: Duration = Duration::from_secs(5 * 60 * 60);
const FETCH_TIMEOUT: Duration = Duration::from_secs(10);

static PURCHASES_ENABLED: AtomicBool = AtomicBool::new(false);

/// Purchases are on only when the template holds exactly the string "true".
pub fn purchases_enabled_in(response: &Value) -> bool {
    response
        .pointer("/entries/purchases_enabled")
        .and_then(Value::as_str)
        == Some("true")
}

/// The last fetched value; false until a fetch says otherwise.
pub fn purchases_enabled() -> bool {
    PURCHASES_ENABLED.load(Ordering::Acquire)
}

/// Fetches once and stores the result; returns whether the switch changed.
pub async fn refresh(installation: &InstallationId) -> bool {
    let enabled = fetch(installation).await.unwrap_or(false);
    PURCHASES_ENABLED.swap(enabled, Ordering::AcqRel) != enabled
}

async fn fetch(installation: &InstallationId) -> Option<bool> {
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
    Some(purchases_enabled_in(&response.json::<Value>().await.ok()?))
}

#[cfg(test)]
mod tests;
