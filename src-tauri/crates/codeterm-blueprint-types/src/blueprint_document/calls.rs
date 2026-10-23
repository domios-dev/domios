use super::{projection as document_projection, Diagnostic, Document, Location, Parsed};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

mod imports;
pub mod lower;
mod path_identity;
pub mod ports;
pub mod projection;
pub use imports::{resolve, resolve_local, resolve_saved, verify_local_sources};
mod analysis;
pub use analysis::{arguments, check_facts, CallFinding, VerifiedCall};
#[cfg(test)]
mod tests;

pub const MAX_CALL_DEPTH: usize = 8;
pub const MAX_CLOSURE_BYTES: usize = 262_144;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CallPin {
    pub id: String,
    pub hash: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FrozenCallee {
    pub id: String,
    pub hash: String,
    pub document: Box<Document>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub locations: BTreeMap<String, Location>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_hash: Option<String>,
}
impl CallPin {
    pub fn key(&self) -> String {
        format!("{}@{}", self.id, self.hash)
    }
}
impl FrozenCallee {
    pub fn verify(&self) -> Result<(), String> {
        if !self.document.dependencies.is_empty() {
            return Err("A callee snapshot must use the flat dependency closure".into());
        }
        if document_projection::identity(&self.document)? != self.hash {
            return Err(format!(
                "Pinned bytes do not match {}@{}",
                self.id, self.hash
            ));
        }
        if self.document.trigger.is_some()
            || self.document.kind != super::Kind::Pipeline
            || self.document.result.port.is_none()
            || self.document.result.from.is_none()
            || self.document.result.to.is_some()
        {
            return Err(format!("{} is not a typed callable Blueprint", self.id));
        }
        if has_unsealed_code_files(&self.document) {
            return Err("Callable code files must be frozen into inline source bytes".into());
        }
        let errors = self.document.template_errors();
        if !errors.is_empty() {
            return Err(format!("Invalid callable: {errors:?}"));
        }
        Ok(())
    }
    pub fn pin(&self) -> CallPin {
        CallPin {
            id: self.id.clone(),
            hash: self.hash.clone(),
        }
    }
    pub fn parsed(&self) -> Parsed {
        Parsed {
            document: (*self.document).clone(),
            locations: self.locations.clone(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FrozenClosure {
    pub root: Parsed,
    pub callees: BTreeMap<String, FrozenCallee>,
}
impl FrozenClosure {
    pub fn from_document(document: Document) -> Result<Self, String> {
        let mut document = document;
        let callees = std::mem::take(&mut document.dependencies);
        let closure = Self {
            root: Parsed {
                document,
                locations: BTreeMap::new(),
            },
            callees,
        };
        closure.verify()?;
        Ok(closure)
    }
    pub fn document(&self) -> Document {
        let mut document = self.root.document.clone();
        document.dependencies = self.callees.clone();
        for callee in document.dependencies.values_mut() {
            let label = if callee.id.starts_with("local:") || saved_reference(&callee.id) {
                callee.id.clone()
            } else {
                format!(
                    "callee:{}",
                    callee.hash.chars().take(12).collect::<String>()
                )
            };
            for location in callee.locations.values_mut() {
                location.file = label.clone();
            }
        }
        document
    }
    pub fn callee(&self, pin: &CallPin) -> Result<&FrozenCallee, String> {
        let callee = self
            .callees
            .get(&pin.key())
            .ok_or_else(|| format!("Missing pinned hash {}@{}", pin.id, pin.hash))?;
        if callee.id != pin.id {
            return Err(format!("Pinned identity mismatch: {}@{}", pin.id, pin.hash));
        }
        callee.verify()?;
        Ok(callee)
    }
    pub fn verify(&self) -> Result<(), String> {
        let bytes = serde_json::to_vec(&self.document()).map_err(|e| e.to_string())?;
        if bytes.len() > MAX_CLOSURE_BYTES {
            return Err("Call closure exceeds 256 KiB".into());
        }
        for (hash, callee) in &self.callees {
            if hash != &callee.pin().key() {
                return Err("Dependency key is not its id@hash".into());
            }
            callee.verify()?;
        }
        self.verify_graph(&self.root.document, &mut Vec::new(), &mut 0)
    }
    fn verify_graph(
        &self,
        document: &Document,
        stack: &mut Vec<CallPin>,
        expanded: &mut usize,
    ) -> Result<(), String> {
        if stack.len() > MAX_CALL_DEPTH {
            return Err("budget_exhausted: call depth exceeds eight".into());
        }
        *expanded = expanded.saturating_add(document.nodes().count());
        if *expanded > 64 {
            return Err("budget_exhausted: expanded call graph exceeds 64 executed nodes".into());
        }
        for node in document.nodes() {
            let Some(call) = &node.call else { continue };
            let pin = call
                .pin
                .as_ref()
                .ok_or("Unresolved call has no immutable pin")?;
            if stack.iter().any(|prior| prior.id == pin.id) {
                return Err("Recursive call closure".into());
            }
            let callee = self.callee(pin)?;
            stack.push(pin.clone());
            self.verify_graph(&callee.document, stack, expanded)?;
            stack.pop();
        }
        Ok(())
    }
    pub fn drift(&self, resolver: &dyn MemoryResolver) -> Result<Vec<Drift>, String> {
        let mut result = Vec::new();
        for callee in self.callees.values().filter(|c| saved_reference(&c.id)) {
            let pinned = resolver.revision(&callee.pin())?;
            if document_projection::identity(&pinned)? != callee.hash {
                return Err("Missing or corrupted pinned revision".into());
            }
            let (mut head, reason) = resolver.head_for_drift(&callee.id)?;
            head.dependencies.clear();
            let hash = document_projection::identity(&head)?;
            if hash != callee.hash || reason.is_some() {
                result.push(Drift {
                    reason: reason.unwrap_or_else(|| "callee content changed".into()),
                    pin: callee.pin(),
                    head_hash: hash,
                    diff: document_projection::diff(&callee.document, &head),
                });
            }
        }
        Ok(result)
    }
}

pub fn saved_reference(id: &str) -> bool {
    id.starts_with("mem:") || id.starts_with("catalog:")
}

pub trait MemoryResolver {
    fn retain(&self, _callee: &FrozenCallee) -> Result<(), String> {
        Ok(())
    }
    fn head(&self, id: &str) -> Result<Document, String>;
    fn revision(&self, pin: &CallPin) -> Result<Document, String>;
    fn head_for_drift(&self, id: &str) -> Result<(Document, Option<String>), String> {
        self.head(id).map(|document| (document, None))
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Drift {
    pub reason: String,
    pub pin: CallPin,
    pub head_hash: String,
    pub diff: document_projection::SemanticDiff,
}

pub(super) fn finding(parsed: &Parsed, field: &str, message: impl Into<String>) -> Diagnostic {
    let location = parsed.at(field);
    Diagnostic::error(
        location,
        field,
        &message.into(),
        "Use a unique local/inline callable or an exact scoped mem id/hash",
    )
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CallFrame {
    pub node: String,
    pub id: String,
    pub hash: String,
}

mod bundle;
pub use bundle::{collect_local, resolve_bundle, LocalBundle, LocalCallee};

mod admission;
pub use admission::check_context;

mod source;
pub use source::has_files as has_unsealed_code_files;
