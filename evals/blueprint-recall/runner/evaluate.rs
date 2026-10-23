use crate::fixtures::{Gold, Pair, Query};
use crate::library::{Library, ALL_AREAS};
use codeterm_mem::blueprint_library::dedupe::{decide, Scores, Thresholds, Verdict};
use codeterm_mem::blueprint_library::{BlueprintHit, RecallConfig};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const KS: [usize; 3] = [1, 3, 5];
pub const DEPTH: usize = 10;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Hit {
    pub id: String,
    pub score: f64,
    pub why: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct QueryResult {
    pub set: String,
    pub original: String,
    pub text: String,
    pub gold: Vec<String>,
    pub hits: Vec<Hit>,
    pub recall: BTreeMap<String, f64>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Recall {
    pub n: usize,
    pub r1: f64,
    pub r3: f64,
    pub r5: f64,
}

pub fn recall_at(hits: &[String], gold: &[String], k: usize) -> f64 {
    let found = hits.iter().take(k).filter(|id| gold.contains(id)).count();
    found as f64 / k.min(gold.len()).max(1) as f64
}

pub fn hits(
    library: &Library,
    text: &str,
    config: &RecallConfig,
    depth: usize,
) -> Result<Vec<BlueprintHit>, String> {
    library
        .mem
        .recall_blueprint_hits(ALL_AREAS, text, None, depth, config)
}

pub fn to_hits(library: &Library, hits: &[BlueprintHit]) -> Vec<Hit> {
    hits.iter()
        .map(|hit| Hit {
            id: library.doc_of[&hit.entry.id].clone(),
            score: (hit.score * 1e9).round() / 1e9,
            why: hit.why.clone(),
        })
        .collect()
}

pub fn score(query: &Query, gold: &Gold, ranked: Vec<Hit>) -> QueryResult {
    let family = gold
        .families
        .get(&query.original)
        .cloned()
        .unwrap_or_default();
    let ids = ranked.iter().map(|hit| hit.id.clone()).collect::<Vec<_>>();
    QueryResult {
        set: query.set.clone(),
        original: query.original.clone(),
        text: query.text.clone(),
        recall: KS
            .iter()
            .map(|k| (format!("@{k}"), recall_at(&ids, &family, *k)))
            .collect(),
        gold: family,
        hits: ranked,
    }
}

pub fn run(
    library: &Library,
    queries: &[Query],
    gold: &Gold,
    config: &RecallConfig,
) -> Result<Vec<QueryResult>, String> {
    queries
        .iter()
        .map(|query| {
            let ranked = hits(library, &query.text, config, DEPTH)?;
            Ok(score(query, gold, to_hits(library, &ranked)))
        })
        .collect()
}

pub fn summarize<'a>(rows: impl IntoIterator<Item = &'a QueryResult>) -> Recall {
    let rows = rows.into_iter().collect::<Vec<_>>();
    let mean = |k: &str| {
        if rows.is_empty() {
            0.0
        } else {
            let total: f64 = rows.iter().map(|row| row.recall[k]).sum();
            (total / rows.len() as f64 * 1e4).round() / 1e4
        }
    };
    Recall {
        n: rows.len(),
        r1: mean("@1"),
        r3: mean("@3"),
        r5: mean("@5"),
    }
}

pub fn tables(rows: &[QueryResult]) -> BTreeMap<String, Recall> {
    let mut out = BTreeMap::from([("pooled".to_owned(), summarize(rows))]);
    let sets = rows
        .iter()
        .map(|row| row.set.clone())
        .collect::<std::collections::BTreeSet<_>>();
    for set in sets {
        out.insert(
            format!("set:{set}"),
            summarize(rows.iter().filter(|row| row.set == set)),
        );
    }
    out
}

pub fn per_original(rows: &[QueryResult]) -> BTreeMap<String, Recall> {
    let originals = rows
        .iter()
        .map(|row| row.original.clone())
        .collect::<std::collections::BTreeSet<_>>();
    originals
        .into_iter()
        .map(|original| {
            let summary = summarize(rows.iter().filter(|row| row.original == original));
            (original, summary)
        })
        .collect()
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ScoredPair {
    pub a: String,
    pub b: String,
    pub label: String,
    pub split: String,
    pub scores: Scores,
}

pub fn pair_scores(library: &Library, pairs: &[Pair]) -> Result<Vec<ScoredPair>, String> {
    pairs
        .iter()
        .filter(|pair| {
            library.item_of.contains_key(&pair.a) && library.item_of.contains_key(&pair.b)
        })
        .map(|pair| {
            let (scores, _) = library
                .mem
                .blueprint_pair_scores(&library.item_of[&pair.a], &library.item_of[&pair.b])?;
            Ok(ScoredPair {
                a: pair.a.clone(),
                b: pair.b.clone(),
                label: pair.label.clone(),
                split: pair.split.clone(),
                scores,
            })
        })
        .collect()
}

fn label(verdict: Verdict) -> &'static str {
    match verdict {
        Verdict::Exact => "exact",
        Verdict::StructuralTwin => "structural twin",
        Verdict::NearTwin => "near twin",
        Verdict::SemanticTwin => "semantic twin",
        Verdict::Distinct => "distinct",
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Dedupe {
    pub pairs: usize,
    pub accuracy: f64,
    pub exact_or_structural_on_distinct: usize,
    pub confusion: BTreeMap<String, usize>,
}

pub fn dedupe(scored: &[ScoredPair], thresholds: &Thresholds) -> Dedupe {
    let mut confusion = BTreeMap::new();
    let mut correct = 0;
    let mut bad = 0;
    for pair in scored {
        let verdict = label(decide(&pair.scores, thresholds));
        *confusion
            .entry(format!("{} -> {verdict}", pair.label))
            .or_insert(0) += 1;
        correct += usize::from(verdict == pair.label);
        bad +=
            usize::from(pair.label == "distinct" && matches!(verdict, "exact" | "structural twin"));
    }
    Dedupe {
        pairs: scored.len(),
        accuracy: if scored.is_empty() {
            0.0
        } else {
            (correct as f64 / scored.len() as f64 * 1e4).round() / 1e4
        },
        exact_or_structural_on_distinct: bad,
        confusion,
    }
}

pub const START: Thresholds = Thresholds {
    near_struct: 0.80,
    desc: 0.90,
    intent: 0.85,
};

pub fn calibrate(dev: &[ScoredPair]) -> (Thresholds, Dedupe) {
    let steps = |from: f64, step: f64, count: usize| {
        (0..count).map(move |i| ((from + step * i as f64) * 1e4).round() / 1e4)
    };
    let mut best: Option<((f64, i64, f64), Thresholds, Dedupe)> = None;
    for near_struct in steps(0.5, 0.05, 10) {
        for desc in steps(0.70, 0.02, 15) {
            for intent in steps(0.60, 0.025, 15) {
                let thresholds = Thresholds {
                    near_struct,
                    desc,
                    intent,
                };
                let metrics = dedupe(dev, &thresholds);
                let distance = (near_struct - START.near_struct).abs()
                    + (desc - START.desc).abs()
                    + (intent - START.intent).abs();
                let key = (
                    metrics.accuracy,
                    -(metrics.exact_or_structural_on_distinct as i64),
                    -distance,
                );
                if best.as_ref().is_none_or(|(current, _, _)| key > *current) {
                    best = Some((key, thresholds, metrics));
                }
            }
        }
    }
    let (_, thresholds, metrics) = best.expect("non-empty grid");
    (thresholds, metrics)
}
