//! mem-recall bench runner — deterministic, offline, LLM-free.
//!
//! Seeds a fixture into a fresh mem store, runs every test case through
//! the real search path (FTS5 hybrid + vectors if available), records
//! Recall@K + latency, and emits both JSON results and a self-contained
//! HTML report.
//!
//! Usage:
//!   cargo run -p codeterm --bin mem-bench -- \
//!       --fixture evals/mem-recall/benchmark.json \
//!       --output-dir evals/mem-recall/results \
//!       --report-dir evals/mem-recall/reports
//!
//! Optional: `--baseline path/to/baseline.json` adds a diff section,
//! `--save-baseline` writes a `baseline-<commit>.json` copy into
//! `evals/mem-recall/baselines/`.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use codeterm_lib::mem::{
    store::{MemKind, MemScope, NewMemItem},
    MemManager, SearchQuery,
};
use serde::{Deserialize, Serialize};

#[path = "../bin_support/mem_bench_decision.rs"]
mod mem_bench_decision;
use mem_bench_decision::{DecisionReport, FixtureGraph, Mode, RankingMetrics};

// ── CLI ─────────────────────────────────────────────────────────────

#[derive(Debug)]
struct Args {
    fixture: PathBuf,
    db: Option<PathBuf>,
    output_dir: PathBuf,
    report_dir: PathBuf,
    baseline: Option<PathBuf>,
    save_baseline: bool,
    top_k: usize,
    decision_rerank: Mode,
    decision_data_dir: Option<PathBuf>,
    decision_graph: usize,
    decision_docs: usize,
    decision_sessions: usize,
}

impl Args {
    fn parse() -> Result<Self, String> {
        let mut it = std::env::args().skip(1);
        let mut fixture = PathBuf::from("evals/mem-recall/benchmark.json");
        let mut db: Option<PathBuf> = None;
        let mut output_dir = PathBuf::from("evals/mem-recall/results");
        let mut report_dir = PathBuf::from("evals/mem-recall/reports");
        let mut baseline: Option<PathBuf> = None;
        let mut save_baseline = false;
        let mut top_k: usize = 10;
        let mut decision_rerank = Mode::Off;
        let mut decision_data_dir = None;
        let mut decision_graph = 0;
        let mut decision_docs = 0;
        let mut decision_sessions = 0;

        while let Some(arg) = it.next() {
            match arg.as_str() {
                "--fixture" => {
                    fixture = it.next().ok_or("--fixture needs value")?.into();
                }
                "--db" => {
                    db = Some(it.next().ok_or("--db needs value")?.into());
                }
                "--output-dir" => {
                    output_dir = it.next().ok_or("--output-dir needs value")?.into();
                }
                "--report-dir" => {
                    report_dir = it.next().ok_or("--report-dir needs value")?.into();
                }
                "--baseline" => {
                    baseline = Some(it.next().ok_or("--baseline needs value")?.into());
                }
                "--save-baseline" => {
                    save_baseline = true;
                }
                "--decision-rerank" => {
                    decision_rerank =
                        Mode::parse(&it.next().ok_or("--decision-rerank needs value")?)?;
                }
                "--decision-data-dir" => {
                    decision_data_dir =
                        Some(it.next().ok_or("--decision-data-dir needs value")?.into());
                }
                "--decision-graph" => {
                    decision_graph = it
                        .next()
                        .ok_or("--decision-graph needs value")?
                        .parse()
                        .map_err(|e| format!("--decision-graph: {}", e))?;
                }
                "--decision-docs" => {
                    decision_docs = it
                        .next()
                        .ok_or("--decision-docs needs value")?
                        .parse()
                        .map_err(|e| format!("--decision-docs: {}", e))?;
                }
                "--decision-sessions" => {
                    decision_sessions = it
                        .next()
                        .ok_or("--decision-sessions needs value")?
                        .parse()
                        .map_err(|e| format!("--decision-sessions: {}", e))?;
                }
                "--top-k" => {
                    top_k = it
                        .next()
                        .ok_or("--top-k needs value")?
                        .parse()
                        .map_err(|e| format!("--top-k: {}", e))?;
                }
                "-h" | "--help" => {
                    print_help();
                    std::process::exit(0);
                }
                other => return Err(format!("unknown arg: {}", other)),
            }
        }

        if decision_graph > 0 && decision_rerank == Mode::Off {
            return Err("--decision-graph requires --decision-rerank stub|live".into());
        }
        if decision_docs > 0 && decision_rerank == Mode::Off {
            return Err("--decision-docs requires --decision-rerank stub|live".into());
        }
        if decision_docs > 10 {
            return Err("--decision-docs must be 0..=10".into());
        }
        if decision_sessions > 0 && decision_rerank == Mode::Off {
            return Err("--decision-sessions requires --decision-rerank stub|live".into());
        }
        if decision_sessions > 5 {
            return Err("--decision-sessions must be 0..=5".into());
        }
        Ok(Self {
            fixture,
            db,
            output_dir,
            report_dir,
            baseline,
            save_baseline,
            top_k,
            decision_rerank,
            decision_data_dir,
            decision_graph,
            decision_docs,
            decision_sessions,
        })
    }
}

