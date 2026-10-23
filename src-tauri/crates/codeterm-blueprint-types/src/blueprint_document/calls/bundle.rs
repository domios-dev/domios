use super::*;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalCallee {
    pub id: String,
    pub source_label: String,
    pub document: Document,
}
#[derive(Clone, Debug)]
pub struct LocalBundle {
    pub document: Document,
    pub local_callees: Vec<LocalCallee>,
}
pub fn collect_local(file: &str, source: &str) -> Result<LocalBundle, Vec<Diagnostic>> {
    let mut root = crate::blueprint_document::parse(file, source)?;
    if !root.document.dependencies.is_empty()
        && root
            .document
            .nodes()
            .filter_map(|node| node.call.as_ref())
            .all(|call| call.pin.is_some())
    {
        FrozenClosure::from_document(root.document.clone())
            .map_err(|e| vec![finding(&root, "dependencies", e)])?;
        return Ok(LocalBundle {
            document: root.document,
            local_callees: Vec::new(),
        });
    }
    let path = std::fs::canonicalize(file).unwrap_or_else(|_| PathBuf::from(file));
    let mut collector = Collector {
        seen: BTreeSet::from([path.clone()]),
        stack: vec![path.clone()],
        bytes: source.len(),
        callees: Vec::new(),
    };
    collector.graph(&mut root, &path)?;
    Ok(LocalBundle {
        document: root.document,
        local_callees: collector.callees,
    })
}
struct Collector {
    seen: BTreeSet<PathBuf>,
    stack: Vec<PathBuf>,
    bytes: usize,
    callees: Vec<LocalCallee>,
}
impl Collector {
    fn graph(&mut self, parsed: &mut Parsed, author: &Path) -> Result<(), Vec<Diagnostic>> {
        self.bytes = self
            .bytes
            .saturating_add(super::source::freeze(parsed, author)?);
        if self.bytes > MAX_CLOSURE_BYTES {
            return Err(vec![finding(
                parsed,
                "steps",
                "budget_exhausted: code source closure",
            )]);
        }
        for (alias, target) in parsed.document.imports.clone() {
            let field = format!("imports.{alias}");
            if parsed.document.blueprints.contains_key(&alias) {
                return Err(vec![finding(
                    parsed,
                    &field,
                    "Duplicate inline/import name",
                )]);
            }
            if saved_reference(&target) {
                continue;
            }
            if target.contains(':') || Path::new(&target).is_absolute() {
                return Err(vec![finding(parsed,&field,"Only relative local imports and scoped mem references are supported; repo trust is unavailable")]);
            }
            let path =
                std::fs::canonicalize(author.parent().unwrap_or(Path::new(".")).join(&target))
                    .map_err(|e| {
                        vec![finding(
                            parsed,
                            &field,
                            format!("Missing import {target}: {e}"),
                        )]
                    })?;
            if self.stack.contains(&path) {
                return Err(vec![finding(parsed, &field, "Cyclic import")]);
            }
            if !self.seen.insert(path.clone()) {
                return Err(vec![finding(
                    parsed,
                    &field,
                    "Duplicate/diamond import identity",
                )]);
            }
            let source =
                super::imports::read(&path).map_err(|e| vec![finding(parsed, &field, e)])?;
            self.bytes = self.bytes.saturating_add(source.len());
            if self.bytes > MAX_CLOSURE_BYTES || self.stack.len() > MAX_CALL_DEPTH {
                return Err(vec![finding(
                    parsed,
                    &field,
                    "budget_exhausted: import closure bound",
                )]);
            }
            let label = path.to_string_lossy().into_owned();
            let mut child = crate::blueprint_document::parse(&label, &source)?;
            self.stack.push(path.clone());
            self.graph(&mut child, &path)?;
            self.stack.pop();
            let id = format!("local:{}", super::imports::digest(label.as_bytes()));
            parsed.document.imports.insert(alias, id.clone());
            self.callees.push(LocalCallee {
                id,
                source_label: label,
                document: child.document,
            });
        }
        Ok(())
    }
}
pub fn resolve_bundle(
    document: Document,
    local_callees: &[LocalCallee],
    memory: &dyn MemoryResolver,
) -> Result<FrozenClosure, Vec<Diagnostic>> {
    let parsed = Parsed {
        document,
        locations: BTreeMap::new(),
    };
    let mut templates = BTreeMap::new();
    for entry in local_callees {
        if !entry.id.starts_with("local:")
            || entry.id.len() > 256
            || entry.source_label.len() > 4096
            || templates.insert(entry.id.clone(), entry.clone()).is_some()
        {
            return Err(vec![finding(
                &parsed,
                "local_callees",
                "Invalid or duplicate local callee identity",
            )]);
        }
    }
    if serde_json::to_vec(&(local_callees, &parsed.document))
        .map_or(true, |v| v.len() > MAX_CLOSURE_BYTES)
    {
        return Err(vec![finding(
            &parsed,
            "local_callees",
            "Import closure exceeds 256 KiB",
        )]);
    }
    super::imports::resolve_templates(parsed, templates, memory)
}
#[cfg(test)]
mod tests;
