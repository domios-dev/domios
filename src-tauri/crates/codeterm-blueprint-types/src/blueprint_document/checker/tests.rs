use super::engine::{atom, no, rule, yes, Atom, Database};
use super::*;
use crate::blueprint_document::{self, ValueForm};
use serde_json::{json, Value};

mod fanin;

fn document(nodes: Value) -> Document {
    serde_json::from_value(json!({"schema_version":2,"kind":"pipeline","name":"checker-fixture","owner":"tab","trigger":{"type":"schedule","every":{"lit":"20s"}},"steps":nodes})).unwrap()
}
fn inspect(doc: &Document) -> Vec<Diagnostic> {
    let source = serde_yaml::to_string(doc).unwrap();
    let parsed = blueprint_document::parse("fixture.yaml", &source).unwrap();
    check_ir(&parsed.ir(), &CheckContext::default())
}
fn contains(ds: &[Diagnostic], rule: &str) -> bool {
    ds.iter()
        .any(|d| d.rule == rule || contains(&d.folded, rule))
}
fn require(doc: Document, rule: &str) {
    let ds = inspect(&doc);
    let found = ds
        .iter()
        .flat_map(|d| std::iter::once(d).chain(&d.folded))
        .find(|d| d.rule == rule)
        .unwrap_or_else(|| panic!("missing {rule}: {ds:#?}"));
    assert!(!found.id.is_empty());
    assert_eq!(found.file, "fixture.yaml");
    assert!(found.line > 1 && found.column > 0);
    assert!(!found.failing.is_empty());
    assert!(!found.passing.is_empty() || rule == "R10");
    assert!(!found.proof.children.is_empty());
    assert_eq!(inspect(&doc), ds);
}
#[test]
fn blueprint_v2_check_r1_unreachable() {
    require(
        document(
            json!([{"id":"a","run":{"lit":"echo a"},"routes":{"done":"@stop","failed":"@stop"}},{"id":"b","run":{"lit":"echo b"}}]),
        ),
        "R1",
    );
}
#[test]
fn blueprint_v2_check_r2_dead_output() {
    require(
        document(
            json!([{"id":"a","uses":"convert","out":{"type":"string"}},{"id":"b","run":{"lit":"echo b"}}]),
        ),
        "R2",
    );
}
#[test]
fn blueprint_v2_check_r3_restricted_subsumption() {
    for (source, target) in [
        (json!({"type":"string"}), json!({"type":"integer"})),
        (
            json!({"type":"object","additionalProperties":false}),
            json!({"type":"object","required":["a"],"properties":{"a":{"type":"integer"}}}),
        ),
        (
            json!({"type":"object","properties":{"a":{"type":"integer"}},"additionalProperties":false}),
            json!({"type":"object","additionalProperties":false}),
        ),
        (
            json!({"type":"string","enum":["a","b"]}),
            json!({"type":"string","enum":["a"]}),
        ),
        (
            json!({"type":"string","maxLength":40}),
            json!({"type":"string","maxLength":10}),
        ),
    ] {
        require(
            document(
                json!([{"id":"a","run":{"lit":"echo a"},"out":source},{"id":"b","uses":"ask","in":target,"bind":{"in":"a.out"}}]),
            ),
            "R3",
        );
    }
    let open: super::super::Port = serde_json::from_value(json!({"type":"object"})).unwrap();
    let closed: super::super::Port =
        serde_json::from_value(json!({"type":"object","additionalProperties":false})).unwrap();
    assert!(!ports::subsumption_conflicts(&open, &closed).is_empty());
}
#[test]
fn blueprint_v2_check_r4_contradictory_outcome() {
    require(
        document(json!([{"id":"a","run":{"lit":"echo a"},"routes":{"invented":"@stop"}}])),
        "R4",
    );
    let source="schema_version: 2\nkind: pipeline\nname: duplicate-route\nsteps:\n- id: a\n  run: {lit: 'echo a'}\n  routes:\n    done: '@stop'\n    done: a\n";
    assert!(!blueprint_document::check("duplicate.yaml", source, &[]).ok);
}
#[test]
fn blueprint_v2_check_r5_backward_route() {
    require(
        document(json!([{"id":"a","run":{"lit":"echo a"},"routes":{"done":"a"}}])),
        "R5",
    );
}
#[test]
fn blueprint_v2_check_r6_unknown_other_warning() {
    let doc = document(json!([{"id":"a","uses":"ask","buckets":["ok"]}]));
    require(doc.clone(), "R6");
    let ds = inspect(&doc);
    assert_eq!(
        ds.iter().find(|d| d.rule == "R6").unwrap().severity,
        "warning"
    );
}
#[test]
fn blueprint_v2_check_r7_observed_and_model_taint() {
    for kind in ["agent", "http", "ask"] {
        require(
            document(json!([{"id":"a","uses":kind},{"id":"b","run":{"bind":"a.out"}}])),
            "R7",
        );
    }
}
#[test]
fn blueprint_v2_check_r8_fixed_slot_without_taint() {
    require(
        document(
            json!([{"id":"a","uses":"convert"},{"id":"b","uses":"send","with":{"target":{"bind":"a.out"}}}]),
        ),
        "R8",
    );
}
#[test]
fn blueprint_v2_check_r9_caps_never_self_grant() {
    let doc = document(json!([{"id":"a","run":{"lit":"echo a"},"caps":["net:example.org"]}]));
    require(doc.clone(), "R9");
    let context = CheckContext {
        grants: BTreeSet::from(["net:example.org".into()]),
        ..Default::default()
    };
    assert!(!contains(&check_document(&doc, &context), "R9"));
}
#[test]
fn blueprint_v2_check_r10_ambient_cli_and_unapproved_cap() {
    require(
        document(json!([{"id":"a","run":{"lit":"codeterm tab list"}}])),
        "R10",
    );
    require(
        document(json!([{"id":"a","run":{"lit":"\"dom\"\"ios\" team list"}}])),
        "R10",
    );
    require(
        document(
            json!([{"id":"a","code":{"lang":"python","src":"import subprocess\ndef main(p):\n    env = {'CODE' + 'TERM_API_PORT': '7686'}\n    return subprocess.run(['C:/Users/u/.dom' + 'ios-dev/bin/dom' + 'ios.exe', 'team', 'list'], env=env).returncode"},"in":{"type":"integer"},"out":{"type":"integer"}}]),
        ),
        "R10",
    );
    let doc = document(json!([{"id":"a","run":{"lit":"echo a"},"caps":["domios-cli"]}]));
    let context = CheckContext {
        grants: BTreeSet::from(["domios-cli".into()]),
        ..Default::default()
    };
    assert!(contains(&check_document(&doc, &context), "R10"));
    assert!(contains(&check_document(&doc, &context), "R9"));
}
#[test]
fn blueprint_v2_check_r11_bucket_bounds() {
    let buckets = (0..33).map(|i| format!("b{i}")).collect::<Vec<_>>();
    require(
        document(json!([{"id":"a","uses":"ask","buckets":buckets}])),
        "R11",
    );
}
#[test]
fn blueprint_v2_check_r12_step_bounds() {
    let nodes = (0..65)
        .map(|i| json!({"id":format!("a{i}"),"run":{"lit":"echo a"}}))
        .collect::<Vec<_>>();
    require(document(json!(nodes)), "R12");
}
#[test]
fn blueprint_v2_check_bad_input_one_root_with_dependent_proofs() {
    let mut nodes = vec![
        json!({"id":"bad","run":{"lit":"echo source"},"out":{"type":"string","x-media":"nonsense"},"outcomes":["one","two","three","four","five"],"routes":{"one":"one","two":"two","three":"three","four":"four","five":"five"}}),
    ];
    for id in ["one", "two", "three", "four", "five"] {
        nodes.push(json!({"id":id,"run":{"lit":"echo receiver"},"in":{"type":"string","x-media":"text"},"routes":{"done":"@stop","failed":"@stop"}}));
    }
    let doc = document(json!(nodes));
    let ds = inspect(&doc);
    let roots = ds
        .iter()
        .filter(|d| d.severity == "error")
        .collect::<Vec<_>>();
    assert_eq!(roots.len(), 1, "{ds:#?}");
    assert_eq!(roots[0].name, "invalid_media");
    assert_eq!(roots[0].folded.len(), 5);
    assert!(roots[0].folded.iter().all(|d| d.rule == "R3"));
    let rendered = render(&ds, true);
    assert!(
        rendered.contains("CAUSED BY:")
            && rendered.contains("GIVEN:")
            && rendered.contains("folded:")
    );
}
#[test]
fn blueprint_v2_check_safe_fix_diff_and_idempotence() {
    let raw="name: repairs\nkind: pipeline\nschema_version: 2\ntrigger: {type: schedule, every: {lit: 1s}}\nsteps:\n- id: a\n  run: {lit: 'echo ready'}\n";
    let parsed = blueprint_document::parse("fix.yaml", raw).unwrap();
    assert!(contains(
        &check_ir(&parsed.ir(), &CheckContext::default()),
        "S2"
    ));
    let fixed = fix::fix(&parsed, raw, &CheckContext::default());
    assert!(fixed.changed && fixed.diff.contains("--- a/fix.yaml"));
    assert_eq!(
        fixed.semantic_diff.as_ref().unwrap().document_fields,
        vec!["trigger"]
    );
    let text = fixed.text.unwrap();
    assert!(blueprint_document::check("fix.yaml", &text, &[]).ok);
    let parsed = blueprint_document::parse("fix.yaml", &text).unwrap();
    let second = fix::fix(&parsed, &text, &CheckContext::default());
    assert!(!second.changed);
    assert!(second.diff.is_empty());
}
#[test]
fn blueprint_v2_check_fix_input_type_is_unambiguous_or_atomic_refusal() {
    let mut doc = document(json!([{"id":"a","run":{"input":"command"}}]));
    let source = serde_yaml::to_string(&doc).unwrap();
    let parsed = blueprint_document::parse("inputs.yaml", &source).unwrap();
    let fixed = fix::fix_with_assignments(
        &parsed,
        &source,
        &CheckContext::default(),
        &["command=echo ready".into()],
    );
    let after = blueprint_document::parse("inputs.yaml", fixed.text.as_ref().unwrap()).unwrap();
    assert_eq!(after.document.inputs["command"].kind, "string");
    doc.steps[0]
        .params
        .insert("custom".into(), ValueForm::input("unknown"));
    let source = serde_yaml::to_string(&doc).unwrap();
    let parsed = blueprint_document::parse("inputs.yaml", &source).unwrap();
    let refused = fix::fix(&parsed, &source, &CheckContext::default());
    assert!(!refused.changed && refused.text.is_none() && !refused.refusals.is_empty());
    doc.steps[0].caps.push("net:example.org".into());
    let source = serde_yaml::to_string(&doc).unwrap();
    let parsed = blueprint_document::parse("caps.yaml", &source).unwrap();
    assert!(fix::fix(&parsed, &source, &CheckContext::default())
        .text
        .is_none());
}
#[test]
fn blueprint_v2_check_other_fix_preserves_previous_terminal_fallthrough() {
    let doc = document(
        json!([{"id":"a","uses":"ask","with":{"question":{"lit":"Ready?"}},"buckets":["ok"]},{"id":"b","run":{"lit":"echo b"}}]),
    );
    let source = serde_yaml::to_string(&doc).unwrap();
    let parsed = blueprint_document::parse("other.yaml", &source).unwrap();
    let fixed = fix::fix(&parsed, &source, &CheckContext::default());
    assert!(!fixed.changed && fixed.text.is_none());
    assert!(fixed
        .refusals
        .iter()
        .any(|reason| reason.contains("Pipeline other")));
    assert!(parsed.document.lower().is_ok());
}
#[test]
fn blueprint_v2_check_call_extension_preserves_taint_and_expanded_bounds() {
    let doc = document(
        json!([{"id":"callee","call":{"ref":"sub"}},{"id":"command","run":{"bind":"callee.out"}}]),
    );
    assert!(contains(&inspect(&doc), "C1"));
    let context = CheckContext {
        calls: BTreeMap::from([(
            "callee".into(),
            VerifiedCall {
                result_tainted: true,
                expanded_steps: 64,
                ..Default::default()
            },
        )]),
        ..Default::default()
    };
    let ds = check_document(&doc, &context);
    assert!(!contains(&ds, "C1"));
    assert!(contains(&ds, "R7") && contains(&ds, "R12"));
}
#[test]
fn blueprint_v2_check_duplicates_are_evidence_or_honest_unavailability() {
    let doc = document(json!([{"id":"a","run":{"lit":"echo a"}}]));
    assert!(inspect(&doc)
        .iter()
        .any(|d| d.name == "duplicate_comparison_unavailable" && d.severity == "info"));
    let context = CheckContext {
        duplicates: DuplicateStatus::Available(vec![DuplicateFinding {
            verdict: "structural_twin".into(),
            other: "solution-id".into(),
            why: "existing vector/WL result".into(),
        }]),
        ..Default::default()
    };
    assert!(contains(&check_document(&doc, &context), "R13"));
}
#[test]
fn blueprint_v2_check_engine_stratification_safety_and_recursive_provenance() {
    let location = blueprint_document::Location {
        file: "engine".into(),
        line: 1,
        column: 1,
    };
    let mut db = Database::default();
    db.edb(
        "edge",
        vec!["a".into(), "b".into()],
        "edge",
        location.clone(),
    );
    db.edb(
        "edge",
        vec!["b".into(), "c".into()],
        "edge",
        location.clone(),
    );
    let rules = vec![
        rule(
            "seed",
            atom("reach", &["A", "B"]),
            vec![yes("edge", &["A", "B"])],
        ),
        rule(
            "transitive",
            atom("reach", &["A", "C"]),
            vec![yes("reach", &["A", "B"]), yes("edge", &["B", "C"])],
        ),
    ];
    db.evaluate(&rules).unwrap();
    let fact = Atom::new("reach", vec!["a".into(), "c".into()]);
    let proof = db.proof(&fact).unwrap();
    assert_eq!(proof.rule, "transitive");
    assert_eq!(db.rows("reach").count(), 3);
    assert_eq!(db.support(&fact).len(), 3);
    let cycle = vec![rule(
        "negative",
        atom("a", &["A"]),
        vec![yes("edge", &["A", "_"]), no("a", &["A"])],
    )];
    assert!(Database::default().evaluate(&cycle).is_err());
    let unsafe_rule = vec![rule(
        "unsafe",
        atom("a", &["B"]),
        vec![yes("edge", &["A", "_"])],
    )];
    assert!(Database::default().evaluate(&unsafe_rule).is_err());
}
#[test]
fn blueprint_v2_check_native_recipes_have_no_static_errors() {
    let root = crate::test_support::fixture_manifest_dir().join("../../../resources/blueprints");
    let mut count = 0;
    for file in std::fs::read_dir(root).unwrap() {
        let file = file.unwrap().path();
        if file.extension().and_then(|s| s.to_str()) != Some("yaml") {
            continue;
        }
        let source = std::fs::read_to_string(&file).unwrap();
        let parsed = blueprint_document::parse(file.to_str().unwrap(), &source).unwrap();
        let ds = check_ir(&parsed.ir(), &CheckContext::default());
        assert!(!has_errors(&ds), "{}: {ds:#?}", file.display());
        count += 1;
    }
    assert!(count >= 15);
}

