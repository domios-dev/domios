use super::LoginBinding;
use crate::multiplexer::types::Tab;
use crate::service::{provider_accounts, ServiceContext};
use serde_json::{json, Value};
use std::path::{Component, Path, PathBuf};

#[derive(Debug, PartialEq, Eq)]
struct Selection {
    provider: String,
    account: String,
    namespace: Option<String>,
}

pub(super) fn recover(
    svc: &ServiceContext,
    tab: &str,
    binding: &LoginBinding,
    login_id: &str,
    owner_confirmed: bool,
) -> Result<Value, String> {
    if !owner_confirmed {
        return Err("Confirm moving the saved sign-in to a backup first".into());
    }
    let selection = {
        let mux = svc.mux.read();
        selected(
            mux.get_tab(tab).ok_or("The sign-in tab is unavailable")?,
            binding,
            login_id,
        )?
    };
    let adapter = svc
        .agent_registry
        .provider_arc(&selection.provider)
        .ok_or("The sign-in harness is unavailable")?;
    let capability = adapter
        .account_capability()
        .ok_or("This harness does not declare credential storage")?;
    let [leaf] = capability.credential_files.as_slice() else {
        return Err("Recovery requires one declared credential file".into());
    };
    let home = provider_accounts::credential_home(svc, tab)?;
    let backup = move_aside(&home, leaf, super::now_ms(), || {
        if provider_accounts::credential_home(svc, tab)? != home {
            return Err("The selected credential home changed".into());
        }
        let mux = svc.mux.read();
        let current = selected(
            mux.get_tab(tab).ok_or("The sign-in tab is unavailable")?,
            binding,
            login_id,
        )?;
        if current != selection {
            return Err("The selected sign-in account changed".into());
        }
        Ok(mux)
    })?;
    Ok(json!({"backedUp":true,"backupPath":backup.to_string_lossy()}))
}

fn selected(tab: &Tab, binding: &LoginBinding, login_id: &str) -> Result<Selection, String> {
    validate_failure(
        tab.session_binding_nonce.as_deref(),
        tab.agent_runtime_state.as_ref(),
        binding,
        login_id,
    )?;
    Ok(Selection {
        provider: tab
            .agent_type
            .clone()
            .ok_or("The sign-in harness is unavailable")?,
        account: provider_accounts::selected_account(tab).into(),
        namespace: tab.restorable_launch.account_namespace.clone(),
    })
}

fn validate_failure(
    nonce: Option<&str>,
    state: Option<&Value>,
    binding: &LoginBinding,
    login_id: &str,
) -> Result<(), String> {
    let state = state.ok_or("The sign-in state is unavailable")?;
    if nonce != Some(binding.nonce.as_str()) || state["epoch"].as_u64() != Some(binding.epoch) {
        return Err("Sign-in belongs to a different runtime session".into());
    }
    let login = &state["login"];
    if login_id.is_empty()
        || login["status"].as_str() != Some("failed")
        || login["loginId"].as_str() != Some(login_id)
        || !login["error"].as_str().is_some_and(owner_failure)
    {
        return Err("This sign-in does not have a current credential owner failure".into());
    }
    Ok(())
}

fn owner_failure(reason: &str) -> bool {
    let reason = reason.to_ascii_lowercase();
    reason.contains("(os error 1307)") || reason.contains("error_invalid_owner")
}

fn credential_leaf(name: &str) -> Result<(), String> {
    let mut components = Path::new(name).components();
    let stem = name
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    let device = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.len() == 4
            && matches!(stem.as_bytes()[3], b'1'..=b'9');
    if device
        || name.ends_with(['.', ' '])
        || name.chars().any(|value| value.is_control())
        || name.contains(['/', '\\', ':', '<', '>', '"', '|', '?', '*'])
        || !matches!(components.next(), Some(Component::Normal(_)))
        || components.next().is_some()
    {
        return Err("Recovery requires a declared credential filename".into());
    }
    Ok(())
}

fn move_aside<G>(
    home: &Path,
    leaf: &str,
    timestamp: u64,
    validate_binding: impl FnOnce() -> Result<G, String>,
) -> Result<PathBuf, String> {
    credential_leaf(leaf)?;
    #[cfg(windows)]
    {
        windows::move_aside(home, leaf, timestamp, validate_binding)
    }
    #[cfg(not(windows))]
    {
        let _ = (home, timestamp, validate_binding);
        Err("Windows credential owner recovery is unavailable on this platform".into())
    }
}

#[cfg(windows)]
mod windows;

#[cfg(test)]
mod tests;
