use super::*;
use crate::blueprint_document::projection as ir_projection;
use crate::blueprint_document::{Port, ValueForm};
use serde_json::{json, Value};
use std::path::PathBuf;

struct Tree(PathBuf);
impl Tree {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("blueprint-v2-call-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn put(&self, name: &str, doc: &Value) -> String {
        self.raw(name, &serde_yaml::to_string(doc).unwrap())
    }
    fn raw(&self, name: &str, source: &str) -> String {
        let path = self.0.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, source).unwrap();
        path.to_str().unwrap().into()
    }
    fn resolve(&self, name: &str) -> Result<FrozenClosure, Vec<Diagnostic>> {
        let path = self.0.join(name);
        resolve_local(
            path.to_str().unwrap(),
            &std::fs::read_to_string(&path).unwrap(),
        )
    }
}
impl Drop for Tree {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}
fn object(kind: &str) -> Value {
    json!({"type":"object","properties":{"n":{"type":kind}},"required":["n"],"additionalProperties":false})
}
fn callee() -> Value {
    json!({"schema_version":2,"kind":"pipeline","name":"increment","inputs":{"p":object("integer")},"steps":[{"id":"increment","code":{"lang":"python","src":"def main(p): return {'n': p['n'] + 1}"},"in":object("integer"),"out":object("integer")}],"result":{"type":object("integer"),"from":"increment.out"}})
}
fn caller() -> Value {
    json!({"schema_version":2,"kind":"pipeline","name":"caller","trigger":{"type":"schedule","every":{"lit":"20s"}},"imports":{"increment":"callee.yaml"},"steps":[{"id":"call","call":{"ref":"increment","with":{"p":{"lit":{"n":7}}}},"in":object("integer"),"out":object("integer")}],"result":{"to":"notify"}})
}
fn good(tree: &Tree) -> FrozenClosure {
    tree.put("callee.yaml", &callee());
    tree.put("caller.yaml", &caller());
    tree.resolve("caller.yaml").unwrap()
}
fn has_rule(closure: &FrozenClosure, rule: &str) -> bool {
    check_facts(closure).1.iter().any(|f| f.rule == rule)
}
#[test]
fn blueprint_v2_call_import_missing_file_is_positioned() {
    let tree = Tree::new();
    tree.put("caller.yaml", &caller());
    let errors = tree.resolve("caller.yaml").unwrap_err();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].field, "imports.increment");
    assert!(errors[0].line > 1);
    assert!(errors[0].message.contains("Missing import"));
}
#[test]
fn blueprint_v2_call_import_malformed_yaml_preserves_callee_source() {
    let tree = Tree::new();
    tree.put("caller.yaml", &caller());
    let file = tree.raw("callee.yaml", "schema_version: [\n");
    let errors = tree.resolve("caller.yaml").unwrap_err();
    assert_eq!(errors.len(), 1);
    assert_eq!(
        errors[0].file,
        std::fs::canonicalize(file).unwrap().to_string_lossy()
    );
    assert!(errors[0].line > 0);
}
#[test]
fn blueprint_v2_call_import_duplicate_mapping_key_is_not_last_writer_wins() {
    let tree = Tree::new();
    let raw = serde_yaml::to_string(&caller())
        .unwrap()
        .replace("imports:", "imports:\n  increment: other.yaml");
    tree.raw("caller.yaml", &raw);
    let errors = tree.resolve("caller.yaml").unwrap_err();
    assert!(errors.iter().any(|e| e.message == "Duplicate mapping key"));
}
#[test]
fn blueprint_v2_call_import_alias_collision_with_inline_is_refused() {
    let tree = Tree::new();
    let mut doc = caller();
    doc["blueprints"] = json!({"increment":{"inputs":{},"steps":[],"result":{}}});
    tree.put("caller.yaml", &doc);
    assert!(tree.resolve("caller.yaml").unwrap_err()[0]
        .message
        .contains("Duplicate inline/import"));
}
#[test]
fn blueprint_v2_call_import_duplicate_identity_is_refused() {
    let tree = Tree::new();
    tree.put("callee.yaml", &callee());
    let mut doc = caller();
    doc["imports"]["duplicate"] = json!("./callee.yaml");
    tree.put("caller.yaml", &doc);
    assert!(tree.resolve("caller.yaml").unwrap_err()[0]
        .message
        .contains("Duplicate/diamond"));
}
#[test]
fn blueprint_v2_call_import_diamond_is_refused() {
    let tree = Tree::new();
    tree.put("leaf.yaml", &callee());
    let mut branch = callee();
    branch["imports"] = json!({"leaf":"leaf.yaml"});
    tree.put("left.yaml", &branch);
    branch["name"] = json!("right");
    tree.put("right.yaml", &branch);
    let mut doc = caller();
    doc["imports"] = json!({"left":"left.yaml","right":"right.yaml"});
    tree.put("caller.yaml", &doc);
    assert!(tree.resolve("caller.yaml").unwrap_err()[0]
        .message
        .contains("Duplicate/diamond"));
}
#[test]
fn blueprint_v2_call_import_relative_nested_is_based_on_exact_author_file() {
    let tree = Tree::new();
    tree.put("nested/leaf.yaml", &callee());
    let mut branch = callee();
    branch["imports"] = json!({"leaf":"leaf.yaml"});
    tree.put("nested/branch.yaml", &branch);
    let mut doc = caller();
    doc["imports"] = json!({"increment":"nested/branch.yaml"});
    tree.put("caller.yaml", &doc);
    let closure = tree.resolve("caller.yaml").unwrap();
    assert_eq!(closure.callees.len(), 2);
    assert!(
        closure
            .callees
            .values()
            .any(|c| c.id.ends_with("nested/leaf.yaml")),
        "callee identities: {:?}",
        closure.callees.values().map(|c| &c.id).collect::<Vec<_>>()
    );
}
#[test]
fn blueprint_v2_call_portable_file_pins_roundtrip_locally_and_offline() {
    let tree = Tree::new();
    let closure = good(&tree);
    let document = closure.document();
    let yaml = document.yaml().unwrap();
    tree.raw("caller.yaml", &yaml);
    let local = tree.resolve("caller.yaml").unwrap();
    let saved = resolve_saved(document, &ReviewMemory(closure.root.document.clone())).unwrap();
    assert_eq!(local.callees, closure.callees);
    assert_eq!(saved.callees, closure.callees);
    verify_local_sources(&local).unwrap();
    for callee in local.callees.values() {
        assert!(!callee.id.contains('\\'));
    }
}
#[test]
fn blueprint_v2_call_import_cycle_is_refused() {
    let tree = Tree::new();
    let mut branch = callee();
    branch["imports"] = json!({"cycle":"caller.yaml"});
    tree.put("callee.yaml", &branch);
    tree.put("caller.yaml", &caller());
    assert!(tree.resolve("caller.yaml").unwrap_err()[0]
        .message
        .contains("Cyclic import"));
}
#[test]
fn blueprint_v2_call_import_noncallable_and_network_and_repo_are_refused() {
    let tree = Tree::new();
    let mut noncallable = callee();
    noncallable["trigger"] = json!({"type":"schedule","every":{"lit":"20s"}});
    tree.put("callee.yaml", &noncallable);
    tree.put("caller.yaml", &caller());
    assert!(tree
        .resolve("caller.yaml")
        .unwrap_err()
        .iter()
        .any(|d| d.message.contains("not a typed callable")));
    for path in [
        "https://example.test/a.yaml",
        "repo:helper",
        "package:helper",
    ] {
        let mut doc = caller();
        doc["imports"]["increment"] = json!(path);
        tree.put("caller.yaml", &doc);
        assert!(tree.resolve("caller.yaml").unwrap_err()[0]
            .message
            .contains("repo trust is unavailable"));
    }
}
#[test]
fn blueprint_v2_call_import_change_after_validation_fails_identity_recheck() {
    let tree = Tree::new();
    let closure = good(&tree);
    verify_local_sources(&closure).unwrap();
    let mut doc = callee();
    doc["steps"][0]["code"]["src"] = json!("def main(p): return {'n': 99}");
    tree.put("callee.yaml", &doc);
    assert!(verify_local_sources(&closure)
        .unwrap_err()
        .contains("identity changed"));
    assert!(closure.callees.values().next().unwrap().document.steps[0]
        .code
        .as_ref()
        .unwrap()
        .src
        .as_ref()
        .unwrap()
        .contains("+ 1"));
}
#[test]
fn blueprint_v2_call_typed_positive_has_fixed_argument_and_exact_transported_closure() {
    let tree = Tree::new();
    let closure = good(&tree);
    let (verified, findings) = check_facts(&closure);
    assert!(findings.is_empty(), "{findings:?}");
    assert_eq!(
        verified["call"].fixed_arguments,
        BTreeMap::from([("p".into(), json!({"lit":{"n":7}}).to_string())])
    );
    assert_eq!(verified["call"].expanded_steps, 1);
    let restored = FrozenClosure::from_document(
        serde_json::from_slice(&serde_json::to_vec(&closure.document()).unwrap()).unwrap(),
    )
    .unwrap();
    assert_eq!(restored.callees, closure.callees);
    assert_eq!(
        restored.root.document.steps[0].call,
        closure.root.document.steps[0].call
    );
}
#[test]
fn blueprint_v2_call_arguments_missing_extra_and_mistyped_are_rejected() {
    let tree = Tree::new();
    for (args, rule) in [
        (json!({}), "C2"),
        (json!({"p":{"lit":{"n":7}},"extra":{"lit":1}}), "C3"),
        (json!({"p":{"lit":{"n":"7"}}}), "C4"),
    ] {
        tree.put("callee.yaml", &callee());
        let mut doc = caller();
        doc["steps"][0]["call"]["with"] = args;
        tree.put("caller.yaml", &doc);
        assert!(
            has_rule(&tree.resolve("caller.yaml").unwrap(), rule),
            "{rule}"
        );
    }
}
#[test]
fn blueprint_v2_call_callee_input_port_mismatch_is_rejected() {
    let tree = Tree::new();
    let mut doc = callee();
    doc["steps"][0]["in"] = object("string");
    tree.put("callee.yaml", &doc);
    tree.put("caller.yaml", &caller());
    assert!(has_rule(&tree.resolve("caller.yaml").unwrap(), "C5"));
}
#[test]
fn blueprint_v2_call_wrong_bound_field_schema_is_rejected() {
    let tree = Tree::new();
    tree.put("callee.yaml", &callee());
    let mut doc = caller();
    doc["steps"].as_array_mut().unwrap().insert(0,json!({"id":"probe","run":{"lit":"echo fixed"},"out":{"type":"object","properties":{"value":object("string")},"required":["value"]}}));
    doc["steps"][1]["call"]["with"]["p"] = json!({"bind":"probe.out.value"});
    doc["steps"][1]["bind"] = json!({"in":"probe.out.value"});
    tree.put("caller.yaml", &doc);
    let closure = tree.resolve("caller.yaml").unwrap();
    assert!(has_rule(&closure, "C4"));
    assert!(has_rule(&closure, "C5"));
}
#[test]
fn blueprint_v2_call_result_port_and_source_mismatch_are_rejected() {
    let tree = Tree::new();
    let mut doc = caller();
    doc["steps"][0]["out"] = object("string");
    tree.put("callee.yaml", &callee());
    tree.put("caller.yaml", &doc);
    assert!(has_rule(&tree.resolve("caller.yaml").unwrap(), "C6"));
    let mut child = callee();
    child["result"]["type"] = object("string");
    tree.put("callee.yaml", &child);
    tree.put("caller.yaml", &caller());
    assert!(has_rule(&tree.resolve("caller.yaml").unwrap(), "C6"));
}
#[test]
fn blueprint_v2_call_inline_and_recursive_ref_resolution() {
    let tree = Tree::new();
    let mut doc = caller();
    doc.as_object_mut().unwrap().remove("imports");
    let mut sub = callee();
    for key in ["schema_version", "kind", "name"] {
        sub.as_object_mut().unwrap().remove(key);
    }
    doc["blueprints"] = json!({"increment":sub});
    tree.put("caller.yaml", &doc);
    assert!(check_facts(&tree.resolve("caller.yaml").unwrap())
        .1
        .is_empty());
    doc["blueprints"]["increment"]["steps"][0]
        .as_object_mut()
        .unwrap()
        .remove("code");
    doc["blueprints"]["increment"]["steps"][0]["call"] =
        json!({"ref":"increment","with":{"p":{"input":"p"}}});
    tree.put("caller.yaml", &doc);
    assert!(tree.resolve("caller.yaml").unwrap_err()[0]
        .message
        .contains("Recursive inline"));
}
#[test]
fn blueprint_v2_call_unresolved_reference_is_not_accepted() {
    let tree = Tree::new();
    let mut doc = caller();
    doc.as_object_mut().unwrap().remove("imports");
    tree.put("caller.yaml", &doc);
    assert!(tree.resolve("caller.yaml").unwrap_err()[0]
        .message
        .contains("Unresolved call"));
}
#[test]
fn blueprint_v2_call_boundary_taint_cannot_reach_executable_input() {
    let tree = Tree::new();
    let mut child = callee();
    child["inputs"] = json!({"command":{"type":"string"}});
    child["steps"] = json!([{"id":"execute","run":{"input":"command"},"out":{"type":"string"}}]);
    child["result"] = json!({"type":{"type":"string"},"from":"execute.out"});
    tree.put("callee.yaml", &child);
    let mut doc = caller();
    doc["steps"] = json!([{"id":"probe","run":{"lit":"echo untrusted"},"out":{"type":"string"}},{"id":"call","call":{"ref":"increment","with":{"command":{"bind":"probe.out"}}},"out":{"type":"string"}}]);
    tree.put("caller.yaml", &doc);
    assert!(has_rule(&tree.resolve("caller.yaml").unwrap(), "C8"));
}
#[test]
fn blueprint_v2_call_view_diff_lower_preserve_identity_and_contract_changes() {
    let tree = Tree::new();
    let closure = good(&tree);
    let before = closure.document();
    for (field, value) in [
        ("imports", json!({"increment":"changed.yaml"})),
        ("inputs", json!({"new":{"type":"integer"}})),
        ("result", json!({"to":"owner"})),
    ] {
        let mut after = serde_json::to_value(&before).unwrap();
        after[field] = value;
        let after: Document = serde_json::from_value(after).unwrap();
        assert!(ir_projection::diff(&before, &after)
            .document_fields
            .contains(&field.into()));
    }
    let graph = ir_projection::canonical(&before);
    assert_eq!(
        graph.nodes[0].node.call.as_ref().unwrap().pin,
        before.steps[0].call.as_ref().unwrap().pin
    );
    for field in ["in", "out", "call", "caps"] {
        let mut after = serde_json::to_value(&before).unwrap();
        after["steps"][0][field] = match field {
            "in" | "out" => object("string"),
            "caps" => json!(["net:example.test"]),
            _ => json!({"ref":"changed","pin":{"id":"missing","hash":"0"}}),
        };
        let after: Document = serde_json::from_value(after).unwrap();
        assert!(ir_projection::diff(&before, &after).changed["call"].contains(&field.into()));
    }
    assert_eq!(
        before.lower().unwrap()["spec"]["steps"][0]["kind"]["kind"],
        "call"
    );
    let mut after = before.clone();
    let key = after.dependencies.keys().next().unwrap().clone();
    after.dependencies.get_mut(&key).unwrap().document.steps[0]
        .caps
        .push("net:changed".into());
    assert!(ir_projection::diff(&before, &after)
        .document_fields
        .contains(&"dependencies".into()));
    assert!(FrozenClosure::from_document(after).is_err());
}
#[test]
fn blueprint_v2_call_structural_arguments_are_never_expression_interpolation() {
    let call = crate::blueprint_document::Call {
        reference: "helper".into(),
        pin: None,
        arguments: BTreeMap::from([
            ("lit".into(), ValueForm::literal("$(effect) {{input}}")),
            ("input".into(), ValueForm::input("p")),
            (
                "bind".into(),
                ValueForm::Bind {
                    bind: "probe.out.n".into(),
                },
            ),
        ]),
    };
    let values = arguments(
        &call,
        &BTreeMap::from([("p".into(), json!({"n":7}))]),
        &BTreeMap::from([("probe".into(), json!({"n":8}))]),
    )
    .unwrap();
    assert_eq!(
        values,
        BTreeMap::from([
            ("lit".into(), json!("$(effect) {{input}}")),
            ("input".into(), json!({"n":7})),
            ("bind".into(), json!(8))
        ])
    );
}
#[test]
fn blueprint_v2_call_closure_missing_hash_and_mismatched_id_are_errors() {
    let tree = Tree::new();
    let mut closure = good(&tree);
    closure.root.document.steps[0]
        .call
        .as_mut()
        .unwrap()
        .pin
        .as_mut()
        .unwrap()
        .id = "mem:foreign".into();
    assert!(closure
        .verify()
        .unwrap_err()
        .contains("Missing pinned hash"));
    let mut closure = good(&tree);
    closure.callees.clear();
    assert!(closure
        .verify()
        .unwrap_err()
        .contains("Missing pinned hash"));
}
#[test]
fn blueprint_v2_call_port_field_comparison_is_recursive_and_ref_bounded() {
    let actual: Port = serde_json::from_value(object("string")).unwrap();
    let expected: Port = serde_json::from_value(object("integer")).unwrap();
    assert!(!ports::compatible(&actual, &expected));
    assert!(ports::expand(
        &Port {
            reference: Some("Loop".into()),
            ..Default::default()
        },
        &BTreeMap::from([(
            "Loop".into(),
            Port {
                reference: Some("Loop".into()),
                ..Default::default()
            }
        )])
    )
    .is_none());
}
#[test]
fn blueprint_v2_call_distinct_ids_share_body_without_collision() {
    let tree = Tree::new();
    let closure = good(&tree);
    let mut callee = closure.callees.values().next().unwrap().clone();
    let mut other = callee.clone();
    callee.id = "mem:first".into();
    other.id = "mem:second".into();
    let mut root = closure.root.document.clone();
    root.steps[0].call.as_mut().unwrap().pin = Some(callee.pin());
    let closure = FrozenClosure {
        root: Parsed {
            document: root,
            locations: BTreeMap::new(),
        },
        callees: BTreeMap::from([(callee.pin().key(), callee), (other.pin().key(), other)]),
    };
    closure.verify().unwrap();
    assert_eq!(closure.callees.len(), 2);
}
#[test]
fn blueprint_v2_call_diagnostic_has_stable_rule_proof_and_position() {
    let tree = Tree::new();
    let mut closure = good(&tree);
    closure.root.document.steps[0]
        .call
        .as_mut()
        .unwrap()
        .arguments
        .clear();
    let diagnostic = check_facts(&closure)
        .1
        .into_iter()
        .next()
        .unwrap()
        .into_diagnostic();
    assert_eq!(diagnostic.rule, "C2");
    assert_eq!(diagnostic.name, "missing_argument");
    assert_eq!(diagnostic.proof.rule, "C2");
    assert_eq!(diagnostic.failing.len(), 1);
    assert!(diagnostic.line > 1);
    assert!(diagnostic.id.starts_with("C2:atom:"));
}
#[test]
fn blueprint_v2_call_expanded_projection_and_semantic_diff_keep_nested_body_changes() {
    let tree = Tree::new();
    let before = good(&tree);
    let view = projection::graph(&before).unwrap();
    assert_eq!(view.calls.len(), 1);
    assert_eq!(view.calls[0].graph["steps"][0]["id"], "increment");
    assert_eq!(
        view.calls[0].pin,
        before.root.document.steps[0]
            .call
            .as_ref()
            .unwrap()
            .pin
            .clone()
            .unwrap()
    );
    let mut child = callee();
    child["steps"][0]["code"]["src"] = json!("def main(p): return {'n': 42}");
    child["steps"][0]["caps"] = json!(["net:example.test"]);
    child["inputs"]["p"] = object("string");
    child["result"]["type"] = object("string");
    tree.put("callee.yaml", &child);
    let after = tree.resolve("caller.yaml").unwrap();
    let diff = projection::diff(&before, &after);
    let changed = diff
        .callees
        .values()
        .next()
        .unwrap()
        .fields
        .as_ref()
        .unwrap();
    assert_eq!(changed.changed["increment"], vec!["caps", "code"]);
    assert_eq!(changed.document_fields, vec!["inputs", "result"]);
}
#[test]
fn blueprint_v2_call_callee_command_timeout_preserves_runtime_default_override_and_bound() {
    use crate::blueprint_pipeline::StepKind;
    let mut document: Document = serde_json::from_value(callee()).unwrap();
    let node = &mut document.steps[0];
    node.code = None;
    node.run = Some(ValueForm::literal("echo ready"));
    for (timeout, expected) in [(None, 10_000), (Some("30s"), 30_000), (Some("60s"), 60_000)] {
        document.steps[0].timeout = timeout.map(ValueForm::literal);
        let lowered = lower::node(&document, &document.steps[0]).unwrap();
        assert!(
            matches!(lowered.kind, StepKind::Command { timeout_ms, .. } if timeout_ms == expected)
        );
    }
    for timeout in ["0s", "61s"] {
        document.steps[0].timeout = Some(ValueForm::literal(timeout));
        assert!(lower::node(&document, &document.steps[0]).is_err());
    }
}

