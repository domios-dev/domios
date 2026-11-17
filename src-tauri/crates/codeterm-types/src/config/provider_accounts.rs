use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const IMPLICIT_ACCOUNT: &str = "default";
pub const MAX_PROVIDER_ACCOUNTS: usize = 32;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderAccountConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ProviderAccountsConfig {
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
            profiles: BTreeMap::new(),
            order: vec![IMPLICIT_ACCOUNT.into()],
            automatic_switch: true,
            workspaces: BTreeMap::new(),
        }
    }
}

impl ProviderAccountsConfig {
    pub fn validate(&self) -> Result<(), String> {
        if self.profiles.len() >= MAX_PROVIDER_ACCOUNTS {
            return Err(format!(
                "A harness supports at most {MAX_PROVIDER_ACCOUNTS} accounts"
            ));
        }
        for (id, profile) in &self.profiles {
            if id == IMPLICIT_ACCOUNT || !valid_account_id(id) {
                return Err("Account ids must be non-default portable identifiers".into());
            }
            if let Some(name) = &profile.name {
                if name.trim() != name
                    || name.is_empty()
                    || name.chars().count() > 80
                    || name.chars().any(char::is_control)
                {
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
