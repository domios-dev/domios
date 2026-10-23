use super::path_identity::portable_path;
use super::*;
use crate::blueprint_document::{parse, Kind, SubBlueprint};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

pub(super) fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
pub(super) fn read(author: &Path, target: &str) -> Result<String, String> {
    let directory = author.parent().unwrap_or(Path::new("."));
    let target = Path::new(target)
        .components()
        .filter(|part| !matches!(part, std::path::Component::CurDir))
        .collect::<PathBuf>();
    let target = target
        .to_str()
        .ok_or("contract_violation: non-UTF-8 import path")?;
    crate::code_source::read_bounded(directory, target, MAX_CLOSURE_BYTES).map_err(|reason| {
        let detail = match reason {
            "budget_exhausted" => "Import exceeds 256 KiB",
            _ => "Imports require a regular file inside the importing Blueprint's directory, reached without traversal, symlinks or junctions",
        };
        format!("{reason}: {detail}")
    })
}
struct LocalOnly;
impl MemoryResolver for LocalOnly {
    fn head(&self, _: &str) -> Result<Document, String> {
        Err("Scoped mem resolution requires the host".into())
    }
    fn revision(&self, _: &CallPin) -> Result<Document, String> {
        Err("Scoped mem resolution requires the host".into())
    }
}
pub fn resolve_local(file: &str, source: &str) -> Result<FrozenClosure, Vec<Diagnostic>> {
    resolve(file, source, &LocalOnly)
}
pub fn resolve(
    file: &str,
    source: &str,
    memory: &dyn MemoryResolver,
) -> Result<FrozenClosure, Vec<Diagnostic>> {
    let parsed = parse(file, source)?;
    let root_path = PathBuf::from(file);
    let mut resolver = Resolver {
        memory,
        callees: parsed.document.dependencies.clone(),
        imported: BTreeSet::new(),
        stack: Vec::new(),
        total_bytes: source.len(),
        local_files: true,
        templates: BTreeMap::new(),
    };
    let root_id = portable_path(&std::fs::canonicalize(&root_path).unwrap_or(root_path.clone()));
    resolver.stack.push(format!("file:{root_id}"));
    let root = resolver.graph(parsed, &root_path, None)?;
    let closure = FrozenClosure {
        root,
        callees: resolver.callees,
    };
    closure
        .verify()
        .map_err(|e| vec![finding(&closure.root, "dependencies", e)])?;
    Ok(closure)
}
pub fn resolve_saved(
    document: Document,
    memory: &dyn MemoryResolver,
) -> Result<FrozenClosure, Vec<Diagnostic>> {
    let mut resolver = Resolver {
        memory,
        callees: document.dependencies.clone(),
        imported: BTreeSet::new(),
        stack: Vec::new(),
        total_bytes: 0,
        local_files: false,
        templates: BTreeMap::new(),
    };
    let root = resolver.graph(
        Parsed {
            document,
            locations: BTreeMap::new(),
        },
        Path::new("saved"),
        None,
    )?;
    let closure = FrozenClosure {
        root,
        callees: resolver.callees,
    };
    closure
        .verify()
        .and_then(|_| closure.drift(memory).map(|_| ()))
        .map_err(|e| vec![finding(&closure.root, "dependencies", e)])?;
    Ok(closure)
}
pub(super) fn resolve_templates(
    parsed: Parsed,
    templates: BTreeMap<String, LocalCallee>,
    memory: &dyn MemoryResolver,
) -> Result<FrozenClosure, Vec<Diagnostic>> {
    let mut resolver = Resolver {
        memory,
        callees: parsed.document.dependencies.clone(),
        imported: BTreeSet::new(),
        stack: Vec::new(),
        total_bytes: 0,
        local_files: false,
        templates,
    };
    let root = resolver.graph(parsed, Path::new("bundle"), None)?;
    let closure = FrozenClosure {
        root,
        callees: resolver.callees,
    };
    closure
        .verify()
        .and_then(|_| closure.drift(memory).map(|_| ()))
        .map_err(|e| vec![finding(&closure.root, "dependencies", e)])?;
    Ok(closure)
}
struct Resolver<'a> {
    memory: &'a dyn MemoryResolver,
    callees: BTreeMap<String, FrozenCallee>,
    imported: BTreeSet<String>,
    stack: Vec<String>,
    total_bytes: usize,
    local_files: bool,
    templates: BTreeMap<String, LocalCallee>,
}
impl Resolver<'_> {
    fn graph(
        &mut self,
        mut parsed: Parsed,
        path: &Path,
        inherited: Option<&BTreeMap<String, SubBlueprint>>,
    ) -> Result<Parsed, Vec<Diagnostic>> {
        if self.local_files {
            self.total_bytes = self
                .total_bytes
                .saturating_add(super::source::freeze(&mut parsed, path)?);
            if self.total_bytes > MAX_CLOSURE_BYTES {
                return Err(vec![finding(
                    &parsed,
                    "steps",
                    "budget_exhausted: code source closure",
                )]);
            }
        }
        let definitions = if parsed.document.blueprints.is_empty() {
            inherited.cloned().unwrap_or_default()
        } else {
            parsed.document.blueprints.clone()
        };
        let mut aliases = BTreeMap::new();
        for (alias, target) in parsed.document.imports.clone() {
            if definitions.contains_key(&alias) {
                return Err(vec![finding(
                    &parsed,
                    &format!("imports.{alias}"),
                    "Duplicate inline/import name",
                )]);
            }
            if !self.local_files
                && !self.templates.contains_key(&target)
                && !parsed.document.nodes().any(|n| {
                    n.call
                        .as_ref()
                        .is_some_and(|call| call.reference == alias && call.pin.is_none())
                })
            {
                continue;
            }
            let callee = if saved_reference(&target) {
                self.mem(&target, None, &parsed, &format!("imports.{alias}"))?
            } else {
                self.local(&target, path, &parsed, &format!("imports.{alias}"))?
            };
            aliases.insert(alias, callee);
        }
        let mut nodes = std::mem::take(&mut parsed.document.steps);
        let mut other = std::mem::take(&mut parsed.document.other);
        let steps_len = nodes.len();
        for (index, node) in nodes.iter_mut().chain(&mut other).enumerate() {
            let field = if index < steps_len {
                format!("steps[{index}].call")
            } else {
                format!("other[{}].call", index - steps_len)
            };
            let Some(call) = node.call.as_mut() else {
                continue;
            };
            let callee = if let Some(pin) = &call.pin {
                let target = parsed
                    .document
                    .imports
                    .get(&call.reference)
                    .map(String::as_str)
                    .unwrap_or(&call.reference);
                let matches = if saved_reference(target) || target.starts_with("local:") {
                    target == pin.id
                } else if definitions.contains_key(&call.reference) {
                    pin.id.starts_with("inline:")
                        && pin.id.ends_with(&format!("#{}", call.reference))
                        && self.callees.get(&pin.key()).is_some_and(|callee| {
                            let mut expected =
                                inline(&call.reference, &definitions[&call.reference]);
                            let mut actual = *callee.document.clone();
                            for document in [&mut expected, &mut actual] {
                                for node in document.steps.iter_mut().chain(&mut document.other) {
                                    if let Some(call) = &mut node.call {
                                        call.pin = None;
                                    }
                                }
                            }
                            expected == actual
                        })
                } else if parsed.document.imports.contains_key(&call.reference) {
                    let candidate = path.parent().unwrap_or(Path::new(".")).join(target);
                    if self.local_files {
                        std::fs::canonicalize(candidate).is_ok_and(|p| {
                            pin.id
                                .strip_prefix("file:")
                                .is_some_and(|id| portable_path(Path::new(id)) == portable_path(&p))
                        })
                    } else {
                        pin.id.strip_prefix("file:").is_some_and(|p| {
                            Path::new(&portable_path(Path::new(p)))
                                .ends_with(portable_path(Path::new(target)))
                        })
                    }
                } else {
                    false
                };
                if !matches {
                    return Err(vec![finding(
                        &parsed,
                        &field,
                        "Call ref does not match its pinned identity",
                    )]);
                }

                if saved_reference(&pin.id) {
                    self.mem(&pin.id, Some(pin), &parsed, &field)?
                } else {
                    let frozen = self
                        .callees
                        .get(&pin.key())
                        .cloned()
                        .ok_or_else(|| vec![finding(&parsed, &field, "Missing pinned hash")])?;
                    if frozen.id != pin.id {
                        return Err(vec![finding(&parsed, &field, "Pinned identity mismatch")]);
                    }
                    frozen
                        .verify()
                        .map_err(|e| vec![finding(&parsed, &field, e)])?;
                    frozen
                }
            } else if let Some(frozen) = aliases.get(&call.reference) {
                frozen.clone()
            } else if let Some(definition) = definitions.get(&call.reference) {
                let identity = format!("inline:{}#{}", portable_path(path), call.reference);
                if self.stack.contains(&identity) {
                    return Err(vec![finding(&parsed, &field, "Recursive inline call")]);
                }
                self.stack.push(identity.clone());
                let document = inline(&call.reference, definition);
                let prefix = format!("blueprints.{}.", call.reference);
                let locations = parsed
                    .locations
                    .iter()
                    .filter_map(|(k, v)| k.strip_prefix(&prefix).map(|k| (k.to_owned(), v.clone())))
                    .collect();
                let resolved = self.graph(
                    Parsed {
                        document,
                        locations,
                    },
                    path,
                    Some(&definitions),
                )?;
                self.stack.pop();
                self.snapshot(identity, resolved, None)?
            } else if saved_reference(&call.reference) {
                self.mem(&call.reference, None, &parsed, &field)?
            } else {
                return Err(vec![finding(
                    &parsed,
                    &format!("{field}.ref"),
                    format!("Unresolved call {}", call.reference),
                )]);
            };
            call.pin = Some(callee.pin());
            self.insert(callee, &parsed, &field)?;
        }
        parsed.document.steps = nodes;
        parsed.document.other = other;
        parsed.document.dependencies.clear();
        Ok(parsed)
    }
    fn insert(
        &mut self,
        callee: FrozenCallee,
        parsed: &Parsed,
        field: &str,
    ) -> Result<(), Vec<Diagnostic>> {
        if self
            .callees
            .get(&callee.pin().key())
            .is_some_and(|prior| prior.document != callee.document || prior.id != callee.id)
        {
            return Err(vec![finding(
                parsed,
                field,
                "Conflicting dependency identity for content hash",
            )]);
        }
        self.callees.insert(callee.pin().key(), callee);
        Ok(())
    }
    fn snapshot(
        &mut self,
        id: String,
        parsed: Parsed,
        source_hash: Option<String>,
    ) -> Result<FrozenCallee, Vec<Diagnostic>> {
        let hash = document_projection::identity(&parsed.document)
            .map_err(|e| vec![finding(&parsed, "", e)])?;
        let frozen = FrozenCallee {
            id: id.clone(),
            hash: hash.clone(),
            document: Box::new(parsed.document.clone()),
            locations: parsed
                .locations
                .iter()
                .map(|(field, location)| {
                    let mut location = location.clone();
                    location.file = if id.starts_with("local:") || saved_reference(&id) {
                        id.clone()
                    } else {
                        format!("callee:{}", &hash[..12])
                    };
                    (field.clone(), location)
                })
                .collect(),
            source_hash,
        };
        frozen
            .verify()
            .map_err(|e| vec![finding(&parsed, "result", e)])?;
        self.memory
            .retain(&frozen)
            .map_err(|e| vec![finding(&parsed, "dependencies", e)])?;
        self.insert(frozen.clone(), &parsed, "")?;
        Ok(frozen)
    }
    fn local(
        &mut self,
        target: &str,
        author: &Path,
        parsed: &Parsed,
        field: &str,
    ) -> Result<FrozenCallee, Vec<Diagnostic>> {
        if target.starts_with("local:") {
            let entry = self.templates.get(target).cloned().ok_or_else(|| {
                vec![finding(
                    parsed,
                    field,
                    "Missing local callee document bytes",
                )]
            })?;
            if self.stack.iter().any(|id| id == target) || !self.imported.insert(target.into()) {
                return Err(vec![finding(
                    parsed,
                    field,
                    "Cyclic or duplicate/diamond local dependency",
                )]);
            }
            if self.stack.len() >= MAX_CALL_DEPTH {
                return Err(vec![finding(
                    parsed,
                    field,
                    "budget_exhausted: import closure depth",
                )]);
            }
            self.stack.push(target.into());
            let source = entry
                .document
                .yaml()
                .map_err(|e| vec![finding(parsed, field, e)])?;
            let child = crate::blueprint_document::parse(&entry.source_label, &source)?;
            let child = self.graph(child, Path::new(target), None)?;
            self.stack.pop();
            return self.snapshot(target.into(), child, None);
        }
        if !self.local_files {
            return Err(vec![finding(
                parsed,
                field,
                "Local imports must be frozen from their authoring file before save/apply",
            )]);
        }
        if target.contains("://")
            || target.starts_with("repo:")
            || target.starts_with("package:")
            || Path::new(target).is_absolute()
        {
            return Err(vec![finding(parsed,field,"Only relative local imports and scoped mem references are supported; repo trust is unavailable")]);
        }
        let candidate = author.parent().unwrap_or(Path::new(".")).join(target);
        let path = std::fs::canonicalize(&candidate).map_err(|e| {
            vec![finding(
                parsed,
                field,
                format!("Missing import {}: {e}", candidate.display()),
            )]
        })?;
        let source = read(author, target).map_err(|e| vec![finding(parsed, field, e)])?;
        let id = format!("file:{}", portable_path(&path));
        if self.stack.contains(&id) {
            return Err(vec![finding(parsed, field, "Cyclic import")]);
        }
        if !self.imported.insert(id.clone()) {
            return Err(vec![finding(
                parsed,
                field,
                "Duplicate/diamond import identity",
            )]);
        }
        self.total_bytes = self.total_bytes.saturating_add(source.len());
        if self.total_bytes > MAX_CLOSURE_BYTES || self.stack.len() > MAX_CALL_DEPTH {
            return Err(vec![finding(
                parsed,
                field,
                "budget_exhausted: import closure bound",
            )]);
        }
        self.stack.push(id.clone());
        let child = parse(&path.to_string_lossy(), &source)?;
        let child = self.graph(child, &path, None)?;
        self.stack.pop();
        self.snapshot(id, child, Some(digest(source.as_bytes())))
    }
    fn mem(
        &mut self,
        id: &str,
        pin: Option<&CallPin>,
        parsed: &Parsed,
        field: &str,
    ) -> Result<FrozenCallee, Vec<Diagnostic>> {
        if self.stack.iter().any(|entry| entry == id) {
            return Err(vec![finding(parsed, field, "Recursive mem call")]);
        }
        if self.stack.len() > MAX_CALL_DEPTH {
            return Err(vec![finding(
                parsed,
                field,
                "budget_exhausted: call closure depth",
            )]);
        }
        let mut document = if let Some(pin) = pin {
            self.memory.revision(pin)
        } else {
            self.memory.head(id)
        }
        .map_err(|e| vec![finding(parsed, field, e)])?;
        let dependencies = std::mem::take(&mut document.dependencies);
        for dependency in dependencies.into_values() {
            self.insert(dependency, parsed, field)?;
        }
        if let Some(pin) = pin {
            if document_projection::identity(&document)
                .map_err(|e| vec![finding(parsed, field, e)])?
                != pin.hash
            {
                return Err(vec![finding(
                    parsed,
                    field,
                    "Missing or corrupted pinned revision",
                )]);
            }
            return self.snapshot(
                id.into(),
                Parsed {
                    document,
                    locations: BTreeMap::new(),
                },
                None,
            );
        }
        if !document.imports.is_empty()
            && document
                .nodes()
                .any(|node| node.call.as_ref().is_some_and(|c| c.pin.is_none()))
        {
            return Err(vec![finding(
                parsed,
                field,
                "Saved imports must already have an immutable closure",
            )]);
        }
        self.stack.push(id.into());
        let resolved = self.graph(
            Parsed {
                document,
                locations: BTreeMap::new(),
            },
            Path::new(id),
            None,
        )?;
        self.stack.pop();
        self.snapshot(id.into(), resolved, None)
    }
}
fn inline(name: &str, sub: &SubBlueprint) -> Document {
    Document {
        schema_version: 2,
        kind: Kind::Pipeline,
        name: name.into(),
        note: sub.note.clone(),
        intent: sub.intent.clone(),
        anchors: sub.anchors.clone(),
        owner: None,
        inputs: sub.inputs.clone(),
        types: sub.types.clone(),
        imports: BTreeMap::new(),
        dependencies: BTreeMap::new(),
        trigger: None,
        steps: sub.steps.clone(),
        other: sub.other.clone(),
        result: sub.result.clone(),
        blueprints: BTreeMap::new(),
    }
}
pub fn verify_local_sources(closure: &FrozenClosure) -> Result<(), String> {
    for callee in closure.callees.values() {
        if let (Some(path), Some(expected)) =
            (callee.id.strip_prefix("file:"), callee.source_hash.as_ref())
        {
            let path = Path::new(path);
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("");
            if digest(read(path, name)?.as_bytes()) != *expected {
                return Err(format!(
                    "Import identity changed after validation: {}",
                    callee.id
                ));
            }
        }
    }
    Ok(())
}
