use serde::{Deserialize, Serialize};
use std::path::Path;

pub use codeterm_types::config::provider_accounts::AccountIdentity;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct AccountCapability {
    pub home_environment: String,
    pub default_home: String,
    pub secret_environment: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub excluded_environment: Vec<String>,
    pub isolated_paths: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub credential_files: Vec<String>,
    // Provisioning consumes this host-only manifest field.
    #[serde(default, skip_serializing)]
    #[ts(skip)]
    pub onboarding_flag: Option<AccountOnboardingFlag>,
    pub shared_paths: Vec<SharedStoragePath>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub shared_index_paths: Vec<String>,
    pub continuation: AccountContinuation,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AccountOnboardingFlag {
    file: String,
    key: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SharedStoragePath {
    pub path: String,
    pub kind: StoragePathKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root_environment: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub configuration: Option<StorageConfiguration>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct StorageConfiguration {
    pub file: String,
    pub format: StorageConfigurationFormat,
    pub key: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "snake_case")]
pub enum StorageConfigurationFormat {
    Toml,
    Json,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "snake_case")]
pub enum StoragePathKind {
    Directory,
    File,
    VersionedSqlite,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "snake_case")]
pub enum AccountContinuation {
    Unverified,
    Unsupported,
    Proven,
}

impl AccountCapability {
    pub fn validate(&self) -> Result<(), String> {
        let variable = |value: &str| !value.is_empty() && !value.contains(['=', '\0']);
        let relative = |value: &str| {
            !value.is_empty()
                && !value.starts_with(['/', '\\'])
                && !value.split(['/', '\\']).any(|part| {
                    part == ".." || part == "." || part.contains(':') || part.contains('\0')
                })
        };
        if !variable(&self.home_environment)
            || self.default_home.is_empty()
            || self.secret_environment.contains(&self.home_environment)
            || self.excluded_environment.contains(&self.home_environment)
            || self.isolated_paths.len() > 64
            || self.credential_files.len() > 16
            || self.credential_files.iter().any(|path| {
                !relative(path) || path.contains(['/', '\\']) || !self.isolated_paths.contains(path)
            })
            || self.onboarding_flag.as_ref().is_some_and(|flag| {
                !relative(&flag.file)
                    || flag.file.contains(['/', '\\'])
                    || !flag.file.ends_with(".json")
                    || !self.isolated_paths.contains(&flag.file)
                    || self
                        .credential_files
                        .iter()
                        .any(|path| path.eq_ignore_ascii_case(&flag.file))
                    || flag.key.is_empty()
                    || flag.key.len() > 64
                    || !flag
                        .key
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
            })
            || self.shared_paths.len() > 64
            || self.shared_index_paths.len() > 64
            || self.secret_environment.len() > 64
            || self.excluded_environment.len() > 64
            || self
                .secret_environment
                .iter()
                .chain(&self.excluded_environment)
                .any(|value| !variable(value))
            || self.isolated_paths.iter().any(|path| !relative(path))
            || self.shared_index_paths.iter().any(|path| !relative(path))
            || self.shared_paths.iter().any(|path| {
                !relative(&path.path)
                    || path
                        .root_environment
                        .as_ref()
                        .is_some_and(|value| !variable(value))
                    || path.configuration.as_ref().is_some_and(|config| {
                        !relative(&config.file)
                            || config.key.is_empty()
                            || config.key.contains('\0')
                    })
            })
        {
            return Err("invalid provider account storage declaration".into());
        }
        Ok(())
    }

    pub fn seed_named_onboarding(&self, home: &Path) -> Option<crate::provider::PreTrustOutcome> {
        let flag = self.onboarding_flag.as_ref()?;
        Some(crate::json_boolean_record::seed_true(
            &home.join(&flag.file),
            std::slice::from_ref(&flag.key),
        ))
    }
}
