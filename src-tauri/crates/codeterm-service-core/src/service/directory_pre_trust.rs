use std::path::{Component, Path, PathBuf};

use crate::agent::registry::AgentRegistry;
use crate::service::ServiceContext;

pub fn for_tab(
    service: &ServiceContext,
    registry: &AgentRegistry,
    provider_id: &str,
    tab_id: &str,
    launch_cwd: Option<&str>,
) {
    for_tab_with_root(
        service,
        registry,
        provider_id,
        tab_id,
        launch_cwd,
        &crate::paths::data_dir_path(),
    );
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PreTrustTarget {
    Provenance(String),
    DomiosOwned {
        raw_cwd: String,
        canonical_cwd: String,
    },
}

impl PreTrustTarget {
    fn write_cwd(&self) -> &str {
        match self {
            Self::Provenance(cwd) => cwd,
            Self::DomiosOwned { canonical_cwd, .. } => canonical_cwd,
        }
    }

    fn still_valid_for_write(&self, domios_root: &Path) -> bool {
        match self {
            Self::Provenance(_) => true,
            Self::DomiosOwned {
                raw_cwd,
                canonical_cwd,
            } => {
                domios_owned_canonical_directory(Path::new(raw_cwd), domios_root).as_deref()
                    == Some(canonical_cwd.as_str())
            }
        }
    }
}

fn for_tab_with_root(
    service: &ServiceContext,
    registry: &AgentRegistry,
    provider_id: &str,
    tab_id: &str,
    launch_cwd: Option<&str>,
    domios_root: &Path,
) {
    for_tab_with_root_and_hook(
        service,
        registry,
        provider_id,
        tab_id,
        launch_cwd,
        domios_root,
        || {},
    );
}

fn for_tab_with_root_and_hook(
    service: &ServiceContext,
    registry: &AgentRegistry,
    provider_id: &str,
    tab_id: &str,
    launch_cwd: Option<&str>,
    domios_root: &Path,
    before_write: impl FnOnce(),
) {
    if provider_id.is_empty() {
        return;
    }
    let tab_state = {
        let mux = service.mux.read();
        mux.get_tab(tab_id).map(|tab| {
            (
                tab.cwd.clone(),
                tab_provenance_authorized(tab),
                crate::service::provider_accounts::selected_account(tab).to_owned(),
                tab.restorable_launch.account_namespace.clone(),
            )
        })
    };
    let Some((tab_cwd, provenance_authorized, selected_account, account_namespace)) = tab_state
    else {
        return;
    };
    let Some(target) = eligible_target_with_root(
        provenance_authorized,
        tab_cwd.as_deref(),
        launch_cwd,
        domios_root,
    ) else {
        return;
    };
    let resolved_invocation = if codeterm_plugin::provider_account::current().is_none()
        && selected_account != codeterm_types::config::provider_accounts::IMPLICIT_ACCOUNT
    {
        match resolve_tab_account_invocation(
            service,
            provider_id,
            &selected_account,
            account_namespace.as_deref(),
        ) {
            Ok(invocation) => Some(invocation),
            Err(error) => {
                log::warn!(
                    "[directory-pre-trust] tab={tab_id} account context unavailable ({error}); skipping pre-trust"
                );
                return;
            }
        }
    } else {
        None
    };
    before_write();
    if !target.still_valid_for_write(domios_root) {
        log::warn!(
            "[directory-pre-trust] tab={tab_id} launch cwd target changed before trust write; skipping"
        );
        return;
    }
    let write_cwd = target.write_cwd();
    let run = || {
        registry.with_provider(provider_id, |provider| {
            provider.pre_trust_directory(write_cwd)
        })
    };
    let result = match resolved_invocation {
        Some(invocation) => codeterm_plugin::provider_account::with_context(invocation, run),
        None => run(),
    };
    match result {
        Some(Ok(outcome)) => log::info!("[directory-pre-trust] tab={tab_id} outcome={outcome:?}"),
        Some(Err(_)) => log::warn!(
            "[directory-pre-trust] tab={tab_id} capability failed; TUI fallback remains active"
        ),
        None => log::debug!("[directory-pre-trust] tab={tab_id} capability unavailable"),
    }
}

fn resolve_tab_account_invocation(
    service: &ServiceContext,
    provider_id: &str,
    account_id: &str,
    saved_namespace: Option<&str>,
) -> Result<Option<std::sync::Arc<codeterm_plugin::provider_account::InvocationContext>>, String> {
    let namespace = crate::service::provider_accounts::credential_namespace(
        service,
        provider_id,
        Some(account_id),
        saved_namespace,
    )?;
    crate::service::provider_accounts::invocation_at(
        service,
        provider_id,
        Some(account_id),
        namespace.as_deref(),
    )
}

/// Pre-trust runs before launch/relaunch even when the prior session is still authenticated.
fn tab_provenance_authorized(tab: &crate::multiplexer::types::Tab) -> bool {
    tab.team_id.is_some() || super::directory_trust::directory_trust_provenance(tab).is_ok()
}

fn eligible_target_with_root(
    provenance_authorized: bool,
    tab_cwd: Option<&str>,
    launch_cwd: Option<&str>,
    domios_root: &Path,
) -> Option<PreTrustTarget> {
    let cwd = exact_launch_cwd(tab_cwd, launch_cwd)?;
    if provenance_authorized {
        return Some(PreTrustTarget::Provenance(cwd.to_string()));
    }
    domios_owned_canonical_directory(Path::new(cwd), domios_root).map(|canonical_cwd| {
        PreTrustTarget::DomiosOwned {
            raw_cwd: cwd.to_string(),
            canonical_cwd,
        }
    })
}

#[cfg(test)]
fn authorized_cwd<'a>(
    authorized: bool,
    tab_cwd: Option<&'a str>,
    launch_cwd: Option<&str>,
) -> Option<&'a str> {
    if !authorized {
        return None;
    }
    exact_launch_cwd(tab_cwd, launch_cwd)
}

