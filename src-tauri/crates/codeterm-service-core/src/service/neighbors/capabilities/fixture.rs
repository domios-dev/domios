use super::*;
use codeterm_account::*;
use std::{sync::Arc, time::Duration};

struct Provider;

fn hosted(mut tokens: Tokens) -> Tokens {
    tokens.session.identity.provider = "neighbor-fixture".into();
    tokens
}

#[async_trait::async_trait]
impl IdentityProvider for Provider {
    fn id(&self) -> &str {
        "neighbor-fixture"
    }
    fn needs_browser_callback(&self) -> bool {
        false
    }
    async fn start_login(&self, flow: &LoginFlow) -> codeterm_account::Result<LoginStart> {
        match adapters::local::Local.start_login(flow).await? {
            LoginStart::Immediate(tokens) => Ok(LoginStart::Immediate(hosted(tokens))),
            LoginStart::Browser(_) => panic!("fixture never opens a browser"),
        }
    }
    async fn redeem(&self, _: Redemption) -> codeterm_account::Result<Tokens> {
        Err(Error::Provider)
    }
    async fn refresh(&self, token: &Secret) -> codeterm_account::Result<Tokens> {
        adapters::local::Local.refresh(token).await.map(hosted)
    }
    async fn revoke(&self, _: &Secret) -> codeterm_account::Result<()> {
        Ok(())
    }
}

#[derive(Default)]
struct Credentials(parking_lot::Mutex<Option<Secret>>);

#[async_trait::async_trait]
impl CredentialStore for Credentials {
    async fn load(&self, _: &str) -> codeterm_account::Result<Option<Secret>> {
        Ok(self.0.lock().clone())
    }
    async fn save(&self, _: &str, token: &Secret) -> codeterm_account::Result<()> {
        *self.0.lock() = Some(token.clone());
        Ok(())
    }
    async fn delete(&self, _: &str) -> codeterm_account::Result<()> {
        *self.0.lock() = None;
        Ok(())
    }
}

struct NoBrowser;

#[async_trait::async_trait]
impl Browser for NoBrowser {
    async fn open(&self, _: &url::Url) -> codeterm_account::Result<()> {
        panic!("fixture never opens a browser")
    }
}

impl crate::service::test_fixture::ServiceFixture {
    pub fn register_neighbor_advisory_provider(&self, id: &str) {
        let manifest = codeterm_plugin::js_runtime::PluginManifest::from_json_str(&serde_json::json!({
            "id":id,"version":"1.0.0","displayName":"Fixture",
            "capabilities":{"agentProvider":true},
            "commands":{"autoApproveFlags":["opaque-full"]},
            "spawn":{"readinessStrategy":{"kind":"fixed_delay","durationMs":0},"taskDelivery":"pty_input","verifySubmit":{"kind":"none"},
                "launchPermissions":{"allowRuleArgsFormat":["opaque-allow","{rule}"],"commandRuleFormat":"OpaqueShell({command}:*)",
                    "scopedFilesystem":{"noPromptArgs":["opaque-mode","opaque-no-prompt"],"readAllRules":["OpaqueRead","OpaqueGlob","OpaqueGrep"],
                        "readRootRule":"OpaqueRead({path}/**)","writeRootRule":"OpaqueEdit({path}/**)","additionalDirectoryArgs":["opaque-dir","{path}"]}}}
        }).to_string()).unwrap();
        let vm = Arc::new(
            codeterm_plugin::js_runtime::PluginVm::from_source(id, "module.exports={}".into())
                .unwrap(),
        );
        self.service.agent_registry.register_provider(Box::new(
            codeterm_plugin::js_runtime::JsProvider::new(manifest, vm),
        ));
    }

