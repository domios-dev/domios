//! Optional launch-profile credentials (`authEnv`): a stored key reaches the agent process
//! environment, and a declared sign-in method that takes a key stores it first.

use super::engine::{self, Session};
use crate::service::ServiceContext;
use codeterm_plugin::agent_runtime::RuntimeLaunchProfile;
use std::{collections::BTreeMap, sync::Arc, time::Duration};

const MAX_KEY_BYTES: usize = 16384;

/// The variables whose secrets are stored; an absent secret leaves its variable unset.
pub(super) fn compose_auth_environment(
    profile: &RuntimeLaunchProfile,
    mut get: impl FnMut(&str) -> Option<String>,
) -> BTreeMap<String, String> {
    profile
        .auth_env
        .iter()
        .filter_map(|(env, entry)| {
            get(&entry.secret)
                .filter(|value| valid_key(value))
                .map(|value| (env.clone(), value))
        })
        .collect()
}

/// The secret that receives an API key for `method`, when the profile declares one.
pub(super) fn key_secret_for_method<'a>(
    profile: &'a RuntimeLaunchProfile,
    method: &str,
) -> Option<&'a str> {
    profile
        .auth_env
        .values()
        .find(|entry| entry.methods.iter().any(|declared| declared == method))
        .map(|entry| entry.secret.as_str())
}

/// Declared sign-in methods that take an API key, in declaration order per variable.
pub(super) fn api_key_methods(profile: &RuntimeLaunchProfile) -> Vec<String> {
    profile
        .auth_env
        .values()
        .flat_map(|entry| entry.methods.iter().cloned())
        .collect()
}

/// The one key-taking method an undirected API-key sign-in uses; several need a choice.
pub(super) fn default_api_key_method(methods: &[String]) -> Result<&str, String> {
    match methods {
        [] => Err("Runtime sign-in is unsupported".into()),
        [method] => Ok(method),
        _ => Err(format!(
            "Choose a sign-in method with --method: {}",
            methods.join(", ")
        )),
    }
}

fn valid_key(value: &str) -> bool {
    !value.trim().is_empty() && value.len() <= MAX_KEY_BYTES && !value.contains('\0')
}

fn in_namespace(provider: &str, secret: &str) -> bool {
    secret.starts_with(&format!("domios:provider:{provider}:"))
}

pub(super) fn auth_environment(
    svc: &ServiceContext,
    provider: &str,
    profile: &RuntimeLaunchProfile,
) -> Result<BTreeMap<String, String>, super::model::RuntimeFailure> {
    if profile
        .auth_env
        .values()
        .any(|entry| !in_namespace(provider, &entry.secret))
    {
        return Err(super::model::RuntimeFailure::Refused(
            "Profile secret reference is outside its provider namespace".into(),
        ));
    }
    Ok(compose_auth_environment(profile, |key| {
        crate::async_runtime::block_on(async {
            let store = svc.secrets.read().await.clone();
            tokio::time::timeout(Duration::from_secs(10), store.get(key))
                .await
                .ok()?
                .ok()
                .map(|secret| secret.value.into_inner())
        })
    }))
}

/// The tab's selected launch profile, read from its runtime configuration.
pub(super) fn selected_profile(
    svc: &ServiceContext,
    tab: &str,
    session: &Session,
) -> Option<RuntimeLaunchProfile> {
    let name = svc.mux.read().get_tab(tab).and_then(|target| {
        target
            .agent_runtime_config
            .as_ref()?
            .get("launchProfile")?
            .as_str()
            .map(str::to_owned)
    })?;
    session
        .adapter
        .lock()
        .as_ref()?
        .agent_runtime()?
        .launch_profiles?
        .remove(&name)
}

/// Stores `key` for a declared method, restarts the runtime so the agent starts with it,
/// then authenticates with that method.
pub(super) async fn sign_in_with_key(
    svc: &ServiceContext,
    tab: &str,
    session: &Arc<Session>,
    method: &str,
    key: &str,
) -> Result<serde_json::Value, String> {
    let key = key.trim();
    if !valid_key(key) {
        return Err("API key required (maximum 16 KiB)".into());
    }
    let secret = selected_profile(svc, tab, session)
        .as_ref()
        .and_then(|profile| key_secret_for_method(profile, method).map(str::to_owned))
        .ok_or("This sign-in method does not take an API key")?;
    if !in_namespace(&session.provider, &secret) {
        return Err("Profile secret reference is outside its provider namespace".into());
    }
    let store = svc.secrets.read().await.clone();
    tokio::time::timeout(
        Duration::from_secs(10),
        store.save(crate::secrets::SaveRequest {
            name: secret,
            value: crate::secrets::SecretString::new(key.to_owned()),
            notes: None,
            folder: None,
            organization: None,
            collection: None,
            overwrite: true,
        }),
    )
    .await
    .map_err(|_| "Saving the API key timed out")?
    .map_err(|_| "The API key could not be saved in the secret store")?;
    let owner = svc.clone();
    let tab_id = tab.to_owned();
    crate::async_runtime::spawn_blocking(move || owner.agent_restart(&tab_id, false, 0))
        .await
        .map_err(|_| "Runtime restart failed")?
        .map_err(restart_reason)?;
    let session = engine::for_tab(tab).ok_or("Runtime binding unavailable")?;
    super::declared_controls::authenticate(svc, tab, &session, method).await
}

fn restart_reason(error: crate::service::agent_restart::AgentRestartError) -> String {
    use crate::service::agent_restart::AgentRestartError;
    match error {
        AgentRestartError::BadRequest(reason)
        | AgentRestartError::NotFound(reason)
        | AgentRestartError::Internal(reason) => format!("Runtime restart refused: {reason}"),
        AgentRestartError::RateLimited { retry_after_secs } => {
            format!("Runtime restart is rate limited; retry in {retry_after_secs} s")
        }
        AgentRestartError::BackgroundWork { .. } => {
            "Runtime restart is blocked by running background work".into()
        }
        AgentRestartError::RuntimeProviderHandoffUnsupported => {
            "Runtime restart is unsupported for this provider".into()
        }
    }
}

#[cfg(test)]
mod tests;
