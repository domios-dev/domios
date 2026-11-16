use codeterm_entitlements::compiled_free;
use codeterm_service_core::service::billing::locks_test_support::{paid, three_workspaces};
use codeterm_service_core::service::billing::{LimitGate, LimitKey};

struct LocalCredential;
#[async_trait::async_trait]
impl codeterm_account::CredentialStore for LocalCredential {
    async fn load(&self, _: &str) -> codeterm_account::Result<Option<codeterm_account::Secret>> {
        Ok(Some(codeterm_account::Secret::new("local-dev-credential")))
    }
    async fn save(&self, _: &str, _: &codeterm_account::Secret) -> codeterm_account::Result<()> {
        Ok(())
    }
    async fn delete(&self, _: &str) -> codeterm_account::Result<()> {
        Ok(())
    }
}

#[tokio::test]
async fn broken_billing_configuration_never_disables_startup_enforcement() {
    let (mut mux, newest) = three_workspaces();
    mux.set_limit_gate(LimitGate::new(
        compiled_free(),
        codeterm_entitlements::enforcing(),
    ));
    let fixture = crate::test_support::fixture(mux);
    let svc = &fixture.service;
    assert!(svc.is_locked(LimitKey::Workspaces, &newest));
    svc.ext::<crate::config::ConfigState>().lock().billing =
        serde_json::json!({"provider": "unavailable-fixture"});
    let account = std::sync::Arc::new(codeterm_account::Account::new(
        std::sync::Arc::new(codeterm_account::adapters::local::Local),
        std::sync::Arc::new(LocalCredential),
        std::time::Duration::from_secs(1),
    ));
    account.refresh().await.unwrap();
    assert!(account.session().await.is_some());
    assert!(svc
        .ext::<crate::service::neighbor_account::AccountCell>()
        .0
        .set(account)
        .is_ok());
    for refresh in [false, true] {
        svc.apply_limit_gate(LimitGate::new(paid(), true));
        let result = if refresh {
            svc.refresh_billing(codeterm_billing::RefreshEvent::SignIn)
                .await
        } else {
            svc.account_plan().await
        };
        assert_eq!(
            result.err().as_deref(),
            Some("Unknown or unavailable billing provider")
        );
        assert!(!svc.limit_gate().summary().enforcing);
        assert!(!svc.is_locked(LimitKey::Workspaces, &newest));
    }
}

#[tokio::test]
async fn account_initialization_errors_restore_compiled_free_enforcement() {
    let (mux, newest) = three_workspaces();
    let fixture = crate::test_support::fixture(mux);
    let svc = &fixture.service;
    svc.ext::<crate::config::ConfigState>().lock().account = serde_json::Value::Null;
    for refresh in [false, true] {
        svc.apply_limit_gate(LimitGate::new(paid(), true));
        let result = if refresh {
            svc.refresh_billing(codeterm_billing::RefreshEvent::SignIn)
                .await
        } else {
            svc.account_plan().await
        };
        assert_eq!(result.err().as_deref(), Some("Invalid account config"));
        assert!(!svc.limit_gate().summary().enforcing);
        assert!(!svc.is_locked(LimitKey::Workspaces, &newest));
    }
}
