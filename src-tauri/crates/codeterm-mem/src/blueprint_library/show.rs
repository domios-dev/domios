use super::*;
use codeterm_blueprint_types::blueprint_document::calls::{
    self, projection::ExpandedGraph, CallPin,
};
pub(crate) fn project(
    mut document: Document,
    mut resolve: impl FnMut(&CallPin) -> Result<Document, String>,
) -> Result<ExpandedGraph, String> {
    let dependencies = std::mem::take(&mut document.dependencies);
    if serde_json::to_vec(&document)
        .map_err(|e| e.to_string())?
        .len()
        > calls::MAX_CLOSURE_BYTES
    {
        return Err("Saved solution exceeds 256 KiB".into());
    }
    calls::projection::graph_resolved(&document, |pin| {
        if !calls::saved_reference(&pin.id) {
            let callee = dependencies.get(&pin.key()).ok_or("Missing local pin")?;
            if callee.pin() != *pin {
                return Err("Local pin identity mismatch".into());
            }
            callee.verify()?;
            return Ok((*callee.document).clone());
        }
        resolve(pin)
    })
}
impl MemManager {
    pub fn blueprint_solution_show(&self, area: &str, id: &str) -> Result<ExpandedGraph, String> {
        self.store.blueprint_solution_show(area, id)
    }
}
#[cfg(test)]
mod tests;
