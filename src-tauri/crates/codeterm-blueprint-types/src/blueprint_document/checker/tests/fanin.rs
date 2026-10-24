use super::*;

#[test]
fn blueprint_v2_dominance_refuses_skipped_producer_at_branch_and_merge() {
    for merge in [false, true] {
        for call in [false, true] {
            let mut doc = document(json!([
                {"id":"branch","uses":"ask","with":{"question":{"lit":"Which?"}},"buckets":["left","right"],"routes":{"left":"producer","right":"merge","other":"@stop"}},
                {"id":"producer","run":{"lit":"echo value"}},
                {"id":"merge","run":{"lit":"echo merged"}},
                {"id":"consumer","run":{"lit":"echo consume"}}
            ]));
            let consumer = if merge { 3 } else { 2 };
            if call {
                doc.steps[consumer].run = None;
                doc.steps[consumer].call = Some(
                    serde_json::from_value(
                        json!({"ref":"mem:helper","with":{"arg":{"bind":"producer.out"}}}),
                    )
                    .unwrap(),
                );
            } else {
                doc.steps[consumer]
                    .bind
                    .insert("in".into(), "producer.out".into());
            }
            let diagnostics = inspect(&doc);
            let diagnostic = diagnostics.iter().find(|d| d.rule == "D1").unwrap();
            assert_eq!(diagnostic.name, "bind_not_dominated");
            assert_eq!(diagnostic.severity, "error");
            assert_eq!(diagnostic.file, "fixture.yaml");
            assert!(diagnostic.line > 1 && diagnostic.column > 0);
            assert!(diagnostic.message.contains("producer"));
            assert!(diagnostic.message.contains("branch.right -> merge"));
            assert!(diagnostic
                .field
                .ends_with(if call { ".call.with.arg" } else { ".bind.in" }));
        }
    }
}

fn fanin() -> Document {
    document(json!([
        {"id":"a","code":{"lang":"python","src":"def main(value): return 7"},"out":{"type":"integer"}},
        {"id":"b","code":{"lang":"python","src":"def main(value): return 'linen'"},"out":{"type":"string"}},
        {"id":"join","code":{"lang":"python","src":"def main(value): return value"},"bind":{"count":"a.out","label":"b.out"}}
    ]))
}

#[test]
fn blueprint_v2_fanin_infers_object_and_keeps_single_bind() {
    let doc = fanin();
    assert!(!has_errors(&inspect(&doc)), "{:#?}", inspect(&doc));
    let lowered = doc.lower().unwrap();
    let execution = &lowered["spec"]["steps"][2]["execution"];
    assert_eq!(
        execution["input_bindings"],
        json!({"count":"a.out","label":"b.out"})
    );
    assert_eq!(execution["input"]["properties"]["count"]["type"], "integer");
    assert_eq!(execution["input"]["properties"]["label"]["type"], "string");
    assert_eq!(execution["input"]["required"], json!(["count", "label"]));
    assert_eq!(execution["input"]["additionalProperties"], false);
    let mut single = doc.clone();
    single.steps[2].bind = BTreeMap::from([("in".into(), "a.out".into())]);
    let single = single.lower().unwrap();
    assert_eq!(
        single["spec"]["steps"][2]["execution"]["input_binding"],
        "a.out"
    );
    assert!(single["spec"]["steps"][2]["execution"]
        .get("input_bindings")
        .is_none());
}

#[test]
fn blueprint_v2_fanin_checks_each_port_and_dominance() {
    for (name, kind) in [("count", "string"), ("label", "integer")] {
        let mut doc = fanin();
        doc.steps[2].input = Some(
            serde_json::from_value(json!({"type":"object","properties":{name:{"type":kind}}}))
                .unwrap(),
        );
        let diagnostics = inspect(&doc);
        assert!(
            diagnostics
                .iter()
                .any(|d| d.rule == "R3" && d.field == format!("steps[2].bind.{name}")),
            "{diagnostics:#?}"
        );
    }
    let mut doc = fanin();
    doc.steps[0].routes.insert("done".into(), "join".into());
    assert!(inspect(&doc)
        .iter()
        .any(|d| d.rule == "D1" && d.field == "steps[2].bind.label"));
    doc.steps[2].bind.insert("in".into(), "a.out".into());
    assert!(has_errors(&inspect(&doc)));
}

#[test]
fn blueprint_v2_dominance_accepts_shared_ancestor_after_merge() {
    let doc = document(json!([
        {"id":"shared","run":{"lit":"echo shared"}},
        {"id":"branch","uses":"ask","buckets":["left"],"with":{"question":{"lit":"Which?"}},"routes":{"left":"detour","other":"merge"}},
        {"id":"detour","run":{"lit":"echo detour"}},
        {"id":"merge","run":{"lit":"echo merge"},"bind":{"in":"shared.out"}}
    ]));
    assert!(!inspect(&doc).iter().any(|d| d.rule == "D1"));
}

