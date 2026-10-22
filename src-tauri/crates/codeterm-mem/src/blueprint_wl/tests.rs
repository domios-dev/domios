use super::*;
use codeterm_blueprint_types::blueprint_memory::parse_document;
use std::collections::HashMap;

fn fixture() -> (HashMap<String, Signature>, Vec<(String, String, f64)>) {
    let fixture: Value = serde_json::from_str(include_str!("parity.json")).unwrap();
    let signatures = fixture["docs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|doc| {
            let id = doc["id"].as_str().unwrap().to_owned();
            let document = parse_document(doc["yaml"].as_str().unwrap()).unwrap();
            let signature = signature(&document);
            assert_eq!(
                signature.hash,
                doc["hash"].as_str().unwrap(),
                "graph hash of {id}"
            );
            (id, signature)
        })
        .collect();
    let pairs = fixture["pairs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|pair| {
            (
                pair["a"].as_str().unwrap().to_owned(),
                pair["b"].as_str().unwrap().to_owned(),
                pair["s_struct"].as_f64().unwrap(),
            )
        })
        .collect();
    (signatures, pairs)
}

#[test]
fn wl_parity_with_poc_oracle_on_corpus_pairs() {
    let (signatures, pairs) = fixture();
    assert_eq!(signatures.len(), 70);
    assert!(pairs.len() >= 10);
    for (a, b, expected) in pairs {
        let actual = kernel(&signatures[&a].features, &signatures[&b].features);
        assert!(
            (actual - expected).abs() < 1e-6,
            "S_struct({a}, {b}) = {actual}, oracle {expected}"
        );
    }
}

#[test]
fn wl_hash_ignores_names_and_values_but_not_routes() {
    let base = parse_document(
        "schema_version: 1\nname: a\npipeline:\n  trigger: {type: schedule, every: 1h}\n  steps:\n    - {name: one, input: text, run: echo a, on: {failed: two}}\n    - {name: two, input: text, run: echo b}\n",
    )
    .unwrap();
    let renamed = parse_document(
        "schema_version: 1\nname: b\npipeline:\n  trigger: {type: schedule, every: 2h}\n  steps:\n    - {name: first, input: text, run: ls, on: {failed: second}}\n    - {name: second, input: text, run: pwd}\n",
    )
    .unwrap();
    let rerouted = parse_document(
        "schema_version: 1\nname: a\npipeline:\n  trigger: {type: schedule, every: 1h}\n  steps:\n    - {name: one, input: text, run: echo a, on: {failed: stop}}\n    - {name: two, input: text, run: echo b}\n",
    )
    .unwrap();
    assert_eq!(signature(&base).hash, signature(&renamed).hash);
    assert_ne!(signature(&base).hash, signature(&rerouted).hash);
    assert!(
        (kernel(&signature(&base).features, &signature(&renamed).features) - 1.0).abs() < 1e-12
    );
    assert!(kernel(&signature(&base).features, &signature(&rerouted).features) < 1.0);
    assert!(shared_edges(&signature(&base), &signature(&renamed))
        .contains(&"command -failure-> command".to_owned()));
}
