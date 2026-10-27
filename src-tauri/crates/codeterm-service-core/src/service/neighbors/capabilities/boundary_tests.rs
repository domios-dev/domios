use super::*;
use crate::{multiplexer::state::MultiplexerState, service::test_fixture::ServiceFixture};
use serde_json::json;

async fn fixture(permission: Permission) -> (ServiceFixture, TabCaller, String) {
    let (mut mux, general) = MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&general).unwrap();
    let concierge = mux.create_tab(&workspace).unwrap();
    let tab = mux.get_tab_mut(&concierge).unwrap();
    tab.agent_type = Some("fixture".into());
    tab.agent_session_id = Some("own-session".into());
    tab.session_binding_nonce = Some("own-nonce".into());
    tab.reported_session_binding_nonce = Some("own-nonce".into());
    let fixture = ServiceFixture::new(mux);
    fixture
        .seed_neighbor_concierge(&concierge, &"a".repeat(64), permission)
        .await;
    let token = fixture
        .service
        .frontend_pty_env(Some(&concierge))
        .remove("CODETERM_API_TOKEN")
        .unwrap();
    (
        fixture,
        TabCaller {
            tab_id: concierge,
            binding_nonce: "own-nonce".into(),
        },
        token,
    )
}

fn allowed() -> Permission {
    Permission {
        allowed: true,
        scope: "opaque".into(),
        ..Default::default()
    }
}

fn save(fixture: &ServiceFixture, area: &str) -> String {
    fixture
        .service
        .mem
        .insert_raw(&crate::mem::store::NewMemItem {
            kind: crate::mem::store::MemKind::Fact,
            scope: crate::mem::store::MemScope::Global,
            title: format!("opaque {area}"),
            summary: None,
            area: Some(area.into()),
            tags: vec![],
            body: format!("opaque {area} fixture"),
            source: None,
            importance: 3.0,
            source_path: None,
            pinned: false,
        })
        .unwrap()
}

#[tokio::test]
async fn authenticated_memory_grants_filter_search_and_refuse_foreign_get_and_restricted_wake_up() {
    let mut permission = allowed();
    permission.memory_areas = Some(vec!["project".into()]);
    let (fixture, caller, token) = fixture(permission.clone()).await;
    let project = save(&fixture, "project");
    let foreign = save(&fixture, "private");
    let service = &fixture.service;
    assert!(service
        .validate_neighbor_memory_areas(&permission)
        .await
        .is_ok());
    permission.memory_areas = Some(vec!["unknown".into()]);
    assert_eq!(
        service.validate_neighbor_memory_areas(&permission).await,
        Err(codeterm_neighbors::Error::MemoryAreaUnknown)
    );
    assert!(service
        .authorize_neighbor_rpc_credential(&token, "mem.get", Some(&json!({"id":project})))
        .await
        .is_ok());
    assert!(service
        .authorize_neighbor_rpc_credential(&token, "mem.get", Some(&json!({"id":foreign})))
        .await
        .unwrap_err()
        .starts_with("neighbor_memory_area_denied:"));
    assert!(service
        .authorize_neighbor_http(&caller, "GET", &format!("/api/mem/items/{foreign}"))
        .await
        .is_err());
    assert!(service
        .authorize_neighbor_rpc_credential(&token, "mem.wake_up", None)
        .await
        .is_err());
    assert!(service
        .authorize_neighbor_http(&caller, "GET", "/api/mem/wake-up")
        .await
        .is_err());
    for area in [None, Some("private".into()), Some("project".into())] {
        let expect_visible = area.as_deref() != Some("private");
        let service = std::sync::Arc::new(service.clone());
        let caller_id = caller.tab_id.clone();
        let foreign = foreign.clone();
        let result = tokio::task::spawn_blocking(move || {
            crate::mem_ops::search::execute_search(
                &service,
                crate::mem_ops::search::SearchParams {
                    q: "opaque".into(),
                    caller_tab_id: Some(caller_id),
                    area,
                    supporter: Some(true),
                    project: Some("foreign-plan".into()),
                    ..Default::default()
                },
            )
        })
        .await
        .unwrap()
        .unwrap();
        assert!(result["hits"]
            .as_array()
            .unwrap()
            .iter()
            .all(|hit| hit["item"]["area"] == "project" && hit["item"]["id"] != foreign));
        assert_eq!(
            !result["hits"].as_array().unwrap().is_empty(),
            expect_visible
        );
    }
    permission = allowed();
    permission.allow_memory = false;
    fixture
        .seed_neighbor_concierge(&caller.tab_id, &"a".repeat(64), permission)
        .await;
    for method in ["mem.search", "mem.get", "mem.wake_up"] {
        assert!(service
            .authorize_neighbor_rpc_credential(&token, method, None)
            .await
            .unwrap_err()
            .starts_with("neighbor_memory_not_allowed:"));
    }
}

