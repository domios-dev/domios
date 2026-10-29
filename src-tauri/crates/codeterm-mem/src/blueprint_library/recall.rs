use super::index::{self, cosine, IndexRow, Slot, Vectors};
use crate::blueprint_wl::{self, Signature};
use crate::solution_search::RELEVANCE_FLOOR;
use crate::MemManager;
use codeterm_blueprint_types::blueprint_memory::{self as model, Entry};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Fusion {
    Rrf,
    Score,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Weights {
    pub intent: f64,
    pub worker_intent: f64,
    pub summary: f64,
    pub canon: f64,
    pub lexical: f64,
    pub structure: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecallConfig {
    pub fusion: Fusion,
    pub rrf_k: f64,
    pub list_depth: usize,
    pub enrichment: bool,
    pub pool_intents: bool,
    pub weights: Weights,
}

impl Default for RecallConfig {
    fn default() -> Self {
        Self {
            fusion: Fusion::Score,
            rrf_k: 60.0,
            list_depth: usize::MAX,
            enrichment: true,
            pool_intents: true,
            weights: Weights {
                intent: 1.0,
                worker_intent: 0.0,
                summary: 0.3,
                canon: 0.3,
                lexical: 0.2,
                structure: 0.5,
            },
        }
    }
}

#[derive(Clone, Debug)]
pub struct BlueprintHit {
    pub entry: Entry,
    pub score: f64,
    pub why: String,
    pub enrichment_pending: bool,
}

#[derive(Clone)]
pub struct Candidate {
    pub entry: Entry,
    pub row: Option<IndexRow>,
    pub vectors: Vectors,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum List {
    Intent,
    WorkerIntent,
    Summary,
    Canon,
    Lexical,
    Structure,
}

impl List {
    fn name(self) -> &'static str {
        match self {
            Self::Intent => "intent",
            Self::WorkerIntent => "worker intent",
            Self::Summary => "summary",
            Self::Canon => "canon",
            Self::Lexical => "lexical",
            Self::Structure => "structure",
        }
    }

    fn weight(self, weights: &Weights) -> f64 {
        match self {
            Self::Intent => weights.intent,
            Self::WorkerIntent => weights.worker_intent,
            Self::Summary => weights.summary,
            Self::Canon => weights.canon,
            Self::Lexical => weights.lexical,
            Self::Structure => weights.structure,
        }
    }
}

struct Signal {
    score: f64,
    text: Option<String>,
}

pub(super) fn rank(
    candidates: Vec<Candidate>,
    query: &str,
    query_vector: Option<&[f32]>,
    draft: Option<&Signature>,
    config: &RecallConfig,
    now: i64,
) -> Vec<BlueprintHit> {
    let mut signals: BTreeMap<List, Vec<Option<Signal>>> = BTreeMap::new();
    let keywords = crate::search::extract_keywords(query);
    let mut relevant = candidates
        .iter()
        .map(|candidate| {
            let text = lexical_text(candidate, config.enrichment);
            let words = tokens(&text).into_iter().collect::<HashSet<_>>();
            !keywords.is_empty()
                && keywords.iter().filter(|word| words.contains(*word)).count() as f64
                    / keywords.len() as f64
                    >= RELEVANCE_FLOOR
        })
        .collect::<Vec<_>>();
    if let Some(query_vector) = query_vector {
        let best = |candidate: &Candidate, slots: &[Slot]| {
            slots
                .iter()
                .flat_map(|slot| candidate.vectors.slot(*slot))
                .map(|(text, vector)| Signal {
                    score: cosine(query_vector, vector),
                    text: Some(text.to_owned()),
                })
                .max_by(|a, b| a.score.total_cmp(&b.score))
        };
        let intent_slots: &[Slot] = if config.enrichment && config.pool_intents {
            &[Slot::Intent, Slot::WorkerIntent]
        } else {
            &[Slot::Intent]
        };
        signals.insert(
            List::Intent,
            candidates.iter().map(|c| best(c, intent_slots)).collect(),
        );
        signals.insert(
            List::Canon,
            candidates
                .iter()
                .map(|c| {
                    best(c, &[Slot::Canon]).map(|signal| Signal {
                        text: None,
                        ..signal
                    })
                })
                .collect(),
        );
        if config.enrichment {
            if !config.pool_intents {
                signals.insert(
                    List::WorkerIntent,
                    candidates
                        .iter()
                        .map(|c| best(c, &[Slot::WorkerIntent]))
                        .collect(),
                );
            }
            signals.insert(
                List::Summary,
                candidates
                    .iter()
                    .map(|c| {
                        best(c, &[Slot::Summary]).map(|signal| Signal {
                            text: None,
                            ..signal
                        })
                    })
                    .collect(),
            );
        }
    }
    let lexical = bm25(&candidates, query, config.enrichment);
    signals.insert(
        List::Lexical,
        lexical
            .into_iter()
            .map(|score| (score > 0.0).then_some(Signal { score, text: None }))
            .collect(),
    );
    if let Some(draft) = draft {
        signals.insert(
            List::Structure,
            candidates
                .iter()
                .map(|candidate| {
                    candidate.row.as_ref().map(|row| Signal {
                        score: blueprint_wl::kernel(&draft.features, &row.signature.features),
                        text: None,
                    })
                })
                .collect(),
        );
    }
    let mut fused = vec![0.0; candidates.len()];
    let mut listed = vec![false; candidates.len()];
    let mut why: Vec<Vec<String>> = vec![Vec::new(); candidates.len()];
    for (list, values) in &signals {
        let weight = list.weight(&config.weights);
        let mut order = values
            .iter()
            .enumerate()
            .filter_map(|(index, signal)| signal.as_ref().map(|signal| (index, signal.score)))
            .collect::<Vec<_>>();
        order.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
        order.truncate(config.list_depth);
        let top = order.first().map_or(0.0, |(_, score)| *score);
        for (rank, (index, score)) in order.iter().enumerate() {
            if weight > 0.0 {
                if *list != List::Lexical && *score >= RELEVANCE_FLOOR {
                    relevant[*index] = true;
                }
                listed[*index] = true;
                fused[*index] += weight
                    * match config.fusion {
                        Fusion::Rrf => 1.0 / (config.rrf_k + rank as f64 + 1.0),
                        Fusion::Score if *list == List::Lexical => {
                            if top > 0.0 {
                                score / top
                            } else {
                                0.0
                            }
                        }
                        Fusion::Score => *score,
                    };
            }
            let signal = values[*index].as_ref();
            let quoted = signal
                .and_then(|signal| signal.text.as_deref())
                .map(|text| format!(" \"{}\"", text.chars().take(80).collect::<String>()))
                .unwrap_or_default();
            why[*index].push(match list {
                List::Lexical => format!("lexical #{}", rank + 1),
                _ => format!("{} {score:.2}{quoted}", list.name()),
            });
        }
    }
    let mut hits = candidates
        .into_iter()
        .zip(fused)
        .zip(why)
        .zip(
            listed
                .into_iter()
                .zip(relevant)
                .map(|(listed, relevant)| listed && relevant),
        )
        .filter(|(_, listed)| *listed)
        .map(|(((candidate, score), mut why), _)| {
            let pending = candidate
                .row
                .as_ref()
                .is_none_or(IndexRow::enrichment_pending);
            why.push(
                match candidate
                    .row
                    .as_ref()
                    .and_then(IndexRow::current_enrichment)
                {
                    Some(enrichment) => format!("enrichment by {}", enrichment.provenance.model),
                    None => "enrichment pending".into(),
                },
            );
            BlueprintHit {
                entry: candidate.entry,
                score,
                why: why.join("; "),
                enrichment_pending: pending,
            }
        })
        .collect::<Vec<_>>();
    hits.sort_by(|a, b| {
        b.score
            .total_cmp(&a.score)
            .then(model::score(&b.entry, now).total_cmp(&model::score(&a.entry, now)))
            .then(a.entry.id.cmp(&b.entry.id))
    });
    hits
}

fn tokens(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|token| !token.is_empty())
        .map(str::to_owned)
        .collect()
}

fn lexical_text(candidate: &Candidate, enrichment: bool) -> String {
    let mut parts = candidate.entry.intents.clone();
    if let Some(row) = &candidate.row {
        parts.push(row.canon.clone());
        if enrichment {
            if let Some(enriched) = row.current_enrichment() {
                parts.push(enriched.summary.clone());
                parts.extend(enriched.intents.iter().cloned());
            }
        }
    }
    parts.join(" ")
}

fn bm25(candidates: &[Candidate], query: &str, enrichment: bool) -> Vec<f64> {
    const K1: f64 = 1.5;
    const B: f64 = 0.75;
    let documents = candidates
        .iter()
        .map(|candidate| tokens(&lexical_text(candidate, enrichment)))
        .collect::<Vec<_>>();
    if documents.is_empty() {
        return Vec::new();
    }
    let count = documents.len() as f64;
    let average = documents.iter().map(Vec::len).sum::<usize>() as f64 / count;
    let mut frequency: HashMap<&str, usize> = HashMap::new();
    for document in &documents {
        for token in document.iter().map(String::as_str).collect::<HashSet<_>>() {
            *frequency.entry(token).or_default() += 1;
        }
    }
    let idf = frequency
        .iter()
        .map(|(token, n)| {
            let n = *n as f64;
            (*token, (1.0 + (count - n + 0.5) / (n + 0.5)).ln())
        })
        .collect::<HashMap<_, _>>();
    let query = tokens(query);
    documents
        .iter()
        .map(|document| {
            let length = document.len() as f64;
            query
                .iter()
                .map(|token| {
                    let f = document.iter().filter(|word| *word == token).count() as f64;
                    idf.get(token.as_str()).copied().unwrap_or(0.0) * f * (K1 + 1.0)
                        / (f + K1 * (1.0 - B + B * length / average.max(1e-9)))
                })
                .sum()
        })
        .collect()
}

impl MemManager {
    pub(super) fn candidates(
        &self,
        area: &str,
        identity: Option<&crate::embedder::EmbedderIdentity>,
    ) -> Result<Vec<Candidate>, String> {
        self.store
            .blueprint_candidates(&Self::blueprint_write_context(), area, None, identity)
    }

    pub fn recall_blueprint_hits(
        &self,
        area: &str,
        intent: &str,
        draft: Option<&Signature>,
        limit: usize,
        config: &RecallConfig,
    ) -> Result<Vec<BlueprintHit>, String> {
        let query_vector = self.embedder.query(intent);
        let candidates =
            self.candidates(area, query_vector.as_ref().map(|vector| &vector.identity))?;
        let mut hits = rank(
            candidates,
            intent,
            query_vector
                .as_ref()
                .filter(|vector| {
                    vector.identity == self.embedder.identity()
                        && index::valid_dimension(&vector.vector, vector.identity.dimension)
                })
                .map(|vector| vector.vector.as_slice()),
            draft,
            config,
            chrono::Utc::now().timestamp(),
        );
        hits.truncate(limit);
        Ok(hits)
    }
}

#[cfg(test)]
mod tests;
