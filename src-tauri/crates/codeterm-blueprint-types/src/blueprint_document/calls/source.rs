use super::*;
use std::path::{Component, Path};

pub(super) fn freeze(parsed: &mut Parsed, author: &Path) -> Result<usize, Vec<Diagnostic>> {
    let mut added = 0usize;
    let mut freeze_node = |node: &mut crate::blueprint_document::Node,
                           field: String|
     -> Result<(), (String, String)> {
        let Some(code) = node.code.as_mut() else {
            return Ok(());
        };
        let Some(relative) = code.file.as_ref() else {
            return Ok(());
        };
        if relative.contains('\\') {
            return Err((
                field,
                format!("Code file `{relative}` uses backslashes; write it with forward slashes"),
            ));
        }
        if code.src.is_some()
            || Path::new(relative)
                .components()
                .any(|part| !matches!(part, Component::Normal(_)))
        {
            return Err((
                field,
                "Code files require one relative path without traversal".into(),
            ));
        }
        let path = author.parent().unwrap_or(Path::new(".")).join(relative);
        let source = super::imports::read(&path).map_err(|e| (field.clone(), e))?;
        if source.len() > 16384 {
            return Err((field, "Code source exceeds 16 KiB".into()));
        }
        added = added.saturating_add(source.len());
        if added > MAX_CLOSURE_BYTES {
            return Err((field, "budget_exhausted: code source closure".into()));
        }
        code.file = None;
        code.src = Some(source);
        Ok(())
    };
    let result = (|| {
        for (i, node) in parsed.document.steps.iter_mut().enumerate() {
            freeze_node(node, format!("steps[{i}].code.file"))?;
        }
        for (i, node) in parsed.document.other.iter_mut().enumerate() {
            freeze_node(node, format!("other[{i}].code.file"))?;
        }
        for (name, sub) in &mut parsed.document.blueprints {
            for (i, node) in sub.steps.iter_mut().enumerate() {
                freeze_node(node, format!("blueprints.{name}.steps[{i}].code.file"))?;
            }
            for (i, node) in sub.other.iter_mut().enumerate() {
                freeze_node(node, format!("blueprints.{name}.other[{i}].code.file"))?;
            }
        }
        Ok::<_, (String, String)>(())
    })();
    result.map_err(|(field, error)| vec![finding(parsed, &field, error)])?;
    Ok(added)
}
pub fn has_files(document: &Document) -> bool {
    document
        .nodes()
        .chain(
            document
                .blueprints
                .values()
                .flat_map(|sub| sub.steps.iter().chain(&sub.other)),
        )
        .any(|node| node.code.as_ref().is_some_and(|code| code.file.is_some()))
}
