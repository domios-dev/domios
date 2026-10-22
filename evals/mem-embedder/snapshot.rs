use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use codeterm_blueprint_types::blueprint_memory::contains_credentials;
use codeterm_mem::embedder::{local, ActiveEmbedder};
use codeterm_mem::store::{MemKind, MemScope, NewMemItem};
use codeterm_mem::{MemManager, SearchQuery};
use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
struct SnapshotItem {
    #[serde(flatten)]
    item: NewMemItem,
    created_at: String,
    updated_at: String,
    accessed_at: Option<String>,
    access_count: i64,
    recurrence_count: i64,
}

#[derive(Deserialize)]
struct Query {
    gold: usize,
    text: String,
    language: String,
}

#[derive(Default, Serialize)]
struct Recall {
    n: usize,
    r1: f64,
    r5: f64,
    r10: f64,
}

impl Recall {
    fn add(&mut self, rank: Option<usize>) {
        self.n += 1;
        self.r1 += f64::from(rank.is_some_and(|rank| rank < 1));
        self.r5 += f64::from(rank.is_some_and(|rank| rank < 5));
        self.r10 += f64::from(rank.is_some_and(|rank| rank < 10));
    }

    fn finish(&mut self) {
        if self.n > 0 {
            self.r1 /= self.n as f64;
            self.r5 /= self.n as f64;
            self.r10 /= self.n as f64;
        }
    }
}

fn prepare(source_root: &Path, private: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let connection = Connection::open_with_flags(
        source_root.join("knowledge.db"),
        OpenFlags::SQLITE_OPEN_READ_ONLY,
    )?;
    let mut statement = connection.prepare(
        "SELECT kind,title,summary,area,tags_json,body_inline,body_blob_path,source,importance,is_pinned_wakeup,scope,created_at,updated_at,accessed_at,access_count,recurrence_count FROM mem_items WHERE kind != 'archived' ORDER BY id",
    )?;
    let raw = statement.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, Option<String>>(2)?,
            row.get::<_, Option<String>>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, Option<String>>(5)?,
            row.get::<_, Option<String>>(6)?,
            row.get::<_, Option<String>>(7)?,
            row.get::<_, f64>(8)?,
            row.get::<_, bool>(9)?,
            row.get::<_, String>(10)?,
            row.get::<_, String>(11)?,
            row.get::<_, String>(12)?,
            row.get::<_, Option<String>>(13)?,
            row.get::<_, i64>(14)?,
            row.get::<_, i64>(15)?,
        ))
    })?;
    let mut items = Vec::new();
    let mut excluded_credentials = 0;
    let mut excluded_short = 0;
    let mut source_files_detached = 0;
    let mut blueprint_solutions = 0;
    let mut kinds: BTreeMap<String, usize> = BTreeMap::new();
    for row in raw {
        let (
            kind,
            title,
            summary,
            area,
            tags,
            inline,
            blob,
            source,
            importance,
            pinned,
            scope,
            created_at,
            updated_at,
            accessed_at,
            access_count,
            recurrence_count,
        ) = row?;
        let body = match (inline, blob) {
            (Some(body), _) => body,
            (None, Some(blob)) => {
                if Path::new(&blob).is_absolute()
                    || Path::new(&blob)
                        .components()
                        .any(|part| matches!(part, std::path::Component::ParentDir))
                {
                    return Err("unsafe snapshot blob path".into());
                }
                std::fs::read_to_string(source_root.join("blobs").join(blob))?
            }
            _ => String::new(),
        };
        let mut item = NewMemItem {
            kind: MemKind::from_str(&kind).ok_or("unknown snapshot kind")?,
            scope: MemScope::from_str(&scope)?,
            title,
            summary,
            area,
            tags: serde_json::from_str(&tags)?,
            body,
            source,
            importance,
            source_path: None,
            pinned,
        };
        if contains_credentials(&serde_json::to_string(&item)?) {
            excluded_credentials += 1;
            continue;
        }
        if !codeterm_mem::body_is_embeddable(&item.body) {
            excluded_short += 1;
            continue;
        }
        if item
            .source
            .as_deref()
            .is_some_and(|value| value.starts_with("file:"))
        {
            item.source = Some("snapshot-file".into());
            source_files_detached += 1;
        }
        if item.kind == MemKind::Skill && item.source.as_deref() == Some("blueprint-library") {
            blueprint_solutions += 1;
        }
        *kinds.entry(kind).or_default() += 1;
        items.push(SnapshotItem {
            item,
            created_at,
            updated_at,
            accessed_at,
            access_count,
            recurrence_count,
        });
    }
    std::fs::create_dir_all(private)?;
    std::fs::write(private.join("corpus.json"), serde_json::to_vec(&items)?)?;
    let aggregate = serde_json::json!({"retained": items.len(), "excluded_credentials": excluded_credentials,
        "excluded_short": excluded_short, "kinds": kinds, "blueprint_solutions": blueprint_solutions,
        "source_files_detached": source_files_detached, "source_read_only": true});
    std::fs::write(
        private.join("prepare-aggregate.json"),
        serde_json::to_vec_pretty(&aggregate)?,
    )?;
    println!("{aggregate}");
    Ok(())
}

