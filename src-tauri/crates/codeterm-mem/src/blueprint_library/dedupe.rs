use super::index::{self, cosine, Slot};
use crate::blueprint_wl;
use crate::MemManager;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Thresholds {
    pub near_struct: f64,
    pub desc: f64,
    pub intent: f64,
}

impl Default for Thresholds {
    fn default() -> Self {
        Self {
            near_struct: 0.80,
            desc: 0.90,
            intent: 0.85,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    Exact,
    StructuralTwin,
    NearTwin,
    SemanticTwin,
    Distinct,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Scores {
    pub exact: bool,
    pub same_hash: bool,
    pub s_struct: f64,
    pub cos_desc: f64,
    pub cos_intent: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Comparison {
    pub verdict: Verdict,
    pub scores: Scores,
    pub why: String,
}

pub fn decide(scores: &Scores, thresholds: &Thresholds) -> Verdict {
    if scores.exact {
        Verdict::Exact
    } else if scores.same_hash {
        Verdict::StructuralTwin
    } else if scores.s_struct >= thresholds.near_struct && scores.cos_desc >= thresholds.desc {
        Verdict::NearTwin
    } else if scores.s_struct < thresholds.near_struct
        && (scores.cos_intent >= thresholds.intent || scores.cos_desc >= thresholds.desc)
    {
        Verdict::SemanticTwin
    } else {
        Verdict::Distinct
    }
}

impl MemManager {
    pub fn blueprint_pair_scores(&self, a: &str, b: &str) -> Result<(Scores, String), String> {
        let conn = self.conn.lock();
        let row = |id: &str| -> Result<_, String> {
            Ok((
                index::read_row(&conn, id)?.ok_or_else(|| format!("{id} is not indexed"))?,
                index::read_vectors(&conn, id, &self.embedder.identity())?,
            ))
        };
        let ((row_a, vectors_a), (row_b, vectors_b)) = (row(a)?, row(b)?);
        let best = |slot: Slot| {
            vectors_a
                .slot(slot)
                .flat_map(|(_, x)| vectors_b.slot(slot).map(move |(_, y)| cosine(x, y)))
                .fold(0.0_f64, f64::max)
        };
        let scores = Scores {
            exact: row_a.document_hash == row_b.document_hash,
            same_hash: row_a.signature.hash == row_b.signature.hash,
            s_struct: blueprint_wl::kernel(&row_a.signature.features, &row_b.signature.features),
            cos_desc: best(Slot::Canon),
            cos_intent: best(Slot::Intent),
        };
        let shared = blueprint_wl::shared_edges(&row_a.signature, &row_b.signature);
        let why = format!(
            "struct {:.2} (WL h=3{}): shared {}; desc cos {:.2}; intent cos {:.2}",
            scores.s_struct,
            if scores.same_hash { ", same hash" } else { "" },
            if shared.is_empty() {
                "none".into()
            } else {
                shared.into_iter().take(6).collect::<Vec<_>>().join("; ")
            },
            scores.cos_desc,
            scores.cos_intent
        );
        Ok((scores, why))
    }

    pub fn compare_blueprints(
        &self,
        a: &str,
        b: &str,
        thresholds: &Thresholds,
    ) -> Result<Comparison, String> {
        let (scores, why) = self.blueprint_pair_scores(a, b)?;
        let verdict = decide(&scores, thresholds);
        Ok(Comparison {
            verdict,
            scores,
            why: format!("{verdict:?}: {why}"),
        })
    }

    pub fn blueprint_twins(
        &self,
        area: &str,
        id: &str,
        thresholds: &Thresholds,
    ) -> Result<Vec<(String, Comparison)>, String> {
        let others = self
            .list_blueprint_solutions(area)?
            .into_iter()
            .map(|entry| entry.id)
            .filter(|other| other != id)
            .collect::<Vec<_>>();
        let mut twins = Vec::new();
        for other in others {
            let comparison = self.compare_blueprints(id, &other, thresholds)?;
            if comparison.verdict != Verdict::Distinct {
                twins.push((other, comparison));
            }
        }
        Ok(twins)
    }
}

pub fn compare_documents(
    a: &codeterm_blueprint_types::blueprint_document::Document,
    b: &codeterm_blueprint_types::blueprint_document::Document,
) -> Comparison {
    use codeterm_blueprint_types::blueprint_memory::{
        abstract_document, semantic_canon::canonical_json,
    };
    let unnamed = |document: &codeterm_blueprint_types::blueprint_document::Document| {
        let mut document = document.clone();
        document.name = "candidate".into();
        document.note = None;
        document.intent = None;
        document.anchors.clear();
        document
    };
    let (a, b) = (unnamed(a), unnamed(b));
    let (shape_a, shape_b) = (abstract_document(a.clone()), abstract_document(b.clone()));
    let (signature_a, signature_b) = (
        blueprint_wl::signature(&shape_a),
        blueprint_wl::signature(&shape_b),
    );
    let same_content = behaviour(&a) == behaviour(&b);
    let scores = Scores {
        // Saved solutions are stored generalized, so a candidate also matches its own generalization.
        exact: canonical_json(&a) == canonical_json(&b)
            || canonical_json(&shape_a) == canonical_json(&b)
            || canonical_json(&a) == canonical_json(&shape_b),
        same_hash: signature_a.hash == signature_b.hash && same_content,
        s_struct: blueprint_wl::kernel(&signature_a.features, &signature_b.features),
        cos_desc: 0.0,
        cos_intent: 0.0,
    };
    let verdict = decide(&scores, &Thresholds::default());
    let content = if same_content {
        "same code, commands and literals"
    } else {
        "code, commands or literals differ"
    };
    Comparison {
        verdict,
        scores,
        why: format!("{verdict:?}: WL h=3 shape over abstracted documents; {content}; candidate vectors unavailable"),
    }
}

// Twins refuse admission, so they must agree on what runs, not only on the graph shape.
fn behaviour(document: &codeterm_blueprint_types::blueprint_document::Document) -> Vec<String> {
    let mut parts = document
        .nodes()
        .map(|node| {
            serde_json::json!([
                node.uses,
                node.run,
                node.code,
                node.call,
                node.params,
                node.matches,
                node.every,
                node.timeout,
                node.until
            ])
            .to_string()
        })
        .collect::<Vec<_>>();
    parts.sort();
    parts.push(serde_json::to_string(&document.trigger).unwrap_or_default());
    parts.push(
        serde_json::to_string(
            &document
                .inputs
                .iter()
                .map(|(key, input)| (key, &input.default))
                .collect::<Vec<_>>(),
        )
        .unwrap_or_default(),
    );
    parts
}
