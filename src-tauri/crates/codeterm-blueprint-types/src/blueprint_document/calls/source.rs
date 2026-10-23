use super::*;
use std::path::Path;

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
        if code.src.is_some() {
            return Err((field, "Code nodes take either file or src, not both".into()));
        }
        if relative.contains('\\') {
            return Err((
                field,
                format!("Code file `{relative}` uses backslashes; write it with forward slashes"),
            ));
        }
        let directory = author.parent().unwrap_or(Path::new("."));
        let source = crate::code_source::read(directory, relative).map_err(|reason| {
            let detail = match reason {
                "budget_exhausted" => "Code source exceeds 16 KiB",
                _ => "Code files require a regular file inside the Blueprint directory, reached without traversal, symlinks or junctions",
            };
            (field.clone(), format!("{reason}: {relative}: {detail}"))
        })?;
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
