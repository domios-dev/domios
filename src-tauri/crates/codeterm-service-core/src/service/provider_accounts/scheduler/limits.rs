use super::*;
use codeterm_types::config::provider_accounts::AccountLimitConfig;

pub(super) fn active(
    policy: &codeterm_types::config::provider_accounts::ProviderAccountsConfig,
    account: &str,
) -> Option<Authentication> {
    let limit = if account == super::super::IMPLICIT_ACCOUNT {
        policy.default_limit.as_ref()
    } else {
        policy
            .profiles
            .get(account)
            .and_then(|profile| profile.limit.as_ref())
    }?;
    if limit
        .resets_at_ms
        .is_none_or(|at| at > crate::delayed_send::now_ms())
    {
        Some(Authentication::Exhausted {
            resets_at: limit.resets_at_ms,
        })
    } else {
        None
    }
}

pub(super) fn remember(
    svc: &ServiceContext,
    provider: &str,
    account: &str,
    resets_at: Option<u64>,
) -> Result<(), String> {
    let applied = super::super::mutate_config(svc, |config| {
        let policy = config
            .agents
            .entry(provider.into())
            .or_default()
            .accounts
            .get_or_insert_with(Default::default);
        let limit = Some(AccountLimitConfig {
            resets_at_ms: resets_at,
        });
        if account == super::super::IMPLICIT_ACCOUNT {
            policy.default_limit = limit;
        } else {
            policy
                .profiles
                .get_mut(account)
                .ok_or("The exhausted account was removed")?
                .limit = limit;
        }
        policy.validate()
    })?;
    svc.ui.emit(
        "config-changed",
        serde_json::to_value(applied).unwrap_or_default(),
    );
    Ok(())
}

pub(super) fn expire(book: &mut Switches, provider: &str, accounts: &[String]) {
    let now = crate::delayed_send::now_ms();
    for account in accounts {
        if matches!(book.authentication(provider,account),Some(Authentication::Exhausted {resets_at:Some(at)}) if at<=now)
        {
            book.set_authentication(provider, account, Authentication::Ready);
        }
    }
}

pub(super) fn clear(svc: &ServiceContext, provider: &str, account: &str) -> Result<(), String> {
    let applied = super::super::mutate_config(svc, |config| {
        let policy = config
            .agents
            .get_mut(provider)
            .and_then(|provider| provider.accounts.as_mut())
            .ok_or("The account policy is unavailable")?;
        if account == super::super::IMPLICIT_ACCOUNT {
            policy.default_limit = None;
        } else {
            policy
                .profiles
                .get_mut(account)
                .ok_or("The account no longer exists")?
                .limit = None;
        }
        Ok(())
    })?;
    svc.ui.emit(
        "config-changed",
        serde_json::to_value(applied).unwrap_or_default(),
    );
    Ok(())
}
