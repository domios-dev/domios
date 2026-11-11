use super::*;

// The checked v2 pipeline from the Blueprint document tests, renamed.
const DIGEST: &str = "schema_version: 2\nkind: pipeline\nname: research-digest\ntrigger: {type: schedule, every: {lit: 20s}}\nsteps:\n- id: prior\n  run: {lit: echo ready}\n  out: {type: string}\n- id: consume\n  run: {lit: echo fixed}\n  in: {type: string}\n  bind: {in: prior.out}\nresult: {to: notify}\n";

fn item(content: &str) -> AutomationItem {
    AutomationItem {
        name: "research-digest".into(),
        description: Some("Hourly digest".into()),
        content: content.into(),
    }
}

#[test]
fn package_blueprints_become_import_recipes_by_their_own_name() {
    let recipe = recipe(&item(DIGEST)).expect("a v2 document imports");
    assert_eq!(recipe.schema_version, SCHEMA_VERSION);
    assert_eq!(recipe_name(&recipe).as_deref(), Some("research-digest"));
    let yaml = Document::from_recipe(&recipe).unwrap().yaml().unwrap();
    let again = recipe_from_yaml(&yaml);
    assert_eq!(
        again.definition, recipe.definition,
        "bake exports what start imported"
    );
}

fn recipe_from_yaml(yaml: &str) -> Recipe {
    recipe(&item(yaml)).expect("an exported document imports again")
}

#[test]
fn invalid_package_blueprints_are_refused_with_their_diagnostics() {
    let error = recipe(&item("schema_version: 2\nname: research-digest\n"))
        .expect_err("a document without steps is not a Blueprint");
    assert!(!error.is_empty());
    let duplicate = format!("{DIGEST}name: twice\n");
    assert!(recipe(&item(&duplicate))
        .unwrap_err()
        .contains("Duplicate mapping key"));
}
