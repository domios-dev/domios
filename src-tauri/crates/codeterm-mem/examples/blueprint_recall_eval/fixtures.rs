use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub fn eval_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../evals/blueprint-recall")
        .canonicalize()
        .expect("evals/blueprint-recall exists")
}

pub fn repo_dir() -> PathBuf {
    eval_dir()
        .join("../..")
        .canonicalize()
        .expect("repository root")
}

pub struct CorpusDoc {
    pub id: String,
    pub source: String,
}

pub fn corpus() -> Result<Vec<CorpusDoc>, String> {
    let dir = eval_dir().join("corpus");
    let mut docs = std::fs::read_dir(&dir)
        .map_err(|error| format!("{}: {error}", dir.display()))?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "yaml"))
        .map(|path| {
            Ok(CorpusDoc {
                id: path
                    .file_stem()
                    .and_then(|stem| stem.to_str())
                    .ok_or("corpus file name")?
                    .to_owned(),
                source: std::fs::read_to_string(&path).map_err(|error| error.to_string())?,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    docs.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(docs)
}

#[derive(Clone, Debug, Deserialize)]
pub struct Pair {
    pub a: String,
    pub b: String,
    pub label: String,
    pub split: String,
}

pub struct Gold {
    pub families: BTreeMap<String, Vec<String>>,
    pub pairs: Vec<Pair>,
}

pub fn gold() -> Result<Gold, String> {
    let value: Value = read_json(&eval_dir().join("gold.json"))?;
    Ok(Gold {
        families: serde_json::from_value(value["families"].clone())
            .map_err(|error| format!("gold families: {error}"))?,
        pairs: serde_json::from_value(value["pairs"].clone())
            .map_err(|error| format!("gold pairs: {error}"))?,
    })
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Query {
    pub set: String,
    pub original: String,
    pub text: String,
}

pub fn queries(path: &Path, set: &str) -> Result<Vec<Query>, String> {
    std::fs::read_to_string(path)
        .map_err(|error| format!("{}: {error}", path.display()))?
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let row: Value = serde_json::from_str(line).map_err(|error| error.to_string())?;
            Ok(Query {
                set: set.into(),
                original: row["original"]
                    .as_str()
                    .ok_or("query without original")?
                    .into(),
                text: row["text"].as_str().ok_or("query without text")?.into(),
            })
        })
        .collect()
}

pub const SELECTION_SETS: [(&str, &str); 3] = [
    ("a", "queries/set_a.jsonl"),
    ("b", "queries/set_b.jsonl"),
    ("b2", "queries/set_b2.jsonl"),
];

pub fn selection_pool() -> Result<Vec<Query>, String> {
    let mut pool = Vec::new();
    for (set, file) in SELECTION_SETS {
        pool.extend(queries(&eval_dir().join(file), set)?);
    }
    Ok(pool)
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct CachedIntents {
    pub document_hash: String,
    pub intents: Vec<String>,
    pub session: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct CachedEnrichment {
    pub document_hash: String,
    pub report: String,
    pub parsed: bool,
    pub session: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SaveInputs<T> {
    pub about: String,
    pub model: String,
    pub command: Vec<String>,
    pub prompt: String,
    pub docs: BTreeMap<String, T>,
}

pub fn intents_path() -> PathBuf {
    eval_dir().join("save-inputs/intents.json")
}

pub fn enrichment_path() -> PathBuf {
    eval_dir().join("save-inputs/enrichment.json")
}

pub fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, String> {
    serde_json::from_str(
        &std::fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?,
    )
    .map_err(|error| format!("{}: {error}", path.display()))
}

pub fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let text = serde_json::to_string_pretty(value).map_err(|error| error.to_string())?;
    std::fs::write(path, text + "\n").map_err(|error| format!("{}: {error}", path.display()))
}