fn print_help() {
    println!(
        "mem-bench — deterministic mem-recall bench runner\n\n\
         Usage: mem-bench [FLAGS]\n\n\
         Flags:\n  \
           --fixture PATH      Fixture JSON (default: evals/mem-recall/benchmark.json)\n  \
           --db PATH           SQLite db path (default: tempfile)\n  \
           --output-dir PATH   Where to write <timestamp>.json\n  \
           --report-dir PATH   Where to write <timestamp>.html\n  \
           --baseline PATH     Baseline results JSON for diff\n  \
           --save-baseline     Also write baseline-<commit>.json to baselines/\n  \
           --top-k N           Top-K to fetch per query (default 10)\n  \
           --decision-rerank MODE  off|stub|live (default off)\n  \
           --decision-data-dir PATH  Data directory for live token\n  \
           --decision-graph M  Add a re-rank+graph arm with up to M graph candidates (default 0)\n  \
           --decision-docs M  Add a re-rank+docs arm with up to M prod-docs sections (default 0)\n  \
           --decision-sessions M  Add a re-rank+sessions arm with up to M fixture snippets (default 0)\n  \
           -h, --help          Show this help\n"
    );
}

// ── Fixture schema ──────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct Fixture {
    #[serde(default)]
    description: String,
    seed_data: Vec<SeedItem>,
    test_cases: Vec<TestCase>,
    #[serde(default)]
    graph: FixtureGraph,
}

