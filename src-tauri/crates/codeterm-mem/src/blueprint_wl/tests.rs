use super::*;
use codeterm_blueprint_types::blueprint_memory::parse_document;

#[test]
fn historical_v1_poc_corpus_is_refused_without_conversion() {
    let fixture: Value = serde_json::from_str(include_str!("parity.json")).unwrap();
    let docs = fixture["docs"].as_array().unwrap();
    assert_eq!(docs.len(), 70);
    for doc in docs {
        assert!(
            parse_document(doc["yaml"].as_str().unwrap()).is_err(),
            "{}",
            doc["id"]
        );
    }
}
#[test]
fn native_v2_wl_kernel_keeps_the_independent_cosine_oracle() {
    let a = BTreeMap::from([("x".into(), 3), ("y".into(), 4)]);
    let b = BTreeMap::from([("x".into(), 4)]);
    assert!((kernel(&a, &b) - 0.6).abs() < 1e-12);
    assert_eq!(kernel(&a, &a), 1.0);
    assert_eq!(kernel(&a, &BTreeMap::new()), 0.0);
}

#[test]
fn wl_hash_ignores_names_and_values_but_not_routes() {
    let base = parse_document(
        "schema_version: 2\nkind: pipeline\nname: alpha\ntrigger: {type: schedule, every: {lit: 1h}}\nsteps:\n- {id: one, run: {lit: echo a}, routes: {failed: two}}\n- {id: two, run: {lit: echo b}}\n",
    )
    .unwrap();
    let renamed = parse_document(
        "schema_version: 2\nkind: pipeline\nname: beta\ntrigger: {type: schedule, every: {lit: 2h}}\nsteps:\n- {id: first, run: {lit: ls}, routes: {failed: second}}\n- {id: second, run: {lit: pwd}}\n",
    )
    .unwrap();
    let rerouted = parse_document(
        "schema_version: 2\nkind: pipeline\nname: alpha\ntrigger: {type: schedule, every: {lit: 1h}}\nsteps:\n- {id: one, run: {lit: echo a}, routes: {failed: '@stop'}}\n- {id: two, run: {lit: echo b}}\n",
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
