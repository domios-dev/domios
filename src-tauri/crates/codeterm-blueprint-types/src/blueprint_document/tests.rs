use super::*;
use serde_json::json;
const ECHO: &str = include_str!("../../../../../resources/blueprints/echo.yaml");
#[test]
fn command_stdin_bind_is_data_but_run_bind_is_positioned_validation() {
    let raw = "schema_version: 2\nkind: pipeline\nname: command-stdin\ntrigger: {type: schedule, every: {lit: 20s}}\nsteps:\n- id: prior\n  run: {lit: echo ready}\n  out: {type: string}\n- id: consume\n  run: {lit: echo fixed}\n  in: {type: string}\n  bind: {in: prior.out}\nresult: {to: notify}\n";
    let checked = check("stdin.yaml", raw, &[]);
    assert!(checked.ok, "{:?}", checked.diagnostics);
    let runtime = parse("stdin.yaml", raw).unwrap().document.lower().unwrap();
    assert_eq!(
        runtime["spec"]["steps"][1]["execution"]["input_binding"],
        "prior.out"
    );
    let invalid = raw.replace("run: {lit: echo fixed}", "run: {bind: prior.out}");
    let checked = check("stdin.yaml", &invalid, &[]);
    assert!(!checked.ok);
    assert!(checked.diagnostics.iter().any(|d| d.field == "steps[1].run"
        && d.file == "stdin.yaml"
        && d.line == 10
        && !d.hint.is_empty()));
}
#[test]
fn native_v2_roundtrip_identity_and_positions() {
    let raw="schema_version: 2\nkind: pipeline\nname: typed-example\nintent: grade a probe\nanchors: [health]\ntrigger: {type: schedule, every: {lit: 20s}}\ntypes: {Count: {type: integer}}\nsteps:\n- id: measure\n  code: {lang: python, src: 'def main(p): return 1', caps: [], limits: {wall_ms: 1000}}\n  out: {$ref: Count}\n  routes: {done: child}\n- id: child\n  call: {ref: helper, with: {count: {bind: measure.out}}}\n  in: {$ref: Count}\n  bind: {in: measure.out}\nother:\n- id: alert\n  uses: notify\n  with: {text: {bind: child.out}}\nblueprints:\n  helper:\n    inputs: {count: {type: integer}}\n    steps: [{id: return-count, run: {lit: 'echo 1'}}]\n    result: {type: {type: integer}, from: return-count.out}\n";
    let parsed = parse("typed.yaml", raw).unwrap();
    assert_eq!(parsed.at("steps[0].code").line, 10);
    assert!(
        parsed.document.template_errors().is_empty(),
        "{:?}",
        parsed.document.template_errors()
    );
    let yaml = parsed.document.yaml().unwrap();
    let reparsed = parse("formatted.yaml", &yaml).unwrap();
    assert_eq!(parsed.document, reparsed.document);
    assert_eq!(
        projection::identity(&parsed.document),
        projection::identity(&reparsed.document)
    );
    assert!(parsed.document.bind(&[]).is_ok());
    assert!(parsed
        .document
        .lower()
        .unwrap_err()
        .iter()
        .any(|(_, m)| m.contains("call execution")));
}
#[test]
fn v1_and_wire_objects_are_refused() {
    for raw in [
        "schema_version: 1\nname: old\nwatch: {}",
        r#"{"create":{"name":"old"}}"#,
        r#"{"spec":{"name":"old"}}"#,
    ] {
        assert!(!check("old.yaml", raw, &[]).ok);
        assert!(Document::from_definition(&serde_yaml::from_str::<Value>(raw).unwrap()).is_err());
    }
}
#[test]
fn serde_schema_and_value_forms_have_one_contract() {
    let schema = schema::schema();
    assert_eq!(schema["properties"]["schema_version"]["minimum"], 2.0);
    assert_eq!(schema["properties"]["schema_version"]["maximum"], 2.0);
    assert!(schema["properties"].get("watch").is_none());
    let bind = &schema["definitions"]["Node"]["properties"]["bind"];
    assert_eq!(bind["additionalProperties"]["type"], "string");
    assert!(bind["description"]
        .as_str()
        .unwrap()
        .contains("named ports"));
    assert!(serde_yaml::from_str::<ValueForm>("input: a\nlit: b").is_err());
    assert!(serde_yaml::from_str::<ValueForm>("echo '{{message}}'").is_err());
}
#[test]
fn semantic_projections_pin_routes_ports_caps_and_calls() {
    let doc = parse("echo", ECHO).unwrap().document;
    let facts = projection::facts(&doc);
    assert_eq!(
        serde_json::to_value(&facts).unwrap(),
        json!([
            {"predicate":"node","arguments":["echo","filter",1]},
            {"predicate":"route","arguments":["echo","matched","@stop"]},
            {"predicate":"param","arguments":["echo","bucket",{"lit":"matched"}]},
            {"predicate":"param","arguments":["echo","pattern",{"lit":"(?s).*"}]}
        ])
    );
    for field in ["routes", "in", "caps", "call"] {
        let mut value = serde_json::to_value(&doc).unwrap();
        value["steps"][0][field] = match field {
            "routes" => json!({"matched":"@next"}),
            "in" => json!({"type":"integer"}),
            "caps" => json!(["net:example.test"]),
            _ => json!({"ref":"helper","with":{}}),
        };
        let changed: Document = serde_json::from_value(value).unwrap();
        assert_ne!(projection::facts(&doc), projection::facts(&changed));
        assert!(projection::diff(&doc, &changed).changed["echo"].contains(&field.into()));
    }
}
#[test]
fn duplicate_keys_and_bind_in_executable_slot_have_source_diagnostics() {
    let raw = ECHO.replace("id: echo", "id: echo\n  id: duplicate");
    assert!(parse("duplicate.yaml", &raw)
        .unwrap_err()
        .iter()
        .any(|d| d.field == "steps[0].id" && d.line > 1));
    let raw = ECHO
        .replace("uses: filter", "run: {bind: echo.out}")
        .replace("id: echo", "id: command");
    let checked = check("bad.yaml", &raw, &[]);
    assert!(!checked.ok);
    assert!(checked
        .diagnostics
        .iter()
        .any(|d| d.field == "steps[0].run" && d.line > 1));
}
#[test]
fn blueprint_v2_runtime_bundled_recipes_bind_and_lower_without_cli_authority() {
    let root = crate::test_support::fixture_manifest_dir().join("../../../resources/blueprints");
    let voice_path = std::env::temp_dir().join("blueprint-v2-voice.wav");
    let mut count = 0;
    for file in std::fs::read_dir(root).unwrap() {
        let path = file.unwrap().path();
        if path.extension().and_then(|s| s.to_str()) != Some("yaml") {
            continue;
        }
        count += 1;
        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(!raw.contains("domios-cli"));
        let doc = parse(path.to_str().unwrap(), &raw).unwrap().document;
        let assignments = doc
            .inputs
            .iter()
            .filter(|(_, p)| p.default.is_none())
            .map(|(key, p)| {
                let value = match key.as_str() {
                    "name" => "example-watch",
                    "repo" => "owner/repo",
                    "org" => "example",
                    "url" => "https://example.test/health",
                    "id" => "42",
                    "tab_id" => "tab-example",
                    "cron_name_or_id" => "cron-example",
                    "path_to_voice_note" => voice_path.to_str().unwrap(),
                    _ if key.contains("probe") => "echo ready",
                    _ => "ready",
                };
                format!("{key}={}", if p.kind == "integer" { "42" } else { value })
            })
            .collect::<Vec<_>>();
        let checked = check(path.to_str().unwrap(), &raw, &assignments);
        assert!(checked.ok, "{}: {:?}", path.display(), checked.diagnostics);
        let bound = Document::from_definition(&checked.definition.unwrap()).unwrap();
        assert!(
            bound.lower().is_ok(),
            "{}: {:?}",
            path.display(),
            bound.lower()
        );
    }
    assert_eq!(count, 15);
}