#[test]
fn blueprint_v2_fanin_refuses_missing_required_extra_and_unproven_fields() {
    for schema in [
        json!({"type":"integer"}),
        json!({"type":"object","properties":{"missing":{"type":"integer"}},"required":["missing"]}),
        json!({"type":"object","properties":{"count":{"type":"integer"}},"additionalProperties":false}),
    ] {
        let mut doc = fanin();
        doc.steps[2].input = Some(serde_json::from_value(schema).unwrap());
        assert!(inspect(&doc)
            .iter()
            .any(|d| d.name == "named_port_conflict"));
    }
    let mut doc = fanin();
    doc.steps[0].output = Some(
        serde_json::from_value(
            json!({"type":"object","properties":{"optional":{"type":"integer"}}}),
        )
        .unwrap(),
    );
    doc.steps[2]
        .bind
        .insert("count".into(), "a.out.optional".into());
    assert!(inspect(&doc)
        .iter()
        .any(|d| d.name == "named_port_conflict" && d.field == "steps[2].bind.count"));
}

#[test]
fn blueprint_v2_named_bind_media_is_positioned_and_typed() {
    for media in ["image", "file", "audio", "video"] {
        for explicit in [false, true] {
            let mut doc = fanin();
            doc.steps[0].output =
                Some(serde_json::from_value(json!({"type":"string", "x-media":media})).unwrap());
            if explicit {
                doc.steps[2].input =
                    Some(serde_json::from_value(json!({"type":"object"})).unwrap());
            }
            let diagnostics = inspect(&doc);
            let diagnostic = diagnostics
                .iter()
                .find(|d| d.name == "named_bind_media")
                .unwrap();
            assert_eq!(diagnostic.rule, "B2");
            assert_eq!(diagnostic.severity, "error");
            assert_eq!(diagnostic.field, "steps[2].bind.count");
            assert_eq!(diagnostic.file, "fixture.yaml");
            assert!(diagnostic.line > 1 && diagnostic.column > 0);
            assert!(diagnostic
                .message
                .contains("media ports in named binds are not supported yet"));
        }
    }
    for producer in ["convert", "input", "call", "reference", "field"] {
        let mut doc = fanin();
        match producer {
            "convert" | "input" => {
                doc.steps[0].code = None;
                doc.steps[0].uses = Some(if producer == "convert" {
                    crate::blueprint_document::NodeKind::Convert
                } else {
                    crate::blueprint_document::NodeKind::Input
                });
                doc.steps[0].output = None;
                doc.steps[0].params.insert(
                    if producer == "convert" { "to" } else { "kind" }.into(),
                    crate::blueprint_document::ValueForm::literal("image"),
                );
            }
            "call" => {
                let mut child = fanin();
                child.trigger = None;
                child.steps.truncate(1);
                child.steps[0].code.as_mut().unwrap().src =
                    Some("def main(value): return 'picture'".into());
                child.steps[0].output = Some(
                    serde_json::from_value(json!({"type":"string", "x-media":"image"})).unwrap(),
                );
                child.result = serde_json::from_value(
                    json!({"type":{"type":"string", "x-media":"image"},"from":"a.out"}),
                )
                .unwrap();
                let hash = crate::blueprint_document::projection::identity(&child).unwrap();
                let callee = crate::blueprint_document::calls::FrozenCallee {
                    id: "mem:media".into(),
                    hash,
                    document: Box::new(child),
                    locations: BTreeMap::new(),
                    source_hash: None,
                };
                let pin = callee.pin();
                doc.dependencies.insert(pin.key(), callee);
                doc.steps[0].code = None;
                doc.steps[0].output = None;
                doc.steps[0].call = Some(crate::blueprint_document::Call {
                    reference: pin.id.clone(),
                    pin: Some(pin),
                    arguments: BTreeMap::new(),
                });
                let closure =
                    crate::blueprint_document::calls::FrozenClosure::from_document(doc.clone())
                        .unwrap();
                let context =
                    crate::blueprint_document::calls::check_context(&closure, &Default::default());
                assert_eq!(
                    context.call_output_media.get("a").map(String::as_str),
                    Some("image")
                );
                assert!(crate::blueprint_document::checker::check_document(
                    &closure.root.document,
                    &context
                )
                .iter()
                .any(|d| d.name == "named_bind_media" && d.field == "steps[2].bind.count"));
            }
            "field" => {
                doc.steps[0].output = Some(serde_json::from_value(json!({"type":"object", "required":["picture"], "properties":{"picture":{"type":"string", "x-media":"image"}}})).unwrap());
                doc.steps[2]
                    .bind
                    .insert("count".into(), "a.out.picture".into());
            }
            _ => {
                doc.types.insert(
                    "Picture".into(),
                    serde_json::from_value(json!({"type":"string", "x-media":"image"})).unwrap(),
                );
                doc.steps[0].output =
                    Some(serde_json::from_value(json!({"$ref":"Picture"})).unwrap());
            }
        }
        assert!(
            inspect(&doc)
                .iter()
                .any(|d| d.name == "named_bind_media" && d.field == "steps[2].bind.count"),
            "{producer}"
        );
    }
    for media in [None, Some("text")] {
        let mut doc = fanin();
        doc.steps[0].output.as_mut().unwrap().media = media.map(str::to_owned);
        assert!(!has_errors(&inspect(&doc)), "{:?}", inspect(&doc));
    }
}

#[test]
fn blueprint_v2_malformed_binding_refuses_single_and_named_inputs() {
    for binding in ["a.outfoo", "a.out.", "a.out..field"] {
        for slot in ["in", "count"] {
            let mut doc = fanin();
            doc.steps[2].bind = BTreeMap::from([(slot.into(), binding.into())]);
            assert!(has_errors(&inspect(&doc)), "{binding}");
            assert!(doc.lower().is_err(), "{binding}");
        }
    }
}
