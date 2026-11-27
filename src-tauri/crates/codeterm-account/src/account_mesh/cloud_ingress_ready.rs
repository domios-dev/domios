use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde::{Deserialize, Serialize};

use super::{
    callable::{refusal, result, HttpCallable},
    cloud_provisioning::provisioning_message,
    wire::RegistryRefusal,
    InstallationKey, RegistryResult,
};

const ROUTE: &str = "recordCloudIngressReady";
const DOMAIN: &str = "domios.cloud-ingress-ready.v1";

#[derive(Serialize)]
struct ReadyBody<'a> {
    schema_version: u8,
    cloud_installation_id: &'a str,
    cloud_installation_key: String,
    boot_id: &'a str,
    boot_started_at_ms: u64,
    timestamp: u64,
}

#[derive(Deserialize)]
struct ReadyReply {
    ingress_ready_at: Option<u64>,
}

pub async fn record(
    project: &str,
    account_id: &str,
    installation_id: &str,
    key: &InstallationKey,
    boot_id: &str,
    boot_started_at_ms: u64,
    timestamp: u64,
) -> RegistryResult<Option<u64>> {
    let endpoints = crate::firebase_endpoints::current();
    if project != endpoints.project {
        return Err(RegistryRefusal::Invalid);
    }
    let body = ReadyBody {
        schema_version: 1,
        cloud_installation_id: installation_id,
        cloud_installation_key: URL_SAFE_NO_PAD.encode(key.public_bytes()),
        boot_id,
        boot_started_at_ms,
        timestamp,
    };
    let message =
        provisioning_message(DOMAIN, project, account_id, &body).ok_or(RegistryRefusal::Invalid)?;
    let data = serde_json::json!({
        "account_id": account_id,
        "body": body,
        "guest_signature": key.sign_cloud_provisioning(&message),
    });
    let client =
        HttpCallable::new(endpoints.functions).map_err(|_| RegistryRefusal::Unavailable)?;
    let reply = client
        .post(ROUTE, None, data)
        .await
        .map_err(|error| refusal(&error))?;
    Ok(result::<ReadyReply>(reply)?.ingress_ready_at)
}
