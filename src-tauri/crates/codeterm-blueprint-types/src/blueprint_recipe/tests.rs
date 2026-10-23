use super::*;
#[test]
fn only_native_v2_recipe_is_accepted() {
    assert!(Recipe::parse(r#"{"schema_version":1,"definition":{"create":{}}}"#).is_err());
    let document = crate::blueprint_document::parse(
        "echo",
        include_str!("../../../../../resources/blueprints/echo.yaml"),
    )
    .unwrap()
    .document;
    let recipe = Recipe {
        schema_version: 2,
        definition: serde_json::to_value(document).unwrap(),
    };
    assert!(recipe.validate().is_ok());
    assert!(recipe.bind(&[]).is_ok());
}
