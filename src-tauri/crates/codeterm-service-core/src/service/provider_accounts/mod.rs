//! Account availability and switch claims; runtime lifecycle owns process and turn delivery.

pub mod policy;
pub mod roster;
mod sign_in;
mod storage;
pub mod switching;

use codeterm_plugin::provider_account::{storage::AccountCapability, InvocationContext};
use codeterm_types::config::provider_accounts::{ProviderAccountsConfig, IMPLICIT_ACCOUNT};
use std::sync::Arc;

pub(crate) struct State {
    root: std::path::PathBuf,
    provisioning: parking_lot::Mutex<()>,
    observations:
        parking_lot::Mutex<std::collections::BTreeMap<(String, String), roster::Observation>>,
    operations: parking_lot::Mutex<
        std::collections::BTreeMap<(String, String), std::sync::Weak<parking_lot::Mutex<()>>>,
    >,
    sign_ins: parking_lot::Mutex<std::collections::BTreeMap<String, sign_in::Entry>>,
    feedback: parking_lot::Mutex<std::collections::BTreeMap<String, roster::Feedback>>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            root: crate::paths::data_dir_path(),
            provisioning: Default::default(),
            observations: Default::default(),
            operations: Default::default(),
            sign_ins: Default::default(),
            feedback: Default::default(),
        }
    }
}

fn operation(
    svc: &super::ServiceContext,
    provider: &str,
    account: &str,
) -> Arc<parking_lot::Mutex<()>> {
    let mut operations = svc.ext_or_init::<State>(State::default).operations.lock();
    operations.retain(|_, operation| operation.strong_count() > 0);
    let key = (provider.to_owned(), account.to_owned());
    if let Some(operation) = operations.get(&key).and_then(std::sync::Weak::upgrade) {
        return operation;
    }
    let operation = Arc::new(parking_lot::Mutex::new(()));
    operations.insert(key, Arc::downgrade(&operation));
    operation
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    revision: String,
    #[serde(default, rename = "signInId")]
    sign_in_id: Option<String>,
    operation: policy::PolicyAction,
}

pub fn update(
    svc: &super::ServiceContext,
    provider: &str,
    request: Request,
) -> Result<roster::Snapshot, String> {
    let cancellation = matches!(
        &request.operation,
        policy::PolicyAction::CancelAdd
            | policy::PolicyAction::RetryAdd
            | policy::PolicyAction::CancelSignIn { .. }
    );
    if !cancellation && policy::revision(&policy(svc, provider))? != request.revision {
        return Err("The account list changed. Refresh it before trying again.".into());
    }
    match &request.operation {
        policy::PolicyAction::Add => sign_in::begin(svc, provider, None)?,
        policy::PolicyAction::RetryAdd => {
            sign_in::retry(svc, provider, request.sign_in_id.as_deref())?
        }
        policy::PolicyAction::CancelAdd => {
            sign_in::cancel(svc, provider, None, request.sign_in_id.as_deref())?
        }
        policy::PolicyAction::SignIn { account_id } => {
            sign_in::begin(svc, provider, Some(account_id))?
        }
        policy::PolicyAction::CancelSignIn { account_id } => sign_in::cancel(
            svc,
            provider,
            Some(account_id),
            request.sign_in_id.as_deref(),
        )?,
        policy::PolicyAction::Check { account_id } => roster::check(svc, provider, account_id)?,
        action => policy::update(svc, provider, &request.revision, action)?,
    }
    roster::snapshot(svc, provider)
}