fn evaluate(model: &str, private: &Path, output: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let row = local::find(model).ok_or("unknown model")?;
    let active = ActiveEmbedder::global();
    active.replace(Arc::new(local::LocalProvider::new(row)));
    let items: Vec<SnapshotItem> =
        serde_json::from_slice(&std::fs::read(private.join("corpus.json"))?)?;
    let queries: Vec<Query> =
        serde_json::from_slice(&std::fs::read(private.join("queries.json"))?)?;
    let directory = tempfile::tempdir_in(private)?;
    let mem = MemManager::open_offline(directory.path())?;
    let mut ids = HashMap::new();
    for (index, item) in items.iter().enumerate() {
        let id = mem.insert_raw(&item.item)?;
        mem.with_conn(|connection| connection.execute(
            "UPDATE mem_items SET created_at=?1,updated_at=?2,accessed_at=?3,access_count=?4,recurrence_count=?5 WHERE id=?6",
            rusqlite::params![item.created_at,item.updated_at,item.accessed_at,item.access_count,item.recurrence_count,id],
        ))?;
        ids.insert(index, id);
    }
    let start = Instant::now();
    let report = mem.reembed_bulk(true, false)?;
    let seconds = start.elapsed().as_secs_f64();
    if !report.errors.is_empty()
        || report.items as usize != items.len()
        || mem.embedder_status().vectors == 0
    {
        return Err("snapshot re-embedding failed; private error text suppressed".into());
    }
    let mut tables: BTreeMap<String, Recall> = BTreeMap::new();
    for query in &queries {
        if contains_credentials(&query.text) {
            return Err("credential filter rejected generated query".into());
        }
        let gold = ids
            .get(&query.gold)
            .ok_or("query gold outside filtered corpus")?;
        let hybrid = mem.recall_with_config_untouched(
            &SearchQuery {
                query: query.text.clone(),
                limit: Some(10),
                ..Default::default()
            },
            &codeterm_mem::search::SearchConfig::default(),
        )?;
        let vector = mem.vector_recall(&query.text, 10)?;
        let ranks = [
            ("hybrid", hybrid.iter().position(|hit| &hit.item.id == gold)),
            ("vector", vector.iter().position(|hit| &hit.0 == gold)),
        ];
        for (arm, rank) in ranks {
            for stratum in [
                "all".to_string(),
                format!("language:{}", query.language),
                format!("kind:{:?}", items[query.gold].item.kind),
            ] {
                tables
                    .entry(format!("{arm}/{stratum}"))
                    .or_default()
                    .add(rank);
            }
        }
    }
    for recall in tables.values_mut() {
        recall.finish();
    }
    let result = serde_json::json!({"model": model, "identity": active.identity(), "items": items.len(),
        "vectors": mem.embedder_status().vectors, "reembed_wall_seconds": seconds,
        "items_per_second": report.items as f64 / seconds, "reembed_report": {"items": report.items, "entities": report.entities, "errors": report.errors.len()},
        "tables": tables, "private_text_in_output": false});
    std::fs::write(output, serde_json::to_vec_pretty(&result)?)?;
    drop(mem);
    directory.close()?;
    println!("snapshot measurement persisted; temporary model store deleted");
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    match args.first().map(String::as_str) {
        Some("prepare") if args.len() == 3 => prepare(Path::new(&args[1]), Path::new(&args[2])),
        Some("evaluate") if args.len() == 4 => {
            evaluate(&args[1], Path::new(&args[2]), Path::new(&args[3]))
        }
        _ => Err("usage: snapshot prepare SOURCE PRIVATE | evaluate MODEL PRIVATE OUTPUT".into()),
    }
}
