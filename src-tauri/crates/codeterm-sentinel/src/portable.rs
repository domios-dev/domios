use super::store::Sentinel;
use codeterm_blueprint_types::{
    blueprint_document::Document,
    blueprint_recipe::{Recipe, SCHEMA_VERSION},
};
use serde_json::Value;
pub fn export(job: &Sentinel) -> Result<Recipe, String> {
    let document = Document::from_definition(&definition(job))?;
    let document = codeterm_blueprint_types::blueprint_memory::portable_document(document);
    if !codeterm_blueprint_types::blueprint_memory::safe_template(&document, "") {
        return Err("Blueprint export contains unsafe literal data".into());
    }
    let recipe = Recipe {
        schema_version: SCHEMA_VERSION,
        definition: serde_json::to_value(document).map_err(|e| e.to_string())?,
    };
    recipe.validate()?;
    Ok(recipe)
}
pub fn definition(job: &Sentinel) -> Value {
    job.document.clone().unwrap_or(Value::Null)
}
#[cfg(test)]
mod tests;
