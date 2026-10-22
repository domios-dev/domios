use super::*;

#[test]
fn blueprint_v2_intent_request_is_one_scoped_recall_without_a_second_ranker() {
    let (method, params) = request(
        BlueprintAction::New {
            kind: "watch".into(),
            like: None,
            intent: Some("exact intent".into()),
            limit: 3,
        },
        Some("caller".into()),
        Some("nonce".into()),
    )
    .unwrap();
    assert_eq!(method, "blueprint.recall");
    assert_eq!(
        params,
        json!({"intent":"exact intent","cards":true,"limit":3,"caller_tab_id":"caller","session_binding_nonce":"nonce"})
    );
}

#[test]
fn blueprint_v2_intent_scaffold_survives_no_fit_and_degradation_and_roundtrips() {
    let intent = "  Exact \"quoted\" intent\nλ and $() `text`  ";
    for recalled in [
        Ok(json!({"cards":[],"recall":{"status":"ready"},"unavailable":[]})),
        Err("embeddings unavailable".into()),
        Ok(json!([])),
    ] {
        let response = memory::intent_response(intent, recalled).unwrap();
        assert_eq!(response["scaffold"]["schema_version"], 2);
        assert_eq!(response["scaffold"]["intent"], intent);
        let yaml = response["yaml"].as_str().unwrap();
        let document: codeterm_blueprint_types::blueprint_document::Document =
            serde_yaml::from_str(yaml).unwrap();
        assert_eq!(document.intent.as_deref(), Some(intent));
        assert!(document.template_errors().is_empty());
        let checked =
            codeterm_blueprint_types::blueprint_document::check("scaffold.yaml", yaml, &[]);
        assert!(checked.ok, "{:?}", checked.diagnostics);
        assert!(response["cards"].as_array().unwrap().is_empty());
        let text = memory::render_intent(&response);
        assert!(text.contains(yaml));
    }
}

#[test]
fn blueprint_v2_intent_renderer_preserves_host_order_scores_and_typed_metadata() {
    let cards = json!([
        {"id":"mem:high","provenance":"mem:","hash":"a","score":1.3,"signature":{"inputs":{"x":{"type":"integer"}},"result":{"type":"integer"}},"node_kinds":{"code":1},"why":"intent vector","when_to_use":"purpose","enrichment_pending":true},
        {"id":"catalog:middle","provenance":"catalog:","hash":"b","score":1.04,"signature":{"inputs":{},"result":{"type":"string"}},"node_kinds":{"command":1},"why":"canon vector","when_to_use":"another purpose","enrichment_pending":false}
    ]);
    let response = memory::intent_response(
        "author a graph",
        Ok(json!({"cards":cards,"recall":{"status":"ready"},"unavailable":[]})),
    )
    .unwrap();
    assert_eq!(response["cards"], cards);
    let rendered = memory::render_intent(&response);
    assert!(rendered.find("mem:high").unwrap() < rendered.find("catalog:middle").unwrap());
    for value in [
        "score=1.3",
        "score=1.04",
        "integer",
        "node kinds:",
        "hash=a",
        "enrichment_pending=true",
    ] {
        assert!(rendered.contains(value), "{rendered}");
    }
    let (mem_card, catalog_card) = rendered.split_once("\ncatalog:middle ").unwrap();
    for (card, purpose, why) in [
        (mem_card, "purpose", "intent vector"),
        (catalog_card, "another purpose", "canon vector"),
    ] {
        assert!(card.lines().any(|line| line.trim() == purpose), "{card}");
        assert!(
            card.lines()
                .any(|line| line.trim() == format!("WHY: {why}")),
            "{card}"
        );
    }
    assert!(!rendered.contains("def main"));
}

#[test]
fn blueprint_v2_intent_ships_only_the_accepted_default_prompt_bytes() {
    use sha2::{Digest, Sha256};
    let decision: Value = serde_json::from_str(include_str!(
        "../../../../../scripts/hand-tests/blueprint-v2-intent/prompt-decision.json"
    ))
    .unwrap();
    let prompt = include_str!("../../../../../prod-docs/BLUEPRINT-AUTHORING.md");
    assert_eq!(decision["task"], "task_qufb8j");
    assert_eq!(decision["decision"], "PASS");
    assert_eq!(decision["audit_pass"], true);
    assert_eq!(decision["default"], "short+bind");
    assert_eq!(
        decision["source_sha256"],
        "bf77023a394f71dfce8bc1bbc0ec5b47c5b8f04002e7714905a71c3fa6d708a9"
    );
    let hash = Sha256::digest(prompt.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    assert_eq!(decision["product_sha256"], hash);
    assert_eq!(
        crate::cmd::docs::render_doc("BLUEPRINT-AUTHORING").unwrap(),
        prompt
    );
}