#[derive(Debug, Deserialize)]
struct SeedItem {
    id: String,
    kind: String,
    title: String,
    body: String,
    #[serde(default)]
    area: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    importance: Option<f64>,
    #[serde(default)]
    summary: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
struct TestCase {
    id: String,
    #[serde(default)]
    name: String,
    query: String,
    #[serde(default)]
    expected_top: Option<String>,
    #[serde(default)]
    expected_top_any: Option<Vec<String>>,
    /// Optional list of substrings the winning id must contain. We accept
    /// the field so fixture JSON validates, but the core metric is
    /// expected_top / expected_top_any.
    #[serde(default)]
    #[allow(dead_code)]
    expected_id_contains: Option<Vec<String>>,
    #[serde(default)]
    category: String,
    #[serde(default)]
    session_candidates: Vec<SessionCandidateFixture>,
}

#[derive(Debug, Deserialize, Clone)]
struct SessionCandidateFixture {
    session_id: String,
    provider: String,
    title: String,
    excerpt: String,
    #[serde(default)]
    modified_ts: Option<i64>,
}

impl TestCase {
    fn expected_set(&self) -> Vec<String> {
        let mut out = Vec::new();
        if let Some(ref e) = self.expected_top {
            out.push(e.clone());
        }
        if let Some(ref list) = self.expected_top_any {
            for id in list {
                if !out.contains(id) {
                    out.push(id.clone());
                }
            }
        }
        out
    }
}

// ── Results ─────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, Clone)]
struct QueryResult {
    test_id: String,
    name: String,
    category: String,
    query: String,
    expected: Vec<String>,
    got_top_k: Vec<String>,
    hit_rank: Option<usize>, // 1-indexed rank of best expected match, None if miss
    hit: bool,
    latency_ms: f64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct Metrics {
    num_queries: usize,
    recall_at_1: f64,
    recall_at_5: f64,
    recall_at_10: f64,
    mrr: f64,
    p50_latency_ms: f64,
    p95_latency_ms: f64,
    total_wall_ms: f64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct BenchReport {
    commit: String,
    timestamp: String,
    fixture_path: String,
    fixture_description: String,
    top_k: usize,
    metrics: Metrics,
    per_query: Vec<QueryResult>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    decision: Option<DecisionReport>,
}

// ── Main ────────────────────────────────────────────────────────────

fn main() {
    if let Err(e) = run() {
        eprintln!("mem-bench error: {}", e);
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let args = Args::parse()?;

    let decision_model =
        mem_bench_decision::prepare_model(args.decision_rerank, args.decision_data_dir.as_deref())?;

    let fixture_raw = fs::read_to_string(&args.fixture)
        .map_err(|e| format!("read fixture {}: {}", args.fixture.display(), e))?;
    let fixture: Fixture =
        serde_json::from_str(&fixture_raw).map_err(|e| format!("parse fixture: {}", e))?;

    let db_dir = match args.db.as_ref() {
        Some(p) => p.clone(),
        None => std::env::temp_dir().join(format!(
            "mem-bench-{}",
            chrono::Utc::now().format("%Y%m%d-%H%M%S%3f")
        )),
    };
    // If db points to a file path, we use its parent. Easier: treat it as a dir.
    let mem = MemManager::open_offline(&db_dir).map_err(|e| format!("open_offline: {}", e))?;

    // 3. Seed — exercise the normal insert path. Keep a map from the engine-generated id back to the fixture id so we can score recall against the fixture's stable identifiers.
    let mut id_map: HashMap<String, String> = HashMap::new();
    for seed in &fixture.seed_data {
        let kind = MemKind::from_str(&seed.kind)
            .ok_or_else(|| format!("unknown kind in seed {}: {}", seed.id, seed.kind))?;
        let new = NewMemItem {
            kind,
            scope: MemScope::Global,
            title: seed.title.clone(),
            summary: seed.summary.clone(),
            area: seed.area.clone(),
            tags: seed.tags.clone(),
            body: seed.body.clone(),
            source: None,
            importance: seed.importance.unwrap_or(3.0),
            source_path: None,
            pinned: false,
        };
        // insert_raw skips auto-extract + embed queue so bench recall isn't polluted by extracted child items.
        let generated = mem
            .insert_raw(&new)
            .map_err(|e| format!("seed insert {}: {}", seed.id, e))?;
        id_map.insert(generated, seed.id.clone());
    }
    let engine_ids: HashMap<String, String> = id_map
        .iter()
        .map(|(engine, seed)| (seed.clone(), engine.clone()))
        .collect();
    mem_bench_decision::seed_graph(&mem, &fixture.graph, &engine_ids)?;

    println!(
        "seeded {} items into {} ({} test cases)",
        fixture.seed_data.len(),
        db_dir.display(),
        fixture.test_cases.len()
    );

    let wall_start = Instant::now();
    let mut results: Vec<QueryResult> = Vec::with_capacity(fixture.test_cases.len());

    for tc in &fixture.test_cases {
        let q = SearchQuery {
            query: tc.query.clone(),
            kind: None,
            scope: None,
            area: None,
            limit: Some(args.top_k),
        };
        let t0 = Instant::now();
        let hits = mem
            .recall(&q)
            .map_err(|e| format!("recall {}: {}", tc.id, e))?;
        let latency_ms = t0.elapsed().as_secs_f64() * 1000.0;

        // Translate engine ids -> fixture ids. Any hit not in the map is a child extraction we don't care about; skip it so it doesn't push real candidates down the ranking.
        let got_top_k: Vec<String> = hits
            .iter()
            .filter_map(|h| id_map.get(&h.item.id).cloned())
            .collect();

        let expected = tc.expected_set();
        let hit_rank = got_top_k
            .iter()
            .position(|id| expected.iter().any(|e| e == id))
            .map(|i| i + 1);

        results.push(QueryResult {
            test_id: tc.id.clone(),
            name: tc.name.clone(),
            category: tc.category.clone(),
            query: tc.query.clone(),
            expected,
            got_top_k,
            hit_rank,
            hit: hit_rank.is_some(),
            latency_ms,
        });
    }
    let total_wall_ms = wall_start.elapsed().as_secs_f64() * 1000.0;

    let metrics = compute_metrics(&results, total_wall_ms);

    let commit = git_commit();
    let timestamp = chrono::Utc::now().format("%Y%m%d-%H%M%S").to_string();
    let decision = match decision_model {
        Some(model) => Some(mem_bench_decision::evaluate(
            args.decision_rerank,
            model,
            &fixture,
            &args.fixture,
            &mem,
            &id_map,
            args.top_k,
            args.decision_graph,
            args.decision_docs,
            args.decision_sessions,
        )?),
        None => None,
    };
    let report = BenchReport {
        commit: commit.clone(),
        timestamp: timestamp.clone(),
        fixture_path: args.fixture.display().to_string(),
        fixture_description: fixture.description.clone(),
        top_k: args.top_k,
        metrics: metrics.clone(),
        per_query: results.clone(),
        decision,
    };

    fs::create_dir_all(&args.output_dir).map_err(|e| format!("create output dir: {}", e))?;
    let json_path = args.output_dir.join(format!("{}.json", timestamp));
    let json_body =
        serde_json::to_string_pretty(&report).map_err(|e| format!("serialize report: {}", e))?;
    fs::write(&json_path, &json_body)
        .map_err(|e| format!("write {}: {}", json_path.display(), e))?;
    println!("wrote {}", json_path.display());

    let baseline: Option<BenchReport> = match args.baseline.as_ref() {
        Some(p) => {
            let raw = fs::read_to_string(p)
                .map_err(|e| format!("read baseline {}: {}", p.display(), e))?;
            let br: BenchReport =
                serde_json::from_str(&raw).map_err(|e| format!("parse baseline: {}", e))?;
            Some(br)
        }
        None => None,
    };

    fs::create_dir_all(&args.report_dir).map_err(|e| format!("create report dir: {}", e))?;
    let html_path = args.report_dir.join(format!("{}.html", timestamp));
    let html = render_html(&report, baseline.as_ref());
    fs::write(&html_path, html).map_err(|e| format!("write {}: {}", html_path.display(), e))?;
    println!("wrote {}", html_path.display());

    if args.save_baseline {
        let baselines_dir = args
            .output_dir
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("baselines");
        fs::create_dir_all(&baselines_dir).map_err(|e| format!("create baselines dir: {}", e))?;
        let short = commit.chars().take(8).collect::<String>();
        let baseline_path = baselines_dir.join(format!("baseline-{}.json", short));
        fs::write(&baseline_path, &json_body).map_err(|e| format!("write baseline copy: {}", e))?;
        println!("wrote {}", baseline_path.display());
    }

    print_summary(&metrics, &results);

    Ok(())
}

// ── Metrics ─────────────────────────────────────────────────────────

fn compute_metrics(results: &[QueryResult], total_wall_ms: f64) -> Metrics {
    let n = results.len().max(1);
    let r_at = |k: usize| -> f64 {
        let hits = results
            .iter()
            .filter(|r| r.hit_rank.map(|rk| rk <= k).unwrap_or(false))
            .count();
        hits as f64 / n as f64
    };
    let mrr: f64 = results
        .iter()
        .map(|r| r.hit_rank.map(|rk| 1.0 / rk as f64).unwrap_or(0.0))
        .sum::<f64>()
        / n as f64;
    let mut lats: Vec<f64> = results.iter().map(|r| r.latency_ms).collect();
    lats.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let p = |q: f64| -> f64 {
        if lats.is_empty() {
            return 0.0;
        }
        let idx = ((lats.len() as f64 - 1.0) * q).round() as usize;
        lats[idx.min(lats.len() - 1)]
    };
    Metrics {
        num_queries: results.len(),
        recall_at_1: r_at(1),
        recall_at_5: r_at(5),
        recall_at_10: r_at(10),
        mrr,
        p50_latency_ms: p(0.5),
        p95_latency_ms: p(0.95),
        total_wall_ms,
    }
}

fn print_summary(m: &Metrics, results: &[QueryResult]) {
    println!("\n── mem-recall bench summary ──");
    println!("queries        : {}", m.num_queries);
    println!("recall@1       : {:.3}", m.recall_at_1);
    println!("recall@5       : {:.3}", m.recall_at_5);
    println!("recall@10      : {:.3}", m.recall_at_10);
    println!("MRR            : {:.3}", m.mrr);
    println!("p50 latency    : {:.2} ms", m.p50_latency_ms);
    println!("p95 latency    : {:.2} ms", m.p95_latency_ms);
    println!("total wall     : {:.2} ms", m.total_wall_ms);
    println!();
    println!(
        "{:<6} {:<6} {:<14} hit rank  query",
        "id", "hit", "category"
    );
    for r in results {
        let rank = r
            .hit_rank
            .map(|n| n.to_string())
            .unwrap_or_else(|| "—".into());
        println!(
            "{:<6} {:<6} {:<14} {:<9} {}",
            r.test_id,
            if r.hit { "OK" } else { "MISS" },
            r.category,
            rank,
            truncate(&r.query, 50)
        );
    }
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(n - 1).collect();
        out.push('…');
        out
    }
}

// ── Git ─────────────────────────────────────────────────────────────

fn git_commit() -> String {
    match Command::new("git").args(["rev-parse", "HEAD"]).output() {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        _ => "unknown".to_string(),
    }
}

// ── HTML report ─────────────────────────────────────────────────────

fn render_html(report: &BenchReport, baseline: Option<&BenchReport>) -> String {
    let mut out = String::new();
    out.push_str("<!doctype html><html><head><meta charset=\"utf-8\">");
    out.push_str(&format!(
        "<title>mem-recall bench {}</title>",
        report.timestamp
    ));
    out.push_str("<style>\n");
    out.push_str(CSS);
    out.push_str("</style></head><body>");

    out.push_str("<header>");
    out.push_str("<h1>mem-recall bench</h1>");
    out.push_str(&format!(
        "<div class=\"meta\">commit <code>{}</code> &middot; run {} &middot; fixture <code>{}</code></div>",
        html_escape(&report.commit),
        html_escape(&report.timestamp),
        html_escape(&report.fixture_path)
    ));
    if !report.fixture_description.is_empty() {
        out.push_str(&format!(
            "<p class=\"desc\">{}</p>",
            html_escape(&report.fixture_description)
        ));
    }
    out.push_str("</header>");

    out.push_str("<section><h2>Metrics</h2><table class=\"metrics\"><thead><tr>");
    out.push_str("<th>metric</th><th>value</th>");
    if baseline.is_some() {
        out.push_str("<th>baseline</th><th>&Delta;</th>");
    }
    out.push_str("</tr></thead><tbody>");

    let rows: &[(&str, fn(&Metrics) -> f64, &str, bool)] = &[
        ("queries", |m| m.num_queries as f64, "{:.0}", false),
        ("recall@1", |m| m.recall_at_1, "{:.3}", true),
        ("recall@5", |m| m.recall_at_5, "{:.3}", true),
        ("recall@10", |m| m.recall_at_10, "{:.3}", true),
        ("MRR", |m| m.mrr, "{:.3}", true),
        ("p50 ms", |m| m.p50_latency_ms, "{:.2}", false),
        ("p95 ms", |m| m.p95_latency_ms, "{:.2}", false),
        ("total wall ms", |m| m.total_wall_ms, "{:.2}", false),
    ];
    for (label, getter, fmt, higher_better) in rows {
        let v = getter(&report.metrics);
        out.push_str("<tr><td>");
        out.push_str(label);
        out.push_str("</td><td>");
        out.push_str(&format_value(v, fmt));
        out.push_str("</td>");
        if let Some(b) = baseline {
            let bv = getter(&b.metrics);
            out.push_str("<td>");
            out.push_str(&format_value(bv, fmt));
            out.push_str("</td><td class=\"");
            let delta = v - bv;
            let cls = if delta.abs() < 1e-9 {
                "zero"
            } else if (*higher_better && delta > 0.0) || (!*higher_better && delta < 0.0) {
                "up"
            } else {
                "down"
            };
            out.push_str(cls);
            out.push_str("\">");
            let sign = if delta >= 0.0 { "+" } else { "" };
            out.push_str(&format!("{}{:.3}", sign, delta));
            out.push_str("</td>");
        }
        out.push_str("</tr>");
    }
    out.push_str("</tbody></table></section>");

    out.push_str("<section><h2>Per-query results</h2><table class=\"queries\"><thead><tr>");
    out.push_str("<th>id</th><th>cat</th><th>query</th><th>expected</th><th>got (top 10)</th><th>rank</th><th>ms</th>");
    out.push_str("</tr></thead><tbody>");
    for r in &report.per_query {
        let row_cls = if r.hit { "hit" } else { "miss" };
        out.push_str(&format!(
            "<tr class=\"{}\"><td>{}</td>",
            row_cls,
            html_escape(&r.test_id)
        ));
        out.push_str(&format!("<td>{}</td>", html_escape(&r.category)));
        out.push_str(&format!("<td>{}</td>", html_escape(&r.query)));
        out.push_str(&format!(
            "<td><code>{}</code></td>",
            html_escape(&r.expected.join(", "))
        ));
        let got_html = r
            .got_top_k
            .iter()
            .enumerate()
            .map(|(i, id)| {
                let cls = if r.expected.contains(id) {
                    "got-hit"
                } else {
                    ""
                };
                format!(
                    "<span class=\"{}\">{}. {}</span>",
                    cls,
                    i + 1,
                    html_escape(id)
                )
            })
            .collect::<Vec<_>>()
            .join("<br>");
        out.push_str(&format!("<td>{}</td>", got_html));
        out.push_str(&format!(
            "<td>{}</td>",
            r.hit_rank
                .map(|n| n.to_string())
                .unwrap_or_else(|| "—".into())
        ));
        out.push_str(&format!("<td>{:.2}</td></tr>", r.latency_ms));
    }
    out.push_str("</tbody></table></section>");

    if let Some(d) = &report.decision {
        let graph_head = if d.graph.is_some() {
            "<th>re-rank+graph</th>"
        } else {
            ""
        };
        let sessions_head = if d.sessions.is_some() {
            "<th>re-rank+sessions</th>"
        } else {
            ""
        };
        out.push_str(&format!("<section><h2>Decision re-rank A/B</h2><table><tr><th>metric</th><th>baseline</th><th>reranked</th>{graph_head}{sessions_head}</tr>"));
        let graph_metrics = d.graph.as_ref().map(|g| &g.metrics);
        let sessions_metrics = d.sessions.as_ref().map(|s| &s.metrics);
        let rows: [(&str, fn(&RankingMetrics) -> f64); 3] = [
            ("Recall@1", |m| m.recall_at_1),
            ("Recall@K", |m| m.recall_at_k),
            ("MRR", |m| m.mrr),
        ];
        for (label, pick) in rows {
            let (before, after) = (pick(&d.baseline), pick(&d.reranked));
            let graph_cell = graph_metrics
                .map(|g| format!("<td>{:.3}</td>", pick(g)))
                .unwrap_or_default();
            let sessions_cell = sessions_metrics
                .map(|s| format!("<td>{:.3}</td>", pick(s)))
                .unwrap_or_default();
            out.push_str(&format!(
                "<tr><td>{label}</td><td>{before:.3}</td><td>{after:.3}</td>{graph_cell}{sessions_cell}</tr>"
            ));
        }
        out.push_str(&format!("</table><p>Negative low-confidence precision: {:.3}; positive false-positive rate: {:.3}</p>", d.negatives.low_confidence_precision, d.negatives.positive_low_confidence_false_positive_rate));
        if let Some(g) = &d.graph {
            out.push_str(&format!(
                "<p>Graph source (M={}): {} candidates added; top-1 from graph {}; graph-only winners {}</p>",
                g.sources_graph, g.added, g.graph_only_top1, g.graph_only_winners
            ));
        }
        if let Some(s) = &d.sessions {
            out.push_str(&format!(
                "<p>Sessions source (M={}): {} fixture candidates added; top-1 from sessions {}; session-only winners {}</p>",
                s.sources_sessions, s.added, s.sessions_top1, s.sessions_only_winners
            ));
        }
        out.push_str("</section>");
    }
    if let Some(b) = baseline {
        out.push_str("<section><h2>vs baseline</h2>");
        out.push_str(&format!(
            "<p>baseline commit <code>{}</code>, run {}</p>",
            html_escape(&b.commit),
            html_escape(&b.timestamp)
        ));
        out.push_str("<table class=\"queries\"><thead><tr><th>id</th><th>was</th><th>now</th><th>delta</th></tr></thead><tbody>");
        let base_map: HashMap<&str, &QueryResult> = b
            .per_query
            .iter()
            .map(|r| (r.test_id.as_str(), r))
            .collect();
        for r in &report.per_query {
            let was = base_map
                .get(r.test_id.as_str())
                .map(|p| {
                    if p.hit {
                        format!("OK @{}", p.hit_rank.unwrap_or(0))
                    } else {
                        "MISS".to_string()
                    }
                })
                .unwrap_or_else(|| "—".into());
            let now = if r.hit {
                format!("OK @{}", r.hit_rank.unwrap_or(0))
            } else {
                "MISS".to_string()
            };
            let delta_cls = match (
                base_map
                    .get(r.test_id.as_str())
                    .map(|p| p.hit)
                    .unwrap_or(false),
                r.hit,
            ) {
                (true, false) => "down",
                (false, true) => "up",
                _ => "zero",
            };
            out.push_str(&format!(
                "<tr><td>{}</td><td>{}</td><td>{}</td><td class=\"{}\">—</td></tr>",
                html_escape(&r.test_id),
                html_escape(&was),
                html_escape(&now),
                delta_cls
            ));
        }
        out.push_str("</tbody></table></section>");
    }

    out.push_str("</body></html>");
    out
}

fn format_value(v: f64, fmt: &str) -> String {
    match fmt {
        "{:.0}" => format!("{:.0}", v),
        "{:.2}" => format!("{:.2}", v),
        "{:.3}" => format!("{:.3}", v),
        _ => format!("{}", v),
    }
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

const CSS: &str = r#"
* { box-sizing: border-box; }
body {
  font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
  margin: 0; padding: 24px; background: #0f1115; color: #e4e6ea;
}
header { border-bottom: 1px solid #2a2f3a; padding-bottom: 16px; margin-bottom: 24px; }
h1 { margin: 0 0 6px 0; font-size: 22px; }
.meta { font-size: 12px; color: #8b93a6; }
.meta code { color: #b4f9a0; }
.desc { color: #a8b0c4; font-size: 13px; margin: 10px 0 0 0; }
section { margin-bottom: 32px; }
h2 { font-size: 16px; border-bottom: 1px solid #2a2f3a; padding-bottom: 6px; }
table { width: 100%; border-collapse: collapse; font-size: 12px; }
th, td { padding: 6px 10px; text-align: left; border-bottom: 1px solid #1e222b; vertical-align: top; }
th { color: #8b93a6; font-weight: 500; text-transform: uppercase; letter-spacing: 0.5px; font-size: 11px; }
code { font-family: "SF Mono", Menlo, monospace; font-size: 11px; }
table.metrics td:nth-child(2) { font-family: "SF Mono", Menlo, monospace; }
table.queries tr.hit { background: rgba(80, 180, 120, 0.06); }
table.queries tr.miss { background: rgba(220, 80, 80, 0.08); }
.got-hit { color: #b4f9a0; font-weight: 500; }
.up { color: #b4f9a0; }
.down { color: #ff8080; }
.zero { color: #6b7280; }
"#;

#[cfg(test)]
mod decision_mode_tests {
    use super::*;
    #[test]
    fn decision_mode_off_adds_no_keys() {
        let report = BenchReport {
            commit: String::new(),
            timestamp: String::new(),
            fixture_path: String::new(),
            fixture_description: String::new(),
            top_k: 10,
            metrics: compute_metrics(&[], 0.0),
            per_query: vec![],
            decision: None,
        };
        let value = serde_json::to_value(report).unwrap();
        assert!(value.get("decision").is_none());
    }
}
