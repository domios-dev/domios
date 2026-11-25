use super::{policy, roster};
use crate::service::{agent_lifecycle::AccountRestartBinding, ServiceContext};
use codeterm_plugin::provider_account::storage::AccountContinuation;
use codeterm_types::config::provider_accounts::ContinuationReceipt;
use sha2::{Digest, Sha256};

fn declaration(svc: &ServiceContext, provider: &str) -> Option<String> {
    let capability = super::capability(svc, provider)?;
    let adapter = svc.agent_registry.provider_arc(provider)?;
    let identity = (
        capability,
        adapter.account_implementation_revision(),
        adapter.agent_runtime(),
    );
    Some(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&identity).ok()?)
    ))
}

fn binary(svc: &ServiceContext, provider: &str) -> Option<String> {
    let adapter = svc.agent_registry.provider_arc(provider)?;
    let path = adapter.resolved_cli_path()?;
    serde_json::to_string(&codeterm_plugin::model_catalog::fingerprint_path(&path)?).ok()
}

pub(super) fn effective(svc: &ServiceContext, provider: &str) -> AccountContinuation {
    let Some(capability) = super::capability(svc, provider) else {
        return AccountContinuation::Unsupported;
    };
    if svc
        .try_ext::<super::State>()
        .is_some_and(|state| state.continuation_refusals.lock().contains(provider))
    {
        return AccountContinuation::Unsupported;
    }
    if capability.continuation != AccountContinuation::Unverified {
        return capability.continuation;
    }
    let Some(receipt) = policy(svc, provider).continuation_receipt else {
        return AccountContinuation::Unverified;
    };
    if codeterm_build_info::sha() != "unknown"
        && receipt.host_sha == codeterm_build_info::sha()
        && declaration(svc, provider).as_deref() == Some(&receipt.capability_hash)
        && binary(svc, provider).as_deref() == Some(&receipt.binary_fingerprint)
    {
        if receipt.supported {
            AccountContinuation::Proven
        } else {
            AccountContinuation::Unsupported
        }
    } else {
        AccountContinuation::Unverified
    }
}

pub(super) fn validate_accounts(
    svc: &ServiceContext,
    provider: &str,
    source: &str,
    target: &str,
) -> Result<(), String> {
    let adapter = svc
        .agent_registry
        .provider_arc(provider)
        .ok_or("The harness is unavailable")?;
    let (source_auth, source_identity) = roster::read_status(svc, provider, source, &adapter)?;
    let (target_auth, target_identity) = roster::read_status(svc, provider, target, &adapter)?;
    if source_auth.is_some_and(|auth| !auth.logged_in)
        || target_auth.is_some_and(|auth| !auth.logged_in)
    {
        return Err("Sign in to both accounts before checking conversation moves".into());
    }
    let source_identity = source_identity
        .ok_or("The harness must report the source account identity for this live check")?;
    let target_identity = target_identity
        .ok_or("The harness must report the target account identity for this live check")?;
    if source_identity.subject == target_identity.subject {
        return Err("This live check requires two different provider accounts".into());
    }
    Ok(())
}

pub(super) fn record(
    svc: &ServiceContext,
    source: &AccountRestartBinding,
    resumed: &AccountRestartBinding,
) -> Result<(), String> {
    let provider = &source.provider;
    let source_account = source
        .account_id
        .as_deref()
        .unwrap_or(super::IMPLICIT_ACCOUNT);
    let target_account = resumed
        .account_id
        .as_deref()
        .unwrap_or(super::IMPLICIT_ACCOUNT);
    if source.provider != resumed.provider
        || source.session_id.is_empty()
        || source.session_id != resumed.session_id
        || source.binding_nonce == resumed.binding_nonce
        || source_account == target_account
        || codeterm_build_info::sha() == "unknown"
    {
        return Err("A live exact-session account receipt is unavailable".into());
    }
    validate_accounts(svc, provider, source_account, target_account)?;
    let receipt = ContinuationReceipt {
        supported: true,
        host_sha: codeterm_build_info::sha().into(),
        capability_hash: declaration(svc, provider)
            .ok_or("The harness declaration is unavailable")?,
        binary_fingerprint: binary(svc, provider)
            .ok_or("The harness binary identity is unavailable")?,
        session_id: source.session_id.clone(),
        source_account: source_account.into(),
        target_account: target_account.into(),
        source_nonce: source.binding_nonce.clone(),
        resumed_nonce: resumed.binding_nonce.clone(),
        verified_at_ms: crate::delayed_send::now_ms(),
    };
    let applied = super::mutate_config(svc, |config| {
        let policy = config
            .agents
            .entry(provider.into())
            .or_default()
            .accounts
            .get_or_insert_with(Default::default);
        policy.continuation_receipt = Some(receipt);
        policy.validate()
    })?;
    svc.ext_or_init::<super::State>(Default::default)
        .continuation_refusals
        .lock()
        .remove(provider);
    svc.ui.emit(
        "config-changed",
        serde_json::to_value(applied).unwrap_or_default(),
    );
    Ok(())
}

pub(super) fn record_refusal(
    svc: &ServiceContext,
    tab: &str,
    target: &crate::service::agent_lifecycle::AccountRestartTarget,
    outcome: &crate::service::agent_lifecycle::RestartOutcome,
) -> Result<bool, String> {
    let source = &target.expected;
    let Some(nonce) = outcome
        .account_refusal_nonce
        .as_deref()
        .filter(|nonce| !nonce.is_empty() && *nonce != source.binding_nonce)
    else {
        return Ok(false);
    };
    let source_account = source
        .account_id
        .as_deref()
        .unwrap_or(super::IMPLICIT_ACCOUNT);
    let target_account = target
        .account_id
        .as_deref()
        .unwrap_or(super::IMPLICIT_ACCOUNT);
    if !outcome.account_target_refused
        || outcome.tab_id != tab
        || outcome.provider != source.provider
        || outcome.session_id.as_deref() != Some(&source.session_id)
        || outcome.session_was_stale
        || source.unbound_login
        || source.refresh_login
        || source_account == target_account
        || codeterm_build_info::sha() == "unknown"
    {
        return Ok(false);
    }
    validate_accounts(svc, &source.provider, source_account, target_account)?;
    let receipt = ContinuationReceipt {
        supported: false,
        host_sha: codeterm_build_info::sha().into(),
        capability_hash: declaration(svc, &source.provider)
            .ok_or("The harness declaration is unavailable")?,
        binary_fingerprint: binary(svc, &source.provider)
            .ok_or("The harness binary identity is unavailable")?,
        session_id: source.session_id.clone(),
        source_account: source_account.into(),
        target_account: target_account.into(),
        source_nonce: source.binding_nonce.clone(),
        resumed_nonce: nonce.into(),
        verified_at_ms: crate::delayed_send::now_ms(),
    };
    let applied = super::mutate_config(svc, |config| {
        let policy = config
            .agents
            .entry(source.provider.clone())
            .or_default()
            .accounts
            .get_or_insert_with(Default::default);
        policy.continuation_receipt = Some(receipt);
        policy.validate()
    })?;
    svc.ui.emit(
        "config-changed",
        serde_json::to_value(applied).unwrap_or_default(),
    );
    Ok(true)
}