#[test]
fn blueprint_v2_check_watch_other_fix_is_admissible_and_idempotent() {
    let doc: Document = serde_json::from_value(json!({"schema_version":2,"kind":"watch","name":"other-fix","steps":[{"id":"source","uses":"probe","with":{"command":{"lit":"echo ready"}},"timeout":{"lit":"1h"}},{"id":"decide","uses":"ask","with":{"question":{"lit":"Ready?"}},"buckets":["ok"],"matches":{"ok":{"lit":"ready"}},"routes":{"ok":"@stop"}}]})).unwrap();
    assert!(
        doc.lower().is_ok(),
        "positive fixture must already satisfy runtime bounds"
    );
    let source = serde_yaml::to_string(&doc).unwrap();
    let parsed = blueprint_document::parse("watch.yaml", &source).unwrap();
    let fixed = fix::fix(&parsed, &source, &CheckContext::default());
    let checked = blueprint_document::check("watch.yaml", fixed.text.as_ref().unwrap(), &[]);
    assert!(checked.ok, "{:?}", checked.diagnostics);
    let after = blueprint_document::parse("watch.yaml", fixed.text.as_ref().unwrap()).unwrap();
    assert_eq!(after.document.steps[1].escalation_timeout_ms, Some(60_000));
    assert!(
        !fix::fix(
            &after,
            fixed.text.as_ref().unwrap(),
            &CheckContext::default()
        )
        .changed
    );
    let mut unbounded = doc;
    unbounded.steps[0].timeout = None;
    let source = serde_yaml::to_string(&unbounded).unwrap();
    let parsed = blueprint_document::parse("unbounded.yaml", &source).unwrap();
    let fixed = fix::fix(&parsed, &source, &CheckContext::default());
    assert!(!fixed.changed && fixed.text.is_none());
    assert!(fixed
        .refusals
        .iter()
        .any(|reason| reason.contains("explicit timeout")));
}
#[test]
fn blueprint_v2_check_missing_input_inference_uses_actual_native_slot_types() {
    for (kind, _slot, expected) in [
        ("github", "id", "integer"),
        ("http", "expected_status", "string"),
    ] {
        let params = if kind == "github" {
            json!({"resource":{"lit":"run"},"repo":{"lit":"owner/repo"},"id":{"input":"missing"}})
        } else {
            json!({"url":{"lit":"https://example.org"},"expected_status":{"input":"missing"}})
        };
        let doc = document(json!([{"id":"a","uses":kind,"with":params}]));
        let source = serde_yaml::to_string(&doc).unwrap();
        let parsed = blueprint_document::parse("types.yaml", &source).unwrap();
        let value = if kind == "github" { "123" } else { "200" };
        let fixed = fix::fix_with_assignments(
            &parsed,
            &source,
            &CheckContext::default(),
            &[format!("missing={value}")],
        );
        let after = blueprint_document::parse("types.yaml", fixed.text.as_ref().unwrap()).unwrap();
        assert_eq!(after.document.inputs["missing"].kind, expected);
    }
}

