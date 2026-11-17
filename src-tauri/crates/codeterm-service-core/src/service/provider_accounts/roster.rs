use super::{capability, policy, State};
use crate::service::ServiceContext;
use codeterm_plugin::provider_account::storage::{AccountCapability, AccountContinuation};
use codeterm_types::config::provider_accounts::{ProviderAccountsConfig, IMPLICIT_ACCOUNT};
use serde::Serialize;
use std::sync::{Arc, LazyLock};

static PROBES: LazyLock<Arc<tokio::sync::Semaphore>> =
    LazyLock::new(|| Arc::new(tokio::sync::Semaphore::new(2)));

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum AccountStatus {
    Ready,
    SignedOut,
    Checking,
    ProbeFailed,
    Unknown,
}

pub(super) struct Observation {
    generation: String,
    status: AccountStatus,
    plan: Option<String>,
    failure: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountRow {
    id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    plan: Option<String>,
    status: AccountStatus,
    tabs_in_use: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    probe_failure_reason: Option<String>,
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
}

pub fn snapshot(svc: &ServiceContext, provider: &str) -> Result<Snapshot, String> {
    let capability =
        capability(svc, provider).ok_or("This harness does not support isolated accounts")?;
    let policy = policy(svc, provider);
    policy.validate()?;
    svc.ext_or_init::<State>(State::default)
        .observations
        .lock()
        .retain(|(id, account), _| id != provider || policy.order.contains(account));
    for account in &policy.order {
        let known = svc
            .ext_or_init::<State>(State::default)
            .observations
            .lock()
            .contains_key(&(provider.into(), account.clone()));
        if !known {
            check(svc, provider, account)?;
        }
    }
    project(svc, provider, &policy, &capability)
}

pub(super) fn starting_account(
    svc: &ServiceContext,
    provider: &str,
    policy: &ProviderAccountsConfig,
    starting: &str,
) -> Result<String, String> {
    let state = svc.ext_or_init::<State>(State::default);
    let observations = state.observations.lock();
    policy
        .first_eligible(starting, |id| {
            observations
                .get(&(provider.into(), id.into()))
                .is_none_or(|entry| !matches!(entry.status, AccountStatus::SignedOut))
        })?
        .map(str::to_owned)
        .ok_or_else(|| "Sign in to a harness account before starting a tab".into())
}

fn project(
    svc: &ServiceContext,
    provider: &str,
    policy: &ProviderAccountsConfig,
    capability: &AccountCapability,
) -> Result<Snapshot, String> {
    let mut counts = std::collections::BTreeMap::<String, usize>::new();
    let workspace_overrides = {
        let mux = svc.mux.read();
        for tab in mux
            .all_tabs()
            .filter(|tab| tab.agent_type.as_deref() == Some(provider))
        {
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
    let observations = state.observations.lock();
    let accounts = policy
        .order
        .iter()
        .map(|id| {
            let observation = observations.get(&(provider.into(), id.clone()));
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
                status: observation
                    .map(|entry| entry.status)
                    .unwrap_or(AccountStatus::Unknown),
                tabs_in_use: counts.get(id).copied().unwrap_or_default(),
                probe_failure_reason: observation.and_then(|entry| entry.failure.clone()),
            }
        })
        .collect();
    Ok(Snapshot {
        provider_id: provider.into(),
        revision: super::policy::revision(policy)?,
        accounts,
        automatic_switch: policy.automatic_switch
            && capability.continuation == AccountContinuation::Proven,
        continuation: capability.continuation,
        workspace_overrides,
    })
}

pub fn check(svc: &ServiceContext, provider: &str, account: &str) -> Result<(), String> {
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
        if observations
            .get(&key)
            .is_some_and(|entry| matches!(entry.status, AccountStatus::Checking))
        {
            return Ok(());
        }
        let plan = observations.get(&key).and_then(|entry| entry.plan.clone());
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
            },
        );
    }
    let Some(probe) = probe else {
        return Ok(());
    };
    let owner = svc.clone();
    crate::async_runtime::spawn(async move {
        let Ok(_permit) = PROBES.clone().acquire_owned().await else {
            return;
        };
        let reader = owner.clone();
        let selected = key.clone();
        let result = codeterm_plugin::js_runtime::spawn_blocking(move || {
            let context = super::invocation(&reader, &selected.0, Some(&selected.1))?;
            codeterm_plugin::provider_account::with_context(context, || {
                let binary = adapter
                    .cli_binary()
                    .ok_or("This harness cannot check sign-in")?;
                let record = crate::service::provider_signals_probe::capture_result(binary, &probe)
                    .map_err(|_| "The harness sign-in check failed or timed out")?;
                adapter
                    .parse_auth_status(&record)
                    .ok_or("The harness returned an unreadable sign-in status")
            })
            .map_err(str::to_string)
        })
        .await
        .map_err(|_| "The sign-in check could not finish".to_owned())
        .and_then(|result| result);
        if !policy(&owner, &key.0).order.contains(&key.1) {
            return;
        }
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
        match result {
            Ok(auth) => {
                entry.status = if auth.logged_in {
                    AccountStatus::Ready
                } else {
                    AccountStatus::SignedOut
                };
                entry.plan = auth.subscription_type;
                entry.failure = None;
            }
            Err(error) => {
                entry.status = AccountStatus::ProbeFailed;
                entry.failure = Some(error);
            }
        }
        drop(observations);
        owner.ui.emit(
            "provider-accounts-changed",
            serde_json::json!({"providerId":key.0}),
        );
    });
    Ok(())
}
