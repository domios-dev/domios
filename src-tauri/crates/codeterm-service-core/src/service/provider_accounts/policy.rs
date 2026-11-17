use codeterm_plugin::provider_account::storage::AccountContinuation;
use codeterm_types::config::provider_accounts::{
    valid_account_name, ProviderAccountsConfig, IMPLICIT_ACCOUNT,
};
use serde::Deserialize;
use sha2::{Digest, Sha256};

#[derive(Deserialize)]
#[serde(
    tag = "action",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum PolicyAction {
    Remove {
        account_id: String,
    },
    DismissRemovalNotice {
        account_id: String,
        notice_id: String,
    },
    Add,
    RetryAdd,
    CancelAdd,
    SignIn {
        account_id: String,
    },
    CancelSignIn {
        account_id: String,
    },
    Check {
        account_id: String,
    },
    Rename {
        account_id: String,
        name: String,
    },
    Reorder {
        account_id: String,
        position: usize,
    },
    AutomaticSwitch {
        enabled: bool,
    },
    ResetWorkspace {
        workspace_id: String,
    },
    Workspace {
        workspace_id: String,
        account_id: Option<String>,
    },
}

pub(super) fn revision(policy: &ProviderAccountsConfig) -> Result<String, String> {
    serde_json::to_vec(policy)
        .map(|bytes| format!("{:x}", Sha256::digest(bytes)))
        .map_err(|error| error.to_string())
}

pub(super) fn apply(
    policy: &mut ProviderAccountsConfig,
    expected: &str,
    continuation: AccountContinuation,
    action: &PolicyAction,
) -> Result<(), String> {
    policy.validate()?;
    if revision(policy)? != expected {
        return Err("The account list changed. Refresh it before trying again.".into());
    }
    let selected = match action {
        PolicyAction::Rename { account_id, .. } | PolicyAction::Reorder { account_id, .. } => {
            Some(account_id.as_str())
        }
        PolicyAction::Workspace {
            account_id: Some(account),
            ..
        } => Some(account.as_str()),
        _ => None,
    };
    if selected.is_some_and(|account| {
        policy
            .profiles
            .get(account)
            .is_some_and(|profile| profile.removal.is_some())
    }) {
        return Err("Finish removing this account before changing it".into());
    }
    match action {
        PolicyAction::Remove { .. } | PolicyAction::DismissRemovalNotice { .. } => {
            return Err("Removal requires a live account ownership check".into())
        }
        PolicyAction::Add
        | PolicyAction::RetryAdd
        | PolicyAction::CancelAdd
        | PolicyAction::SignIn { .. }
        | PolicyAction::CancelSignIn { .. } => {
            return Err("Sign-in does not change account policy directly".into())
        }
        PolicyAction::Check { .. } => {
            return Err("A sign-in check does not change account policy".into())
        }
        PolicyAction::Rename { account_id, name } => {
            if !valid_account_name(name) {
                return Err("Account names must contain 1 to 80 printable characters".into());
            }
            if account_id == IMPLICIT_ACCOUNT {
                policy.default_name = Some(name.clone());
            } else {
                policy
                    .profiles
                    .get_mut(account_id)
                    .ok_or("The selected harness account no longer exists")?
                    .name = Some(name.clone());
            }
        }
        PolicyAction::Reorder {
            account_id,
            position,
        } => {
            if *position >= policy.order.len() {
                return Err("The account position is outside the list".into());
            }
            let current = policy
                .order
                .iter()
                .position(|id| id == account_id)
                .ok_or("The selected harness account no longer exists")?;
            let account = policy.order.remove(current);
            policy.order.insert(*position, account);
        }
        PolicyAction::AutomaticSwitch { enabled } => {
            if *enabled && continuation != AccountContinuation::Proven {
                return Err("Moving this conversation to another account is not confirmed".into());
            }
            policy.automatic_switch = *enabled;
        }
        PolicyAction::ResetWorkspace { workspace_id } => {
            policy.workspaces.remove(workspace_id);
        }
        PolicyAction::Workspace {
            workspace_id,
            account_id,
        } => {
            if let Some(account) = account_id {
                if !policy.order.contains(account) {
                    return Err("The selected harness account no longer exists".into());
                }
                policy
                    .workspaces
                    .insert(workspace_id.clone(), account.clone());
            } else {
                policy.workspaces.remove(workspace_id);
            }
        }
    }
    policy.validate()
}

pub fn update(
    svc: &super::super::ServiceContext,
    provider: &str,
    expected: &str,
    action: &PolicyAction,
) -> Result<(), String> {
    super::capability(svc, provider).ok_or("This harness does not support isolated accounts")?;
    let continuation = super::continuation(svc, provider);
    if let PolicyAction::Workspace { workspace_id, .. } = action {
        if svc.mux.read().get_workspace(workspace_id).is_none() {
            return Err("Workspace is unavailable".into());
        }
    }
    let applied = super::mutate_config(svc, |config| {
        let policy = config
            .agents
            .entry(provider.into())
            .or_default()
            .accounts
            .get_or_insert_with(Default::default);
        apply(policy, expected, continuation, action)
    })?;
    svc.ui.emit(
        "config-changed",
        serde_json::to_value(applied).map_err(|error| error.to_string())?,
    );
    if let PolicyAction::Reorder {
        account_id,
        position,
    } = action
    {
        svc.ext_or_init::<super::State>(super::State::default)
            .feedback
            .lock()
            .insert(
                provider.into(),
                super::roster::Feedback {
                    id: uuid::Uuid::new_v4().to_string(),
                    kind: "moved",
                    identity: None,
                    account_id: account_id.clone(),
                    position: *position,
                },
            );
    }
    svc.ui.emit(
        "provider-accounts-changed",
        serde_json::json!({"providerId":provider}),
    );
    Ok(())
}

#[cfg(test)]
mod tests;