fn exact_launch_cwd<'a>(tab_cwd: Option<&'a str>, launch_cwd: Option<&str>) -> Option<&'a str> {
    let cwd = tab_cwd.filter(|cwd| !cwd.is_empty())?;
    ServiceContext::paths_equal(cwd, launch_cwd.unwrap_or(cwd)).then_some(cwd)
}

/// Returns the canonical directory path iff `cwd` resolves to an existing directory
/// equal to or inside `data_root`.
pub(crate) fn domios_owned_canonical_directory(cwd: &Path, data_root: &Path) -> Option<String> {
    let canonical_root = std::fs::canonicalize(data_root).ok()?;
    let canonical_cwd = std::fs::canonicalize(cwd).ok()?;
    if !canonical_root.is_dir() || !canonical_cwd.is_dir() {
        return None;
    }
    let root = PathBuf::from(codeterm_paths::strip_verbatim_prefix(
        &canonical_root.to_string_lossy(),
    ));
    let target_str = codeterm_paths::strip_verbatim_prefix(&canonical_cwd.to_string_lossy());
    let target = PathBuf::from(&target_str);
    path_is_same_or_descendant(&target, &root).then_some(target_str)
}

#[cfg(test)]
pub(crate) fn is_domios_owned_directory(cwd: &Path, data_root: &Path) -> bool {
    domios_owned_canonical_directory(cwd, data_root).is_some()
}

fn path_is_same_or_descendant(candidate: &Path, root: &Path) -> bool {
    let mut candidate_components = candidate.components();
    for root_component in root.components() {
        let Some(candidate_component) = candidate_components.next() else {
            return false;
        };
        if !component_matches(&candidate_component, &root_component) {
            return false;
        }
    }
    true
}

fn component_matches(a: &Component<'_>, b: &Component<'_>) -> bool {
    #[cfg(windows)]
    {
        a.as_os_str()
            .to_string_lossy()
            .eq_ignore_ascii_case(&b.as_os_str().to_string_lossy())
    }
    #[cfg(not(windows))]
    {
        a == b
    }
}

#[cfg(test)]
mod tests;
