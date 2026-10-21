use super::*;
use serde_json::json;

#[test]
fn blueprint_recipe_refuses_unknown_schema_and_missing_placeholders() {
    let mut recipe = Recipe {
        schema_version: 2,
        placeholders: BTreeMap::from([
            ("tab".into(), "Destination tab".into()),
            ("path".into(), "Probe path".into()),
        ]),
        definition: json!({"create":{"observe":"{{tab}}", "probe":"{{path}}"}}),
    };
    assert!(recipe.bind(&[]).unwrap_err().contains("schema version"));
    recipe.schema_version = SCHEMA_VERSION;
    assert_eq!(
        recipe.bind(&[]).unwrap_err(),
        "missing recipe placeholders: path, tab"
    );
    assert_eq!(
        recipe
            .bind(&["tab=other".into(), "path=/scratch".into()])
            .unwrap(),
        json!({"create":{"observe":"other", "probe":"/scratch"}})
    );
}

#[test]
fn blueprint_recipe_bind_is_simultaneous_and_refuses_extra_fields() {
    let mut value = json!({"probe":"{{a}} {{b}}"});
    substitute(&mut value, &["a=$b".into(), "b=ready".into()]).unwrap();
    assert_eq!(value["probe"], "$b ready");
    assert!(Recipe::parse(
        r#"{"schema_version":1,"placeholders":{},"definition":{},"runtime":{}}"#
    )
    .is_err());
}

#[test]
fn portable_source_recipe_binding_keeps_kind_and_customized_knobs() {
    for (kind, probe, assignments) in [
        (
            "github",
            serde_json::json!({"repo":"owner/project","resource":"runners","label":"{{label}}","timeout_ms":"{{timeout}}"}),
            vec!["label=linux-build".into(), "timeout=1000".into()],
        ),
        (
            "http",
            serde_json::json!({"url":"{{url}}","expected_status":"204","timeout_ms":2000}),
            vec!["url=https://example.com/health".into()],
        ),
    ] {
        let definition = serde_json::json!({"spec":{"name":"Custom source","trigger":{"type":"schedule","every":"20s"},"steps":[{"name":"check","input":"text","kind":{"kind":kind,"probe":probe}}]}});
        let names = placeholder_names(&definition).unwrap();
        let recipe = Recipe {
            schema_version: SCHEMA_VERSION,
            definition,
            placeholders: names
                .into_iter()
                .map(|name| (name, "Custom input".into()))
                .collect(),
        };
        let restored = Recipe::parse(&serde_json::to_string(&recipe).unwrap()).unwrap();
        let bound = restored.bind(&assignments).unwrap();
        let parsed: crate::blueprint_pipeline::BlueprintSpec =
            serde_json::from_value(bound["spec"].clone()).unwrap();
        assert_eq!(parsed.steps[0].kind.name(), kind);
        assert!(crate::blueprint_pipeline::validate(&parsed, &[]).is_ok());
    }
}