    pub async fn pause_neighbor_fixture(&self, tab: &str) {
        let authority = self
            .service
            .mux
            .read()
            .get_tab(tab)
            .unwrap()
            .neighbor_authority
            .clone()
            .unwrap();
        let metadata = Metadata::new(self.service.account_data_root().unwrap(), &authority.uid);
        let mut settings = metadata.settings().await.unwrap();
        settings.paused.insert(authority.friendship_id);
        metadata.save_settings(settings).await.unwrap();
    }
    pub fn seed_neighbor_author(&self, owner_uid: &str, friendship: &str, peer_uid: &str) {
        self.service
            .extensions
            .get_or_init(crate::mem_ops::author::FixtureNeighborAuthors::default)
            .0
            .write()
            .insert((owner_uid.into(), friendship.into()), peer_uid.into());
    }
    pub async fn save_neighbor_restore_settings(
        &self,
        uid: &str,
        friendship: &str,
        tab: &str,
        permission: Permission,
    ) {
        let metadata = Metadata::new(self.service.account_data_root().unwrap(), uid);
        let mut settings = metadata.settings().await.unwrap();
        settings.bindings.insert(friendship.into(), tab.into());
        settings.permissions.insert(friendship.into(), permission);
        metadata.save_settings(settings).await.unwrap();
    }

    pub async fn load_neighbor_restore_settings(&self, uid: &str) {
        let metadata = Metadata::new(self.service.account_data_root().unwrap(), uid);
        let settings = metadata.settings().await.unwrap();
        for friendship in settings.bindings.keys() {
            self.service
                .set_neighbor_friend_receiver_for_test(friendship, Some(true), 1);
        }
        self.service
            .remember_neighbor_concierge_accounts(uid, &settings);
    }

    pub async fn seed_neighbor_saved_file(
        &self,
        friendship: &str,
        file_id: &str,
        bytes: &[u8],
    ) -> std::path::PathBuf {
        assert!(codeterm_neighbors::wire::hex_id(file_id, 32));
        let account = self
            .service
            .ext::<crate::service::account::AccountCell>()
            .0
            .get()
            .unwrap();
        let session = account.session().await.unwrap();
        let metadata = Metadata::new(
            self.service.account_data_root().unwrap(),
            &session.identity.uid,
        );
        let folder = metadata.friend_folder(friendship).unwrap();
        let path = folder.join(format!("{file_id}-opaque.bin"));
        let destination = path.clone();
        let bytes = bytes.to_vec();
        let size = bytes.len();
        tokio::task::spawn_blocking(move || {
            codeterm_util::private_file::create_dir_all(&folder).unwrap();
            std::fs::write(destination, bytes).unwrap();
        })
        .await
        .unwrap();
        metadata.write(format!("files-{friendship}.json"), serde_json::json!([{"file_id":file_id,"message_id":"1".repeat(32),"name":"opaque.bin","mime":"application/octet-stream","size_bytes":size,"direction":"in","state":"saved"}])).await.unwrap();
        path
    }

    pub async fn seed_neighbor_concierge(
        &self,
        tab: &str,
        friendship: &str,
        permission: Permission,
    ) {
        self.service
            .set_neighbor_friend_receiver_for_test(friendship, Some(true), 1);
        let account = self
            .service
            .ext::<crate::service::account::AccountCell>()
            .0
            .get_or_init(|| async {
                let account = Arc::new(Account::new(
                    Arc::new(Provider),
                    Arc::new(Credentials::default()),
                    Duration::from_secs(1),
                ));
                account.login(Arc::new(NoBrowser)).await.unwrap();
                account
            })
            .await;
        let session = account.session().await.unwrap();
        self.service
            .extensions
            .get_or_init(crate::mem_ops::author::FixtureNeighborAuthors::default)
            .0
            .write()
            .insert(
                (session.identity.uid.clone(), friendship.into()),
                "friend-account-uid".into(),
            );
        let metadata = Metadata::new(
            self.service.account_data_root().unwrap(),
            &session.identity.uid,
        );
        let mut settings = metadata.settings().await.unwrap();
        if let Some(tab) = self.service.mux.write().get_tab_mut(tab) {
            if tab.neighbor_authority.is_none() {
                tab.neighbor_authority = Some(crate::multiplexer::types::NeighborAuthority {
                    uid: session.identity.uid.clone(),
                    friendship_id: friendship.into(),
                    root_tab_id: None,
                });
            }
        }
        settings.enabled = true;
        settings.bindings.insert(friendship.into(), tab.into());
        settings.permissions.insert(friendship.into(), permission);
        metadata.save_settings(settings.clone()).await.unwrap();
        self.service
            .remember_neighbor_concierge_accounts(&session.identity.uid, &settings);
    }
}