#[test]
fn blueprint_v2_check_implicit_stdin_flow_checks_every_incoming_port() {
    require(
        document(
            json!([{"id":"source","run":{"lit":"echo text"},"out":{"type":"string"}},{"id":"receiver","uses":"ask","in":{"type":"integer"}}]),
        ),
        "R3",
    );
}

#[test]
fn blueprint_v2_check_shared_ref_expansion_budget() {
    for levels in [4, 15] {
        let mut doc = document(json!([{"id":"a","run":{"lit":"echo a"}}]));
        for i in 0..levels {
            let properties = (0..10)
                .map(|j| (format!("p{j}"), json!({"$ref":format!("t{}", i + 1)})))
                .collect::<serde_json::Map<_, _>>();
            doc.types.insert(
                format!("t{i}"),
                serde_json::from_value(json!({"type":"object","properties":properties})).unwrap(),
            );
        }
        doc.types.insert(
            format!("t{levels}"),
            serde_json::from_value(json!({"type":"string"})).unwrap(),
        );
        let start = std::time::Instant::now();
        let diagnostics = inspect(&doc);
        let refusal = diagnostics
            .iter()
            .find(|d| d.message.contains("expanded-node budget"));
        assert!(
            refusal.is_some(),
            "expected expansion refusal: {diagnostics:#?}"
        );
        let refusal = refusal.unwrap();
        assert_eq!(refusal.severity, "error");
        assert_eq!(refusal.file, "fixture.yaml");
        assert!(refusal.field.starts_with("types."));
        assert!(refusal.line > 1);
        assert!(start.elapsed() < std::time::Duration::from_secs(1));
    }
    let mut doc = document(json!([{"id":"a","run":{"lit":"echo a"}}]));
    doc.types.insert(
        "leaf".into(),
        serde_json::from_value(json!({"type":"string","description":"x".repeat(4096)})).unwrap(),
    );
    for (name, target) in [("small", "leaf"), ("wide", "small"), ("root", "wide")] {
        let properties = (0..10)
            .map(|i| (format!("p{i}"), json!({"$ref":target})))
            .collect::<serde_json::Map<_, _>>();
        doc.types.insert(
            name.into(),
            serde_json::from_value(json!({"type":"object","properties":properties})).unwrap(),
        );
    }
    assert!(inspect(&doc)
        .iter()
        .any(|d| d.severity == "error" && d.message.contains("expanded-node budget")));
}

