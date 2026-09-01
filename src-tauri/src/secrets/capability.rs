//! What a secret backend declares it can do, and the host-side questions that
//! branch on those declarations instead of on a backend id. A declaration
//! decides what is OFFERED; it never authorizes a vault operation.

use serde::{Deserialize, Serialize};

use super::secret_file::SecretFile;
use super::SecretStore;

/// Account name a backend stores its headless-unlock credential under, inside
/// the host-owned plugin bucket. Shared so the host and the backend agree on
/// one location rather than two.
pub const AUTO_UNLOCK_CREDENTIAL_KEY: &str = "master_password";

/// The host-owned secret bucket for one plugin id. `host.secretGet/secretSet`
/// from inside a plugin resolve to exactly this file+service pair.
pub fn plugin_secret_bucket(plugin_id: &str) -> SecretFile {
    SecretFile::new(&format!("codeterm.plugin.{plugin_id}"))
}

/// Capabilities a backend declares about itself.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoreCapabilities {
    /// The backend can restore a usable session with no interactive input,
    /// from a credential it persisted itself under its own consent rule.
    pub auto_unlock: bool,
}

impl StoreCapabilities {
    pub const NONE: Self = Self { auto_unlock: false };

    pub const AUTO_UNLOCK: Self = Self { auto_unlock: true };
}

/// Whether the status surface may report a stored master password: only when
/// the active backend declares auto-unlock AND its credential is actually
/// present. An undeclared capability reports false even if bytes exist, because
/// a backend that cannot use them has nothing saved as far as the user is
/// concerned.
pub fn master_password_saved(store: &dyn SecretStore) -> bool {
    store.capabilities().auto_unlock && store.auto_unlock_armed()
}

