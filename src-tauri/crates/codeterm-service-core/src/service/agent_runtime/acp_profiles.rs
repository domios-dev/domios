use super::launch_profile_install as installer;
use crate::service::ServiceContext;
use codeterm_plugin::agent_runtime::{owner_profiles, AgentRuntimeCommand, RuntimeLaunchProfile};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::LazyLock,
};

mod registry;
#[cfg(test)]
mod tests;

const PROVIDER: &str = "acp";
static MUTATION: LazyLock<parking_lot::Mutex<()>> = LazyLock::new(Default::default);

pub fn authorize(svc: &ServiceContext, credential: &str) -> Result<(), String> {
    let Some(caller) = svc.authenticate_local_credential(credential)? else {
        return Ok(());
    };
    let mux = svc.mux.read();
    let allowed = mux.get_tab(&caller.tab_id).is_some_and(|tab| {
        tab.session_binding_nonce.as_deref() == Some(caller.binding_nonce.as_str())
            && tab.anchors_team()
            && tab
                .team_id
                .as_deref()
                .is_some_and(|id| mux.get_team(id).is_some())
    });
    if allowed {
        Ok(())
    } else {
        Err("ACP profile management requires the local owner or a live Orchestrator".into())
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Add {
    name: String,
    binary: String,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    env: Vec<String>,
    #[serde(default)]
    secret_env: Vec<String>,
    #[serde(default)]
    auth_env: Vec<String>,
}

fn variable(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= 128
        && !key.to_ascii_uppercase().starts_with("CODETERM")
        && key
            .bytes()
            .enumerate()
            .all(|(i, b)| b == b'_' || b.is_ascii_alphabetic() || i > 0 && b.is_ascii_digit())
}

fn secret(name: &str) -> bool {
    name.starts_with("domios:provider:acp:")
        && name.len() > "domios:provider:acp:".len()
        && name.len() <= 128
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_:-".contains(&b))
}

fn entries(
    values: Vec<String>,
    seen: &mut BTreeSet<String>,
) -> Result<BTreeMap<String, String>, String> {
    if values.len() > 128 {
        return Err("Too many environment entries".into());
    }
    values
        .into_iter()
        .map(|entry| {
            let (key, value) = entry
                .split_once('=')
                .ok_or("Environment entries require KEY=VALUE")?;
            if !variable(key)
                || !seen.insert(key.to_ascii_uppercase())
                || value.len() > 16384
                || value.contains('\0')
            {
                return Err("Invalid, reserved or duplicate environment variable".into());
            }
            Ok((key.to_owned(), value.to_owned()))
        })
        .collect()
}

fn parse_add(request: Add) -> Result<(String, RuntimeLaunchProfile), String> {
    owner_profiles::valid_name(&request.name)?;
    let mut seen = BTreeSet::new();
    let env = entries(request.env, &mut seen)?;
    let secret_env = entries(request.secret_env, &mut seen)?;
    if secret_env.values().any(|name| !secret(name)) {
        return Err("Secrets must be ACP secret-store names, never values".into());
    }
    let auth_env = entries(request.auth_env, &mut seen)?
        .into_iter()
        .map(|(key, value)| {
            let (name, method) = value
                .rsplit_once(':')
                .ok_or("Authentication references require secret-name:method")?;
            if !secret(name)
                || method.is_empty()
                || method.len() > 128
                || !method
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_-.".contains(&b))
            {
                return Err("Invalid authentication secret reference or method".into());
            }
            Ok((key, json!({"secret":name,"methods":[method]})))
        })
        .collect::<Result<BTreeMap<_, _>, String>>()?;
    let profile: RuntimeLaunchProfile = serde_json::from_value(json!({
        "binary":request.binary,"args":request.args,"env":env,"secretEnv":secret_env,"authEnv":auth_env,"ownerConfigured":true
    })).map_err(|_| "Invalid owner profile")?;
    if !codeterm_plugin::agent_runtime::install_validation::validate(&profile) {
        return Err("Local commands must name the installed agent binary; package launchers require a pinned registry install".into());
    }
    Ok((request.name, profile))
}

fn validate(svc: &ServiceContext, id: &str, profile: &RuntimeLaunchProfile) -> Result<(), String> {
    owner_profiles::valid_name(id)?;
    let mut capability = svc
        .agent_registry
        .with_provider(PROVIDER, |p| p.agent_runtime())
        .flatten()
        .ok_or("ACP provider unavailable")?;
    let profiles = capability
        .launch_profiles
        .as_mut()
        .ok_or("ACP launch profiles unavailable")?;
    if profiles
        .keys()
        .any(|existing| existing.eq_ignore_ascii_case(id))
    {
        return Err("Profile already exists; remove the owner profile before replacing it".into());
    }
    profiles.insert(id.into(), profile.clone());
    capability
        .validate_declaration()
        .map_err(|_| "Invalid ACP launch profile declaration".to_string())
}

fn changed() -> Result<(), String> {
    crate::provider::invalidate_profile_readiness(&installer::profile_root(PROVIDER)?);
    Ok(())
}

fn save(id: &str, profile: RuntimeLaunchProfile) -> Result<Value, String> {
    owner_profiles::update(PROVIDER, |profiles| {
        if profiles
            .keys()
            .any(|existing| existing.eq_ignore_ascii_case(id))
        {
            return Err("Profile already exists".into());
        }
        profiles.insert(id.into(), profile);
        Ok(())
    })?;
    changed()?;
    Ok(json!({"id":id}))
}

fn catalog(svc: &ServiceContext) -> Result<Value, String> {
    let capability = svc
        .agent_registry
        .with_provider(PROVIDER, |p| p.agent_runtime())
        .flatten()
        .ok_or("ACP provider unavailable")?;
    let profiles = capability
        .launch_profiles
        .ok_or("ACP launch profiles unavailable")?;
    let root = installer::profile_root(PROVIDER)?;
    let store = crate::async_runtime::block_on(async { svc.secrets.read().await.clone() });
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    let mut states = Vec::new();
    for (id, profile) in profiles {
        let mut auth = if profile.secret_env.is_empty() && profile.auth_env.is_empty() {
            "unknown"
        } else {
            "configured"
        };
        for key in profile
            .secret_env
            .values()
            .chain(profile.auth_env.values().map(|entry| &entry.secret))
        {
            let available = crate::async_runtime::block_on(async {
                tokio::time::timeout(
                    deadline.saturating_duration_since(std::time::Instant::now()),
                    store.get(key),
                )
                .await
                .ok()
                .and_then(Result::ok)
                .is_some_and(|value| !value.value.into_inner().trim().is_empty())
            });
            if !available {
                auth = "sign_in_required";
                break;
            }
        }
        states.push(json!({"id":id,"install":installer::detect(&root,&id,&profile),"auth":auth,
            "ownerConfigured":profile.owner_configured,"canInstall":profile.install.is_some(),"registryId":profile.registry_id,"iconUrl":profile.icon_url}));
    }
    Ok(Value::Array(states))
}

pub fn execute(svc: &ServiceContext, params: &Value) -> Result<Value, String> {
    if params.get("authenticated_mesh_peer_id").is_some() {
        return Err("ACP profile management is local-only".into());
    }
    let text = |key| {
        params
            .get(key)
            .and_then(Value::as_str)
            .ok_or_else(|| format!("{key} is required"))
    };
    match text("operation")? {
        "list" => catalog(svc),
        "add" => {
            let _mutation = MUTATION
                .try_lock()
                .ok_or("ACP profile change already in progress")?;
            let request =
                serde_json::from_value(params.clone()).map_err(|_| "Invalid ACP add parameters")?;
            let (id, profile) = parse_add(request)?;
            validate(svc, &id, &profile)?;
            let root = installer::profile_root(PROVIDER)?;
            installer::confirm_local(
                &root,
                &id,
                &profile,
                AgentRuntimeCommand {
                    binary: profile.binary.clone(),
                    args: profile.args.clone(),
                },
                true,
            )?;
            save(&id, profile)
        }
        "remove" => {
            let _mutation = MUTATION
                .try_lock()
                .ok_or("ACP profile change already in progress")?;
            let id = text("name")?;
            owner_profiles::valid_name(id)?;
            owner_profiles::update(PROVIDER, |profiles| {
                profiles
                    .remove(id)
                    .map(|_| ())
                    .ok_or("Only owner-added profiles can be removed".into())
            })?;
            changed()?;
            Ok(json!({"id":id}))
        }
        "registry_search" => registry::search(text("query")?),
        "registry_install" => {
            let _mutation = MUTATION
                .try_lock()
                .ok_or("ACP profile change already in progress")?;
            let id = text("id")?;
            let mut profile = registry::profile(id)?;
            validate(svc, id, &profile)?;
            crate::async_runtime::block_on(installer::install(
                &installer::profile_root(PROVIDER)?,
                id,
                &profile,
                true,
            ))?;
            if let Some(url) = &profile.icon_url {
                profile.icon_svg = crate::async_runtime::block_on(
                    codeterm_plugin::plugin_icon::registry::resolve(
                        crate::paths::data_dir_path()
                            .join(PROVIDER)
                            .join("cache/icons"),
                        url,
                    ),
                );
            }
            save(id, profile)
        }
        _ => Err("Unknown ACP operation".into()),
    }
}