#[test]
fn blueprint_v2_check_pipeline_repair_is_admissible_or_refused() {
    let doc = document(
        json!([{"id":"decide","uses":"ask","with":{"question":{"lit":"Ready?"}},"buckets":["ready"]}]),
    );
    let source = serde_yaml::to_string(&doc).unwrap();
    assert!(blueprint_document::check("pipeline.yaml", &source, &[]).ok);
    assert!(doc.lower().is_ok());
    let parsed = blueprint_document::parse("pipeline.yaml", &source).unwrap();
    let fixed = fix::fix(&parsed, &source, &CheckContext::default());
    if let Some(text) = fixed.text {
        let checked = blueprint_document::check("pipeline.yaml", &text, &[]);
        assert!(checked.ok, "{:?}", checked.diagnostics);
        assert!(blueprint_document::parse("pipeline.yaml", &text)
            .unwrap()
            .document
            .lower()
            .is_ok());
    } else {
        assert!(!fixed.changed && !fixed.refusals.is_empty());
    }
}

#[test]
fn blueprint_v2_check_lower_refuses_self_routed_watch_effect() {
    let doc: Document = serde_json::from_value(json!({"schema_version":2,"kind":"watch","name":"self-route","steps":[{"id":"source","uses":"probe","with":{"command":{"lit":"echo ready"}},"timeout":{"lit":"1h"}},{"id":"decide","uses":"ask","with":{"question":{"lit":"Ready?"}},"buckets":["ready"],"matches":{"ready":{"lit":"ready"}},"routes":{"ready":"alert"}},{"id":"alert","uses":"notify","routes":{"done":"alert"}}]})).unwrap();
    // Guard the direct lowering assertion from the pre-fix infinite loop.
    assert!(doc
        .template_errors()
        .iter()
        .any(|(_, message)| message.contains("forward")));
    let errors = doc.lower().unwrap_err();
    assert!(errors
        .iter()
        .any(|(path, message)| path == "steps[2].routes.done" && message.contains("forward")));
}