/// Fail-closed guard on the consent bit: a caller may ask a backend to remember
/// a credential only if that backend declared it can use one.
pub fn check_persist_consent(
    store: &dyn SecretStore,
    persist_requested: bool,
) -> Result<(), super::SecretError> {
    if persist_requested && !store.capabilities().auto_unlock {
        return Err(super::SecretError::BadRequest {
            message: "active backend declares no auto-unlock capability".into(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secrets::{
        ListFilter, SaveRequest, SecretError, SecretRef, SecretValue, StoreStatus,
        UnlockCredentials,
    };
    use async_trait::async_trait;

    struct FakeStore {
        id: &'static str,
        caps: StoreCapabilities,
        armed: bool,
    }

    #[async_trait]
    impl SecretStore for FakeStore {
        fn id(&self) -> &'static str {
            self.id
        }
        fn capabilities(&self) -> StoreCapabilities {
            self.caps
        }
        fn auto_unlock_armed(&self) -> bool {
            self.armed
        }
        async fn status(&self) -> StoreStatus {
            StoreStatus::Locked { endpoint: None }
        }
        async fn unlock(&self, _: UnlockCredentials) -> Result<(), SecretError> {
            Ok(())
        }
        async fn lock(&self) -> Result<(), SecretError> {
            Ok(())
        }
        async fn logout(&self) -> Result<(), SecretError> {
            Ok(())
        }
        async fn save(&self, _: SaveRequest) -> Result<SecretRef, SecretError> {
            Err(SecretError::Locked)
        }
        async fn get(&self, _: &str) -> Result<SecretValue, SecretError> {
            Err(SecretError::Locked)
        }
        async fn list(&self, _: ListFilter) -> Result<Vec<SecretRef>, SecretError> {
            Err(SecretError::Locked)
        }
        async fn search(&self, _: &str) -> Result<Vec<SecretRef>, SecretError> {
            Err(SecretError::Locked)
        }
        async fn delete(&self, _: &str) -> Result<(), SecretError> {
            Err(SecretError::Locked)
        }
    }

    fn store(id: &'static str, caps: StoreCapabilities, armed: bool) -> FakeStore {
        FakeStore { id, caps, armed }
    }

    const STATUS_SIGNATURE: &str = "pub async fn status(";

    /// One top-level `fn`, sliced by lines so the check never depends on the
    /// checkout's line endings - `str::lines` drops a trailing carriage return.
    fn top_level_fn(src: &str, signature: &str) -> String {
        let mut lines = src.lines().skip_while(|l| !l.contains(signature));
        let mut body = vec![lines.next().expect("source declares the function")];
        for line in lines {
            body.push(line);
            if line == "}" {
                return body.join("\n");
            }
        }
        panic!("{signature} is not terminated by a closing brace at column zero");
    }

    #[test]
    fn master_password_saved_agrees_with_active_backend() {
        // Declared and present — the only combination the flag may report true.
        assert!(master_password_saved(&store(
            "declaring-backend",
            StoreCapabilities::AUTO_UNLOCK,
            true
        )));
        // Declared but nothing persisted: the backend cannot auto-unlock.
        assert!(!master_password_saved(&store(
            "declaring-backend",
            StoreCapabilities::AUTO_UNLOCK,
            false
        )));
        // Credential bytes present under a backend that never declared the
        // capability: it still cannot auto-unlock, so the flag stays false.
        assert!(!master_password_saved(&store(
            "silent-backend",
            StoreCapabilities::NONE,
            true
        )));
    }

    #[test]
    fn master_password_saved_is_false_when_only_a_legacy_file_exists() {
        // A declaring backend whose own bucket is empty reports false...
        let legacy_only = store("declaring-backend", StoreCapabilities::AUTO_UNLOCK, false);
        assert!(!master_password_saved(&legacy_only));
        // ...and no module answering the status question names the legacy drop.
        // The only one that does is the module that erases it.
        let own_decision = include_str!("capability.rs")
            .split("#[cfg(test)]")
            .next()
            .expect("capability.rs has a non-test half");
        for src in [
            include_str!("api.rs"),
            own_decision,
            include_str!("mod.rs"),
            include_str!("js_backend.rs"),
        ] {
            assert!(
                !src.contains("bitwarden-master.env"),
                "no status-path module may read the retired credential drop"
            );
        }
        assert!(
            include_str!("legacy.rs").contains("remove_file"),
            "the one module naming the legacy drop must be the one that erases it"
        );
    }

    #[test]
    fn autounlock_branches_on_capability_not_provider_id() {
        // Two synthetic backends with different ids and identical declarations
        // get identical treatment — the id contributes nothing to the decision.
        let armed_a = store("alpha-vault", StoreCapabilities::AUTO_UNLOCK, true);
        let armed_b = store("bitwarden", StoreCapabilities::AUTO_UNLOCK, true);
        assert_eq!(
            master_password_saved(&armed_a),
            master_password_saved(&armed_b)
        );

        let silent_a = store("alpha-vault", StoreCapabilities::NONE, true);
        let silent_b = store("bitwarden", StoreCapabilities::NONE, true);
        assert_eq!(
            master_password_saved(&silent_a),
            master_password_saved(&silent_b)
        );
        assert!(!master_password_saved(&silent_b));

        // Structural: the handler that publishes the auto-unlock surface names
        // no provider, and neither does the decision it delegates to.
        let status_handler = top_level_fn(include_str!("api.rs"), STATUS_SIGNATURE);
        assert!(
            status_handler.contains("master_password_saved"),
            "extraction missed the real status handler"
        );
        assert!(
            !status_handler.contains("bitwarden"),
            "the status handler must not branch on a provider id"
        );
        let decision = include_str!("capability.rs")
            .split("#[cfg(test)]")
            .next()
            .expect("capability.rs has a non-test half");
        assert!(
            !decision.contains("bitwarden"),
            "the auto-unlock decision must not name a provider"
        );
    }

    #[test]
    fn the_status_extractor_catches_a_provider_named_handler() {
        // Positive control: a status handler that does name a provider must
        // be caught, under LF and CRLF alike.
        let named = concat!(
            "pub async fn status(\n",
            "    state: State,\n",
            ") -> Json {\n",
            "    let saved = store.id() == \"bitwarden\";\n",
            "}\n",
            "pub fn unrelated() -> u8 { 0 }\n",
        );
        let crlf = named.replace('\n', "\r\n");
        assert!(crlf.contains("\r\n"), "the CRLF fixture must really be CRLF");
        for src in [named, crlf.as_str()] {
            let body = top_level_fn(src, STATUS_SIGNATURE);
            assert!(body.contains("bitwarden"), "control must detect the id");
            assert!(
                !body.contains("unrelated"),
                "extraction must stop at the handler's closing brace"
            );
        }
    }

    #[test]
    fn consent_is_refused_by_a_backend_that_declares_no_auto_unlock() {
        let silent = store("silent-backend", StoreCapabilities::NONE, false);
        assert!(matches!(
            check_persist_consent(&silent, true),
            Err(crate::secrets::SecretError::BadRequest { .. })
        ));
        // Not asking is always fine, whatever the backend declares.
        assert!(check_persist_consent(&silent, false).is_ok());
    }

    #[test]
    fn consent_is_allowed_only_by_declaration_never_by_identity() {
        for id in ["alpha-vault", "bitwarden"] {
            let declaring = store(id, StoreCapabilities::AUTO_UNLOCK, false);
            assert!(check_persist_consent(&declaring, true).is_ok());
            let silent = store(id, StoreCapabilities::NONE, false);
            assert!(check_persist_consent(&silent, true).is_err());
        }
    }

    #[test]
    fn plugin_bucket_name_matches_the_host_api_convention() {
        // `host.secretSet` from inside a plugin lands in this exact service, so
        // the status surface and the plugin address one location, not two.
        let src = include_str!("../service/plugin_host.rs");
        assert!(
            src.contains("crate::secrets::plugin_secret_bucket"),
            "plugin_host must resolve its bucket through the shared helper"
        );
    }
}
