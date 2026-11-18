use super::{capability, policy, State};
use crate::service::ServiceContext;
use codeterm_plugin::provider_account::storage::{AccountContinuation, AccountIdentity};
use codeterm_types::config::provider_accounts::{ProviderAccountsConfig, IMPLICIT_ACCOUNT};
use serde::Serialize;
use std::sync::{Arc, LazyLock};

pub(super) static PROBES: LazyLock<Arc<tokio::sync::Semaphore>> =
    LazyLock::new(|| Arc::new(tokio::sync::Semaphore::new(2)));

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum AccountStatus {
    Ready,
    SignedOut,
    Checking,
    ProbeFailed,
    Unknown,
    SigningIn,
    Removing,
    Exhausted,
}

pub(super) struct Observation {
    generation: String,
    status: AccountStatus,
    plan: Option<String>,
    failure: Option<String>,
    identity: Option<AccountIdentity>,
    pub(super) authenticated: Option<bool>,
    pending: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountRow {
    id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    identity: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    plan: Option<String>,
    status: AccountStatus,
    tabs_in_use: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    probe_failure_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    login: Option<super::sign_in::LoginContext>,
    #[serde(skip_serializing_if = "Option::is_none")]
    removal_notice: Option<super::removal::Notice>,
    #[serde(skip_serializing_if = "Option::is_none")]
    removal_failure_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tabs_to_finish: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    resets_at_ms: Option<u64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceOverride {
    workspace_id: String,
    workspace_name: String,
    account_id: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    provider_id: String,
    revision: String,
    accounts: Vec<AccountRow>,
    automatic_switch: bool,
    continuation: AccountContinuation,
    workspace_overrides: Vec<WorkspaceOverride>,
    #[serde(skip_serializing_if = "Option::is_none")]
    adding: Option<super::sign_in::Adding>,
    #[serde(skip_serializing_if = "Option::is_none")]
    feedback: Option<Feedback>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sign_in_id: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Feedback {
    pub id: String,
    pub kind: &'static str,
    pub account_id: String,
    pub position: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub identity: Option<String>,
}

pub fn snapshot(svc: &ServiceContext, provider: &str) -> Result<Snapshot, String> {
    super::removal::restore(svc, provider);
    capability(svc, provider).ok_or("This harness does not support isolated accounts")?;
    let policy = policy(svc, provider);
    policy.validate()?;
    svc.ext_or_init::<State>(State::default)
        .observations
        .lock()
        .retain(|(id, account), _| id != provider || policy.order.contains(account));
    for account in &policy.order {
        if super::sign_in::active(svc, provider, account)
            || !super::admission::available(svc, provider, account)
        {
            continue;
        }
        let known = svc
            .ext_or_init::<State>(State::default)
            .observations
            .lock()
            .contains_key(&(provider.into(), account.clone()));
        if !known {
            check(svc, provider, account)?;
        }
    }
    project(svc, provider, &policy)
}

pub(super) fn starting_account(
    svc: &ServiceContext,
    provider: &str,
    policy: &ProviderAccountsConfig,
    starting: &str,
    explicit: bool,
) -> Result<String, String> {
    let state = svc.ext_or_init::<State>(State::default);
    let availability = super::scheduler::availability(svc, provider, &policy.order);
    let observations = state.observations.lock();
    policy
        .first_eligible(starting, |id| {
            if explicit && id != starting {
                return false;
            }
            if !super::admission::available(svc, provider, id) {
                return false;
            }
            if matches!(
                availability.get(id),
                Some(super::switching::Authentication::Exhausted { .. })
            ) {
                return false;
            }
            observations
                .get(&(provider.into(), id.into()))
                .is_none_or(|entry| !matches!(entry.status, AccountStatus::SignedOut))
        })?
        .map(str::to_owned)
        .ok_or_else(|| {
            if explicit {
                "The selected harness account is unavailable"
            } else {
                "Sign in to a harness account before starting a tab"
            }
            .into()
        })
}

fn project(
    svc: &ServiceContext,
    provider: &str,
    policy: &ProviderAccountsConfig,
) -> Result<Snapshot, String> {
    let login = super::sign_in::project(svc, provider);
    let (adding, reauth, sign_in_id) = (login.adding, login.reauth, login.id);
    let mut counts = std::collections::BTreeMap::<String, usize>::new();
    let workspace_overrides = {
        let mux = svc.mux.read();
        for tab in mux.all_tabs().filter(|tab| {
            tab.agent_type.as_deref() == Some(provider)
                && tab.description.as_deref() != Some("domios:provider-account-sign-in")
        }) {
            *counts
                .entry(super::selected_account(tab).into())
                .or_default() += 1;
        }
        policy
            .workspaces
            .iter()
            .filter_map(|(id, account)| {
                mux.get_workspace(id).map(|workspace| WorkspaceOverride {
                    workspace_id: id.clone(),
                    workspace_name: workspace.name.clone(),
                    account_id: account.clone(),
                })
            })
            .collect()
    };
    let state = svc.ext_or_init::<State>(State::default);
    let availability = super::scheduler::availability(svc, provider, &policy.order);
    let observations = state.observations.lock();
    let accounts = policy
        .order
        .iter()
        .map(|id| {
            let observation = observations.get(&(provider.into(), id.clone()));
            let removal = state
                .admissions
                .lock()
                .removing
                .get(&(provider.into(), id.clone()))
                .map(|removal| removal.error.clone());
            AccountRow {
                id: id.clone(),
                name: if id == IMPLICIT_ACCOUNT {
                    policy.default_name.clone()
                } else {
                    policy
                        .profiles
                        .get(id)
                        .and_then(|profile| profile.name.clone())
                },
                plan: observation.and_then(|entry| entry.plan.clone()),
                identity: observation
                    .and_then(|entry| entry.identity.as_ref())
                    .or_else(|| {
                        if id == IMPLICIT_ACCOUNT {
                            policy.default_identity.as_ref()
                        } else {
                            policy
                                .profiles
                                .get(id)
                                .and_then(|profile| profile.identity.as_ref())
                        }
                    })
                    .map(|identity| identity.label.clone()),
                status: if removal.is_some() {
                    AccountStatus::Removing
                } else if matches!(
                    availability.get(id),
                    Some(super::switching::Authentication::Exhausted { .. })
                ) {
                    AccountStatus::Exhausted
                } else if reauth
                    .as_ref()
                    .is_some_and(|(account, context, _)| account == id && context.is_some())
                {
                    AccountStatus::SigningIn
                } else {
                    observation
                        .map(|entry| entry.status)
                        .unwrap_or(AccountStatus::Unknown)
                },
                tabs_in_use: counts.get(id).copied().unwrap_or_default(),
                probe_failure_reason: observation.and_then(|entry| entry.failure.clone()),
                login: reauth
                    .as_ref()
                    .filter(|(account, _, _)| account == id)
                    .and_then(|(_, context, _)| context.clone()),
                removal_notice: super::removal::notice(svc, provider, id),
                removal_failure_reason: removal.clone().flatten(),
                tabs_to_finish: removal.map(|_| counts.get(id).copied().unwrap_or_default()),
                resets_at_ms: availability.get(id).and_then(|authentication| {
                    if let super::switching::Authentication::Exhausted { resets_at } =
                        authentication
                    {
                        *resets_at
                    } else {
                        None
                    }
                }),
            }
        })
        .collect();
    Ok(Snapshot {
        provider_id: provider.into(),
        revision: super::policy::revision(policy)?,
        accounts,
        automatic_switch: policy.automatic_switch
            && super::continuation(svc, provider) == AccountContinuation::Proven,
        continuation: super::continuation(svc, provider),
        workspace_overrides,
        adding,
        feedback: state.feedback.lock().get(provider).cloned(),
        sign_in_id,
    })
}

pub(super) fn record_authenticated(
    svc: &ServiceContext,
    provider: &str,
    account: &str,
    identity: Option<AccountIdentity>,
) {
    super::scheduler::authenticated(svc, provider, account);
    let status = if identity.is_some() {
        AccountStatus::Ready
    } else {
        AccountStatus::Unknown
    };
    svc.ext_or_init::<State>(State::default)
        .observations
        .lock()
        .insert(
            (provider.into(), account.into()),
            Observation {
                generation: uuid::Uuid::new_v4().to_string(),
                status,
                plan: None,
                failure: None,
                identity,
                authenticated: Some(true),
                pending: false,
            },
        );
}

pub fn check(svc: &ServiceContext, provider: &str, account: &str) -> Result<(), String> {
    if !super::admission::available(svc, provider, account) {
        return Err("This account is being removed".into());
    }
    if super::sign_in::active(svc, provider, account) {
        return Err("Finish this account's sign-in before checking it again".into());
    }
    capability(svc, provider).ok_or("This harness does not support isolated accounts")?;
    if !policy(svc, provider).order.iter().any(|id| id == account) {
        return Err("The selected harness account no longer exists".into());
    }
    let adapter = svc
        .agent_registry
        .provider_arc(provider)
        .ok_or("Harness is unavailable")?;
    let probe = adapter.auth_status_probe();
    let state = svc.ext_or_init::<State>(State::default);
    let key = (provider.to_owned(), account.to_owned());
    let generation = uuid::Uuid::new_v4().to_string();
    {
        let mut observations = state.observations.lock();
        if observations.get(&key).is_some_and(|entry| entry.pending) {
            return Ok(());
        }
        let plan = observations.get(&key).and_then(|entry| entry.plan.clone());
        let identity = observations
            .get(&key)
            .and_then(|entry| entry.identity.clone());
        let authenticated = observations.get(&key).and_then(|entry| entry.authenticated);
        observations.insert(
            key.clone(),
            Observation {
                generation: generation.clone(),
                status: if probe.is_some() {
                    AccountStatus::Checking
                } else {
                    AccountStatus::Unknown
                },
                plan,
                failure: None,
                identity,
                authenticated,
                pending: true,
            },
        );
    }
    let owner = svc.clone();
    crate::async_runtime::spawn(async move {
        let Ok(_permit) = PROBES.clone().acquire_owned().await else {
            return;
        };
        let reader = owner.clone();
        let selected = key.clone();
        let expected_adapter = adapter.clone();
        let result = codeterm_plugin::js_runtime::spawn_blocking(move || {
            read_status(&reader, &selected.0, &selected.1, &adapter)
        })
        .await
        .map_err(|_| "The sign-in check could not finish".to_owned())
        .and_then(|result| result);
        if !policy(&owner, &key.0).order.contains(&key.1) {
            return;
        }
        let result = if owner
            .agent_registry
            .provider_arc(&key.0)
            .is_some_and(|current| Arc::ptr_eq(&current, &expected_adapter))
        {
            result
        } else {
            Err("The harness changed during the sign-in check. Check again.".into())
        };
        let mut observations = owner
            .ext_or_init::<State>(State::default)
            .observations
            .lock();
        let Some(entry) = observations
            .get_mut(&key)
            .filter(|entry| entry.generation == generation)
        else {
            return;
        };
        let new_identity = result
            .as_ref()
            .ok()
            .and_then(|(_, identity)| identity.clone());
        match result {
            Ok((auth, identity)) => {
                entry.authenticated = auth.as_ref().map(|auth| auth.logged_in);
                entry.status = match entry.authenticated {
                    Some(false) => AccountStatus::SignedOut,
                    Some(true) if identity.is_some() => AccountStatus::Ready,
                    _ => AccountStatus::Unknown,
                };
                if let Some(identity) = identity {
                    entry.identity = Some(identity);
                }
                if let Some(auth) = auth {
                    entry.plan = auth.subscription_type;
                }
                entry.failure = None;
            }
            Err(error) => {
                entry.status = AccountStatus::ProbeFailed;
                entry.failure = Some(error);
            }
        }
        entry.pending = false;
        let authenticated = entry.authenticated;
        drop(observations);
        if let Some(identity) = new_identity {
            if let Err(error) = remember_identity(&owner, &key.0, &key.1, identity) {
                log::debug!("[provider-accounts] could not remember account identity: {error}");
            }
        }
        match authenticated {
            Some(true) => super::scheduler::authenticated(&owner, &key.0, &key.1),
            Some(false) => super::scheduler::signed_out(&owner, &key.0, &key.1),
            None => {}
        }
        owner.ui.emit(
            "provider-accounts-changed",
            serde_json::json!({"providerId":key.0}),
        );
    });
    Ok(())
}

pub(super) fn read_status(
    svc: &ServiceContext,
    provider: &str,
    account: &str,
    adapter: &Arc<dyn crate::agent::provider::AgentProvider>,
) -> Result<
    (
        Option<crate::agent::provider::AuthStatus>,
        Option<AccountIdentity>,
    ),
    String,
> {
    let operation = super::operation(svc, provider, account);
    let _operation = operation
        .try_lock_for(std::time::Duration::from_secs(30))
        .ok_or("The account is still finishing another operation")?;
    let context = super::invocation(svc, provider, Some(account))?;
    codeterm_plugin::provider_account::with_context(context, || {
        let record = if let Some(probe) = adapter.auth_status_probe() {
            let binary = adapter
                .cli_binary()
                .ok_or("This harness cannot check sign-in".to_owned())?;
            Some(
                crate::service::provider_signals_probe::capture_result(binary, &probe)
                    .map_err(|_| "The harness sign-in check failed or timed out".to_owned())?,
            )
        } else {
            None
        };
        let auth = record
            .as_ref()
            .map(|record| {
                adapter
                    .parse_auth_status(record)
                    .ok_or("The harness returned an unreadable sign-in status".to_owned())
            })
            .transpose()?;
        let identity = adapter.account_identity(record.as_ref())?;
        Ok((auth, identity))
    })
}

fn remember_identity(
    svc: &ServiceContext,
    provider: &str,
    account: &str,
    identity: AccountIdentity,
) -> Result<(), String> {
    let policy = policy(svc, provider);
    let previous = if account == IMPLICIT_ACCOUNT {
        policy.default_identity.as_ref()
    } else {
        policy
            .profiles
            .get(account)
            .and_then(|profile| profile.identity.as_ref())
    };
    if previous == Some(&identity) {
        return Ok(());
    }
    let applied = super::mutate_config(svc, |config| {
        let policy = config
            .agents
            .entry(provider.into())
            .or_default()
            .accounts
            .get_or_insert_with(Default::default);
        if account == IMPLICIT_ACCOUNT {
            policy.default_identity = Some(identity);
        } else {
            policy
                .profiles
                .get_mut(account)
                .ok_or("The account was removed during its sign-in check")?
                .identity = Some(identity);
        }
        policy.validate()
    })?;
    svc.ui.emit(
        "config-changed",
        serde_json::to_value(applied).map_err(|error| error.to_string())?,
    );
    Ok(())
}