#[test]
fn blueprint_v2_check_template_enforces_storage_bounds() {
    let mut doc = document(json!([{"id":"a","run":{"lit":"echo a"}}]));
    doc.steps = (0..65)
        .map(|i| {
            let mut node = doc.steps[0].clone();
            node.id = format!("n{i}");
            node
        })
        .collect();
    assert!(doc
        .template_errors()
        .iter()
        .any(|(_, message)| message.contains("64")));
    assert!(doc.lower().is_err());
    doc.steps.truncate(1);
    doc.steps[0].buckets = (0..33).map(|i| format!("b{i}")).collect();
    assert!(doc
        .template_errors()
        .iter()
        .any(|(_, message)| message.contains("32")));
    assert!(doc.lower().is_err());
}

#[test]
fn blueprint_v2_runtime_memory_warning_is_positioned_and_does_not_refuse() {
    let source = "schema_version: 2\nkind: pipeline\nname: memory-warning\ntrigger: {type: input, kind: {lit: text}, source: {lit: {text: hi}}}\nsteps:\n  - id: bounded\n    code:\n      lang: python\n      src: 'def main(value): return value'\n      limits: {mem_mb: 64}\nresult: {to: notify}\n";
    let parsed = super::super::parse("memory.yaml", source).unwrap();
    let diagnostics = check_ir_on_platform(&parsed.ir(), &CheckContext::default(), "macos");
    assert!(!has_errors(&diagnostics), "{diagnostics:?}");
    let warnings = diagnostics
        .into_iter()
        .filter(|d| d.message == "mem_mb not enforced on this OS")
        .collect::<Vec<_>>();
    assert_eq!(warnings.len(), 1);
    assert_eq!(warnings[0].severity, "warning");
    assert_eq!(warnings[0].message, "mem_mb not enforced on this OS");
    assert_eq!(warnings[0].field, "steps[0].code.limits.mem_mb");
    assert_eq!(warnings[0].line, 10);
    assert!(!has_errors(&warnings));
    assert!(memory_warnings(&parsed, "linux").is_empty());
    assert!(memory_warnings(&parsed, "windows").is_empty());
    assert!(parsed.document.lower().is_ok());
    assert!(super::super::check("memory.yaml", source, &[]).ok);
}
#[test]
fn blueprint_v2_check_unknown_result_target_is_refused_offline() {
    let mut doc = document(json!([{"id":"a","run":{"lit":"echo a"}}]));
    doc.result.to = Some(blueprint_document::ResultTarget::Fixed("nowhere".into()));
    let ds = inspect(&doc);
    let found = ds
        .iter()
        .find(|d| d.field == "result.to")
        .unwrap_or_else(|| panic!("{ds:#?}"));
    assert_eq!(found.severity, "error");
    assert!(found.message.contains("nowhere"));
}
#[test]
fn blueprint_v2_check_folds_rule_findings_under_document_errors() {
    let at = || blueprint_document::Location {
        file: "fixture.yaml".into(),
        line: 2,
        column: 1,
    };
    let mut ds = vec![
        Diagnostic::error(at(), "steps[0].every", "Malformed duration", "h"),
        Diagnostic::error(at(), "steps[1].run", "other", "h"),
    ];
    ds[1].rule = "R6".into();
    let mut duplicate = ds[0].clone();
    duplicate.rule = "R12".into();
    ds.push(duplicate);
    fold_into_document_errors(&mut ds);
    assert_eq!(ds.len(), 2, "{ds:#?}");
    let root = ds.iter().find(|d| d.rule == "IR").unwrap();
    assert_eq!(root.folded.len(), 1);
    assert_eq!(root.folded[0].rule, "R12");
}
#[test]
fn blueprint_v2_check_render_omits_unknown_locations() {
    let doc = document(json!([{"id":"a","run":{"input":"command"}}]));
    let ds = check_document(&doc, &CheckContext::default());
    let rendered = render(&ds, true);
    assert!(!rendered.contains(":1:1:"), "{rendered}");
    assert!(rendered.contains("steps[0]"), "{rendered}");
}
