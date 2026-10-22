use crate::evaluate::{self, Dedupe, QueryResult, Recall};
use crate::fixtures::{self, eval_dir};
use crate::guard;
use crate::library::{self, Intents};
use codeterm_mem::blueprint_library::dedupe::Thresholds;
use codeterm_mem::blueprint_library::{Fusion, RecallConfig, Weights};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

fn config(fusion: Fusion, pool: bool, w: [f64; 5]) -> RecallConfig {
    RecallConfig {
        fusion,
        rrf_k: 60.0,
        list_depth: 50,
        enrichment: true,
        pool_intents: pool,
        weights: Weights {
            intent: w[0],
            worker_intent: w[1],
            summary: w[2],
            canon: w[3],
            lexical: w[4],
            structure: 0.0,
        },
    }
}

pub fn configs() -> Vec<(String, RecallConfig)> {
    use Fusion::{Rrf, Score};
    [
        config(Rrf, false, [1.0, 1.0, 1.0, 1.0, 1.0]),
        config(Rrf, false, [1.0, 1.0, 1.0, 1.0, 0.0]),
        config(Rrf, false, [1.0, 1.0, 0.0, 0.0, 0.0]),
        config(Rrf, true, [1.0, 0.0, 0.0, 0.0, 0.0]),
        config(Rrf, true, [1.0, 0.0, 1.0, 1.0, 0.0]),
        config(Rrf, true, [2.0, 0.0, 1.0, 1.0, 0.0]),
        config(Rrf, true, [2.0, 0.0, 1.0, 1.0, 0.5]),
        config(Score, true, [1.0, 0.0, 0.0, 0.0, 0.0]),
        config(Score, true, [1.0, 0.0, 0.5, 0.5, 0.0]),
        config(Score, true, [1.0, 0.0, 0.3, 0.3, 0.2]),
        config(Score, false, [1.0, 1.0, 0.5, 0.5, 0.0]),
        config(Score, true, [1.0, 0.0, 1.0, 1.0, 0.0]),
    ]
    .into_iter()
    .enumerate()
    .map(|(index, config)| (format!("C{}", index + 1), config))
    .collect()
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GridRow {
    pub id: String,
    pub config: RecallConfig,
    pub tables: BTreeMap<String, Recall>,
    pub per_query: Vec<QueryResult>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Grid {
    pub about: String,
    pub ran_at: String,
    pub head: String,
    pub prereg_sha: String,
    pub embedder: String,
    pub enrichment_applied: usize,
    pub refused: std::collections::BTreeMap<String, String>,
    pub rows: Vec<GridRow>,
    pub selected: String,
    pub selection_key: String,
    pub thresholds: Thresholds,
    pub dedupe_dev: Dedupe,
    pub dedupe_test_informational: Dedupe,
}

pub fn prereg() -> std::path::PathBuf {
    eval_dir().join("PREREG-bdomios.md")
}

pub fn grid_path() -> std::path::PathBuf {
    eval_dir().join("grid.json")
}

pub fn run_grid() -> Result<(), String> {
    guard::committed_unmodified(&prereg())?;
    if grid_path().exists() {
        return Err("refused: grid.json exists; the grid runs once".into());
    }
    let prereg_sha = guard::last_commit(&prereg())?.ok_or("PREREG has no commit")?;
    let corpus = fixtures::corpus()?;
    let gold = fixtures::gold()?;
    let intents = fixtures::read_json(&fixtures::intents_path())?;
    let enrichment = fixtures::read_json(&fixtures::enrichment_path())?;
    let library = library::ingest(&corpus, Intents::Cached(&intents), Some(&enrichment))?;
    let pool = fixtures::selection_pool()?;
    let mut rows = Vec::new();
    for (id, config) in configs() {
        let per_query = evaluate::run(&library, &pool, &gold, &config)?;
        let tables = evaluate::tables(&per_query);
        eprintln!("{id}: pooled {:?}", tables["pooled"]);
        rows.push(GridRow {
            id,
            config,
            tables,
            per_query,
        });
    }
    let selected = rows
        .iter()
        .enumerate()
        .max_by(|(ia, a), (ib, b)| {
            let (pa, pb) = (&a.tables["pooled"], &b.tables["pooled"]);
            pa.r5
                .total_cmp(&pb.r5)
                .then(pa.r3.total_cmp(&pb.r3))
                .then(ib.cmp(ia))
        })
        .map(|(_, row)| row.id.clone())
        .ok_or("empty grid")?;
    let dev = evaluate::pair_scores(
        &library,
        &gold
            .pairs
            .iter()
            .filter(|p| p.split == "dev")
            .cloned()
            .collect::<Vec<_>>(),
    )?;
    let test = evaluate::pair_scores(
        &library,
        &gold
            .pairs
            .iter()
            .filter(|p| p.split == "test")
            .cloned()
            .collect::<Vec<_>>(),
    )?;
    let (thresholds, dedupe_dev) = evaluate::calibrate(&dev);
    let grid = Grid {
        about: "B-domios grid on the selection pool (E2 sets a and b, E2b set b2); b3 never used. Gate arm = enrichment on, no rerank.".into(),
        ran_at: chrono::Utc::now().to_rfc3339(),
        head: guard::head()?,
        prereg_sha,
        embedder: library.mem.embedder().identity().to_string(),
        enrichment_applied: library.enriched,
        refused: library.refused.clone(),
        rows,
        selected: selected.clone(),
        selection_key: "max pooled recall@5, then pooled recall@3, then lower config index".into(),
        thresholds,
        dedupe_dev,
        dedupe_test_informational: evaluate::dedupe(&test, &thresholds),
    };
    fixtures::write_json(&grid_path(), &grid)?;
    eprintln!("selected {selected}; thresholds {thresholds:?}");
    Ok(())
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Frozen {
    pub about: String,
    pub config_id: String,
    pub config: RecallConfig,
    pub thresholds: Thresholds,
    pub embedder: String,
    pub grid_sha256: String,
    pub prereg_sha: String,
}

pub fn frozen_path() -> std::path::PathBuf {
    eval_dir().join("config.frozen.json")
}

pub fn freeze() -> Result<(), String> {
    guard::committed_unmodified(&grid_path())?;
    if frozen_path().exists() {
        return Err("refused: config.frozen.json exists".into());
    }
    let grid: Grid = fixtures::read_json(&grid_path())?;
    let row = grid
        .rows
        .iter()
        .find(|row| row.id == grid.selected)
        .ok_or("selected row")?;
    fixtures::write_json(
        &frozen_path(),
        &Frozen {
            about: "C* frozen from grid.json before any b3 file entered the worktree; gate arm = this config, degraded arm = enrichment false, rerank arm = this config + rerank.".into(),
            config_id: row.id.clone(),
            config: row.config.clone(),
            thresholds: grid.thresholds,
            embedder: grid.embedder.clone(),
            grid_sha256: guard::sha256_file(&grid_path())?,
            prereg_sha: grid.prereg_sha,
        },
    )
}
