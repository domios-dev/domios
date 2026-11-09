use super::*;
use crate::*;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::Duration;

#[test]
fn recorded_foreign_target_is_a_mismatch_with_both_targets_in_the_reason() {
    let mismatch = TargetMismatch::detect(Some("domios-signin-qa"), "domios-d9357").unwrap();
    assert_eq!(mismatch.saved.as_deref(), Some("domios-signin-qa"));
    assert_eq!(mismatch.build, "domios-d9357");
    assert!(mismatch.reason.contains("domios-signin-qa"));
    assert!(mismatch.reason.contains("domios-d9357"));
    for saved in [None, Some(""), Some(" "), Some("domios-d9357")] {
        assert_eq!(TargetMismatch::detect(saved, "domios-d9357"), None);
    }
}

#[test]
fn refusal_without_a_record_names_the_build_and_takes_a_recorded_saved_target() {
    let refused = TargetMismatch::refused("domios-d9357");
    assert_eq!(refused.saved, None);
    assert!(refused.reason.contains("domios-d9357"));
    assert_eq!(refused.clone().with_saved(None), refused);
    assert_eq!(refused.clone().with_saved(Some("domios-d9357")), refused);
    assert_eq!(
        refused.with_saved(Some("domios-signin-qa")),
        TargetMismatch::detect(Some("domios-signin-qa"), "domios-d9357").unwrap()
    );
}

#[derive(Default)]
struct Saved(tokio::sync::Mutex<Option<Secret>>);
#[async_trait]
impl CredentialStore for Saved {
    async fn load(&self, _: &str) -> Result<Option<Secret>> {
        Ok(self.0.lock().await.clone())
    }
    async fn save(&self, _: &str, token: &Secret) -> Result<()> {
        *self.0.lock().await = Some(token.clone());
        Ok(())
    }
    async fn delete(&self, _: &str) -> Result<()> {
        *self.0.lock().await = None;
        Ok(())
    }
}

#[derive(Default)]
struct ForeignAudience(AtomicBool);
fn tokens() -> Tokens {
    Tokens {
        session: Session {
            login_generation: 0,
            identity: Identity {
                uid: "fixture-uid".into(),
                email: None,
                display_name: None,
                provider: "fixture".into(),
                photo_url: None,
            },
            id_token: Secret::new("fixture-id-token"),
            expires_at: None,
        },
        refresh_token: Secret::new("fixture-saved-refresh"),
    }
}
#[async_trait]
impl IdentityProvider for ForeignAudience {
    fn id(&self) -> &str {
        "fixture"
    }
    fn needs_browser_callback(&self) -> bool {
        false
    }
    async fn start_login(&self, _: &LoginFlow) -> Result<LoginStart> {
        Ok(LoginStart::Immediate(tokens()))
    }
    async fn redeem(&self, _: Redemption) -> Result<Tokens> {
        Err(Error::Provider)
    }
    async fn refresh(&self, _: &Secret) -> Result<Tokens> {
        if self.0.load(Ordering::SeqCst) {
            Err(Error::TargetMismatch)
        } else {
            Ok(tokens())
        }
    }
    async fn revoke(&self, _: &Secret) -> Result<()> {
        Ok(())
    }
}
struct NoBrowser;
#[async_trait]
impl Browser for NoBrowser {
    async fn open(&self, _: &url::Url) -> Result<()> {
        panic!("Immediate sign-in must not open a browser")
    }
}

#[tokio::test]
async fn foreign_audience_credential_is_signed_out_with_a_reason_and_kept() {
    let provider = Arc::new(ForeignAudience::default());
    let store = Arc::new(Saved::default());
    *store.0.lock().await = Some(Secret::new("fixture-foreign-refresh"));
    provider.0.store(true, Ordering::SeqCst);
    let account = Arc::new(Account::new(
        provider.clone(),
        store.clone(),
        Duration::from_secs(1),
    ));

    assert_eq!(account.refresh().await.unwrap_err(), Error::TargetMismatch);
    let status = account.status().await;
    assert_eq!(status.identity, None);
    assert_eq!(status.last_sign_in_failure, None);
    assert_eq!(
        status.target_mismatch,
        Some(TargetMismatch::refused(build_target()))
    );
    assert_eq!(
        store.0.lock().await.as_ref().map(Secret::expose),
        Some("fixture-foreign-refresh")
    );
    assert_eq!(
        account
            .current_session()
            .await
            .unwrap()
            .map(|s| s.identity.uid),
        None
    );

    provider.0.store(false, Ordering::SeqCst);
    let status = account.login(Arc::new(NoBrowser)).await.unwrap();
    assert_eq!(
        status.identity.map(|identity| identity.uid).as_deref(),
        Some("fixture-uid")
    );
    assert_eq!(status.target_mismatch, None);
}

#[tokio::test]
async fn signed_in_session_whose_refresh_hits_another_target_stops_showing_signed_in() {
    let provider = Arc::new(ForeignAudience::default());
    let store = Arc::new(Saved::default());
    let account = Arc::new(Account::new(
        provider.clone(),
        store.clone(),
        Duration::from_secs(1),
    ));
    account.login(Arc::new(NoBrowser)).await.unwrap();
    assert!(account.status().await.identity.is_some());

    provider.0.store(true, Ordering::SeqCst);
    assert_eq!(account.refresh().await.unwrap_err(), Error::TargetMismatch);
    let status = account.status().await;
    assert_eq!(status.identity, None);
    assert!(status.target_mismatch.is_some());
    assert_eq!(
        store.0.lock().await.as_ref().map(Secret::expose),
        Some("fixture-saved-refresh")
    );
}