#[test]
fn blueprint_v2_call_lowered_runtime_keeps_exact_pin_and_full_v2_document() {
    let tree = Tree::new();
    let closure = good(&tree);
    let lowered = closure.document().lower().unwrap();
    assert_eq!(lowered["document"]["schema_version"], 2);
    assert_eq!(lowered["spec"]["steps"][0]["kind"]["kind"], "call");
    assert_eq!(
        lowered["spec"]["steps"][0]["kind"]["call"]["pin"],
        serde_json::to_value(&closure.root.document.steps[0].call.as_ref().unwrap().pin).unwrap()
    );
    assert_eq!(
        lowered["document"]["dependencies"],
        serde_json::to_value(&closure.callees).unwrap()
    );
}
#[test]
fn blueprint_v2_call_no_false_final_pass_without_checker_engine() {
    let tree = Tree::new();
    let closure = good(&tree);
    let source = serde_yaml::to_string(&closure.document()).unwrap();
    assert!(!crate::blueprint_document::check("caller.yaml", &source, &[]).ok);
}

fn scheduler_effect(kind: &str) -> FrozenClosure {
    let tree = Tree::new();
    let mut closure = good(&tree);
    let frozen = closure.callees.values_mut().next().unwrap();
    frozen
        .document
        .steps
        .push(serde_json::from_value(json!({"id":"scheduler-effect","uses":kind})).unwrap());
    frozen.hash = ir_projection::identity(&frozen.document).unwrap();
    let pin = frozen.pin();
    let frozen = closure.callees.pop_first().unwrap().1;
    closure.callees.insert(pin.key(), frozen);
    closure.root.document.steps[0].call.as_mut().unwrap().pin = Some(pin);
    closure
}
#[test]
fn blueprint_v2_call_checker_refuses_nested_blueprint_activation_with_positioned_call_rule() {
    let closure = scheduler_effect("blueprint");
    let context = check_context(&closure, &Default::default());
    let diagnostics = crate::blueprint_document::checker::check_ir(&closure.root.ir(), &context);
    assert!(diagnostics.iter().any(|d| d.rule == "C11"
        && d.field == "steps[0].call"
        && d.line > 0
        && d.message.contains("move it to the caller")));
}
#[test]
fn blueprint_v2_call_checker_refuses_nested_stop() {
    assert!(has_rule(&scheduler_effect("stop"), "C11"));
}
#[test]
fn blueprint_v2_call_checker_refuses_nested_respond_prompt() {
    assert!(has_rule(&scheduler_effect("respond_prompt"), "C11"));
}
#[test]
fn blueprint_v2_call_static_expanded_graph_budget_stops_repeated_dag_expansion() {
    let tree = Tree::new();
    let mut closure = good(&tree);
    let repeated = closure.root.document.steps[0].clone();
    for index in 1..64 {
        let mut node = repeated.clone();
        node.id = format!("call-{index}");
        closure.root.document.steps.push(node);
    }
    assert!(closure
        .verify()
        .unwrap_err()
        .contains("expanded call graph exceeds 64"));
}

