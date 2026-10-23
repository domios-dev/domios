use super::*;
use codeterm_blueprint_types::blueprint_memory::Entry;

#[test]
fn blueprint_v2_intent_card_is_typed_compact_and_never_contains_bodies() {
    let mut document =
        codeterm_mem::blueprint_library::catalog::document("catalog:severity-grade").unwrap();
    document.steps[0].code.as_mut().unwrap().src = Some("SECRET_FULL_BODY".into());
    document.inputs.get_mut("args").unwrap().default = Some(json!({"signal":"SECRET_DEFAULT"}));
    let entry = Entry {
        id: "raw-id".into(),
        document: document.clone(),
        intents: vec!["one\nline purpose".into()],
        uses: 1,
        successes: 1,
        failures: 0,
        kept: true,
        last_used: 0,
    };
    let output = card(BlueprintHit {
        entry,
        score: 0.8125,
        why: "intent 0.81\nenrichment pending".into(),
        enrichment_pending: true,
    })
    .unwrap();
    assert_eq!(output["id"], "mem:raw-id");
    assert_eq!(output["provenance"], "mem:");
    assert_eq!(output["score"], 0.8125);
    assert_eq!(output["hash"], projection::identity(&document).unwrap());
    assert_eq!(output["node_kinds"], json!({"code":1}));
    assert_eq!(
        output["signature"]["inputs"]["args"]["properties"]["signal"]["type"],
        "object"
    );
    assert_eq!(
        output["signature"]["result"]["properties"]["severity"]["enum"],
        json!(["ok", "low", "high", "critical"])
    );
    assert!(!output["when_to_use"].as_str().unwrap().contains('\n'));
    assert!(!output["why"].as_str().unwrap().contains('\n'));
    let text = output.to_string();
    for forbidden in ["SECRET_FULL_BODY", "SECRET_DEFAULT", "yaml", "src"] {
        assert!(!text.contains(forbidden), "{text}");
    }
    for key in ["yaml", "document", "steps", "code", "intents", "how"] {
        assert!(output.get(key).is_none());
    }
}

#[test]
fn blueprint_v2_intent_mem_and_catalog_show_use_the_card_identity_and_open_bodies_explicitly() {
    use codeterm_service_core::{
        multiplexer::state::MultiplexerState, service::test_fixture::ServiceFixture,
    };
    let (mux, tab) = MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&tab).unwrap();
    let fixture = ServiceFixture::new(mux);
    let caller = SentinelCaller {
        tab,
        team: Some("team".into()),
        workspace: workspace.clone(),
        cwd: String::new(),
        nonce: None,
    };
    let document =
        codeterm_mem::blueprint_library::catalog::document("catalog:severity-grade").unwrap();
    let saved = fixture
        .service
        .mem
        .save_blueprint_scoped(
            &area(&workspace),
            "team",
            document,
            &["Grade a signal".into()],
            codeterm_blueprint_types::blueprint_memory::Evidence {
                kept: true,
                ..Default::default()
            },
        )
        .unwrap();
    for id in [
        "catalog:severity-grade".to_owned(),
        format!("mem:{}", saved.id),
    ] {
        let document = fixture
            .service
            .mem
            .blueprint_callable(&area(&workspace), "team", &id)
            .unwrap();
        let metadata = card(BlueprintHit {
            entry: Entry {
                id: id.clone(),
                document,
                intents: vec!["Grade a signal".into()],
                uses: 0,
                successes: 0,
                failures: 0,
                kept: true,
                last_used: 0,
            },
            score: 1.0,
            why: "intent vector".into(),
            enrichment_pending: false,
        })
        .unwrap();
        let shown = crate::blueprint_calls::show_callable(&fixture.service, &caller, &id).unwrap();
        assert_eq!(shown["id"], metadata["id"]);
        assert_eq!(shown["hash"], metadata["hash"]);
        assert!(shown["call_graph"]["steps"][0]["code"]["src"].is_string());
        assert!(metadata.get("call_graph").is_none());
    }
    let foreign = SentinelCaller {
        team: Some("other-team".into()),
        ..caller
    };
    assert!(crate::blueprint_calls::show_callable(
        &fixture.service,
        &foreign,
        &format!("mem:{}", saved.id)
    )
    .is_err());
}

#[test]
fn blueprint_v2_intent_card_hash_clears_dependencies_but_keeps_nested_pins() {
    use codeterm_blueprint_types::blueprint_document::calls::FrozenCallee;
    let child =
        codeterm_mem::blueprint_library::catalog::document("catalog:severity-grade").unwrap();
    let child_hash = projection::identity(&child).unwrap();
    let mut document = child.clone();
    document.steps[0].code = None;
    document.steps[0].call = Some(
        serde_json::from_value(json!({
            "ref":"catalog:severity-grade", "pin":{"id":"catalog:severity-grade","hash":child_hash},
            "with":{"args":{"input":"args"}}
        }))
        .unwrap(),
    );
    let expected = projection::identity(&document).unwrap();
    document.dependencies.insert(
        format!("catalog:severity-grade@{child_hash}"),
        FrozenCallee {
            id: "catalog:severity-grade".into(),
            hash: child_hash,
            document: Box::new(child),
            locations: Default::default(),
            source_hash: None,
        },
    );
    assert_ne!(projection::identity(&document).unwrap(), expected);
    let value = card(BlueprintHit {
        entry: Entry {
            id: "nested".into(),
            document,
            intents: vec!["typed nested operation".into()],
            uses: 0,
            successes: 0,
            failures: 0,
            kept: true,
            last_used: 0,
        },
        score: 1.0,
        why: "intent vector".into(),
        enrichment_pending: false,
    })
    .unwrap();
    assert_eq!(value["hash"], expected);
    assert_eq!(value["node_kinds"], json!({"call":1}));
}

#[test]
fn blueprint_v2_intent_catalog_card_and_mem_prefix_are_stable() {
    for id in ["catalog:severity-grade", "mem:already-prefixed"] {
        let document =
            codeterm_mem::blueprint_library::catalog::document("catalog:severity-grade").unwrap();
        let entry = Entry {
            id: id.into(),
            document,
            intents: vec!["Grade a signal".into()],
            uses: 0,
            successes: 0,
            failures: 0,
            kept: true,
            last_used: 0,
        };
        let value = card(BlueprintHit {
            entry,
            score: 1.0,
            why: "vector".into(),
            enrichment_pending: false,
        })
        .unwrap();
        assert_eq!(value["id"], id);
        assert_eq!(
            value["provenance"],
            if id.starts_with("mem:") {
                "mem:"
            } else {
                "catalog:"
            }
        );
    }
}

mod native;
