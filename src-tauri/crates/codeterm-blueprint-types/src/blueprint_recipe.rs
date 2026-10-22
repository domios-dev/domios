use crate::blueprint_document::Document;
use serde::{Deserialize, Serialize};
use serde_json::Value;
pub const SCHEMA_VERSION: u32 = 2;
pub const MAX_RECIPE_BYTES: usize = 256 * 1024;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Recipe {
    pub schema_version: u32,
    pub definition: Value,
}
impl Recipe {
    pub fn parse(raw: &str) -> Result<Self, String> {
        if raw.len() > MAX_RECIPE_BYTES {
            return Err("Blueprint recipe exceeds 256 KiB".into());
        }
        let recipe: Self = serde_json::from_str(raw).map_err(|e| e.to_string())?;
        recipe.validate()?;
        Ok(recipe)
    }
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != SCHEMA_VERSION {
            return Err("Only Blueprint schema_version: 2 is supported".into());
        }
        if serde_json::to_vec(self).map_err(|e| e.to_string())?.len() > MAX_RECIPE_BYTES {
            return Err("Blueprint recipe exceeds 256 KiB".into());
        }
        Document::from_definition(&self.definition).map(|_| ())
    }
    pub fn bind(&self, assignments: &[String]) -> Result<Value, String> {
        self.validate()?;
        Document::from_definition(&self.definition)?
            .bind(assignments)
            .map_err(|e| format!("{e:?}"))
    }
}
#[cfg(test)]
mod tests;
