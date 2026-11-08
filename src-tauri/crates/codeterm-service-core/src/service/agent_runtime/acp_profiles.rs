use super::launch_profile_install as installer;
use crate::service::ServiceContext;
use codeterm_plugin::agent_runtime::{
    owner_profiles, AgentRuntimeCommand, RuntimeLaunchProfile, RuntimeProfileClientCapabilities,
    RuntimeProfileLimitsSource,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

mod registry;
#[cfg(test)]
mod tests;

const PROVIDER: &str = "acp";

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
    #[serde(default)]
    like: Option<String>,
    #[serde(default)]
    client_capabilities: Vec<String>,
    #[serde(default)]
    limits_source: Option<String>,
}

/// Declared identity and agent behaviour copied from a base profile; its launch command
/// and credentials are never copied.
fn inherit(profile: &mut RuntimeLaunchProfile, base: RuntimeLaunchProfile) {
    profile.host_working_directory = base.host_working_directory;
    profile.icon = base.icon;
    profile.icon_svg = base.icon_svg;
    profile.icon_url = base.icon_url;
    profile.registry_id = base.registry_id;
    profile.client_capabilities = base.client_capabilities;
    profile.limits_source = base.limits_source;
    profile.full_access_mode = base.full_access_mode;
    profile.ask_scope = base.ask_scope;
}

fn client_capabilities(
    entries: Vec<String>,
    inherited: Option<RuntimeProfileClientCapabilities>,
) -> Result<Option<RuntimeProfileClientCapabilities>, String> {
    if entries.is_empty() {
        return Ok(inherited);
    }
    if entries.len() > 16 {
        return Err("Too many client capability overrides".into());
    }
    let invalid = || {
        "Client capabilities require a declared NAME=true|false, such as fs.readTextFile=false"
            .to_string()
    };
    let mut value = serde_json::to_value(inherited.unwrap_or_default()).map_err(|_| invalid())?;
    let mut seen = BTreeSet::new();
    for entry in entries {
        let (path, flag) = entry.split_once('=').ok_or_else(invalid)?;
        let flag = match flag {
            "true" => true,
            "false" => false,
            _ => return Err(invalid()),
        };
        let segments: Vec<&str> = path.split('.').collect();
        if !seen.insert(path.to_owned())
            || segments.len() > 2
            || segments.iter().any(|segment| segment.is_empty())
        {
            return Err(invalid());
        }
        let (leaf, parents) = segments.split_last().ok_or_else(invalid)?;
        let mut node = &mut value;
        for parent in parents {
            node = node
                .as_object_mut()
                .ok_or_else(invalid)?
                .entry(*parent)
                .or_insert_with(|| json!({}));
        }
        node.as_object_mut()
            .ok_or_else(invalid)?
            .insert((*leaf).to_owned(), Value::Bool(flag));
    }
    // The declaration type refuses unknown names, so overrides track its fields exactly.
    serde_json::from_value(value)
        .map(Some)
        .map_err(|_| invalid())
}

fn limits_source(spec: &str) -> Result<RuntimeProfileLimitsSource, String> {
    let invalid = || {
        "Limits source must be account:PROVIDER, balance:READER:ENV or unavailable:REASON"
            .to_string()
    };
    let (kind, rest) = spec.split_once(':').ok_or_else(invalid)?;
    let value = match kind {
        "account" => json!({"kind":kind,"provider":rest}),
        "balance" => {
            let (reader, env) = rest.split_once(':').ok_or_else(invalid)?;
            json!({"kind":kind,"reader":reader,"credentialEnv":env})
        }
        "unavailable" => json!({"kind":kind,"reason":rest}),
        _ => return Err(invalid()),
    };
    serde_json::from_value(value).map_err(|_| invalid())
}

fn checked_limits_source(profile: &RuntimeLaunchProfile) -> Result<(), String> {
    if profile
        .limits_source
        .as_ref()
        .is_some_and(|source| !source.valid_for(profile))
    {
        return Err("Invalid limits source: a balance credential must be one of the profile's --secret-env or --auth-env variables, a provider must be a provider id, and a reason must be one line of at most 240 characters".into());
    }
    Ok(())
}