#[tokio::test]
async fn file_read_grant_is_own_friend_only_and_revocation_is_immediate() {
    let (fixture, caller, _) = fixture(allowed()).await;
    assert!(fixture
        .service
        .authorize_neighbor_file_read(&caller, &"a".repeat(64))
        .await
        .is_ok());
    for friend in [
        "b".repeat(64),
        "../outside".into(),
        "\\\\server\\share".into(),
        "CON".into(),
    ] {
        assert!(fixture
            .service
            .authorize_neighbor_file_read(&caller, &friend)
            .await
            .is_err());
    }
    let mut permission = allowed();
    permission.allow_files = false;
    fixture
        .seed_neighbor_concierge(&caller.tab_id, &"a".repeat(64), permission)
        .await;
    assert!(fixture
        .service
        .authorize_neighbor_file_read(&caller, &"a".repeat(64))
        .await
        .unwrap_err()
        .starts_with("neighbor_files_not_allowed:"));
}

#[tokio::test]
async fn credential_authority_cannot_be_forged_or_widened_by_role_and_nonce_changes() {
    let (fixture, caller, token) = fixture(allowed()).await;
    for params in [
        None,
        Some(json!({})),
        Some(json!({"caller_tab_id":"foreign"})),
    ] {
        for method in [
            "model_provider.discover",
            "mem.secret.get",
            "mem.save",
            "tab.new",
            "cron.trigger",
        ] {
            assert!(fixture
                .service
                .authorize_neighbor_rpc_credential(&token, method, params.as_ref())
                .await
                .is_err());
        }
    }
    fixture
        .service
        .mux
        .write()
        .get_tab_mut(&caller.tab_id)
        .unwrap()
        .role = Some(crate::multiplexer::types::AgentRole::Worker);
    assert!(fixture
        .service
        .authorize_neighbor_rpc_credential(&token, "mem.secret.get", None)
        .await
        .is_err());
    fixture
        .service
        .mux
        .write()
        .get_tab_mut(&caller.tab_id)
        .unwrap()
        .session_binding_nonce = Some("replacement".into());
    assert!(fixture
        .service
        .authorize_neighbor_rpc_credential(&token, "mem.search", None)
        .await
        .is_err());
}

#[tokio::test]
async fn rebind_and_signout_never_turn_a_former_concierge_into_an_unrestricted_tab() {
    let (fixture, caller, token) = fixture(allowed()).await;
    fixture
        .seed_neighbor_concierge("replacement", &"a".repeat(64), allowed())
        .await;
    assert!(fixture
        .service
        .authorize_neighbor_rpc_credential(&token, "mem.search", None)
        .await
        .unwrap_err()
        .starts_with("neighbor_forbidden:"));
    fixture
        .seed_neighbor_concierge(&caller.tab_id, &"a".repeat(64), allowed())
        .await;
    assert!(fixture
        .service
        .authorize_neighbor_rpc_credential(&token, "mem.search", None)
        .await
        .is_ok());
    fixture
        .service
        .ext::<crate::service::account::AccountCell>()
        .0
        .get()
        .unwrap()
        .logout()
        .await
        .unwrap();
    assert!(fixture
        .service
        .authorize_neighbor_rpc_credential(&token, "mem.search", None)
        .await
        .unwrap_err()
        .starts_with("neighbor_forbidden:"));
}

#[tokio::test]
async fn authenticated_api_request_can_report_own_launch_hooks_before_attestation_only() {
    let (fixture, caller, token) = fixture(allowed()).await;
    {
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(&caller.tab_id).unwrap();
        tab.reported_session_binding_nonce = None;
    }
    for event in ["session_start", "stop", "context", "permission"] {
        for (target, allowed) in [(caller.tab_id.as_str(), true), ("foreign", false)] {
            let result = fixture
                .service
                .authorize_neighbor_rpc_credential(
                    &token,
                    "api.request",
                    Some(
                        &json!({"method":"POST","path":format!("/api/tabs/{target}/hook/{event}")}),
                    ),
                )
                .await;
            assert_eq!(result.is_ok(), allowed);
        }
    }
    assert!(fixture
        .service
        .authorize_neighbor_rpc_credential(&token, "mem.search", Some(&json!({"q":"opaque"})))
        .await
        .is_err());
}
