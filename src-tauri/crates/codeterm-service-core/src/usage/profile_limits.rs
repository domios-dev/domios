//! Plugin-read balances declared by a launch profile's `balance` limits source.
//!
//! The plugin performs the request; the host resolves only the credential that profile
//! names as its balance credential and places it into the outbound header.

use std::collections::BTreeSet;
use std::sync::OnceLock;
use std::time::Duration;

use codeterm_plugin::agent_runtime::{
    limits_source, RuntimeLaunchProfile, RuntimeProfileLimitsSource,
};
use parking_lot::Mutex;

use super::store::{UsageFailureKind, UsageStoreHandle};
use crate::agent::usage_parser::UsageSnapshot;
use crate::service::ServiceContext;

fn in_flight() -> &'static Mutex<BTreeSet<(String, String)>> {
    static IN_FLIGHT: OnceLock<Mutex<BTreeSet<(String, String)>>> = OnceLock::new();
    IN_FLIGHT.get_or_init(Default::default)
}

fn declared(svc: &ServiceContext, provider: &str, profile: &str) -> Option<RuntimeLaunchProfile> {
    svc.agent_registry
        .with_provider(provider, |p| p.agent_runtime())
        .flatten()?
        .launch_profiles?
        .remove(profile)
}

/// The secret name the profile maps to its declared balance credential, inside the plugin's namespace.
pub(crate) fn balance_secret_name(
    plugin_id: &str,
    profile: &RuntimeLaunchProfile,
    env: &str,
) -> Option<String> {
    let Some(RuntimeProfileLimitsSource::Balance { credential_env, .. }) = &profile.limits_source
    else {
        return None;
    };
    if credential_env != env {
        return None;
    }
    let name = profile
        .secret_env
        .get(env)
        .cloned()
        .or_else(|| profile.auth_env.get(env).map(|entry| entry.secret.clone()))?;
    name.starts_with(&format!("domios:provider:{plugin_id}:"))
        .then_some(name)
}

pub(crate) fn launch_profile_secret(
    svc: &ServiceContext,
    plugin_id: &str,
    profile: &str,
    env: &str,
) -> Option<String> {
    let name = balance_secret_name(plugin_id, &declared(svc, plugin_id, profile)?, env)?;
    // A thread outside any runtime context keeps block_on safe whichever VM thread asks.
    let stored = std::thread::scope(|scope| {
        scope
            .spawn(|| {
                crate::async_runtime::block_on(async {
                    let store = svc.secrets.read().await.clone();
                    tokio::time::timeout(Duration::from_secs(10), store.get(&name))
                        .await
                        .ok()?
                        .ok()
                        .map(|secret| secret.value.into_inner())
                })
            })
            .join()
            .ok()
            .flatten()
    })?;
    header_credential(&stored)
}

/// A pasted key keeps its line ending in the store, and HTTP refuses a control byte in a header.
fn header_credential(stored: &str) -> Option<String> {
    let credential = stored.trim();
    (!credential.is_empty()).then(|| credential.to_owned())
}

pub(crate) fn decode(provider: &str, profile: &str, raw: &str) -> Result<UsageSnapshot, String> {
    if raw.len() > 64 * 1024 {
        return Err("Balance reading exceeds size limit".into());
    }
    let mut snapshot: UsageSnapshot =
        serde_json::from_str(raw).map_err(|_| "Invalid balance reading".to_string())?;
    snapshot.provider = provider.to_owned();
    snapshot.account_id = Some(limits_source::balance_account_key(profile));
    snapshot.raw_text.clear();
    Ok(snapshot)
}

fn read(
    svc: &ServiceContext,
    provider: &str,
    profile: &str,
) -> Result<UsageSnapshot, (UsageFailureKind, String)> {
    let source = declared(svc, provider, profile)
        .and_then(|declaration| declaration.limits_source)
        .filter(|source| matches!(source, RuntimeProfileLimitsSource::Balance { .. }))
        .ok_or((
            UsageFailureKind::SourceUnavailable,
            String::from("Profile declares no balance source"),
        ))?;
    let raw = svc
        .agent_registry
        .with_provider(provider, |p| p.read_profile_limits(profile, &source))
        .flatten()
        .ok_or((
            UsageFailureKind::NoParser,
            String::from("Provider has no balance reader"),
        ))?
        .map_err(|error| {
            let kind = if crate::plugin::provider::is_capability_denied(&error) {
                UsageFailureKind::CapabilityDenied
            } else {
                UsageFailureKind::FetchError
            };
            (kind, error)
        })?;
    decode(provider, profile, &raw).map_err(|error| (UsageFailureKind::ParseError, error))
}

/// One read per profile at a time; the result lands in the usage store under the profile's key.
pub(crate) fn spawn_refresh(
    svc: ServiceContext,
    store: UsageStoreHandle,
    provider: String,
    profile: String,
) {
    let key = (provider.clone(), profile.clone());
    if !in_flight().lock().insert(key.clone()) {
        return;
    }
    crate::async_runtime::spawn(async move {
        let account = limits_source::balance_account_key(&profile);
        let reader = svc.clone();
        let (reader_provider, reader_profile) = (provider.clone(), profile.clone());
        let outcome =
            tokio::task::spawn_blocking(move || read(&reader, &reader_provider, &reader_profile))
                .await
                .unwrap_or_else(|_| {
                    Err((UsageFailureKind::Internal, "Balance read stopped".into()))
                });
        in_flight().lock().remove(&key);
        let account = Some(account.as_str());
        let inserted = match outcome {
            Ok(snapshot) => store.insert_snapshot(&super::poller::usage_sample(&snapshot)),
            Err((kind, reason)) => {
                store.insert_failure_kind(&provider, account, None, kind, &reason, None)
            }
        };
        match inserted {
            Ok(id) => {
                crate::usage::projection::invalidate_cache(&provider, account);
                super::poller::emit_snapshot_event(&svc, &provider, account, id);
            }
            Err(error) => log::warn!("[usage.balance] store failed: {error}"),
        }
    });
}

#[cfg(test)]
mod tests;