fn base_profile(svc: &ServiceContext, name: &str) -> Result<RuntimeLaunchProfile, String> {
    owner_profiles::valid_name(name)?;
    svc.agent_registry
        .with_provider(PROVIDER, |p| p.agent_runtime())
        .flatten()
        .and_then(|capability| capability.launch_profiles?.get(name).cloned())
        .ok_or_else(|| {
            format!("Unknown base profile {name}; list profiles with domios agent acp list")
        })
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

fn parse_add(
    request: Add,
    base: Option<RuntimeLaunchProfile>,
) -> Result<(String, RuntimeLaunchProfile), String> {
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
    let mut profile: RuntimeLaunchProfile = serde_json::from_value(json!({
        "binary":request.binary,"args":request.args,"env":env,"secretEnv":secret_env,"authEnv":auth_env,"ownerConfigured":true
    })).map_err(|_| "Invalid owner profile")?;
    if !codeterm_plugin::agent_runtime::install_validation::validate(&profile) {
        return Err("Local commands must name the installed agent binary; package launchers require a pinned registry install".into());
    }
    if let Some(base) = base {
        inherit(&mut profile, base);
    }
    profile.client_capabilities = client_capabilities(
        request.client_capabilities,
        profile.client_capabilities.take(),
    )?;
    if let Some(spec) = request.limits_source.as_deref() {
        profile.limits_source = Some(limits_source(spec)?);
    }
    checked_limits_source(&profile)?;
    Ok((request.name, profile))
}

fn validate(svc: &ServiceContext, id: &str, profile: &RuntimeLaunchProfile) -> Result<(), String> {
    let mut capability = available_name(svc, id)?;
    capability
        .launch_profiles
        .as_mut()
        .ok_or("ACP launch profiles unavailable")?
        .insert(id.into(), profile.clone());
    capability
        .validate_declaration()
        .map_err(|_| "Invalid ACP launch profile declaration".to_string())
}

fn available_name(
    svc: &ServiceContext,
    id: &str,
) -> Result<codeterm_plugin::agent_runtime::AgentRuntimeCapability, String> {
    owner_profiles::valid_name(id)?;
    let capability = svc
        .agent_registry
        .with_provider(PROVIDER, |p| p.agent_runtime())
        .flatten()
        .ok_or("ACP provider unavailable")?;
    let profiles = capability
        .launch_profiles
        .as_ref()
        .ok_or("ACP launch profiles unavailable")?;
    if let Some(existing) = profiles
        .keys()
        .find(|existing| existing.eq_ignore_ascii_case(id))
    {
        return Err(name_conflict(
            existing,
            owner_profiles::load(PROVIDER)?.contains_key(existing),
        ));
    }
    Ok(capability)
}

fn name_conflict(id: &str, owner_added: bool) -> String {
    if owner_added {
        format!("{id} already exists; remove the owner profile before replacing it")
    } else {
        format!("{id} is a built-in profile; choose another name")
    }
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
    super::launch_profile_setup::forget_finished(PROVIDER, id);
    changed()?;
    Ok(json!({"id":id}))
}

fn no_active_setup(id: &str) -> Result<(), String> {
    if super::launch_profile_setup::current(PROVIDER, id).is_some_and(|job| job.active()) {
        return Err(
            "Profile setup is still in progress; wait for completion before changing it".into(),
        );
    }
    Ok(())
}

pub fn remove(id: &str) -> Result<Value, String> {
    let _mutation = super::launch_profile_setup::mutation()?;
    owner_profiles::valid_name(id)?;
    no_active_setup(id)?;
    owner_profiles::update(PROVIDER, |profiles| {
        profiles
            .remove(id)
            .map(|_| ())
            .ok_or("Only owner-added profiles can be removed".into())
    })?;
    super::launch_profile_setup::forget_finished(PROVIDER, id);
    changed()?;
    Ok(json!({"id":id}))
}

pub fn registry_search(query: &str) -> Result<Value, String> {
    registry::search(query)
}

fn install_registry(
    svc: &ServiceContext,
    id: &str,
    limits: Option<RuntimeProfileLimitsSource>,
) -> Result<Value, String> {
    let mut profile = registry::profile(id)?;
    profile.limits_source = limits;
    checked_limits_source(&profile)?;
    validate(svc, id, &profile)?;
    crate::async_runtime::block_on(installer::install(
        &installer::profile_root(PROVIDER)?,
        id,
        &profile,
        true,
    ))?;
    if let Some(url) = &profile.icon_url {
        profile.icon_svg =
            crate::async_runtime::block_on(codeterm_plugin::plugin_icon::registry::resolve(
                crate::paths::data_dir_path()
                    .join(PROVIDER)
                    .join("cache/icons"),
                url,
            ));
    }
    save(id, profile)
}

pub fn begin_registry_install(
    svc: &ServiceContext,
    id: String,
) -> Result<super::launch_profile_setup::SetupJob, String> {
    let _mutation = super::launch_profile_setup::mutation()?;
    available_name(svc, &id)?;
    let service = svc.clone();
    super::launch_profile_setup::start(svc, PROVIDER.into(), id.clone(), move || {
        install_registry(&service, &id, None).map(|_| ())
    })
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
            let _mutation = super::launch_profile_setup::mutation()?;
            let request: Add =
                serde_json::from_value(params.clone()).map_err(|_| "Invalid ACP add parameters")?;
            let base = request
                .like
                .as_deref()
                .map(|name| base_profile(svc, name))
                .transpose()?;
            let (id, profile) = parse_add(request, base)?;
            no_active_setup(&id)?;
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
        "remove" => remove(text("name")?),
        "registry_search" => registry_search(text("query")?),
        "registry_install" => {
            let _mutation = super::launch_profile_setup::mutation()?;
            let id = text("id")?;
            let limits = params
                .get("limitsSource")
                .and_then(Value::as_str)
                .map(limits_source)
                .transpose()?;
            no_active_setup(id)?;
            install_registry(svc, id, limits)
        }
        _ => Err("Unknown ACP operation".into()),
    }
}
