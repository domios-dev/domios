use super::{render_radar_result, run_radar};
use crate::args::radar::RadarAction;
use crate::client::{RpcClient, RpcError, SocketCallError};
use serde_json::{json, Value};

#[derive(Default)]
struct RecordingRpc {
    calls: Vec<(String, Value)>,
    remote: bool,
    reject: bool,
}

impl RpcClient for RecordingRpc {
    fn call_detailed(
        &mut self,
        method: &str,
        params: Option<Value>,
    ) -> Result<Value, SocketCallError> {
        self.calls
            .push((method.to_string(), params.unwrap_or(Value::Null)));
        if self.reject {
            return Err(SocketCallError::Rpc(RpcError {
                code: -32001,
                message: "radar item 'rd_zzzzzz' not found".into(),
                data: None,
            }));
        }
        Ok(json!({}))
    }

    fn is_remote(&self) -> bool {
        self.remote
    }
}

fn add(body_file: Option<String>, project: Option<String>) -> RadarAction {
    RadarAction::Add {
        title: "Noticed".into(),
        body: None,
        body_file,
        tag: vec!["ux".into()],
        source_provider: Some("github".into()),
        source_id: Some("acme/widgets#4".into()),
        source_url: None,
        project,
    }
}

#[test]
fn radar_add_sends_workspace_title_tags_and_source_identity() {
    let mut client = RecordingRpc::default();

    run_radar(&mut client, add(None, Some("/work/app".into()))).unwrap();

    let (method, params) = &client.calls[0];
    assert_eq!(method, "radar.add");
    assert_eq!(params["project"], json!("/work/app"));
    assert_eq!(params["title"], json!("Noticed"));
    assert_eq!(params["tags"], json!(["ux"]));
    assert_eq!(params["source_provider"], json!("github"));
    assert_eq!(params["source_id"], json!("acme/widgets#4"));
    assert!(params.get("source_url").is_none());
}

#[test]
fn radar_add_unreadable_body_file_is_rejected_before_any_rpc() {
    let mut client = RecordingRpc::default();
    let missing = std::env::temp_dir()
        .join("codeterm-absent-radar-dir")
        .join("body.md");
    assert!(!missing.exists(), "the fixture must name an absent path");

    let refused = run_radar(
        &mut client,
        add(
            Some(missing.display().to_string()),
            Some("/work/app".into()),
        ),
    );

    assert!(refused.is_err());
    assert!(client.calls.is_empty());
}

#[test]
fn radar_add_over_mesh_needs_an_explicit_workspace() {
    let mut client = RecordingRpc {
        remote: true,
        ..Default::default()
    };

    assert!(run_radar(&mut client, add(None, None)).is_err());
    assert!(client.calls.is_empty());
    run_radar(&mut client, add(None, Some("/peer/app".into()))).unwrap();
    assert_eq!(client.calls[0].1["project"], json!("/peer/app"));
}

#[test]
fn radar_list_across_workspaces_omits_the_project() {
    let mut client = RecordingRpc::default();

    run_radar(
        &mut client,
        RadarAction::List {
            status: Some("all".into()),
            tag: None,
            provider: Some("github".into()),
            query: None,
            project: None,
            all_workspaces: true,
        },
    )
    .unwrap();

    let (method, params) = &client.calls[0];
    assert_eq!(method, "radar.list");
    assert!(params.get("project").is_none());
    assert_eq!(params["status"], json!("all"));
    assert_eq!(params["provider"], json!("github"));
}

#[test]
fn radar_edit_clear_tags_sends_an_empty_tag_list() {
    let mut client = RecordingRpc::default();

    run_radar(
        &mut client,
        RadarAction::Edit {
            item_id: "rd_abc123".into(),
            title: None,
            body: None,
            body_file: None,
            tag: Vec::new(),
            clear_tags: true,
        },
    )
    .unwrap();

    let (method, params) = &client.calls[0];
    assert_eq!(method, "radar.update");
    assert_eq!(params["tags"], json!([]));
    assert!(params.get("title").is_none());
}

#[test]
fn radar_pull_names_the_plan_and_kind() {
    let mut client = RecordingRpc::default();

    run_radar(
        &mut client,
        RadarAction::Pull {
            item_id: "rd_abc123".into(),
            plan: "pl_target".into(),
            kind: Some("fix".into()),
            sections: Some("sec_a,sec_b".into()),
        },
    )
    .unwrap();

    let (method, params) = &client.calls[0];
    assert_eq!(method, "radar.pull");
    assert_eq!(params["item_id"], json!("rd_abc123"));
    assert_eq!(params["plan_id"], json!("pl_target"));
    assert_eq!(params["type"], json!("fix"));
    assert_eq!(params["sections"], json!("sec_a,sec_b"));
}

#[test]
fn radar_daemon_rejection_reaches_the_caller() {
    let mut client = RecordingRpc {
        reject: true,
        ..Default::default()
    };

    let refused = run_radar(
        &mut client,
        RadarAction::Show {
            item_id: "rd_zzzzzz".into(),
        },
    );

    assert!(refused.unwrap_err().contains("rd_zzzzzz"));
    assert_eq!(client.calls.len(), 1);
}

#[test]
fn radar_list_render_shows_status_source_and_pull_link() {
    let rendered = render_radar_result(&json!({ "items": [{
        "id": "rd_abc123",
        "status": "pulled",
        "title": "Noticed",
        "tags": ["ux"],
        "source": { "provider": "github", "external_id": "acme/widgets#4" },
        "pulled": { "plan_id": "pl_1", "task_id": "task_1", "pulled_at": 1 },
    }]}));

    let fields: Vec<&str> = rendered.trim_end().split('\t').collect();
    assert_eq!(fields[0], "rd_abc123");
    assert_eq!(fields[1], "[pulled]");
    assert_eq!(fields[2], "Noticed");
    assert!(fields.contains(&"github:acme/widgets#4"));
    assert!(fields.contains(&"-> pl_1/task_1"));
    assert_eq!(rendered.lines().count(), 1);
    assert_eq!(
        render_radar_result(&json!({ "items": [] })).lines().count(),
        1
    );
}
