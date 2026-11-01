use super::*;
use crate::{multiplexer::state::MultiplexerState, service::test_fixture::ServiceFixture};

#[tokio::test]
async fn local_admission_uses_live_credentials_and_preserves_ordinary_tab_maintenance() {
    let (mut mux, ordinary) = MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&ordinary).unwrap();
    let concierge = mux.create_tab(&workspace).unwrap();
    let fixture = ServiceFixture::new(mux);
    let svc = &fixture.service;
    svc.ext::<crate::config::ConfigState>().lock().api.token = Some("owner-secret".into());
    fixture
        .seed_neighbor_concierge(
            &concierge,
            &"a".repeat(64),
            serde_json::from_value(json!({"allowed":true,"scope":"opaque"})).unwrap(),
        )
        .await;
    let ordinary_token = svc
        .frontend_pty_env(Some(&ordinary))
        .remove("CODETERM_API_TOKEN")
        .unwrap();
    let concierge_token = svc
        .frontend_pty_env(Some(&concierge))
        .remove("CODETERM_API_TOKEN")
        .unwrap();
    for operation in ["maintenance", "host-owner", "local-input"] {
        let params = json!({"operation":operation});
        assert_eq!(
            authorize(svc, "owner-secret", Some(&params)).await.unwrap(),
            json!({"authorized":true})
        );
        assert!(authorize(svc, &concierge_token, Some(&params))
            .await
            .is_err());
        assert!(authorize(svc, "forged", Some(&params)).await.is_err());
        assert_eq!(
            authorize(svc, &ordinary_token, Some(&params)).await.is_ok(),
            operation != "host-owner"
        );
    }
    assert!(
        authorize(svc, "owner-secret", Some(&json!({"operation":"unknown"})))
            .await
            .is_err()
    );
    assert!(authorize(svc, "owner-secret", None).await.is_err());
    svc.revoke_tab_credential(&ordinary);
    assert!(authorize(
        svc,
        &ordinary_token,
        Some(&json!({"operation":"maintenance"}))
    )
    .await
    .is_err());
}
