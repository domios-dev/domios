use crate::blueprint_wl::Signature;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Provenance {
    pub model: String,
    pub document_hash: String,
    pub created_at: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Enrichment {
    pub summary: String,
    pub intents: Vec<String>,
    pub provenance: Provenance,
}

#[derive(Clone, Debug, PartialEq)]
pub struct IndexRow {
    pub document_hash: String,
    pub canon: String,
    pub gists: String,
    pub signature: Signature,
    pub enrichment: Option<Enrichment>,
}

impl IndexRow {
    pub fn current_enrichment(&self) -> Option<&Enrichment> {
        self.enrichment
            .as_ref()
            .filter(|enrichment| enrichment.provenance.document_hash == self.document_hash)
    }

    pub fn enrichment_pending(&self) -> bool {
        self.current_enrichment().is_none()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Slot {
    Intent,
    Canon,
    WorkerIntent,
    Summary,
}

impl Slot {
    pub fn name(self) -> &'static str {
        match self {
            Self::Intent => "intent",
            Self::Canon => "canon",
            Self::WorkerIntent => "worker_intent",
            Self::Summary => "summary",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        [Self::Intent, Self::Canon, Self::WorkerIntent, Self::Summary]
            .into_iter()
            .find(|slot| slot.name() == name)
    }
}

pub type VectorKey = (Slot, usize);

#[derive(Clone, Debug, Default)]
pub struct Vectors {
    pub by_slot: BTreeMap<VectorKey, (String, Vec<f32>)>,
}

impl Vectors {
    pub fn slot(&self, slot: Slot) -> impl Iterator<Item = (&str, &[f32])> {
        self.by_slot
            .iter()
            .filter(move |((kind, _), _)| *kind == slot)
            .map(|(_, (text, vector))| (text.as_str(), vector.as_slice()))
    }
}

pub fn valid_dimension(vector: &[f32], dimension: usize) -> bool {
    dimension > 0 && vector.len() == dimension && vector.iter().all(|value| value.is_finite())
}

pub fn cosine(a: &[f32], b: &[f32]) -> f64 {
    if a.len() != b.len() {
        return 0.0;
    }
    let dot: f64 = a
        .iter()
        .zip(b)
        .map(|(x, y)| f64::from(*x) * f64::from(*y))
        .sum();
    let norm = |vector: &[f32]| {
        vector
            .iter()
            .map(|value| f64::from(*value).powi(2))
            .sum::<f64>()
            .sqrt()
    };
    let denominator = norm(a) * norm(b);
    if denominator > 0.0 {
        dot / denominator
    } else {
        0.0
    }
}
