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
    /// No readable limits; the reason is shown instead of an empty bubble.
    Unavailable { reason: String },
}

impl RuntimeProfileLimitsSource {
    pub fn valid_for(&self, profile: &RuntimeLaunchProfile) -> bool {
        match self {
            Self::Account { provider } => crate::plugin_id::valid(provider) && provider.len() <= 64,
            Self::Balance {
                reader,
                credential_env,
            } => {
                valid_reader(reader)
                    && (profile.secret_env.contains_key(credential_env)
                        || profile.auth_env.contains_key(credential_env))
            }
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

/// The account-id key under which a profile's plugin-read balance is stored.
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
