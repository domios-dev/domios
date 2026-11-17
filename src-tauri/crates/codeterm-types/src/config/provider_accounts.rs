use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const IMPLICIT_ACCOUNT: &str = "default";
pub const MAX_PROVIDER_ACCOUNTS: usize = 32;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
pub struct AccountIdentity {
    pub subject: String,
    pub label: String,
}

impl AccountIdentity {
    pub fn validate(&self) -> Result<(), String> {
        let bounded = |value: &str, limit: usize| {
            !value.is_empty()
                && value.trim() == value
                && value.chars().count() <= limit
                && !value.chars().any(char::is_control)
        };
        if bounded(&self.subject, 512) && bounded(&self.label, 320) {
            Ok(())
        } else {
            Err("The harness returned an invalid account identity".into())
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderAccountConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<AccountLimitConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub removal: Option<AccountRemovalConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity: Option<AccountIdentity>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credential_namespace: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub retained_namespaces: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AccountRemovalConfig {
    pub id: String,
    pub failure: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ProviderAccountsConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_limit: Option<AccountLimitConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub continuation_receipt: Option<ContinuationReceipt>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_identity: Option<AccountIdentity>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub profiles: BTreeMap<String, ProviderAccountConfig>,
    pub order: Vec<String>,
    pub automatic_switch: bool,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub workspaces: BTreeMap<String, String>,
}

impl Default for ProviderAccountsConfig {
    fn default() -> Self {
        Self {
            default_name: None,
            default_limit: None,
            continuation_receipt: None,
            default_identity: None,
            profiles: BTreeMap::new(),
            order: vec![IMPLICIT_ACCOUNT.into()],
            automatic_switch: true,
            workspaces: BTreeMap::new(),
        }
    }
}

impl ProviderAccountsConfig {
    pub fn validate(&self) -> Result<(), String> {
        for limit in self.default_limit.iter().chain(
            self.profiles
                .values()
                .filter_map(|profile| profile.limit.as_ref()),
        ) {
            if limit
                .resets_at_ms
                .is_some_and(|at| at > 9_007_199_254_740_991)
            {
                return Err("The account limit reset time is invalid".into());
            }
        }
        if let Some(receipt) = &self.continuation_receipt {
            let values = [
                (&receipt.host_sha, 128),
                (&receipt.capability_hash, 128),
                (&receipt.binary_fingerprint, 2048),
                (&receipt.session_id, 512),
                (&receipt.source_account, 64),
                (&receipt.target_account, 64),
                (&receipt.source_nonce, 128),
                (&receipt.resumed_nonce, 128),
            ];
            if values.iter().any(|(value, limit)| {
                value.is_empty() || value.len() > *limit || value.chars().any(char::is_control)
            }) || receipt.source_account == receipt.target_account
                || receipt.source_nonce == receipt.resumed_nonce
                || receipt.verified_at_ms > 9_007_199_254_740_991
            {
                return Err("The account continuation receipt is invalid".into());
            }
        }
        if let Some(identity) = &self.default_identity {
            identity.validate()?;
        }
        if self
            .default_name
            .as_deref()
            .is_some_and(|name| !valid_account_name(name))
        {
            return Err("Account names must contain 1 to 80 printable characters".into());
        }
        if self.profiles.len() >= MAX_PROVIDER_ACCOUNTS {
            return Err(format!(
                "A harness supports at most {MAX_PROVIDER_ACCOUNTS} accounts"
            ));
        }
        let mut owners = BTreeMap::<&str, &str>::new();
        for (id, profile) in &self.profiles {
            if let Some(removal) = &profile.removal {
                if !valid_account_id(&removal.id)
                    || removal.failure.is_empty()
                    || removal.failure.chars().count() > 2048
                    || removal.failure.chars().any(char::is_control)
                {
                    return Err("Account removal metadata must be bounded printable text".into());
                }
            }
            for namespace in std::iter::once(profile.credential_namespace.as_deref().unwrap_or(id))
                .chain(profile.retained_namespaces.iter().map(String::as_str))
            {
                if owners.insert(namespace, id).is_some() {
                    return Err("Account credential namespaces cannot be shared".into());
                }
            }
            let namespaces = profile
                .credential_namespace
                .iter()
                .chain(profile.retained_namespaces.iter());
            if profile.retained_namespaces.len() > MAX_PROVIDER_ACCOUNTS
                || namespaces
                    .clone()
                    .any(|namespace| !valid_account_id(namespace) || namespace == IMPLICIT_ACCOUNT)
            {
                return Err(
                    "Account credential namespaces must be bounded portable identifiers".into(),
                );
            }
            let declared: Vec<_> = namespaces.collect();
            if declared.iter().collect::<BTreeSet<_>>().len() != declared.len() {
                return Err("Account credential namespaces must be unique".into());
            }
            if let Some(identity) = &profile.identity {
                identity.validate()?;
            }
            if id == IMPLICIT_ACCOUNT || !valid_account_id(id) {
                return Err("Account ids must be non-default portable identifiers".into());
            }
            if let Some(name) = &profile.name {
                if !valid_account_name(name) {
                    return Err("Account names must contain 1 to 80 printable characters".into());
                }
            }
        }
        let ordered: BTreeSet<_> = self.order.iter().map(String::as_str).collect();
        if self.order.len() != self.profiles.len() + 1
            || ordered.len() != self.order.len()
            || !ordered.contains(IMPLICIT_ACCOUNT)
            || self
                .profiles
                .keys()
                .any(|id| !ordered.contains(id.as_str()))
        {
            return Err(
                "Account order must contain every account and the usual sign-in exactly once"
                    .into(),
            );
        }
        if self.workspaces.len() > 256 {
            return Err("A harness supports at most 256 workspace account choices".into());
        }
        for (workspace, id) in &self.workspaces {
            if workspace.is_empty()
                || workspace.len() > 256
                || workspace.chars().any(char::is_control)
                || !ordered.contains(id.as_str())
            {
                return Err(
                    "Workspace account choices must name an existing workspace and account".into(),
                );
            }
        }
        Ok(())
    }

    pub fn starting_account<'a>(
        &'a self,
        workspace: Option<&str>,
        requested: Option<&str>,
    ) -> Result<&'a str, String> {
        self.validate()?;
        let choice = requested.or_else(|| {
            workspace.and_then(|workspace| self.workspaces.get(workspace).map(String::as_str))
        });
        match choice {
            Some(choice) => self
                .order
                .iter()
                .find(|id| id.as_str() == choice)
                .map(String::as_str)
                .ok_or_else(|| "The selected harness account no longer exists".into()),
            None => Ok(self.order[0].as_str()),
        }
    }

    pub fn first_eligible<'a>(
        &'a self,
        starting: &str,
        mut eligible: impl FnMut(&str) -> bool,
    ) -> Result<Option<&'a str>, String> {
        self.validate()?;
        let start = self
            .order
            .iter()
            .position(|id| id == starting)
            .ok_or("The selected harness account no longer exists")?;
        Ok((0..self.order.len())
            .map(|offset| self.order[(start + offset) % self.order.len()].as_str())
            .find(|id| eligible(id)))
    }

    pub fn next_eligible<'a>(
        &'a self,
        current: &str,
        mut eligible: impl FnMut(&str) -> bool,
    ) -> Result<Option<&'a str>, String> {
        self.first_eligible(current, |id| id != current && eligible(id))
    }

    pub fn remove(&mut self, id: &str) -> Result<(), String> {
        self.validate()?;
        if id == IMPLICIT_ACCOUNT {
            return Err("The usual sign-in cannot be removed".into());
        }
        if self.profiles.remove(id).is_none() {
            return Err("The selected harness account no longer exists".into());
        }
        self.order.retain(|account| account != id);
        self.workspaces.retain(|_, account| account != id);
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContinuationReceipt {
    pub supported: bool,
    pub host_sha: String,
    pub capability_hash: String,
    pub binary_fingerprint: String,
    pub session_id: String,
    pub source_account: String,
    pub target_account: String,
    pub source_nonce: String,
    pub resumed_nonce: String,
    pub verified_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AccountLimitConfig {
    pub resets_at_ms: Option<u64>,
}

pub fn valid_account_name(name: &str) -> bool {
    name.trim() == name
        && !name.is_empty()
        && name.chars().count() <= 80
        && !name.chars().any(char::is_control)
}

pub fn valid_account_id(id: &str) -> bool {
    (1..=64).contains(&id.len())
        && id.as_bytes()[0].is_ascii_lowercase()
        && id
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        && !matches!(id, "con" | "prn" | "aux" | "nul")
        && !(id.len() == 4
            && (id.starts_with("com") || id.starts_with("lpt"))
            && matches!(id.as_bytes()[3], b'1'..=b'9'))
}

#[cfg(test)]
mod tests;