#[test]
fn blueprint_v2_call_diff_keeps_all_revisions_of_one_stable_identity() {
    let tree = Tree::new();
    let before = good(&tree);
    let mut after = before.clone();
    let original = after.callees.pop_first().unwrap().1;
    for name in ["First revised body", "Second revised body"] {
        let mut callee = original.clone();
        callee.document.note = Some(name.into());
        callee.hash = ir_projection::identity(&callee.document).unwrap();
        if after.callees.is_empty() {
            after.root.document.steps[0].call.as_mut().unwrap().pin = Some(callee.pin());
        }
        after.callees.insert(callee.pin().key(), callee);
    }
    let diff = projection::diff(&before, &after);
    assert_eq!(diff.callees.len(), 3);
    assert_eq!(
        diff.callees
            .values()
            .filter(|change| change.before.is_some())
            .count(),
        1
    );
    assert_eq!(
        diff.callees
            .values()
            .filter(|change| change.after.is_some())
            .count(),
        2
    );
}

#[test]
fn blueprint_v2_call_review_other_is_refused_by_positioned_call_rule() {
    let tree = Tree::new();
    let mut closure = good(&tree);
    let (_, mut callee) = closure.callees.pop_first().unwrap();
    callee
        .document
        .other
        .push(serde_json::from_value(json!({"id":"failure-only","uses":"notify"})).unwrap());
    callee.hash = ir_projection::identity(&callee.document).unwrap();
    closure.root.document.steps[0].call.as_mut().unwrap().pin = Some(callee.pin());
    closure.callees.insert(callee.pin().key(), callee);
    let context = check_context(&closure, &Default::default());
    assert!(context
        .call_diagnostics
        .iter()
        .any(|d| d.rule.starts_with('C')
            && d.message.contains("other")
            && d.field == "steps[0].call"
            && d.line > 0));
}
struct ReviewMemory(Document);
impl MemoryResolver for ReviewMemory {
    fn head(&self, _: &str) -> Result<Document, String> {
        Ok(self.0.clone())
    }
    fn revision(&self, _: &CallPin) -> Result<Document, String> {
        Ok(self.0.clone())
    }
}
#[test]
fn blueprint_v2_call_review_saved_ref_cannot_disagree_with_pin_identity() {
    let tree = Tree::new();
    let closure = good(&tree);
    let mut callee = closure.callees.values().next().unwrap().clone();
    callee.id = "mem:pinned".into();
    let mut document = closure.root.document;
    document.imports.clear();
    document.steps[0].call.as_mut().unwrap().reference = "mem:displayed".into();
    document.steps[0].call.as_mut().unwrap().pin = Some(callee.pin());
    document.dependencies = BTreeMap::from([(callee.pin().key(), callee.clone())]);
    assert!(resolve_saved(document, &ReviewMemory(callee.document.as_ref().clone())).is_err());
}
#[test]
fn blueprint_v2_call_review_unknown_alias_cannot_keep_an_unrelated_pin() {
    let tree = Tree::new();
    let mut closure = good(&tree);
    closure.root.document.steps[0]
        .call
        .as_mut()
        .unwrap()
        .reference = "unknown-alias".into();
    let document = closure.document();
    let leaf = closure
        .callees
        .values()
        .next()
        .unwrap()
        .document
        .as_ref()
        .clone();
    assert!(resolve_saved(document, &ReviewMemory(leaf)).is_err());
}
