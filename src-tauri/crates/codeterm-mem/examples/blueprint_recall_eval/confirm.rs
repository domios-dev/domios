use crate::evaluate::{self, Dedupe, QueryResult, Recall};
use crate::fixtures::{self, eval_dir, Query};
use crate::grid::{self, Frozen};
use crate::guard;
use crate::library::{self, Intents, Library};
use crate::rerank::{self, RerankLog};
use codeterm_blueprint_types::blueprint_memory::AREA_CAP;
use codeterm_mem::blueprint_library::RecallConfig;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Arm {
    pub tables: BTreeMap<String, Recall>,
    pub per_original: BTreeMap<String, Recall>,
    pub starved: Recall,
    pub per_query: Vec<QueryResult>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Results {
    pub label: String,
    pub posthoc: bool,
    pub ran_at: String,
    pub head: String,
    pub prereg_sha: String,
    pub frozen_sha: String,
    pub frozen_sha256: String,
    pub queries_sha256: String,
    pub manifest_sha256: Option<String>,
    pub embedder: String,
    pub config_id: String,
    pub enrichment_applied: usize,
    pub refused: BTreeMap<String, String>,
    pub starved_originals: Vec<String>,
    pub gate: Arm,
    pub degraded: Arm,
    pub rerank: Arm,
    pub rerank_log: Vec<RerankLog>,
    pub dedupe: Dedupe,
    pub gate_b: String,
}

fn arm(rows: Vec<QueryResult>, starved: &[String]) -> Arm {
    Arm {
        tables: evaluate::tables(&rows),
        per_original: evaluate::per_original(&rows),
        starved: evaluate::summarize(rows.iter().filter(|row| starved.contains(&row.original))),
        per_query: rows,
    }
}

pub fn frozen() -> Result<Frozen, String> {
    fixtures::read_json(&grid::frozen_path())
}

pub fn fixture_library() -> Result<Library, String> {
    let corpus = fixtures::corpus()?;
    let intents = fixtures::read_json(&fixtures::intents_path())?;
    let enrichment = fixtures::read_json(&fixtures::enrichment_path())?;
    library::ingest(&corpus, Intents::Cached(&intents), Some(&enrichment))
}

fn evaluate_set(
    label: &str,
    posthoc: bool,
    queries_path: &Path,
    manifest: Option<(&Value, String)>,
) -> Result<Results, String> {
    let frozen = frozen()?;
    let gold = fixtures::gold()?;
    let queries: Vec<Query> = fixtures::queries(queries_path, label)?;
    let starved = manifest
        .as_ref()
        .map(|(value, _)| guard::starved(value, label))
        .unwrap_or_default();
    let library = fixture_library()?;
    let gate_rows = evaluate::run(&library, &queries, &gold, &frozen.config)?;
    let degraded = RecallConfig {
        enrichment: false,
        ..frozen.config.clone()
    };
    let degraded_rows = evaluate::run(&library, &queries, &gold, &degraded)?;
    let mut rerank_rows = Vec::new();
    let mut rerank_log = Vec::new();
    for query in &queries {
        let fused = evaluate::hits(&library, &query.text, &frozen.config, AREA_CAP)?;
        let baseline = evaluate::to_hits(&library, &fused);
        let (ranked, log) = rerank::rerank(&library, &query.text, &fused, baseline);
        rerank_log.push(log);
        rerank_rows.push(evaluate::score(
            query,
            &gold,
            ranked.into_iter().take(evaluate::DEPTH).collect(),
        ));
    }
    let test_pairs = gold
        .pairs
        .iter()
        .filter(|pair| pair.split == "test")
        .cloned()
        .collect::<Vec<_>>();
    let dedupe = evaluate::dedupe(
        &evaluate::pair_scores(&library, &test_pairs)?,
        &frozen.thresholds,
    );
    let gate = arm(gate_rows, &starved);
    let pooled = gate.tables["pooled"].r5;
    Ok(Results {
        label: label.into(),
        posthoc,
        ran_at: chrono::Utc::now().to_rfc3339(),
        head: guard::head()?,
        prereg_sha: frozen.prereg_sha.clone(),
        frozen_sha: guard::last_commit(&grid::frozen_path())?.unwrap_or_default(),
        frozen_sha256: guard::sha256_file(&grid::frozen_path())?,
        queries_sha256: guard::sha256_file(queries_path)?,
        manifest_sha256: manifest.map(|(_, hash)| hash),
        embedder: library.mem.embedder().identity().to_string(),
        config_id: frozen.config_id,
        enrichment_applied: library.enriched,
        refused: library.refused.clone(),
        starved_originals: starved.clone(),
        gate_b: format!(
            "{} (pooled recall@5 {pooled:.4} vs 0.80)",
            if pooled >= 0.80 { "PASS" } else { "FAIL" }
        ),
        gate,
        degraded: arm(degraded_rows, &starved),
        rerank: arm(rerank_rows, &starved),
        rerank_log,
        dedupe,
    })
}

pub fn confirm(set: &str, queries: &Path, manifest_path: &Path) -> Result<(), String> {
    guard::committed_unmodified(&grid::prereg())?;
    guard::committed_unmodified(&grid::frozen_path())?;
    let out = eval_dir().join(format!("results-{set}.json"));
    guard::never_produced(&out)?;
    let manifest: Value = fixtures::read_json(manifest_path)?;
    guard::manifest_matches(queries, &manifest, set)?;
    let manifest_hash = guard::sha256_file(manifest_path)?;
    let results = evaluate_set(set, false, queries, Some((&manifest, manifest_hash)))?;
    fixtures::write_json(&out, &results)?;
    eprintln!("confirmed {set}: gate B {}", results.gate_b);
    Ok(())
}

pub fn posthoc(queries: &Path, label: &str, out: &Path) -> Result<(), String> {
    let name = out
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    if name.starts_with("results-") || name == "results.json" {
        return Err("refused: post-hoc runs never write results-*.json".into());
    }
    let results = evaluate_set(label, true, queries, None)?;
    fixtures::write_json(out, &results)?;
    eprintln!("post-hoc {label}: gate arm {}", results.gate_b);
    Ok(())
}

pub fn query(text: &str, k: usize, json: bool) -> Result<(), String> {
    let frozen = frozen()?;
    let library = fixture_library()?;
    let hits = evaluate::to_hits(
        &library,
        &evaluate::hits(&library, text, &frozen.config, k)?,
    );
    if json {
        println!(
            "{}",
            serde_json::json!({"query": text, "config": frozen.config_id, "embedder": library.mem.embedder().identity().to_string(), "hits": hits})
        );
    } else {
        for (rank, hit) in hits.iter().enumerate() {
            println!("{}. {} {:.4} WHY {}", rank + 1, hit.id, hit.score, hit.why);
        }
    }
    Ok(())
}
