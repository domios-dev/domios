use super::*;
#[test]
fn ui_import_accepts_the_same_yaml_and_json_document_as_cli() {
    let source = include_str!("../../../../../resources/blueprints/all-finished.yaml");
    let assignments = vec![
        "name=ui-watch".into(),
        "one_verdict_probe=echo alldone".into(),
        "all_terminal_pattern=alldone".into(),
    ];
    let yaml = import_recipe(&json!(source), &assignments).unwrap();
    let document = serde_json::to_value(
        codeterm_blueprint_types::blueprint_document::parse("ui.yaml", source)
            .unwrap()
            .document,
    )
    .unwrap();
    let json = import_recipe(&document, &assignments).unwrap();
    assert_eq!(yaml.definition, json.definition);
    assert_eq!(
        yaml.definition["steps"][0]["with"]["command"]["lit"],
        "echo alldone"
    );
    assert!(import_recipe(&json!(source), &[])
        .unwrap_err()
        .contains("import.yaml:"));
    assert!(import_recipe(&json!({"schema_version":1,"watch":{}}), &[]).is_err());
}

#[test]
fn import_destination_is_explicit_without_requiring_a_team() {
    let body: ImportBody = serde_json::from_value(json!({
        "caller_tab_id":"creator", "recipe":{}, "assignments":[], "scope":"general"
    })).unwrap();
    assert_eq!(body.scope.as_deref(), Some("general"));
}
