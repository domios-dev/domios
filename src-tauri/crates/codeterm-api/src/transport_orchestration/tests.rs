use super::*;
use codeterm_bridge::bridge::threads::{OpenedThread, ThreadMenuState, TopicsState};
use codeterm_service_core::sentinel::graph::FileReference;
use codeterm_service_core::sentinel::graph::Identity;

#[test]
fn transport_orchestration_contract_caller_identity_denial() {
    assert!(authority::require_owner(true).is_ok());
    assert_eq!(
        authority::require_owner(false).unwrap_err().0,
        StatusCode::FORBIDDEN
    );
    for (tab, supplied, current, exists) in [
        (Some("caller"), None, Some("nonce"), true),
        (Some("caller"), Some("stale"), Some("nonce"), true),
        (Some("missing"), Some("nonce"), Some("nonce"), false),
        (None, Some("nonce"), None, false),
    ] {
        assert_eq!(
            authority::validate_nonce(tab, supplied, current, exists)
                .unwrap_err()
                .0,
            StatusCode::FORBIDDEN
        );
    }
    assert!(authority::validate_nonce(Some("caller"), Some("nonce"), Some("nonce"), true).is_ok());
    assert!(authority::validate_nonce(None, None, None, false).is_ok());
}

#[test]
fn transport_orchestration_contract_cross_tab_claims_and_missing_target() {
    assert!(authorize_open(
        ThreadCaller::Tab {
            tab_id: "caller",
            is_general: false
        },
        "other"
    )
    .is_err());
    assert!(authorize_open(
        ThreadCaller::Tab {
            tab_id: "caller",
            is_general: true
        },
        "other"
    )
    .is_ok());
    assert_eq!(
        thread_error(ThreadError::UnknownTab("missing".into())).0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        thread_error(ThreadError::BridgeInactive).0,
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert_eq!(
        thread_error(ThreadError::CreationFailed("provider refusal".into())).0,
        StatusCode::BAD_GATEWAY
    );
}

#[test]
fn transport_orchestration_contract_bridge_open_status_close_result() {
    let opened = OpenedThread {
        tab_id: "target".into(),
        chat_id: 7,
        thread_id: 9,
        topic_name: "Target".into(),
        link: Some("fixture-link".into()),
        created: true,
    };
    let result = serde_json::to_value(opened).unwrap();
    assert_eq!(result["tab_id"], "target");
    assert_eq!(result["thread_id"], 9);
    let state = ThreadMenuState {
        bridge_active: true,
        topics: TopicsState::Available,
        bound: true,
        link: Some("fixture-link".into()),
    };
    let status = serde_json::to_value(state).unwrap();
    assert_eq!(status["bound"], true);
    assert_eq!(status["link"], result["link"]);
    let closed = json!({"outcomes": [codeterm_bridge::bridge::threads::CloseOutcome::Deleted { thread_id: 9 }]});
    assert_eq!(closed["outcomes"][0]["outcome"], "deleted");
    assert!(serde_json::from_value::<ThreadAction>(json!({"tab_id":"target"})).is_err());
    assert!(serde_json::from_value::<ThreadAction>(json!({"selected_tab_id":"target"})).is_ok());
}

fn identity() -> Identity {
    Identity {
        kind: "blueprint".into(),
        id: "node".into(),
        workspace_id: "workspace".into(),
        team_id: None,
        owner_tab_id: Some("creator".into()),
    }
}

#[test]
fn transport_orchestration_contract_blueprint_scope_and_node_file_bounds() {
    let node = identity();
    assert!(node_files::validate_request(&node, None, None).is_ok());
    assert!(node_files::validate_request(&node, Some("ready"), Some("handler")).is_ok());
    assert!(node_files::validate_request(&node, Some("ready"), None).is_err());
    assert!(node_files::validate_request(
        &Identity {
            workspace_id: "".into(),
            ..node.clone()
        },
        None,
        None
    )
    .is_err());
    assert!(node_files::validate_request(
        &Identity {
            kind: "worker".into(),
            ..node
        },
        None,
        None
    )
    .is_err());
    let file = || FileReference {
        name: "file".into(),
        path: "/fixture/file".into(),
        short_path: "file".into(),
        exists: true,
    };
    assert_eq!(
        node_files::validate_paths((0..64).map(|_| file()).collect())
            .unwrap()
            .len(),
        64
    );
    assert!(node_files::validate_paths((0..65).map(|_| file()).collect()).is_err());
    assert!(node_files::validate_paths(vec![FileReference {
        path: "x".repeat(4097),
        ..file()
    }])
    .is_err());
}

struct FakeTopics(std::sync::atomic::AtomicUsize);
#[async_trait::async_trait]
impl codeterm_bridge::bridge::threads::TopicTransport for FakeTopics {
    async fn create_topic(
        &self,
        _: i64,
        _: &str,
    ) -> Result<i64, codeterm_bridge::bridge::threads::TopicFailure> {
        self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(9)
    }
    async fn probe_topic(
        &self,
        _: i64,
        _: i64,
        _: &str,
    ) -> codeterm_bridge::bridge::threads::TopicProbe {
        codeterm_bridge::bridge::threads::TopicProbe::Open
    }
    async fn delete_topic(
        &self,
        _: i64,
        _: i64,
    ) -> Result<(), codeterm_bridge::bridge::threads::TopicFailure> {
        Ok(())
    }
    async fn rename_topic(
        &self,
        _: i64,
        _: i64,
        _: &str,
    ) -> Result<(), codeterm_bridge::bridge::threads::TopicFailure> {
        Ok(())
    }
    async fn send_html(
        &self,
        _: i64,
        _: Option<i64>,
        _: &str,
        _: Option<&codeterm_bridge::bridge::traits::InlineKeyboard>,
    ) -> Result<i64, codeterm_bridge::bridge::threads::TopicFailure> {
        Ok(1)
    }
    async fn edit_html(&self, _: i64, _: i64, _: &str) -> bool {
        true
    }
}
struct FakeTabs;
impl codeterm_bridge::bridge::threads::ThreadTabs for FakeTabs {
    fn sender_identity(
        &self,
        tab_id: &str,
    ) -> Option<codeterm_bridge::bridge::threads::SenderIdentity> {
        self.is_live_tab(tab_id)
            .then(|| codeterm_bridge::bridge::threads::SenderIdentity {
                tab_id: tab_id.into(),
                title: "Target".into(),
                role: codeterm_bridge::bridge::threads::SenderRole::Worker,
                workspace: "Fixture".into(),
                machine: "Fixture".into(),
            })
    }
    fn live_tab_titles(&self) -> Vec<(String, String)> {
        vec![("target".into(), "Target".into())]
    }
    fn is_live_tab(&self, id: &str) -> bool {
        id == "target"
    }
    fn is_visible_tab(&self, id: &str) -> bool {
        self.is_live_tab(id)
    }
    fn is_general_tab(&self, _: &str) -> bool {
        false
    }
}

#[tokio::test]
async fn transport_orchestration_contract_delivered_bridge_binding_and_denial_without_mutation() {
    use codeterm_bridge::bridge::threads::{ThreadRegistry, ThreadService};
    let service = ThreadService::new(ThreadRegistry::in_memory());
    let topics = std::sync::Arc::new(FakeTopics(std::sync::atomic::AtomicUsize::new(0)));
    service.attach(topics.clone(), 4);
    let denied = service
        .open(
            &FakeTabs,
            ThreadCaller::Tab {
                tab_id: "caller",
                is_general: false,
            },
            "target",
            7,
        )
        .await;
    assert!(denied.is_err());
    assert!(!service.has_open_binding("target"));
    assert_eq!(topics.0.load(std::sync::atomic::Ordering::SeqCst), 0);
    let opened = service
        .open(&FakeTabs, ThreadCaller::Owner, "target", 7)
        .await
        .unwrap();
    assert_eq!(opened.thread_id, 9);
    assert!(service.has_open_binding("target"));
    let state = service.menu_state("target", Some(7)).await;
    assert!(state.bound);
    assert_eq!(state.link, opened.link);
    assert_eq!(topics.0.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert_eq!(service.close_tab("target").await.len(), 1);
    assert!(!service.has_open_binding("target"));
    assert!(!service.menu_state("target", Some(7)).await.bound);
}

#[test]
fn transport_orchestration_contract_plan_envelopes_and_project_scope() {
    use crate::plan_routes::plans::{filter_plan_project, plan_list_response, ListPlansQuery};
    use codeterm_service_core::plan::{OrchestratorRef, Plan, PlanStatus};
    use codeterm_service_core::service::plan::IndexEntry;
    let query: ListPlansQuery = axum::extract::Query::try_from_uri(
        &"/api/plans?full=1&project=%2Frepo%2Fa".parse().unwrap(),
    )
    .unwrap()
    .0;
    assert_eq!(query.full, Some(true));
    assert_eq!(query.project.as_deref(), Some("/repo/a"));
    let mut first = Plan::new("First".into(), "/repo/a".into(), OrchestratorRef::default());
    first.id = "first".into();
    let mut second = first.clone();
    second.id = "second".into();
    second.project_path = "/repo/b".into();
    let entry = |id: &str| IndexEntry {
        plan_id: id.into(),
        file_path: std::path::PathBuf::from("fixture.json"),
        canonical_project_path: "/repo".into(),
        owner: OrchestratorRef::default(),
        team_id: None,
        title: id.into(),
        status: PlanStatus::Active,
        updated_at: 1,
        assigned_tab_ids: vec![],
        task_count: 0,
        open_task_count: 0,
        section_count: 0,
        element_count: 0,
        loop_count: 0,
        has_goal: false,
        orphaned: None,
    };
    let mut entries = vec![entry("first"), entry("second"), entry("missing")];
    let get = |id: &str| match id {
        "first" => Some(first.clone()),
        "second" => Some(second.clone()),
        _ => None,
    };
    filter_plan_project(&mut entries, query.project.as_deref(), get);
    assert_eq!(entries.len(), 1);
    let index = plan_list_response(&entries, false, get);
    let full = plan_list_response(&entries, true, get);
    assert_eq!(index["plans"][0]["plan_id"], "first");
    assert_eq!(full["plans"][0]["id"], "first");
    assert_eq!(full["plans"][0]["project_path"], "/repo/a");
    assert_eq!(
        plan_list_response(&entries, true, |_| None)["plans"],
        json!([])
    );
}

#[test]
fn transport_orchestration_contract_node_identity_cannot_drop_scope_fields() {
    let full = json!({"kind":"blueprint", "id":"node", "workspace_id":"workspace", "team_id":null, "owner_tab_id":null});
    assert!(serde_json::from_value::<node_files::NodeFilesBody>(json!({"identity":full})).is_ok());
    for field in ["kind", "id", "workspace_id", "team_id", "owner_tab_id"] {
        let mut partial = full.clone();
        partial.as_object_mut().unwrap().remove(field);
        assert!(
            serde_json::from_value::<node_files::NodeFilesBody>(json!({"identity":partial}))
                .is_err()
        );
    }
}
