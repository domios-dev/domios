use std::collections::BTreeSet;
use std::path::Path;
use std::time::Duration;

use codeterm_plugin::agent_runtime::RuntimeLoginMethod;
use parking_lot::Mutex;

use crate::multiplexer::types::{Tab, TabBacking, TabType};
use crate::service::agent_runtime::{
    authority as runtime_authority, engine as runtime_engine,
    launch_profile_install::{self, InstallState},
};
use crate::service::{MuxEmitCause, ServiceContext};

const CLOUD_SIGNIN_TAB_MARKER: &str = "domios:cloud-provider-sign-in";
// Serializes reuse-or-create + runtime_authority::record_spawn across concurrent prepare calls.
static SIGNIN_TAB_PREPARE_LOCK: Mutex<()> = Mutex::new(());

use super::types::{
    PrepareProviderSignInItem, PrepareProviderSignInRequest, PrepareProviderSignInResponse,
    ProviderSignInCardStatus, ProviderSignInState,
};

/// Pure capability probe input for deciding a provider's truthful sign-in card state.
/// Core never branches on provider IDs or display strings — only on declared capabilities.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderCapabilityProbe {
    pub provider_id: String,
    pub profile_id: Option<String>,
    pub display_name: String,
    pub installed: bool,
    pub signed_in: Option<bool>,
    pub supported_methods: Vec<String>,
    pub code_input: bool,
    pub login_tab_id: Option<String>,
}

pub fn classify_provider_capability(probe: ProviderCapabilityProbe) -> ProviderSignInCardStatus {
    if probe.signed_in == Some(true) {
        return ProviderSignInCardStatus {
            provider_id: probe.provider_id,
            profile_id: probe.profile_id,
            display_name: probe.display_name,
            state: ProviderSignInState::Ready,
            supported_methods: probe.supported_methods,
            code_input: probe.code_input,
            login_tab_id: probe.login_tab_id,
            requires_owner_input: false,
            unsupported_reason: None,
        };
    }
    if !probe.installed {
        return ProviderSignInCardStatus {
            provider_id: probe.provider_id,
            profile_id: probe.profile_id,
            display_name: probe.display_name,
            state: ProviderSignInState::Unsupported,
            supported_methods: Vec::new(),
            code_input: false,
            login_tab_id: None,
            requires_owner_input: false,
            unsupported_reason: Some("provider_not_installed".into()),
        };
    }
    if probe.supported_methods.is_empty() {
        return ProviderSignInCardStatus {
            provider_id: probe.provider_id,
            profile_id: probe.profile_id,
            display_name: probe.display_name,
            state: ProviderSignInState::Unsupported,
            supported_methods: Vec::new(),
            code_input: false,
            login_tab_id: None,
            requires_owner_input: false,
            unsupported_reason: Some("login_flow_unsupported".into()),
        };
    }
    ProviderSignInCardStatus {
        provider_id: probe.provider_id,
        profile_id: probe.profile_id,
        display_name: probe.display_name,
        state: ProviderSignInState::SignInRequired,
        supported_methods: probe.supported_methods,
        code_input: probe.code_input,
        login_tab_id: probe.login_tab_id,
        requires_owner_input: true,
        unsupported_reason: None,
    }
}

fn runtime_method_name(method: &RuntimeLoginMethod) -> &'static str {
    match method {
        RuntimeLoginMethod::Browser => "browser",
        RuntimeLoginMethod::Device => "device",
        RuntimeLoginMethod::ApiKey => "apiKey",
    }
}

fn is_usable_login_tab_candidate(
    tab: &Tab,
    provider_id: &str,
    profile_id: Option<&str>,
    has_runtime: bool,
) -> bool {
    if tab.agent_type.as_deref() != Some(provider_id) {
        return false;
    }
    if !profile_id.is_none_or(|wanted| {
        crate::service::agent_runtime::launch_profile::of_tab(tab).as_deref() == Some(wanted)
    }) {
        return false;
    }
    if !tab
        .cwd
        .as_deref()
        .is_some_and(|cwd| !cwd.trim().is_empty())
    {
        return false;
    }
    if !tab
        .session_binding_nonce
        .as_deref()
        .is_some_and(|nonce| !nonce.trim().is_empty())
    {
        return false;
    }
    if has_runtime
        && !matches!(
            &tab.backing,
            TabBacking::ChatBackend { plugin_id, .. } if plugin_id == provider_id
        )
    {
        return false;
    }
    true
}

