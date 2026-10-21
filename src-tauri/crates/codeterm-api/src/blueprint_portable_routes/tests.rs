use super::*;

#[test]
fn ui_import_accepts_the_same_yaml_and_json_document_as_cli() {
    let source="schema_version: 1\nname: UI imported watch\ninputs:\n  command: {type: string}\nwatch:\n  probe: '{{command}}'\n  question: Ready?\n  buckets: [done]\n  matches: {done: done}\n  timeout: 1h\n";
    let yaml = import_recipe(&json!(source), &["command=echo done".into()]).unwrap();
    let document = json!({"schema_version":1,"name":"UI imported watch","inputs":{"command":{"type":"string"}},"watch":{"probe":"{{command}}","question":"Ready?","buckets":["done"],"matches":{"done":"done"},"timeout":"1h"}});
    let json = import_recipe(
        &serde_json::to_value(document).unwrap(),
        &["command=echo done".into()],
    )
    .unwrap();
    assert_eq!(yaml.definition, json.definition);
    assert_eq!(yaml.definition["create"]["probe"], "echo done");
    assert!(import_recipe(&json!(source), &[])
        .unwrap_err()
        .contains("import.yaml:4:3"));
}
