mod embedding;
mod fixture;
mod report;

use codeterm_mem::backend::SqliteMemStore;
use codeterm_mem_postgres::PostgresOptions;
use sha2::{Digest, Sha256};
use std::{path::PathBuf, process::Command};

const POSTGRES_URL: &str = "postgresql://domios_parity:domios_parity@127.0.0.1:55439/domios_parity";

struct Options {
    embedder: String,
    model: Option<String>,
    out: PathBuf,
}

fn options() -> Result<Options, String> {
    let mut args = std::env::args().skip(1);
    let mut mode = "synthetic".to_string();
    let mut model = None;
    let mut out = None;
    while let Some(flag) = args.next() {
        let value = args
            .next()
            .ok_or_else(|| format!("missing value for {flag}"))?;
        match flag.as_str() {
            "--embedder" => mode = value,
            "--local-model" => model = Some(value),
            "--out" => out = Some(PathBuf::from(value)),
            _ => return Err(format!("unknown argument: {flag}")),
        }
    }
    if !matches!(mode.as_str(), "synthetic" | "local") || (mode != "local" && model.is_some()) {
        return Err("use --embedder synthetic|local; --local-model requires local".into());
    }
    Ok(Options {
        embedder: mode,
        model,
        out: out.ok_or("--out is required")?,
    })
}

fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn run(options: Options) -> Result<i32, String> {
    let (corpus, queries) = fixture::read()?;
    let Some(prepared) = embedding::prepare(
        &options.embedder,
        options.model.as_deref(),
        &corpus,
        &queries,
    )?
    else {
        std::fs::write(
            options.out.join("SKIPPED"),
            "Local model unavailable; see run.log for the exact reason.\n",
        )
        .map_err(|e| e.to_string())?;
        return Ok(77);
    };
    let root = tempfile::tempdir().map_err(|e| e.to_string())?;
    let pages = fixture::seed(&root.path().join("seed"), &corpus, &prepared.passages)?;
    let input = serde_json::json!({
        "vector_source": options.embedder,
        "synthetic_notice": "Default vectors are synthetic normalized hashed-token vectors; storage and ranking parity, not neural retrieval quality.",
        "embedder": prepared.embedder.identity(),
        "corpus_items": corpus.len(), "queries": queries.len(),
        "corpus_sha256": hash(fixture::CORPUS.as_bytes()),
        "queries_sha256": hash(fixture::QUERIES.as_bytes()),
        "frozen_rows_sha256": hash(&serde_json::to_vec(&pages).map_err(|e| e.to_string())?),
        "query_vectors_sha256": hash(&serde_json::to_vec(&prepared.queries).map_err(|e| e.to_string())?),
        "search_config": format!("{:?}", codeterm_mem::search::SearchConfig::default()),
        "sqlite_vector_implementation": "exact scan without codeterm-mem/embeddings; sqlite-vec when that feature is enabled",
        "postgres_url": POSTGRES_URL,
    });
    println!("INPUTS {input}");
    std::fs::write(
        options.out.join("inputs.json"),
        serde_json::to_vec_pretty(&input).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let container = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../evals/mem-backend-parity/container.py");
    if !Command::new("python3")
        .arg(container)
        .status()
        .map_err(|e| e.to_string())?
        .success()
    {
        return Err("disposable Postgres container verification/readiness failed".into());
    }
    let schema = format!("parity_{}", ulid::Ulid::new().to_string().to_lowercase());
    let postgres = codeterm_mem_postgres::open(
        POSTGRES_URL,
        PostgresOptions {
            schema: schema.clone(),
            ..PostgresOptions::default()
        },
    )
    .map_err(|e| e.to_string())?;
    let result = (|| {
        let sqlite =
            SqliteMemStore::open(&root.path().join("measured.db"), &root.path().join("blobs"))?;
        fixture::load(&sqlite, &pages)?;
        fixture::load(postgres.as_ref(), &pages)?;
        println!(
            "LOADED: {} identical items and vectors; all exported rows match both stores",
            corpus.len()
        );
        let rows = report::measure(&sqlite, postgres.as_ref(), &queries, &prepared.embedder);
        let summary = report::summary(&rows);
        if fixture::pages(&sqlite)? != pages || fixture::pages(postgres.as_ref())? != pages {
            return Err("recall mutated the frozen fixture despite record_access=false".into());
        }
        let output = serde_json::json!({"inputs": input, "per_query": rows, "summary": summary});
        std::fs::write(
            options.out.join("report.json"),
            serde_json::to_vec_pretty(&output).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        println!("SUMMARY {}", output["summary"]);
        Ok(if output["summary"]["passed"] == true {
            0
        } else {
            1
        })
    })();
    drop(postgres);
    let cleanup = postgres::Client::connect(POSTGRES_URL, postgres::NoTls)
        .and_then(|mut client| client.batch_execute(&format!("DROP SCHEMA {schema} CASCADE")))
        .map_err(|e| format!("disposable schema cleanup failed: {e}"));
    cleanup?;
    result
}

fn main() {
    println!("Default vectors are synthetic (normalized hashed-token vectors): storage and ranking parity, not neural retrieval quality.");
    let cache = std::env::var_os("HF_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(codeterm_mem::model_cache_dir);
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("CODETERM_") {
            std::env::remove_var(key);
        }
    }
    std::env::set_var("HF_HOME", cache);
    std::env::set_var("HF_HUB_OFFLINE", "1");
    let outcome = options().and_then(run);
    match outcome {
        Ok(code) => std::process::exit(code),
        Err(error) => {
            eprintln!("ERROR: {error}");
            std::process::exit(2);
        }
    }
}