pub(crate) fn login_permitted(svc: &super::ServiceContext, tab: &str) -> bool {
    sign_in::permits_login(svc, tab)
}
pub(crate) fn login_changed(svc: &super::ServiceContext, tab: &str) {
    sign_in::changed(svc, tab);
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

pub(crate) fn usual_home_for_login(capability: &AccountCapability) -> Result<String, String> {
    storage::usual_home(capability).map(|path| path.to_string_lossy().into_owned())
}

pub fn credential_home(
    svc: &super::ServiceContext,
    tab: &str,
) -> Result<std::path::PathBuf, String> {
    let provider = svc
        .mux
        .read()
        .get_tab(tab)
        .and_then(|tab| tab.agent_type.clone())
        .ok_or("The sign-in harness is unavailable")?;
    let capability = capability(svc, &provider)
        .ok_or("This harness does not declare account credential storage")?;
    match for_tab(svc, tab)? {
        Some(context) => context
            .environment_entries()
            .find(|(name, _)| *name == capability.home_environment)
            .and_then(|(_, value)| value.map(std::path::PathBuf::from))
            .ok_or_else(|| "The selected credential home is unavailable".into()),
        None => storage::usual_home(&capability),
    }
}

fn tabs_in_use(svc: &super::ServiceContext, provider: &str, account: &str) -> usize {
    svc.mux
        .read()
        .all_tabs()
        .filter(|tab| {
            tab.agent_type.as_deref() == Some(provider)
                && selected_account(tab) == account
                && tab.description.as_deref() != Some("domios:provider-account-sign-in")
        })
        .count()
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
    let namespace = credential_namespace(
        svc,
        &provider,
        params.account_id.as_deref(),
        params.account_namespace.as_deref(),
    )?;
    let context = invocation_at(
        svc,
        &provider,
        params.account_id.as_deref(),
        namespace.as_deref(),
    )?;
    params.account_namespace = namespace;
    params.workspace_id = Some(workspace);
    Ok((params, context))
}

/// Call from a blocking operation; provisioning never changes daemon environment or the usual home.
pub fn invocation(
    svc: &super::ServiceContext,
    provider: &str,
    account: Option<&str>,
) -> Result<Option<Arc<InvocationContext>>, String> {
    let namespace = credential_namespace(svc, provider, account, None)?;
    invocation_at(svc, provider, account, namespace.as_deref())
}

pub(crate) fn credential_namespace(
    svc: &super::ServiceContext,
    provider: &str,
    account: Option<&str>,
    requested: Option<&str>,
) -> Result<Option<String>, String> {
    let account = account.unwrap_or(IMPLICIT_ACCOUNT);
    if account == IMPLICIT_ACCOUNT {
        return if requested.is_none() {
            Ok(None)
        } else {
            Err("The usual sign-in has no private credential namespace".into())
        };
    }
    let policy = policy(svc, provider);
    if let Some(profile) = policy.profiles.get(account) {
        let current = profile.credential_namespace.as_deref().unwrap_or(account);
        let namespace = requested.unwrap_or(current);
        if namespace != current
            && !profile
                .retained_namespaces
                .iter()
                .any(|retained| retained == namespace)
        {
            return Err("The saved account credential context no longer exists".into());
        }
        return Ok(Some(namespace.into()));
    }
    if sign_in::draft_allows(svc, provider, account)
        && requested.is_none_or(|namespace| namespace == account)
    {
        return Ok(Some(account.into()));
    }
    Err("The selected harness account no longer exists".into())
}

pub(crate) fn invocation_at(
    svc: &super::ServiceContext,
    provider: &str,
    account: Option<&str>,
    namespace: Option<&str>,
) -> Result<Option<Arc<InvocationContext>>, String> {
    let account = account.unwrap_or(IMPLICIT_ACCOUNT);
    if account == IMPLICIT_ACCOUNT {
        return Ok(None);
    }
    let capability =
        capability(svc, provider).ok_or("This harness does not support isolated accounts")?;
    let policy = policy(svc, provider);
    policy.validate()?;
    if !policy.profiles.contains_key(account) && !sign_in::draft_allows(svc, provider, account) {
        return Err("The selected harness account no longer exists".into());
    }
    let state = svc.ext_or_init::<State>(State::default);
    let _provisioning = state.provisioning.lock();
    let usual = storage::usual_home(&capability)?;
    storage::provision_namespace(
        &state.root,
        provider,
        account,
        namespace.ok_or("The selected account credential context is unavailable")?,
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
            target.restorable_launch.account_namespace.clone(),
        ))
    };
    let before = selection()?;
    let namespace = credential_namespace(svc, &before.0, Some(&before.1), before.3.as_deref())?;
    let context = invocation_at(svc, &before.0, Some(&before.1), namespace.as_deref())?;
    if selection()? != before {
        return Err("Account selection changed during provisioning".into());
    }
    Ok(context)
}
