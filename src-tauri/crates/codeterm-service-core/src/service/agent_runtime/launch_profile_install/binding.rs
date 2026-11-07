use codeterm_plugin::agent_runtime::{
    RuntimeLaunchProfile, RuntimeProfileAuthEnv, RuntimeProfileClientCapabilities,
    RuntimeProfileDetection, RuntimeProfileInstall,
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

#[cfg(test)]
mod tests;

const INSTALL_IDENTITY_SCHEME: &str = "install-identity-v1";

/// Which scheme a stored declaration hash still holds under.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Binding {
    InstallIdentity,
    LegacyDeclaration,
}

/// Only what defines the installed bits: presentation, launch policy and host overlays
/// stay out, so declaring them never stales an install.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct InstallIdentity<'a> {
    scheme: &'static str,
    binary: &'a str,
    args: &'a [String],
    install: Option<&'a RuntimeProfileInstall>,
}

/// The whole declaration exactly as receipts written before install identity hashed it;
/// frozen, so fields declared later can never reach it.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct LegacyDeclaration<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    icon: Option<&'a String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    registry_id: Option<&'a String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    icon_url: Option<&'a String>,
    binary: &'a str,
    args: &'a [String],
    #[serde(skip_serializing_if = "empty")]
    env: &'a BTreeMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    install: Option<&'a RuntimeProfileInstall>,
    #[serde(skip_serializing_if = "empty")]
    secret_env: &'a BTreeMap<String, String>,
    #[serde(skip_serializing_if = "empty")]
    auth_env: &'a BTreeMap<String, RuntimeProfileAuthEnv>,
    #[serde(skip_serializing_if = "is_false")]
    owner_configured: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    detection: Option<&'a RuntimeProfileDetection>,
    #[serde(skip_serializing_if = "Option::is_none")]
    client_capabilities: Option<&'a RuntimeProfileClientCapabilities>,
}

fn empty<T>(map: &&BTreeMap<String, T>) -> bool {
    map.is_empty()
}

fn is_false(value: &bool) -> bool {
    !value
}

fn digest(value: &impl Serialize) -> Result<String, String> {
    let bytes = serde_json::to_vec(value).map_err(|_| "Invalid launch profile")?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

pub(crate) fn fingerprint(profile: &RuntimeLaunchProfile) -> Result<String, String> {
    digest(&InstallIdentity {
        scheme: INSTALL_IDENTITY_SCHEME,
        binary: &profile.binary,
        args: &profile.args,
        install: profile.install.as_ref(),
    })
}

fn legacy_fingerprint(profile: &RuntimeLaunchProfile) -> Result<String, String> {
    digest(&LegacyDeclaration {
        icon: profile.icon.as_ref(),
        registry_id: profile.registry_id.as_ref(),
        icon_url: profile.icon_url.as_ref(),
        binary: &profile.binary,
        args: &profile.args,
        env: &profile.env,
        install: profile.install.as_ref(),
        secret_env: &profile.secret_env,
        auth_env: &profile.auth_env,
        owner_configured: profile.owner_configured,
        detection: profile.detection.as_ref(),
        client_capabilities: profile.client_capabilities.as_ref(),
    })
}

pub(crate) fn bound_by(
    profile: &RuntimeLaunchProfile,
    stored: &str,
) -> Result<Option<Binding>, String> {
    if stored == fingerprint(profile)? {
        return Ok(Some(Binding::InstallIdentity));
    }
    Ok((stored == legacy_fingerprint(profile)?).then_some(Binding::LegacyDeclaration))
}
