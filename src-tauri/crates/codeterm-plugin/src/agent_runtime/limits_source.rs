use super::RuntimeLaunchProfile;
use serde::{Deserialize, Serialize};

pub const MAX_UNAVAILABLE_REASON_CHARS: usize = 240;

/// Where a launch profile's account limits come from. Core branches only on the kind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum RuntimeProfileLimitsSource {
    /// The account limits of a native provider, shown exactly as its own tabs show them.
    Account { provider: String },
    /// A balance the plugin reads with one of the profile's own declared credentials.
    Balance {
        reader: String,
        #[serde(rename = "credentialEnv")]
        credential_env: String,
    },
    /// Account quota the plugin reads with credentials its own manifest declares.
    Quota { reader: String },
    /// No readable limits; the reason is shown instead of an empty bubble.
    Unavailable { reason: String },
}

impl RuntimeProfileLimitsSource {
    /// Whether the plugin's `readProfileLimits` answers for this source.
    pub fn plugin_read(&self) -> bool {
        matches!(self, Self::Balance { .. } | Self::Quota { .. })
    }

    pub fn valid_for(&self, profile: &RuntimeLaunchProfile) -> bool {
        self.valid_with(|env| {
            profile.secret_env.contains_key(env) || profile.auth_env.contains_key(env)
        })
    }

    fn valid_with(&self, declares_credential: impl Fn(&str) -> bool) -> bool {
        match self {
            Self::Account { provider } => crate::plugin_id::valid(provider) && provider.len() <= 64,
            Self::Balance {
                reader,
                credential_env,
            } => valid_reader(reader) && declares_credential(credential_env.as_str()),
            Self::Quota { reader } => valid_reader(reader),
            Self::Unavailable { reason } => valid_reason(reason),
        }
    }
}

fn valid_reader(reader: &str) -> bool {
    (1..=64).contains(&reader.len())
        && reader.starts_with(|c: char| c.is_ascii_lowercase())
        && reader
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

fn valid_reason(reason: &str) -> bool {
    !reason.trim().is_empty()
        && reason.trim() == reason
        && reason.chars().count() <= MAX_UNAVAILABLE_REASON_CHARS
        && !reason.chars().any(char::is_control)
}

/// Declared data for profiles installed from one registry id.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeRegistryDefaults {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limits_source: Option<RuntimeProfileLimitsSource>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub full_access_mode: Option<String>,
}

/// Registry ids follow the registry's own identity rule; a default must hold for a profile
/// that declares no credentials, since registry installs carry none.
pub fn valid_registry_defaults(
    defaults: &std::collections::BTreeMap<String, RuntimeRegistryDefaults>,
) -> bool {
    defaults.len() <= 128
        && defaults.iter().all(|(id, entry)| {
            valid_registry_id(id)
                && entry
                    .limits_source
                    .as_ref()
                    .is_none_or(|source| source.valid_with(|_| false))
                && entry
                    .full_access_mode
                    .as_deref()
                    .is_none_or(RuntimeLaunchProfile::valid_session_mode)
        })
}

pub fn valid_registry_id(id: &str) -> bool {
    (1..=128).contains(&id.len())
        && id.starts_with(|c: char| c.is_ascii_lowercase())
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// A profile's own declarations win; otherwise the defaults its registry id declares, if they hold.
pub fn resolve_registry_defaults(
    profile: &mut RuntimeLaunchProfile,
    defaults: &std::collections::BTreeMap<String, RuntimeRegistryDefaults>,
) {
    let Some(declared) = profile.registry_id.as_ref().and_then(|id| defaults.get(id)) else {
        return;
    };
    if profile.full_access_mode.is_none() {
        profile.full_access_mode = declared.full_access_mode.clone();
    }
    if profile.limits_source.is_none() {
        profile.limits_source = declared
            .limits_source
            .clone()
            .filter(|source| source.valid_for(profile));
    }
}

/// The account-id key under which a profile's plugin-read balance or quota is stored.
pub fn balance_account_key(profile: &str) -> String {
    format!("profile:{profile}")
}

pub fn profile_from_balance_account_key(account: &str) -> Option<&str> {
    account
        .strip_prefix("profile:")
        .filter(|profile| super::owner_profiles::valid_name(profile).is_ok())
}

#[cfg(test)]
mod tests;
