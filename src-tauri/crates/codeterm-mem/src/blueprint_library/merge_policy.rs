use super::{index, Entry};
use crate::{embedder::EmbedderIdentity, MemManager};
use codeterm_blueprint_types::blueprint_document::Document;

pub const MIN_PURPOSE_COSINE: f64 = 0.92;

pub fn callable(document: &Document) -> bool {
    document.trigger.is_none() && document.result.port.is_some()
}

pub fn check_name(
    library: &[Entry],
    document: &Document,
    intents: &[String],
) -> Result<(), String> {
    if !callable(document) {
        return Ok(());
    }
    if let Some(existing) = library.iter().find(|entry| {
        entry
            .document
            .name
            .trim()
            .eq_ignore_ascii_case(document.name.trim())
            && codeterm_blueprint_types::blueprint_memory::structure(&entry.document)
                != codeterm_blueprint_types::blueprint_memory::structure(document)
    }) {
        let purpose = document
            .intent
            .as_deref()
            .or_else(|| intents.first().map(String::as_str))
            .unwrap_or("describe callable contents");
        let suggested = purpose
            .split(|c: char| !c.is_alphanumeric())
            .filter(|word| !word.is_empty())
            .take(12)
            .map(str::to_lowercase)
            .collect::<Vec<_>>()
            .join("-");
        return Err(format!("blueprint_name_conflict: {:?} already names saved solution {} with different contents. Choose a content-describing name based on the purpose, for example {:?}; an intended revision must explicitly update the same solution ID.", document.name, existing.id, suggested));
    }
    Ok(())
}

pub struct Purpose {
    pub identity: EmbedderIdentity,
    pub texts: Vec<String>,
    pub vectors: Vec<Vec<f32>>,
}

impl Purpose {
    pub fn candidate(mem: &MemManager, intents: &[String]) -> Self {
        let identity = mem.embedder.identity();
        let vectors = intents
            .iter()
            .filter_map(|text| {
                mem.embedder
                    .passage(text)
                    .ok()
                    .filter(|v| {
                        v.identity == identity
                            && index::valid_dimension(&v.vector, identity.dimension)
                    })
                    .map(|v| v.vector)
            })
            .collect();
        Self {
            identity,
            texts: intents.to_vec(),
            vectors,
        }
    }

    pub fn stored(
        stored: &index::Vectors,
        identity: &EmbedderIdentity,
        entry: &Entry,
    ) -> Result<Self, String> {
        let vectors = stored
            .slot(index::Slot::Intent)
            .filter(|(text, _)| entry.intents.iter().any(|intent| intent == text))
            .map(|(_, vector)| vector.to_vec())
            .collect();
        Ok(Self {
            identity: identity.clone(),
            texts: entry.intents.clone(),
            vectors,
        })
    }

    pub fn agrees(&self, other: &Self) -> bool {
        if self.identity != other.identity {
            return false;
        }
        if self.vectors.len() != self.texts.len() || other.vectors.len() != other.texts.len() {
            return !self.texts.is_empty() && self.texts == other.texts;
        }
        let covered = |a: &[Vec<f32>], b: &[Vec<f32>]| {
            !a.is_empty()
                && a.iter()
                    .all(|x| b.iter().any(|y| index::cosine(x, y) >= MIN_PURPOSE_COSINE))
        };
        covered(&self.vectors, &other.vectors) && covered(&other.vectors, &self.vectors)
    }
}
