//! A/B bench for search ranking parameters.
//!
//! Runs the same queries against a mem DB with multiple SearchConfig variants
//! and prints recall@1, recall@5, MRR for each. No re-indexing needed —
//! vectors already in the store are reused.
//!
//! Usage:
//!   cargo run --release --bin mem-ab-bench -- --db ~/.codeterm-dev/mem/knowledge.db

use std::path::PathBuf;
use std::time::Instant;

use codeterm_lib::mem::{MemManager, SearchQuery};
use codeterm_lib::mem::search::SearchConfig;
use serde::Deserialize;

#[derive(Deserialize, Debug)]
struct TestCase {
    id: String,
    cat: String,
    q: String,
    expect: Vec<String>,
}

fn load_cases() -> Vec<TestCase> {
    // Queries targeting items in evals/seeds/loaded.py (first 50 items).
    // `expect` is a list of keywords — a hit counts if any title+body in top-3
    // contains ≥2 of them (or ≥1 for short expect lists).
    let raw = r#"[
        {"id":"K01","cat":"keyword","q":"PgBouncer connection pooling","expect":["pgbouncer","pooling","bouncer"]},
        {"id":"K02","cat":"keyword","q":"Core Audio AVAudioEngine","expect":["audio engine","avaudioengine","core audio"]},
        {"id":"K03","cat":"keyword","q":"StoreKit 2 subscriptions","expect":["storekit","subscription"]},
        {"id":"K04","cat":"keyword","q":"Deepgram transcription","expect":["deepgram","transcription","whisper"]},
        {"id":"K05","cat":"keyword","q":"LangGraph agent pipeline","expect":["langgraph","agent"]},
        {"id":"S01","cat":"semantic","q":"database connection issues","expect":["pgbouncer","pooling","postgres"]},
        {"id":"S02","cat":"semantic","q":"audio processing architecture","expect":["audio engine","core audio","mixer"]},
        {"id":"S03","cat":"semantic","q":"how to handle payments in the app","expect":["storekit","subscription","purchase"]},
        {"id":"S04","cat":"semantic","q":"speech recognition performance","expect":["whisper","transcription","latency"]},
        {"id":"S05","cat":"semantic","q":"background task reliability","expect":["celery","idempotent","retry"]},
        {"id":"P01","cat":"person","q":"what does Alex work on","expect":["alex","mixer","refactor"]},
        {"id":"P02","cat":"person","q":"Morgan's design work","expect":["morgan","onboarding","design"]},
        {"id":"P03","cat":"person","q":"what did Taylor improve","expect":["taylor","whisper","latency"]},
        {"id":"P04","cat":"person","q":"Jordan's proposal","expect":["jordan","django"]},
        {"id":"P05","cat":"person","q":"Sam audio tuning","expect":["sam","reverb","preset"]},
        {"id":"L01","cat":"learning","q":"JWT authentication bug","expect":["jwt","kid","401","auth0"]},
        {"id":"L02","cat":"learning","q":"audio crashes in background","expect":["background","interruption","audio"]},
        {"id":"L03","cat":"learning","q":"memory leak in Combine","expect":["combine","retain","memory"]},
        {"id":"L04","cat":"learning","q":"SwiftUI performance problems","expect":["swiftui","list","scroll","performance"]},
        {"id":"L05","cat":"learning","q":"purchase not completing","expect":["transaction","finish","storekit"]},
        {"id":"X01","cat":"cross","q":"which project uses LiveKit","expect":["livekit","beacon"]},
        {"id":"X02","cat":"cross","q":"which apps use HealthKit","expect":["healthkit","aurora","sleep"]},
        {"id":"X03","cat":"cross","q":"where do we use Redis","expect":["redis","beacon","celery"]},
        {"id":"H01","cat":"howto","q":"how to run tests locally","expect":["xcodebuild","test","simulator"]},
        {"id":"H02","cat":"howto","q":"how to deploy to staging","expect":["staging","deploy","ecs"]},
        {"id":"R01","cat":"rule","q":"audio thread safety rules","expect":["realtime","lock","malloc","callback"]},
        {"id":"R02","cat":"rule","q":"AWS credentials policy","expect":["aws","key","commit","secret"]},
        {"id":"M01","cat":"metrics","q":"Aurora monthly active users","expect":["12k","monthly","active","growth"]},
        {"id":"M02","cat":"metrics","q":"app crash rate","expect":["crash","99.6","99.2"]},
        {"id":"D01","cat":"dedup","q":"Beacon database connection pool config","expect":["pgbouncer","400","transaction"]}
    ]"#;
    serde_json::from_str(raw).unwrap()
}