fn find_existing_login_tab(
    svc: &ServiceContext,
    provider_id: &str,
    profile_id: Option<&str>,
) -> Option<String> {
    let has_runtime = svc
        .agent_registry
        .provider_arc(provider_id)
        .is_some_and(|provider| provider.agent_runtime().is_some());
    let candidates: Vec<(String, String, bool)> = {
        let mux = svc.mux.read();
        mux.all_tabs()
            .filter(|tab| is_usable_login_tab_candidate(tab, provider_id, profile_id, has_runtime))
            .filter_map(|tab| {
                let nonce = tab.session_binding_nonce.clone()?;
                let dedicated = tab.description.as_deref() == Some(CLOUD_SIGNIN_TAB_MARKER);
                Some((tab.id.clone(), nonce, dedicated))
            })
            .collect()
    };
    let ordered = candidates
        .iter()
        .filter(|(_, _, dedicated)| *dedicated)
        .chain(candidates.iter().filter(|(_, _, dedicated)| !*dedicated));
    for (tab_id, nonce, _) in ordered {
        if !has_runtime
            || runtime_engine::for_tab(tab_id).is_some()
            || runtime_authority::verifies(svc, tab_id, provider_id, nonce)
        {
            return Some(tab_id.clone());
        }
    }
    None
}

fn resolve_signin_working_dir(svc: &ServiceContext) -> String {
    let (workspace_cwd, general_tab_cwd) = {
        let mux = svc.mux.read();
        let ws_id = crate::service::general_agent::general_workspace_id(&mux)
            .or_else(|| mux.list_workspaces().first().map(|ws| ws.id.clone()));
        let workspace_cwd = ws_id.as_deref().and_then(|id| {
            mux.list_workspaces()
                .iter()
                .find(|ws| ws.id == id)
                .and_then(|ws| ws.cwd.clone())
        });
        let general_tab_cwd = mux
            .resolve_tab_id(crate::service::general_agent::GENERAL_ALIAS)
            .and_then(|id| mux.get_tab(&id))
            .and_then(|tab| tab.cwd.clone());
        (workspace_cwd, general_tab_cwd)
    };
    if let Some(dir) = workspace_cwd.filter(|dir| Path::new(dir).is_dir()) {
        return dir;
    }
    if let Some(dir) = general_tab_cwd.filter(|dir| Path::new(dir).is_dir()) {
        return dir;
    }
    let general_dir = crate::service::general_agent::general_dir();
    if general_dir.is_dir() {
        return general_dir.to_string_lossy().into_owned();
    }
    if let Some(home) = dirs::home_dir().filter(|dir| dir.is_dir()) {
        return home.to_string_lossy().into_owned();
    }
    std::env::current_dir()
        .unwrap_or_else(|_| std::env::temp_dir())
        .to_string_lossy()
        .into_owned()
}

fn configure_signin_tab(
    tab: &mut Tab,
    provider_id: &str,
    profile_id: Option<&str>,
    cwd: String,
    nonce: &str,
    has_runtime: bool,
) {
    tab.tab_type = TabType::Choice;
    tab.hidden = true;
    tab.show_in_tabbar = false;
    tab.skip_snapshot = true;
    tab.agent_type = Some(provider_id.to_owned());
    tab.description = Some(CLOUD_SIGNIN_TAB_MARKER.into());
    tab.cwd = Some(cwd);
    tab.session_binding_nonce = Some(nonce.to_owned());
    if let Some(profile) = profile_id {
        tab.agent_runtime_config = Some(serde_json::json!({ "launchProfile": profile }));
    }
    if has_runtime {
        tab.chat_view = true;
        tab.backing = TabBacking::ChatBackend {
            plugin_id: provider_id.to_owned(),
            session_id: String::new(),
        };
        tab.agent_runtime_state = Some(serde_json::json!({
            "state": "loading",
            "epoch": 0,
            "auth": "unknown",
            "loginBinding": { "nonce": nonce, "epoch": 0 }
        }));
    }
}

fn is_dedicated_signin_tab(tab: &Tab, provider_id: &str, profile_id: Option<&str>) -> bool {
    tab.description.as_deref() == Some(CLOUD_SIGNIN_TAB_MARKER)
        && tab.agent_type.as_deref() == Some(provider_id)
        && profile_id.is_none_or(|wanted| {
            crate::service::agent_runtime::launch_profile::of_tab(tab).as_deref() == Some(wanted)
        })
}