#[test]
fn typed_binding_keeps_schema_properties_and_literal_executable_text_opaque() {
    let raw="schema_version: 2\nkind: pipeline\nname: typed-binding\ninputs: {count: {$ref: Count}}\ntypes: {Count: {type: integer}, Payload: {type: object, properties: {input: {type: string}}}}\ntrigger: {type: schedule, every: {lit: 20s}}\nsteps: [{id: emit, run: {lit: \"echo '{{count}}'\"}, with: {count: {input: count}}}]\n";
    let doc = parse("opaque.yaml", raw).unwrap().document;
    assert!(doc.bind(&["count=\"wrong\"".into()]).is_err());
    let bound = doc.bind(&["count=3".into()]).unwrap();
    assert_eq!(bound["steps"][0]["run"]["lit"], "echo '{{count}}'");
    assert_eq!(bound["steps"][0]["with"]["count"]["lit"], 3);
    assert_eq!(
        bound["types"]["Payload"]["properties"]["input"]["type"],
        "string"
    );
    assert!(parse("opaque.yaml", raw)
        .unwrap()
        .ir()
        .locations
        .contains_key("steps[0].run"));
}

#[test]
fn blueprint_document_plugin_node_needs_no_spending_block() {
    let raw = "schema_version: 2\nkind: pipeline\nname: plugin-fixture\ntrigger: {type: schedule, every: {lit: 20s}}\nsteps:\n- id: echo\n  uses: fixture.echo\n  with: {text: {lit: opaque}}\nresult: {to: notify}\n";
    let parsed = parse("plugin.yaml", raw).unwrap();
    assert!(parsed.document.template_errors().is_empty());
    let lowered = parsed.document.lower().unwrap();
    assert_eq!(lowered["spec"]["steps"][0]["kind"], "plugin");
    assert_eq!(lowered["spec"]["steps"][0]["params"]["text"], "opaque");
    let invalid = raw.replace("  uses: fixture.echo", "  uses: fixture.echo\n  plugin: {revision: 1, spending: {unit: credits, run_ceiling: 2, step_allowance: 1}}");
    assert!(parse("plugin.yaml", &invalid).is_err());
}
