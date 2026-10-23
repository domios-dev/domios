use std::collections::HashMap;
use std::sync::Arc;

use super::mock_http::MockServer;
use crate::embedder::catalog::{self, BuildContext, BuildError, EmbedderSelection};
use crate::embedder::key::{ApiKey, KeySources, KeyVault};
use crate::embedder::openai;
use crate::embedder::provider::Content;

struct MockVault(HashMap<String, String>);

impl KeyVault for MockVault {
    fn contains(&self, name: &str) -> Result<bool, String> {
        Ok(self.0.contains_key(name))
    }

    fn read(&self, name: &str) -> Result<Option<ApiKey>, String> {
        Ok(self
            .0
            .get(name)
            .and_then(|value| ApiKey::new(value.clone())))
    }
}

fn vault(key: Option<&str>) -> Arc<MockVault> {
    Arc::new(MockVault(
        key.map(|key| (KeySources::vault_name(openai::ID), key.to_string()))
            .into_iter()
            .collect(),
    ))
}

fn remote_selection() -> EmbedderSelection {
    EmbedderSelection {
        provider: openai::ID.into(),
        model: Some("text-embedding-3-small".into()),
        dimension: Some(32),
    }
}

#[test]
fn remote_switch_needs_a_stored_key_and_the_opt_in_and_the_key_file_wins() {
    let home = tempfile::tempdir().unwrap();
    let defaults = codeterm_types::config::EmbedderConfig::default();
    assert!(!defaults.remote_opt_in);
    assert_eq!(defaults.provider, "local");
    assert_eq!(
        defaults,
        serde_json::from_str::<codeterm_types::config::EmbedderConfig>("{}").unwrap()
    );
    let server = MockServer::embeddings(32);
    let ctx = |vault: Arc<MockVault>| BuildContext {
        keys: KeySources::new(home.path().join("secrets"), Some(vault)),
        endpoint: Some(server.url.clone()),
    };
    let local = catalog::activate(
        &EmbedderSelection::default(),
        &ctx(vault(None)),
        defaults.remote_opt_in,
    )
    .unwrap();
    assert!(!local.remote());
    let mut external = ctx(vault(Some("vault-key")));
    external.endpoint = None;
    assert!(matches!(
        catalog::activate(&remote_selection(), &external, true),
        Err(BuildError::Failed(reason)) if reason == "unit tests require a loopback mock endpoint"
    ));
    let refusal = |vault, opt_in| match catalog::activate(&remote_selection(), &ctx(vault), opt_in)
    {
        Err(BuildError::Refused(refusal)) => refusal,
        other => panic!("expected a refusal, got {:?}", other.map(|p| p.identity())),
    };

    let no_key = refusal(vault(None), true);
    assert!(no_key.missing_key && !no_key.missing_opt_in);
    let hints = no_key.hints().join("\n");
    assert!(
        hints.contains("mem embedder key set openai")
            && hints.contains("openai-embedder.key")
            && hints.contains("mem secret save --name openai-embedder")
    );
    let no_opt_in = refusal(vault(Some("vault-key")), defaults.remote_opt_in);
    assert!(!no_opt_in.missing_key && no_opt_in.missing_opt_in);
    assert!(no_opt_in
        .message()
        .contains("memory.embedder.remote_opt_in"));
    let neither = refusal(vault(None), false);
    assert!(neither.missing_key && neither.missing_opt_in);
    assert_eq!(
        neither.key_file,
        home.path().join("secrets/openai-embedder.key")
    );
    assert_eq!(server.count(), 0);

    let from_vault =
        catalog::activate(&remote_selection(), &ctx(vault(Some("vault-key"))), true).unwrap();
    assert!(from_vault.remote());
    from_vault.embed_query(&Content::text("hello")).unwrap();
    assert_eq!(
        server.last().authorization.as_deref(),
        Some("Bearer vault-key")
    );

    let secrets = home.path().join("secrets");
    std::fs::create_dir_all(&secrets).unwrap();
    let file = secrets.join("openai-embedder.key");
    std::fs::write(&file, "file-key\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(matches!(
            catalog::activate(&remote_selection(), &ctx(vault(Some("vault-key"))), true),
            Err(BuildError::KeyUnreadable(_))
        ));
    }
    #[cfg(windows)]
    assert!(matches!(
        catalog::activate(&remote_selection(), &ctx(vault(Some("vault-key"))), true),
        Err(BuildError::KeyUnreadable(_))
    ));
    codeterm_util::private_file::restrict(&file).unwrap();
    let from_file =
        catalog::activate(&remote_selection(), &ctx(vault(Some("vault-key"))), true).unwrap();
    from_file.embed_query(&Content::text("hello")).unwrap();
    assert_eq!(
        server.last().authorization.as_deref(),
        Some("Bearer file-key")
    );
}
