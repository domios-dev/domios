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
