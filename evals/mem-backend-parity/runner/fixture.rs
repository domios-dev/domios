use codeterm_mem::{
    backend::{ExportPage, MemStore, SqliteMemStore, StoreTable, StoreValue, WriteContext},
    embedder::{index::InsertOutcome, StampedVector},
    MemKind, MemScope, NewMemItem,
};
use serde::Deserialize;
use std::{collections::HashSet, path::Path};

pub const CORPUS: &str = include_str!("../corpus.json");
pub const QUERIES: &str = include_str!("../queries.json");

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CorpusItem {
    pub title: String,
    pub body: String,
    pub area: String,
    pub kind: MemKind,
    pub tags: Vec<String>,
    pub importance: f64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Query {
    pub id: String,
    pub text: String,
    pub exact_title: String,
    pub area: Option<String>,
    pub kind: Option<String>,
}

pub fn read() -> Result<(Vec<CorpusItem>, Vec<Query>), String> {
    let corpus: Vec<CorpusItem> = serde_json::from_str(CORPUS).map_err(|e| e.to_string())?;
    let queries: Vec<Query> = serde_json::from_str(QUERIES).map_err(|e| e.to_string())?;
    if !(11..=50).contains(&corpus.len()) || queries.is_empty() {
        return Err("fixture needs 11..=50 items and at least one query".into());
    }
    let mut titles = HashSet::new();
    let mut ids = HashSet::new();
    for item in &corpus {
        if item.title.is_empty() || !titles.insert(&item.title) || item.body.trim().is_empty() {
            return Err("empty or duplicate corpus item".into());
        }
    }
    for query in &queries {
        let target = corpus.iter().find(|item| item.title == query.exact_title)
            .ok_or_else(|| format!("{}: expected title absent from corpus", query.id))?;
        if query.id.is_empty() || !ids.insert(&query.id) || query.text.trim().is_empty()
            || query.area.as_ref().is_some_and(|area| area != &target.area)
            || query.kind.as_ref().is_some_and(|kind| kind != target.kind.as_str())
        {
            return Err(format!("{}: invalid query or target excluded by filters", query.id));
        }
    }
    Ok((corpus, queries))
}

pub fn pages(store: &dyn MemStore) -> Result<Vec<ExportPage>, String> {
    let mut cursor = None;
    let mut pages = Vec::new();
    loop {
        let page = store.export_page(cursor.as_ref(), 1000)?;
        cursor = page.next.clone();
        pages.push(page);
        if cursor.is_none() { return Ok(pages); }
    }
}

pub fn seed(
    root: &Path,
    corpus: &[CorpusItem],
    vectors: &[StampedVector],
) -> Result<Vec<ExportPage>, String> {
    if vectors.len() != corpus.len() {
        return Err("passage-vector count differs from corpus count".into());
    }
    let store = SqliteMemStore::open_in_memory(root)?;
    let ctx = WriteContext::system("mem-backend-parity");
    store.init_vectors(&ctx, &vectors[0].identity)?;
    for (item, vector) in corpus.iter().zip(vectors) {
        let payload = NewMemItem {
            kind: item.kind, scope: MemScope::Global, title: item.title.clone(),
            summary: None, area: Some(item.area.clone()), tags: item.tags.clone(),
            body: item.body.clone(), source: None, importance: item.importance,
            source_path: None, pinned: false,
        };
        let saved = store.save(&ctx, &payload, false).map_err(|e| format!("{e:?}"))?;
        if store.insert_vector(&ctx, &saved.id, 0, vector)? != InsertOutcome::Inserted {
            return Err(format!("fixture vector not inserted: {}", item.title));
        }
    }
    let mut pages = pages(&store)?;
    for page in &mut pages {
        for row in &mut page.rows {
            for column in ["created_at", "updated_at", "stamped_at", "changed_at", "completed_at"] {
                if matches!(row.values.get(column), Some(StoreValue::Text(_))) {
                    row.values.insert(column.into(), StoreValue::Text("2026-01-01T00:00:00Z".into()));
                }
            }
            if page.table == StoreTable::Items {
                let id = row.values.get("id").ok_or("fixture item has no id")?.clone();
                row.values.insert("uid".into(), id);
            }
        }
    }
    Ok(pages)
}

pub fn load(store: &dyn MemStore, pages: &[ExportPage]) -> Result<(), String> {
    let ctx = WriteContext::system("mem-backend-parity");
    for page in pages { store.import_page(&ctx, page)?; }
    if self::pages(store)? != pages {
        return Err("loaded store differs from the frozen corpus/metadata/vector rows".into());
    }
    Ok(())
}
