use super::*;
#[test]
fn browser_snapshot_ref_depth_validation_and_pruning() {
    let args = |words: &[&str]| words.iter().map(|s| (*s).into()).collect::<Vec<String>>();
    assert_eq!(
        parse(&args(&["--ref", "@a", "--depth", "2"]))
            .unwrap()
            .depth,
        Some(2)
    );
    for words in [
        vec!["--ref"],
        vec!["--ref", "button"],
        vec!["--depth", "65"],
        vec!["--depth", "-1"],
        vec!["--depth"],
        vec!["--ref", "@a", "--scope", "dialog"],
    ] {
        assert!(parse(&args(&words)).is_err());
    }
    let nodes = parse_full_ax_tree(&serde_json::json!({"nodes":[
        {"nodeId":"root","childIds":["child"],"backendDOMNodeId":1},
        {"nodeId":"child","parentId":"root","childIds":["leaf"],"backendDOMNodeId":2},
        {"nodeId":"leaf","parentId":"child","backendDOMNodeId":3}
    ]}))
    .unwrap();
    let (subtree, truncated) = restrict(nodes.clone(), Some(2), Some(0)).unwrap();
    assert!(truncated);
    assert_eq!(subtree.len(), 1);
    assert!(subtree[0].parent_id.is_none() && subtree[0].child_ids.is_empty());
    let (subtree, truncated) = restrict(nodes.clone(), Some(2), None).unwrap();
    assert!(!truncated);
    assert_eq!(subtree.len(), 2);
    assert!(restrict(nodes, Some(99), None).is_err());
}
#[test]
fn bounded_snapshot_options_clamp_items_and_preserve_scope() {
    let args = [
        "--json",
        "--scope",
        "dialog:Settings",
        "--actions-only",
        "--max-items",
        "5",
    ]
    .map(String::from);
    assert_eq!(
        parse(&args).unwrap(),
        Options {
            json: true,
            scope: Some("dialog:Settings".into()),
            actions: true,
            limit: 5,
            with_text: false,
            reference: None,
            depth: None,
        }
    );
    assert_eq!(
        parse(&["--max-items".into(), "9000".into()]).unwrap().limit,
        MAX_LINES
    );
    assert!(parse(&["--max-items".into(), "0".into()]).is_err());
    assert!(parse(&["--scope".into()]).is_err());
}

#[test]
fn with_text_snapshot_options_can_be_combined_with_controls_only_and_bounds() {
    let options =
        parse(&["--with-text", "--actions-only", "--max-items", "7"].map(String::from)).unwrap();
    assert!(options.with_text && options.actions);
    assert_eq!(options.limit, 7);
}
#[test]
fn snapshot_unknown_flags_name_the_positional_limit_syntax() {
    let error = super::parse(&["--limit".into(), "20".into()]).unwrap_err();
    assert!(error.contains("snapshot [limit]"));
    assert!(error.contains("--limit is not supported"));
}

use codeterm_browser::cdp::{CdpClient, CdpConnection, CdpError, EventStream};
use codeterm_browser::cdp_wire::EventFilter;
use parking_lot::Mutex;
use serde_json::Value;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, watch};

struct DeepDom {
    calls: Mutex<Vec<(String, Value)>>,
    closed: watch::Sender<Option<String>>,
}

#[async_trait::async_trait]
impl CdpConnection for DeepDom {
    fn endpoint(&self) -> &str {
        "ws://127.0.0.1:1/deep"
    }
    fn closed(&self) -> watch::Receiver<Option<String>> {
        self.closed.subscribe()
    }
    fn close(&self, reason: &str) {
        self.closed.send_replace(Some(reason.into()));
    }
    fn subscribe(&self, _: EventFilter) -> EventStream {
        EventStream::from_receiver(mpsc::channel(1).1)
    }
    async fn send_to(
        &self,
        _: Option<&str>,
        method: &str,
        params: Value,
        _: Duration,
    ) -> Result<Value, CdpError> {
        self.calls.lock().push((method.into(), params.clone()));
        Ok(match method {
            "Target.attachToTarget" => json!({"sessionId":"session"}),
            "Accessibility.enable" => json!({}),
            "Accessibility.getFullAXTree" => json!({"nodes":[
                {"nodeId":"root","role":{"value":"RootWebArea"},"backendDOMNodeId":1,"childIds":["secret","public"]},
                {"nodeId":"secret","parentId":"root","backendDOMNodeId":200,"role":{"value":"textbox"},"name":{"value":"opaque-secret"},"value":{"value":"opaque-secret"}},
                {"nodeId":"public","parentId":"root","backendDOMNodeId":201,"role":{"value":"combobox"},"name":{"value":"Search"}}
            ]}),
            "DOM.getDocument" => {
                assert_eq!(params, json!({"depth":-1,"pierce":true}));
                let mut node = json!({"backendNodeId":200});
                for id in (2..200).rev() {
                    let mut parent = json!({"backendNodeId":id,"attributes":if id == 80 {json!(["data-sensitive",""])} else {json!([])}});
                    parent["children"] = Value::Array(vec![node]);
                    node = parent;
                }
                let mut root = json!({"backendNodeId":1});
                root["children"] = Value::Array(vec![node, json!({"backendNodeId":201})]);
                let mut document = json!({});
                document["root"] = root;
                document
            }
            _ => panic!("unexpected snapshot command: {method}"),
        })
    }
}

#[tokio::test]
async fn deep_dom_and_consecutive_snapshots_keep_public_names_and_redact_sensitive_fields() {
    let connection = Arc::new(DeepDom {
        calls: Mutex::new(Vec::new()),
        closed: watch::channel(None).0,
    });
    let page = CdpClient::from_connection(connection.clone())
        .attach_page("target")
        .await
        .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let runtime = BrowserRuntime::new(
        codeterm_browser::BrowserHostConfig::under_data_dir(dir.path()),
        "Agent",
        "tab",
        Arc::new(tokio::sync::OnceCell::new()),
        Default::default(),
    );
    runtime.shared.bind(&page);
    for capture in 1..=2 {
        let output = runtime
            .snapshot_verb(&page, &["--json".into()])
            .await
            .unwrap();
        let snapshot: Value = serde_json::from_str(&output.text).unwrap();
        let items = snapshot["items"].as_array().unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(
            items
                .iter()
                .filter(|item| item["name"] == "REDACTED")
                .count(),
            1
        );
        assert_eq!(
            items
                .iter()
                .filter(|item| item["role"] == "combobox" && item["name"] == "Search")
                .count(),
            1
        );
        assert!(!output.text.contains("opaque-secret"));
        let calls = connection.calls.lock();
        assert_eq!(
            calls
                .iter()
                .filter(|(method, _)| method == "DOM.getDocument")
                .count(),
            capture
        );
        assert!(!calls.iter().any(|(method, _)| method == "DOM.disable"
            || method == "DOM.pushNodesByBackendIdsToFrontend"));
    }
}