/// Inspects configured providers and compatible launch profiles on this machine using only
/// declared capabilities and non-secret readiness probes. Never reads or returns credentials.
pub fn inspect_provider_readiness(
    svc: &ServiceContext,
    configured_provider_ids: &[String],
    configured_profile_ids: &[String],
) -> Vec<ProviderSignInCardStatus> {
    let platform = crate::agent::provider::current_platform_tag();
    let mut wanted_providers: BTreeSet<String> = configured_provider_ids
        .iter()
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
        .collect();
    if wanted_providers.is_empty() {
        for id in svc.agent_registry.provider_ids() {
            wanted_providers.insert(id);
        }
    }

    let wanted_profiles: BTreeSet<String> = configured_profile_ids
        .iter()
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
        .collect();

    let mut statuses = Vec::new();
    let store = crate::async_runtime::block_on(async { svc.secrets.read().await.clone() });

    for provider_id in wanted_providers {
        let Some(provider) = svc.agent_registry.provider_arc(&provider_id) else {
            statuses.push(classify_provider_capability(ProviderCapabilityProbe {
                provider_id: provider_id.clone(),
                profile_id: None,
                display_name: provider_id,
                installed: false,
                signed_in: Some(false),
                supported_methods: Vec::new(),
                code_input: false,
                login_tab_id: None,
            }));
            continue;
        };

        let runtime = provider.agent_runtime();
        // Profile-based runtimes (e.g. ACP profiles such as Gemini/Antigravity) report one entry per compatible profile.
        if let Some(profiles) = runtime
            .as_ref()
            .and_then(|cap| cap.launch_profiles.as_ref())
            .filter(|profiles| !profiles.is_empty())
        {
            let dirs = provider.cli_search_dirs();
            let root = launch_profile_install::profile_root(&provider_id).ok();
            for (profile_id, profile) in profiles {
                if !wanted_profiles.is_empty()
                    && !wanted_profiles.contains(profile_id)
                    && !profile.owner_configured
                {
                    continue;
                }
                let installed = root.as_ref().is_some_and(|root| {
                    matches!(
                        launch_profile_install::detect(root, profile_id, profile, &dirs),
                        InstallState::Installed | InstallState::Detected
                    )
                });
                let mut methods = Vec::new();
                for auth_entry in profile.auth_env.values() {
                    for m in &auth_entry.methods {
                        if !methods.contains(m) {
                            methods.push(m.clone());
                        }
                    }
                }
                if !profile.secret_env.is_empty() && !methods.iter().any(|m| m == "apiKey") {
                    methods.push("apiKey".into());
                }
                if let Some(rt_login) = runtime.as_ref().and_then(|r| r.login.as_ref()) {
                    for m in &rt_login.methods {
                        let name = runtime_method_name(m).to_owned();
                        if !methods.contains(&name) {
                            methods.push(name);
                        }
                    }
                }
                // Check secret presence without ever returning or logging any value.
                let secret_keys: Vec<&String> = profile
                    .secret_env
                    .values()
                    .chain(profile.auth_env.values().map(|e| &e.secret))
                    .collect();
                let signed_in = if secret_keys.is_empty() {
                    Some(false)
                } else {
                    let all_present = secret_keys.iter().all(|key| {
                        crate::async_runtime::block_on(async {
                            tokio::time::timeout(Duration::from_secs(2), store.get(key))
                                .await
                                .ok()
                                .and_then(Result::ok)
                                .is_some_and(|secret| !secret.value.into_inner().trim().is_empty())
                        })
                    });
                    Some(all_present)
                };
                let login_tab_id = find_existing_login_tab(svc, &provider_id, Some(profile_id));
                statuses.push(classify_provider_capability(ProviderCapabilityProbe {
                    provider_id: provider_id.clone(),
                    profile_id: Some(profile_id.clone()),
                    display_name: format!("{} ({})", provider.display_name(), profile_id),
                    installed,
                    signed_in,
                    supported_methods: methods,
                    code_input: false,
                    login_tab_id,
                }));
            }
            continue;
        }

        let install_dirs = provider.cli_install_dirs(platform);
        let installed = provider.cli_binary().is_some_and(|binary| {
            matches!(
                codeterm_plugin::js_runtime::probe_bin_with_extra_dirs(binary, &install_dirs)
                    .resolution,
                codeterm_plugin::js_runtime::BinResolution::Found(_)
            )
        }) || runtime.is_some();

        let mut supported_methods = Vec::new();
        let mut code_input = false;
        if let Some(cli_cap) = provider.cli_login() {
            supported_methods.push("browser".into());
            code_input = cli_cap.code_input;
        }
        if let Some(rt_login) = runtime.as_ref().and_then(|r| r.login.as_ref()) {
            for m in &rt_login.methods {
                let name = runtime_method_name(m).to_owned();
                if !supported_methods.contains(&name) {
                    supported_methods.push(name);
                }
            }
        }

        let signed_in = if installed {
            provider.auth_status_probe().and_then(|probe| {
                let binary = provider.cli_binary()?;
                let record = crate::service::provider_signals_probe::capture_in(
                    binary,
                    &install_dirs,
                    &probe,
                )?;
                provider
                    .parse_auth_status(&record)
                    .map(|auth| auth.logged_in)
            })
        } else {
            Some(false)
        };

        let login_tab_id = find_existing_login_tab(svc, &provider_id, None);
        statuses.push(classify_provider_capability(ProviderCapabilityProbe {
            provider_id: provider_id.clone(),
            profile_id: None,
            display_name: provider.display_name().to_owned(),
            installed,
            signed_in,
            supported_methods,
            code_input,
            login_tab_id,
        }));
    }

    statuses
}