fn check_hit(item: &codeterm_lib::mem::MemItem, expect: &[String]) -> bool {
    let hay = format!("{} {}", item.title.to_lowercase(), item.body.to_lowercase());
    let matches: usize = expect.iter().filter(|w| hay.contains(&w.to_lowercase())).count();
    if expect.len() <= 2 { matches >= 1 } else { matches >= 2 }
}

fn run_variant(name: &str, mgr: &MemManager, cases: &[TestCase], cfg: &SearchConfig) {
    let mut hits_at_1 = 0;
    let mut hits_at_3 = 0;
    let mut hits_at_5 = 0;
    let mut mrr = 0.0;
    let mut latencies: Vec<u128> = Vec::new();
    let mut cat_stats: std::collections::BTreeMap<String, (usize, usize)> = std::collections::BTreeMap::new();

    for tc in cases {
        let q = SearchQuery {
            query: tc.q.clone(),
            kind: None,
            scope: None,
            area: None,
            limit: Some(5),
        };
        let t0 = Instant::now();
        let hits = mgr.recall_with_config(&q, cfg).unwrap_or_default();
        latencies.push(t0.elapsed().as_micros());

        let mut rank_hit: Option<usize> = None;
        for (rank, h) in hits.iter().enumerate() {
            if check_hit(&h.item, &tc.expect) {
                rank_hit = Some(rank + 1);
                break;
            }
        }

        let entry = cat_stats.entry(tc.cat.clone()).or_insert((0, 0));
        entry.1 += 1;
        if let Some(r) = rank_hit {
            entry.0 += 1;
            mrr += 1.0 / r as f64;
            if r == 1 { hits_at_1 += 1; }
            if r <= 3 { hits_at_3 += 1; }
            if r <= 5 { hits_at_5 += 1; }
        }
    }

    let total = cases.len() as f64;
    let p50 = percentile(&latencies, 50);
    let p95 = percentile(&latencies, 95);

    println!("┌─ {} ─────────────────────────────────────────────────", name);
    println!("│ Recall@1  = {:.1}% ({}/{})", 100.0 * hits_at_1 as f64 / total, hits_at_1, cases.len());
    println!("│ Recall@3  = {:.1}% ({}/{})", 100.0 * hits_at_3 as f64 / total, hits_at_3, cases.len());
    println!("│ Recall@5  = {:.1}% ({}/{})", 100.0 * hits_at_5 as f64 / total, hits_at_5, cases.len());
    println!("│ MRR       = {:.3}", mrr / total);
    println!("│ p50/p95   = {:.1}ms / {:.1}ms", p50 as f64 / 1000.0, p95 as f64 / 1000.0);
    print!("│ by cat:");
    for (cat, (pass, total)) in &cat_stats {
        print!(" {}={}/{}", cat, pass, total);
    }
    println!();
    println!("└─────────────────────────────────────────────────────────\n");
}

fn percentile(v: &[u128], p: usize) -> u128 {
    if v.is_empty() { return 0; }
    let mut s = v.to_vec();
    s.sort();
    let idx = (s.len() * p / 100).min(s.len() - 1);
    s[idx]
}

fn main() {
    let data_dir = std::env::args()
        .skip_while(|a| a != "--data-dir")
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let home = std::env::var("HOME").unwrap_or_default();
            PathBuf::from(home).join(".codeterm-dev/mem")
        });

    println!("Loading mem from: {}", data_dir.display());
    let mgr = MemManager::open_offline(&data_dir)
        .expect("open mem manager");

    let cases = load_cases();
    println!("Running {} queries across variants...\n", cases.len());

    let variants: Vec<(&str, SearchConfig)> = vec![
        ("BASELINE (current defaults)", SearchConfig::default()),
        ("A: RRF k=20 (top-1 dominates more)", SearchConfig { rrf_k: 20.0, ..Default::default() }),
        ("B: drop stop-words from FTS query", SearchConfig { fts_drop_stopwords: true, ..Default::default() }),
        ("C: softer auto-extracted penalty (0.8)", SearchConfig { auto_extracted_penalty: 0.8, ..Default::default() }),
        ("D: higher recurrence cap (0.5)", SearchConfig { recurrence_cap: 0.5, ..Default::default() }),
        ("E: ALL COMBINED (k=20 + drop stops + 0.8 extracted + 0.5 recurrence)", SearchConfig {
            rrf_k: 20.0,
            auto_extracted_penalty: 0.8,
            recurrence_cap: 0.5,
            fts_drop_stopwords: true,
            ..Default::default()
        }),
        ("F: stronger keyword overlap (0.5)", SearchConfig { keyword_overlap_weight: 0.5, ..Default::default() }),
    ];

    for (name, cfg) in &variants {
        run_variant(name, &mgr, &cases, cfg);
    }
}
