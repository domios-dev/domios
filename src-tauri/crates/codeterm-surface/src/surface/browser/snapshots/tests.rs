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
use codeterm_browser::cdp_wire::{CdpEvent, EventFilter};
use parking_lot::Mutex;
use serde_json::Value;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, watch};

struct DeepDom {
    calls: Mutex<Vec<(String, Value)>>,
    subscribers: Mutex<Vec<mpsc::Sender<Arc<CdpEvent>>>>,
    emitted: AtomicBool,
    closed: watch::Sender<Option<String>>,
    mutation: u8,
}

impl DeepDom {
    fn emit(&self, method: &str, params: Value) {
        for subscriber in self.subscribers.lock().iter() {
            subscriber
                .try_send(Arc::new(CdpEvent {
                    method: method.into(),
                    params: params.clone(),
                    session_id: Some("session".into()),
                }))
                .unwrap();
        }
    }

    fn dom_node(id: i64) -> Value {
        let mut node = json!({"nodeId":id,"backendNodeId":id});
        if id == 80 {
            node["attributes"] = json!(["data-sensitive", ""]);
        }
        node
    }
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
        let (sender, receiver) = mpsc::channel(256);
        self.subscribers.lock().push(sender);
        EventStream::from_receiver(receiver)
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
            "Accessibility.getFullAXTree" => {
                let mut nodes = vec![
                    json!({"nodeId":"root","role":{"value":"RootWebArea"},"backendDOMNodeId":1,"childIds":
                    (2..=1800).map(|id| id.to_string()).collect::<Vec<_>>()}),
                ];
                for id in 2..=1800 {
                    nodes.push(json!({"nodeId":id.to_string(),"parentId":"root","backendDOMNodeId":id,
                        "role":{"value":"textbox"},"name":{"value":if id <= 162 {"opaque-secret"} else {"Public"}},
                        "value":{"value":if id <= 162 {"opaque-secret"} else {"visible"}}}));
                }
                json!({"nodes":nodes})
            }
            "DOM.getDocument" => {
                assert_eq!(params, json!({"depth":0,"pierce":true}));
                json!({"root":Self::dom_node(1)})
            }
            "DOM.pushNodesByBackendIdsToFrontend" => {
                let ids = params["backendNodeIds"].as_array().unwrap();
                assert!(ids.len() <= 64);
                if !self.emitted.swap(true, Ordering::SeqCst) {
                    let mut children = vec![Self::dom_node(2)];
                    children.extend((163..=1800).map(Self::dom_node));
                    self.emit("DOM.setChildNodes", json!({"parentId":1,"nodes":children}));
                    for parent in 2..162 {
                        let mut node = Self::dom_node(parent + 1);
                        if parent == 90 {
                            node["shadowRoots"] = json!([Self::dom_node(92)]);
                        } else if parent == 120 {
                            node["contentDocument"] = Self::dom_node(122);
                        }
                        if parent != 91 && parent != 121 {
                            self.emit(
                                "DOM.setChildNodes",
                                json!({"parentId":parent,"nodes":[node]}),
                            );
                        }
                    }
                    if self.mutation == 2 {
                        self.emit("DOM.documentUpdated", json!({}));
                    }
                    if self.mutation == 1 {
                        self.emit(
                            "DOM.attributeModified",
                            json!({"nodeId":1700,"name":"data-sensitive","value":""}),
                        );
                    }
                }
                json!({"nodeIds":ids.iter().map(|id| if id == 79 {json!(0)} else {id.clone()}).collect::<Vec<_>>()})
            }
            _ => panic!("unexpected snapshot command: {method}"),
        })
    }
}

#[tokio::test]
async fn deep_dom_snapshot_uses_bounded_ancestry_and_redacts_flattened_ax_descendants() {
    for mutation in 0..=2 {
        let connection = Arc::new(DeepDom {
            calls: Mutex::new(Vec::new()),
            subscribers: Mutex::new(Vec::new()),
            emitted: AtomicBool::new(false),
            closed: watch::channel(None).0,
            mutation,
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
        let started = Instant::now();
        let output = runtime
            .snapshot_verb(
                &page,
                &["--json".into(), "--max-items".into(), "2000".into()],
            )
            .await
            .unwrap();
        eprintln!(
            "1800-node AX / 162-level DOM snapshot: {:?}; mutation={mutation}",
            started.elapsed()
        );
        let snapshot: Value = serde_json::from_str(&output.text).unwrap();
        let items = snapshot["items"].as_array().unwrap();
        assert_eq!(items.len(), 1799);
        let redacted = items
            .iter()
            .filter(|item| item["name"] == "REDACTED")
            .count();
        assert_eq!(
            redacted,
            match mutation {
                0 => 84,
                1 => 85,
                _ => 1799,
            }
        );
        assert!(!items.iter().any(|item| item["name"] == "REDACTED"
            && item["text"].as_str().unwrap().contains("opaque-secret")));
        assert_eq!(
            items.iter().filter(|item| item["name"] == "Public").count(),
            match mutation {
                0 => 1638,
                1 => 1637,
                _ => 0,
            }
        );
        let calls = connection.calls.lock();
        assert_eq!(
            calls
                .iter()
                .filter(|(method, _)| method == "DOM.pushNodesByBackendIdsToFrontend")
                .count(),
            29
        );
        assert_eq!(
            calls
                .iter()
                .filter(|(method, _)| method == "DOM.getDocument")
                .count(),
            1
        );
    }
}
