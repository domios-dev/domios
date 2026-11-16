//! Who a vault entry belongs to. Domios keeps its own credentials (account refresh tokens,
//! Blueprint webhook keys, bridge and analytics keys) in the same vault as the owner's; only
//! the owner's entries may be offered for binding into a package.

use serde::{Deserialize, Serialize};

use crate::SecretRef;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "snake_case")]
pub enum SecretOwner {
    User,
    System,
}

/// Name prefixes the host writes its own entries under.
const SYSTEM_PREFIXES: [&str; 7] = [
    "domios-",
    "domios.",
    "domios:",
    "codeterm-",
    "codeterm.",
    "codeterm:",
    "blueprint-webhook-",
];

/// Host-owned entries with a fixed name.
const SYSTEM_NAMES: [&str; 1] = ["telegram_bot_token"];

pub fn owner_of(name: &str) -> SecretOwner {
    let name = name.trim();
    if SYSTEM_NAMES.contains(&name)
        || SYSTEM_PREFIXES
            .iter()
            .any(|prefix| name.len() > prefix.len() && name.starts_with(prefix))
    {
        SecretOwner::System
    } else {
        SecretOwner::User
    }
}

/// One entry of the vault listing: metadata only, never a value, and whose it is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
pub struct SecretItem {
    pub item_id: String,
    pub name: String,
    pub folder: Option<String>,
    pub organization: Option<String>,
    pub collection: Option<String>,
    /// Unix milliseconds.
    pub last_modified: Option<i64>,
    pub owner: SecretOwner,
}

impl From<SecretRef> for SecretItem {
    fn from(entry: SecretRef) -> Self {
        Self {
            owner: owner_of(&entry.name),
            item_id: entry.item_id,
            name: entry.name,
            folder: entry.folder,
            organization: entry.organization,
            collection: entry.collection,
            last_modified: entry.last_modified,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_written_entries_are_system_and_everything_else_is_the_owners() {
        for name in [
            "domios-account-local-refresh",
            "domios-account-mesh-installation-key",
            "domios.amplitude.prod.api_key",
            "domios:provider:opaque:KEY",
            "blueprint-webhook-opaque",
            "telegram_bot_token",
        ] {
            assert_eq!(owner_of(name), SecretOwner::System, "{name}");
        }
        for name in [
            "SEARCH_API_KEY",
            "weaverqa/token",
            "domios",
            "my-domios-key",
        ] {
            assert_eq!(owner_of(name), SecretOwner::User, "{name}");
        }
    }

    #[test]
    fn a_listed_item_keeps_its_metadata_and_names_its_owner() {
        let item = SecretItem::from(SecretRef {
            item_id: "opaque-id".into(),
            name: "blueprint-webhook-opaque".into(),
            folder: None,
            organization: None,
            collection: None,
            last_modified: Some(7),
        });
        assert_eq!(item.owner, SecretOwner::System);
        assert_eq!(item.last_modified, Some(7));
        assert_eq!(
            serde_json::to_value(&item).unwrap()["owner"],
            serde_json::json!("system")
        );
    }
}
