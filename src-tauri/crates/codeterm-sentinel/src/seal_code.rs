use codeterm_blueprint_types::blueprint_document::{Code, Document};
use std::path::Path;

pub fn code(code: &mut Code, directory: &Path) -> Result<(), String> {
    if let Some(file) = &code.file {
        if code.src.is_some() {
            return Err("Choose exactly one src or file".into());
        }
        let source = read(directory, file)?;
        code.src = Some(source);
        code.file = None;
    }
    if !codeterm_blueprint_types::blueprint_document::code_contract::has_main(code) {
        return Err("Declare main(value)".into());
    }
    Ok(())
}

pub fn document(document: &mut Document, directory: &Path) -> Result<(), String> {
    for node in document.steps.iter_mut().chain(&mut document.other).chain(
        document
            .blueprints
            .values_mut()
            .flat_map(|sub| sub.steps.iter_mut().chain(&mut sub.other)),
    ) {
        if let Some(body) = &mut node.code {
            code(body, directory).map_err(|error| format!("node {}.code: {error}", node.id))?;
        }
    }
    Ok(())
}

fn read(directory: &Path, file: &str) -> Result<String, String> {
    crate::code_source::read(directory, file).map_err(str::to_owned)
}