fn ensure_signin_tab_for_provider(
    svc: &ServiceContext,
    provider_id: &str,
    profile_id: Option<&str>,
) -> Option<String> {
    let provider = svc.agent_registry.provider_arc(provider_id)?;
    let has_runtime = provider.agent_runtime().is_some();
    let has_cli_login = provider.cli_login().is_some();
    if !has_runtime && !has_cli_login {
        return None;
    }
    // Resolve the working directory outside the mux write lock so no filesystem stats run under it.
    let cwd = resolve_signin_working_dir(svc);
    let _prepare_guard = SIGNIN_TAB_PREPARE_LOCK.lock();
    if let Some(existing) = find_existing_login_tab(svc, provider_id, profile_id) {
        return Some(existing);
    }
    let nonce = uuid::Uuid::new_v4().to_string();
    let (tab_id, stale_duplicates) = {
        let mut mux = svc.mux.write();
        let dedicated_ids: Vec<String> = mux
            .all_tabs()
            .filter(|tab| is_dedicated_signin_tab(tab, provider_id, profile_id))
            .map(|tab| tab.id.clone())
            .collect();
        if let Some((reused_id, extras)) = dedicated_ids.split_first() {
            if let Some(tab) = mux.get_tab_mut(reused_id) {
                configure_signin_tab(tab, provider_id, profile_id, cwd, &nonce, has_runtime);
            }
            (reused_id.clone(), extras.to_vec())
        } else {
            let ws_id = crate::service::general_agent::general_workspace_id(&mux)
                .or_else(|| mux.list_workspaces().first().map(|ws| ws.id.clone()))?;
            let tab_id = mux.create_tab(&ws_id).ok()?;
            let title = match profile_id {
                Some(profile) => format!("Sign in ({profile})"),
                None => format!("Sign in ({provider_id})"),
            };
            let _ = mux.rename_tab(&tab_id, &title);
            if let Some(tab) = mux.get_tab_mut(&tab_id) {
                configure_signin_tab(tab, provider_id, profile_id, cwd, &nonce, has_runtime);
            }
            (tab_id, Vec::new())
        }
    };
    for extra in stale_duplicates {
        let _ = svc.close_tab(&extra);
    }
    if has_runtime {
        if let Err(error) = runtime_authority::record_spawn(svc, &tab_id, provider_id, &nonce) {
            log::warn!(
                "[cloud-warm-bootstrap] failed to authorize runtime sign-in tab {tab_id} for {provider_id}: {error}"
            );
            let _ = svc.close_tab(&tab_id);
            return None;
        }
    }
    drop(_prepare_guard);
    svc.emit_state_changed(MuxEmitCause::TabLifecycle);
    Some(tab_id)
}

/// Prepares sign-in card bindings on the cloud target for requested (or all signed-out supported)
/// configured providers. Returns ONLY typed capability/status metadata, never credentials or codes.
pub fn prepare_provider_signin_tabs(
    svc: &ServiceContext,
    configured_provider_ids: &[String],
    configured_profile_ids: &[String],
    request: &PrepareProviderSignInRequest,
    remote_peer_prefix: Option<&str>,
) -> PrepareProviderSignInResponse {
    let statuses = inspect_provider_readiness(svc, configured_provider_ids, configured_profile_ids);
    let mut items = Vec::new();
    for status in statuses {
        if let Some(filter_provider) = request.provider_id.as_deref() {
            if status.provider_id != filter_provider {
                continue;
            }
        }
        if let Some(filter_profile) = request.profile_id.as_deref() {
            if status.profile_id.as_deref() != Some(filter_profile) {
                continue;
            }
        }
        let tab_id = if status.state == ProviderSignInState::SignInRequired {
            status.login_tab_id.clone().or_else(|| {
                ensure_signin_tab_for_provider(
                    svc,
                    &status.provider_id,
                    status.profile_id.as_deref(),
                )
            })
        } else {
            status.login_tab_id.clone()
        };
        let remote_tab_id = tab_id
            .as_deref()
            .and_then(|id| remote_peer_prefix.map(|peer| format!("mesh:{peer}/{id}")));
        let card_ready = status.state == ProviderSignInState::SignInRequired && tab_id.is_some();
        items.push(PrepareProviderSignInItem {
            provider_id: status.provider_id,
            profile_id: status.profile_id,
            display_name: status.display_name,
            state: status.state,
            supported_methods: status.supported_methods,
            code_input: status.code_input,
            tab_id,
            remote_tab_id,
            card_ready,
            requires_owner_input: status.requires_owner_input,
            unsupported_reason: status.unsupported_reason,
        });
    }
    PrepareProviderSignInResponse { providers: items }
}
