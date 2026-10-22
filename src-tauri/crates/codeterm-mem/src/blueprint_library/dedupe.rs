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
