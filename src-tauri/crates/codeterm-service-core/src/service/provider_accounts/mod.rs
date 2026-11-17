//! Account availability and switch claims; runtime lifecycle owns process and turn delivery.

pub mod policy;
pub mod roster;
mod storage;
pub mod switching;

use codeterm_plugin::provider_account::{storage::AccountCapability, InvocationContext};
use codeterm_types::config::provider_accounts::{ProviderAccountsConfig, IMPLICIT_ACCOUNT};
use std::sync::Arc;

#[derive(Default)]
pub(crate) struct State {
    provisioning: parking_lot::Mutex<()>,
    observations:
        parking_lot::Mutex<std::collections::BTreeMap<(String, String), roster::Observation>>,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    revision: String,
    operation: policy::PolicyAction,
}

pub fn update(
    svc: &super::ServiceContext,
    provider: &str,
    request: Request,
) -> Result<roster::Snapshot, String> {
    if let policy::PolicyAction::Check { account_id } = &request.operation {
        if policy::revision(&policy(svc, provider))? != request.revision {
            return Err("The account list changed. Refresh it before trying again.".into());
        }
        roster::check(svc, provider, account_id)?;
    } else {
        policy::update(svc, provider, &request.revision, &request.operation)?;
    }
    roster::snapshot(svc, provider)
}

pub(crate) fn invocation_matches(
    context: Option<&InvocationContext>,
    provider: &str,
    account: &str,
) -> bool {
    match context {
        Some(context) => {
            context.provider() == provider
                && context.account() == account
                && account != IMPLICIT_ACCOUNT
        }
        None => account == IMPLICIT_ACCOUNT,
    }
}

pub fn selected_account(tab: &crate::multiplexer::types::Tab) -> &str {
    tab.restorable_launch
        .account_id
        .as_deref()
        .unwrap_or(codeterm_types::config::provider_accounts::IMPLICIT_ACCOUNT)
}

fn capability(svc: &super::ServiceContext, provider: &str) -> Option<AccountCapability> {
    svc.agent_registry
        .with_provider(provider, |provider| provider.account_capability())
        .flatten()
}

fn policy(svc: &super::ServiceContext, provider: &str) -> ProviderAccountsConfig {
    svc.configuration()
        .agents
        .get(provider)
        .and_then(|config| config.accounts.clone())
        .unwrap_or_default()
}

pub fn starting_account(
    svc: &super::ServiceContext,
    provider: &str,
    workspace: Option<&str>,
    requested: Option<&str>,
) -> Result<Option<String>, String> {
    if capability(svc, provider).is_none() {
        return if requested.is_none_or(|account| account == IMPLICIT_ACCOUNT) {
            Ok(None)
        } else {
            Err("This harness does not support isolated accounts".into())
        };
    }
    let policy = policy(svc, provider);
    let starting = policy.starting_account(workspace, requested)?;
    roster::starting_account(svc, provider, &policy, starting).map(Some)
}

pub(crate) fn spawn_params(
    svc: &super::ServiceContext,
    mut params: super::SpawnAgentParams,
) -> Result<(super::SpawnAgentParams, Option<Arc<InvocationContext>>), String> {
    let provider = params
        .agent
        .clone()
        .unwrap_or_else(|| svc.configuration().general_agent.agent_provider);
    let workspace = {
        let mux = svc.mux.read();
        super::spawn::resolve_spawn_workspace_id(
            params.workspace_id.clone(),
            params.parent_tab_id.as_deref(),
            super::spawn::params_caller_tab_id(&params),
            |tab| mux.find_workspace_for_tab(tab),
            mux.get_active_workspace()
                .map(|workspace| workspace.id.clone()),
            mux.list_workspaces()
                .first()
                .map(|workspace| workspace.id.clone()),
        )?
    };
    params.account_id = starting_account(
        svc,
        &provider,
        Some(&workspace),
        params.account_id.as_deref(),
    )?;
    let context = invocation(svc, &provider, params.account_id.as_deref())?;
    params.workspace_id = Some(workspace);
    Ok((params, context))
}

/// Call from a blocking operation; provisioning never changes daemon environment or the usual home.
pub fn invocation(
    svc: &super::ServiceContext,
    provider: &str,
    account: Option<&str>,
) -> Result<Option<Arc<InvocationContext>>, String> {
    let account = account.unwrap_or(IMPLICIT_ACCOUNT);
    if account == IMPLICIT_ACCOUNT {
        return Ok(None);
    }
    let capability =
        capability(svc, provider).ok_or("This harness does not support isolated accounts")?;
    let policy = policy(svc, provider);
    policy.validate()?;
    if !policy.profiles.contains_key(account) {
        return Err("The selected harness account no longer exists".into());
    }
    let state = svc.ext_or_init::<State>(State::default);
    let _provisioning = state.provisioning.lock();
    let usual = storage::usual_home(&capability)?;
    storage::provision(
        &crate::paths::data_dir_path(),
        provider,
        account,
        &capability,
        &usual,
    )
    .map(Some)
}

pub fn for_tab(
    svc: &super::ServiceContext,
    tab: &str,
) -> Result<Option<Arc<InvocationContext>>, String> {
    let selection = || {
        let mux = svc.mux.read();
        let target = mux.get_tab(tab).ok_or("Account tab is unavailable")?;
        Ok::<_, String>((
            target
                .agent_type
                .clone()
                .ok_or("Account tab has no harness")?,
            selected_account(target).to_owned(),
            target.session_binding_nonce.clone(),
        ))
    };
    let before = selection()?;
    let context = invocation(svc, &before.0, Some(&before.1))?;
    if selection()? != before {
        return Err("Account selection changed during provisioning".into());
    }
    Ok(context)
}
