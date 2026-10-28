use super::ProfileLayout;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::Path;

const BINDING: &str = ".domios-instance.json";

#[derive(Serialize, Deserialize)]
struct Binding {
    origin: String,
    ephemeral: bool,
}

pub fn normalize_origin(raw: &str) -> Result<String, String> {
    let url = url::Url::parse(raw).map_err(|_| "invalid_origin: expected an HTTP origin")?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.path() != "/"
    {
        return Err(
            "invalid_origin: use a bare HTTP origin without credentials, path, query or fragment"
                .into(),
        );
    }
    Ok(url.origin().ascii_serialization())
}

impl ProfileLayout {
    pub fn prepare_instance(
        &self,
        name: &str,
        origin: &str,
        ephemeral: bool,
    ) -> Result<(), String> {
        super::validate_named_profile(name)?;
        let origin = normalize_origin(origin)?;
        std::fs::create_dir_all(self.profiles_dir()).map_err(|e| e.to_string())?;
        let dir = self.profiles_dir().join(name);
        match std::fs::create_dir(&dir) {
            Ok(()) => {
                let binding = serde_json::to_vec(&Binding { origin, ephemeral })
                    .map_err(|e| e.to_string())?;
                std::fs::write(dir.join(BINDING), binding)
                    .map_err(|e| format!("profile_binding_failed: {e}"))
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                self.require_named(name)?;
                let binding: Binding = serde_json::from_slice(
                    &std::fs::read(dir.join(BINDING))
                        .map_err(|_| "unbound_profile: choose a new per-instance profile name")?,
                )
                .map_err(|_| "invalid_profile_binding")?;
                if binding.origin != origin {
                    return Err("origin_mismatch: profile belongs to another instance".into());
                }
                if binding.ephemeral != ephemeral {
                    return Err(
                        "profile_lifecycle_mismatch: ephemeral flag differs from existing profile"
                            .into(),
                    );
                }
                Ok(())
            }
            Err(error) => Err(format!("create_instance_profile: {error}")),
        }
    }
}

pub(super) async fn is_ephemeral(dir: &Path) -> bool {
    let Ok(bytes) = tokio::fs::read(dir.join(BINDING)).await else {
        return false;
    };
    serde_json::from_slice::<Binding>(&bytes).is_ok_and(|binding| binding.ephemeral)
}

pub fn verify_build(health: &Value, expected: &str) -> Result<(), String> {
    let actual = health
        .get("build_sha")
        .and_then(Value::as_str)
        .ok_or("build_unavailable: health response has no build_sha")?;
    if actual != expected {
        return Err("build_mismatch: instance does not run the expected SHA".into());
    }
    Ok(())
}

pub async fn health(origin: &str) -> Result<Value, String> {
    let origin = normalize_origin(origin)?;
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(crate::cdp::CONNECT_DEADLINE)
        .build()
        .map_err(|_| "health_client_failed")?;
    let health: Value = client
        .get(format!("{origin}/api/health"))
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(|_| "health_unavailable")?
        .json()
        .await
        .map_err(|_| "invalid_health")?;
    Ok(health)
}

pub async fn check_build(origin: &str, expected: &str) -> Result<(), String> {
    verify_build(&health(origin).await?, expected)
}

pub fn verify_dev_identity(health: &Value, name: &str, port: u16) -> Result<(), String> {
    let instance = health
        .get("instance")
        .ok_or("instance_identity_unavailable")?;
    let actual_port = instance.get("api_port").and_then(|v| {
        v.as_str()
            .and_then(|s| s.parse::<u16>().ok())
            .or_else(|| v.as_u64().and_then(|p| u16::try_from(p).ok()))
    });
    if port == 0
        || port == 7685
        || instance.get("name").and_then(Value::as_str) != Some(name)
        || instance.get("identifier").and_then(Value::as_str)
            != Some(format!("com.domios.dev.{name}").as_str())
        || actual_port != Some(port)
    {
        return Err(
            "instance_identity_mismatch: requested DEV name and API port were not confirmed".into(),
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests;
